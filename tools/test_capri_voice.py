#!/usr/bin/env python3
"""Tests for capri-voice that need no mic, speaker or model, on Linux and on Windows (CI runs both):
the sidecar's own --selftest rules (no models: the graph checks skip), the capture seam, and the
Windows platform layer (capri_voice_win): the link's handshake, the duck and pause rules with their
crash repair on fake sessions, the call check's reading of the privacy store, and on Windows that
the sidecar wires it all in. What needs a real desktop is [VM] (docs/WINDOWS.md).

python3 tools/test_capri_voice.py   (needs numpy; on Windows also sounddevice, for the smoke checks)"""
import atexit, collections, importlib.machinery, importlib.util, io, json, os, shutil, socket, sys, tempfile, threading, time, unittest
from contextlib import redirect_stderr, redirect_stdout
from unittest import mock
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import capri_voice_win as win  # noqa: E402

WIN = sys.platform == 'win32'
TMP = tempfile.mkdtemp(prefix='capri-voice-test-')
atexit.register(shutil.rmtree, TMP, ignore_errors=True)
# The sidecar's paths, forced: nothing of his (models, state, his running sidecar's link) is touched.
# The socket points into TMP, never at the default $XDG_RUNTIME_DIR/capri-voice.sock: serve() starts
# by removing SOCK, so a future test that calls it must not be able to delete his live socket.
os.environ['CAPRI_VOICE_MODELS'] = os.path.join(TMP, 'no-models')
os.environ['CAPRI_VOICE_STATE'] = os.path.join(TMP, 'media.json')
os.environ['CAPRI_VOICE_SOCK'] = os.path.join(TMP, 'capri-voice.sock')
os.environ.pop('CAPRI_VOICE_ENDPOINT', None)
win.log = lambda *a: None


def load_sidecar():
    loader = importlib.machinery.SourceFileLoader('capri_voice', str(HERE / 'capri-voice'))
    spec = importlib.util.spec_from_loader('capri_voice', loader)
    m = importlib.util.module_from_spec(spec)
    with redirect_stderr(io.StringIO()):
        loader.exec_module(m)
    return m


CV = load_sidecar()
win.log = lambda *a: None       # (on Windows the sidecar's setup gave it its own)


class Sidecar(unittest.TestCase):
    def test_selftest_without_models(self):
        out = io.StringIO()
        with redirect_stdout(out), redirect_stderr(io.StringIO()):
            CV.selftest()
        text = out.getvalue()
        for part in ('parsing ok', 'plain Kokoro fallback ok', 'unprompted gate ok', 'junk filter ok', 'endpointer ok', 'hands-free helpers ok',
                     'wake word phrase check ok', 'no models, skipping the graph checks'):
            self.assertIn(part, text)
        self.assertIn('duck rules: Windows' if WIN else 'duck rules ok', text)

    def test_the_platform_is_wired(self):
        if WIN:
            self.assertIs(CV.Media, win.Media)
            for n in ('screen_locked', 'locked_hint', 'in_call', 'dnd_on', 'sink_kind', 'node_missing'):
                self.assertIs(getattr(CV, n), getattr(win, n), n)
            self.assertTrue(CV.MODELS.endswith(os.path.join('no-models')) and CV.SOCK is None)
        else:
            self.assertFalse(hasattr(CV, '_win'), 'Linux never loads the Windows layer')
            self.assertEqual(CV.Media.__module__, 'capri_voice')
            self.assertEqual(CV.SOCK, os.path.join(TMP, 'capri-voice.sock'))

    def test_mic_chunk(self):
        """The capture seam: None while nothing came, the bytes, b'' at its end (as os.read)."""
        if WIN:
            class P:
                q = [None, b'\x01\x02', b'']

                def read(self, wait):
                    return self.q.pop(0)
            p = P()
            self.assertEqual([CV.mic_chunk(p, -1) for _ in range(3)], [None, b'\x01\x02', b''])
            return
        r, w = os.pipe()
        try:
            self.assertIsNone(CV.mic_chunk(None, r, 0.01))
            os.write(w, b'\x00' * 640)
            self.assertEqual(CV.mic_chunk(None, r), b'\x00' * 640)
            os.write(w, b'\x01' * 4000)
            self.assertEqual(len(CV.mic_chunk(None, r)), 3200, 'at most 3200 bytes a read')
            CV.mic_chunk(None, r)
            os.close(w)
            w = None
            self.assertEqual(CV.mic_chunk(None, r), b'')
        finally:
            os.close(r)
            if w is not None:
                os.close(w)


class Link(unittest.TestCase):
    def setUp(self):
        self.path = os.path.join(tempfile.mkdtemp(dir=TMP), 'endpoint.json')
        self.got, self.stop, ready = [], threading.Event(), threading.Event()
        self.t = threading.Thread(target=win.serve, args=(self.got.append, self.path, lambda port: ready.set(), self.stop), daemon=True)
        self.t.start()
        self.assertTrue(ready.wait(5))
        with open(self.path) as f:
            self.ep = json.load(f)

    def tearDown(self):
        self.stop.set()
        self.t.join(5)
        for c in self.got:
            c.close()

    def dial(self, hello):
        c = socket.create_connection(('127.0.0.1', self.ep['port']), timeout=5)
        if hello is not None:
            c.sendall(hello)
        return c

    def test_the_endpoint_file(self):
        self.assertEqual(set(self.ep), {'v', 'port', 'token', 'reply', 'pid'})
        self.assertEqual((len(self.ep['token']), len(self.ep['reply'])), (64, 64))
        self.assertNotEqual(self.ep['token'], self.ep['reply'])

    def test_the_right_hello_is_answered_then_she_is_the_avatar(self):
        c = self.dial((json.dumps({'op': 'hello', 'token': self.ep['token']}) + '\n').encode())
        line = win.read_line(c)
        self.assertEqual(json.loads(line), {'ev': 'hello', 'token': self.ep['reply']})
        t0 = time.time()
        while not self.got and time.time() - t0 < 5:
            time.sleep(0.01)
        self.assertEqual(len(self.got), 1)
        self.got[0].sendall(b'{"ev":"ready"}\n')
        self.assertEqual(win.read_line(c), b'{"ev":"ready"}')
        c.close()

    def test_a_wrong_or_missing_hello_is_closed_and_never_the_avatar(self):
        def closed(c):
            try:
                return win.read_line(c) is None
            except ConnectionResetError:  # closed with his bytes unread
                return True
        for hello in (b'{"op":"hello","token":"guess"}\n', b'{"op":"config"}\n', b'not json\n', b'x' * 600):
            c = self.dial(hello)
            self.assertTrue(closed(c), hello[:20])
            c.close()
        saved, win.HELLO_S = win.HELLO_S, 0.3
        try:
            c = self.dial(None)           # silent: closed after HELLO_S
            self.assertTrue(closed(c))
            c.close()
        finally:
            win.HELLO_S = saved
        self.assertEqual(self.got, [])

    def test_the_file_goes_with_the_sidecar(self):
        self.stop.set()
        self.t.join(5)
        self.assertFalse(os.path.exists(self.path))


class FakePlayer:
    """pw-play as _play_one sees it: its input, its exit code (0 played to the end; -9 killed, 1 its
    sink gone), kill."""

    def __init__(self, rc):
        self.rc, self.stdin, self.killed = rc, io.BytesIO(), False

    def wait(self, timeout=None):
        return self.rc

    def kill(self):
        self.killed = True


class SayEnd(unittest.TestCase):
    """How a sentence's say-end reads: the avatar counts a reply read only from whole ones (no cut,
    no error), so a sentence cut short or trimmed to the cap must say cut."""

    def voice(self, rc):
        v = object.__new__(CV.Voice)
        v.lock = threading.Lock()
        v.cv = threading.Condition(v.lock)
        v.gate = CV.Gate()
        v.gate.now_why = lambda unprompted=True: None          # not locked, no call
        v.gate.cache['dnd'] = (time.monotonic() + 1e6, False)   # no do-not-disturb
        v.play_q, v.hush_gen, v.player, v.playing, v.played = [], 0, None, None, None
        v.said = collections.deque(maxlen=40)
        v.events = []
        v.emit = lambda **ev: v.events.append(ev)
        v._player = lambda sid: FakePlayer(rc)
        return v

    def play(self, rc, secs, unprompted=False):
        v = self.voice(rc)
        pcm = CV.np.full(int(secs * CV.TTS_RATE), 0.1, CV.np.float32)
        v.play_q.append((7, 'A sentence.', pcm, [], CV.now_ms(), 0.0, 0, unprompted))
        with mock.patch.object(CV, 'TEST', None), mock.patch.object(CV, 'node_missing', lambda kind, name: False), \
                redirect_stderr(io.StringIO()):
            v._play_one()
        self.assertEqual(v.events[0]['ev'], 'say-start', v.events)
        self.assertEqual([e['ev'] for e in v.events[1:]], ['say-end'], v.events)
        return v.events[0], v.events[1]

    def test_played_to_its_end_is_whole(self):
        _, end = self.play(0, 1.0)
        self.assertEqual(end, {'ev': 'say-end', 'id': 7})
        _, end = self.play(0, 4.0, unprompted=True)
        self.assertEqual(end, {'ev': 'say-end', 'id': 7}, 'an unprompted one under the cap')

    def test_a_player_that_ended_early_says_cut(self):
        for rc in (-9, 1):     # killed from outside (Python's -SIGKILL); its sink unloaded mid-play
            _, end = self.play(rc, 1.0)
            self.assertEqual(end, {'ev': 'say-end', 'id': 7, 'cut': True}, rc)

    def test_a_first_sentence_trimmed_to_the_cap_says_cut(self):
        start, end = self.play(0, 13.0, unprompted=True)
        self.assertEqual(start['dur_ms'], round(CV.CAP_S * 1000), 'what plays is the cap')
        self.assertEqual(end, {'ev': 'say-end', 'id': 7, 'cut': True}, 'its player exits 0, but it was not said whole')
        _, end = self.play(0, 13.0)
        self.assertEqual(end, {'ev': 'say-end', 'id': 7}, 'his own turn has no cap: whole')


class WindowsPlayer(unittest.TestCase):
    """capri_voice_win.Player.wait answers as pw-play exits: 0 only when all it was given played."""

    class Stream:
        def close(self):
            pass

        def abort(self):
            pass

    def player(self, n, played, eof=True):
        p = object.__new__(win.Player)
        p.buf, p.pos, p.eof = bytearray(n), played, eof
        p.lk, p.done = threading.Lock(), threading.Event()
        p.stream = self.Stream()
        return p

    def test_wait(self):
        p = self.player(4800, 0)
        with self.assertRaises(win.subprocess.TimeoutExpired):
            p.wait(timeout=0.01)
        p.pos = 4800
        p.done.set()
        self.assertEqual(p.wait(timeout=0.01), 0, 'played to its end')
        p = self.player(4800, 1200)
        p.kill()
        self.assertEqual(p.wait(timeout=0.01), 1, 'killed mid-sentence')
        p = self.player(4800, 4800, eof=False)
        p.kill()
        self.assertEqual(p.wait(timeout=0.01), 1, 'killed before its input closed')


class FakeSessions:
    def __init__(self, *ss):
        self.ss = {s['key']: dict(s) for s in ss}

    def list(self):
        return [dict(s) for s in self.ss.values()]

    def set(self, key, vol):
        self.ss[key]['vol'] = vol
        return True


def sess(key, app, vol=0.8, active=True, pid=100, system=False):
    return {'key': key, 'app': app, 'bin': f'{app.lower()}.exe', 'pid': pid, 'vol': vol, 'active': active, 'system': system}


class FakeSmtc:
    def __init__(self, **players):
        self.p = dict(players)

    def players(self):
        return list(self.p.items())

    def pause(self, a):
        self.p[a] = win.PAUSED

    def play(self, a):
        self.p[a] = win.PLAYING


class Duck(unittest.TestCase):
    def setUp(self):
        self.state = os.path.join(tempfile.mkdtemp(dir=TMP), 'media.json')
        self.s = FakeSessions(sess('ff', 'Firefox'), sess('dc', 'Discord'), sess('me', 'python', pid=os.getpid()),
                              sess('sys', 'System', pid=0, system=True), sess('sp', 'Spotify', active=False, vol=0.6))

    def media(self, call=''):
        m = win.Media(sessions=self.s, smtc=FakeSmtc(), state=self.state, call=lambda: call, every=3600)
        m.ignore = ['discord']
        return m

    def vol(self, k):
        return self.s.ss[k]['vol']

    def test_duck_and_back(self):
        m = self.media()
        m.hold('duck')
        self.assertAlmostEqual(self.vol('ff'), 0.1)
        self.assertEqual((self.vol('dc'), self.vol('me'), self.vol('sys'), self.vol('sp')), (0.8, 0.8, 0.8, 0.6),
                         'calls (the ignore list), her own, system sounds, a quiet session: untouched')
        self.assertTrue(os.path.exists(self.state), 'saved at once')
        m.hold('duck')
        self.assertAlmostEqual(self.vol('ff'), 0.1, msg='nested: the first hold stands')
        self.s.ss['sp']['active'] = True       # it starts playing mid-hold
        m.poll(set())
        self.assertAlmostEqual(self.vol('sp'), 0.075)
        m.release()
        self.assertEqual((self.vol('ff'), self.vol('sp')), (0.8, 0.6))
        self.assertFalse(os.path.exists(self.state))

    def test_his_change_wins(self):
        m = self.media()
        m.hold('duck')
        self.s.ss['ff']['vol'] = 0.5
        m.release()
        self.assertEqual(self.vol('ff'), 0.5)

    def test_a_call_or_unknown_touches_nothing(self):
        for call in ('Teams', None):
            m = self.media(call)
            m.hold('duck')
            self.assertEqual(self.vol('ff'), 0.8)
            self.assertEqual(m.held['call'], call or 'unknown')
            m.release()

    def test_a_crash_is_repaired_at_the_next_start(self):
        self.media().hold('duck')              # and the sidecar dies here
        self.assertAlmostEqual(self.vol('ff'), 0.1)
        self.media()
        self.assertEqual(self.vol('ff'), 0.8)
        self.assertFalse(os.path.exists(self.state))

    def test_one_that_died_ducked_comes_back_whole(self):
        m = self.media()
        m.hold('duck')
        del self.s.ss['ff']                    # Firefox quits while ducked
        m.poll({'ff'})
        m.release()
        self.assertEqual(len(m.vanished), 1)
        self.assertTrue(os.path.exists(self.state), 'the vanished originals are kept')
        self.s.ss['ff2'] = sess('ff2', 'Firefox', vol=0.1)   # Windows put its saved (ducked) volume back
        m.poll(set())
        self.assertEqual(self.vol('ff2'), 0.8)
        self.assertEqual(m.vanished, [])

    def test_pause_plays_only_what_it_paused(self):
        m = self.media()
        m.smtc = FakeSmtc(spotify=win.PLAYING, edge=win.PAUSED, vlc=win.PLAYING)
        m.hold('pause')
        self.assertEqual(m.smtc.p, {'spotify': win.PAUSED, 'edge': win.PAUSED, 'vlc': win.PAUSED})
        m.smtc.p['vlc'] = win.PLAYING          # he played it himself
        m.release()
        self.assertEqual(m.smtc.p, {'spotify': win.PLAYING, 'edge': win.PAUSED, 'vlc': win.PLAYING})


class Calls(unittest.TestCase):
    def test_the_privacy_store(self):
        py = r'C:\Users\h\AppData\Local\synaps-avatar\voice\python.exe'
        own = (py.replace('\\', '#'), False, 133, 0)
        self.assertIsNone(win.capturing_app([own], py), 'her own capture is not a call')
        discord = (r'C:#Users#h#AppData#Local#Discord#app-1.0#Discord.exe', False, 133, 0)
        self.assertEqual(win.capturing_app([own, discord], py), 'Discord')
        self.assertIsNone(win.capturing_app([(discord[0], False, 133, 134)], py), 'stopped: not now')
        self.assertIsNone(win.capturing_app([(discord[0], False, 0, 0)], py), 'never used')
        self.assertEqual(win.capturing_app([('MSTeams_8wekyb3d8bbwe', True, 133, 0)], py), 'MSTeams')

    def test_the_watch_stops(self):
        hits = []
        w = win.CallWatch(lambda: hits.append(1) or len(hits) >= 2, every=0.01)
        time.sleep(0.2)
        w.terminate()
        self.assertEqual(len(hits), 2, 'a call found ends the watch')

    def test_headphones_by_name(self):
        for n in ('Headphones (Realtek(R) Audio)', 'Headset Earphone (HyperX Cloud)', 'AirPods Pro Hands-Free AG Audio'):
            self.assertTrue(win.HEADPHONES.search(n), n)
        self.assertFalse(win.HEADPHONES.search('Speakers (Realtek(R) Audio)'))


class LockUnknown(unittest.TestCase):
    """An unknown lock state is logged once per reason, not every second it is asked."""

    def test_logged_once(self):
        seen, keep = [], win.log
        win.log = lambda *a: seen.append(' '.join(map(str, a)))
        try:
            win._lock_unknown.clear()
            for _ in range(3):
                self.assertIsNone(win.lock_unknown('SessionFlags 7'))
            self.assertIsNone(win.lock_unknown('WTSQuerySessionInformation failed: 5'))
        finally:
            win.log = keep
            win._lock_unknown.clear()
        self.assertEqual(len(seen), 2, seen)
        self.assertIn('SessionFlags 7', seen[0])


@unittest.skipUnless(WIN, 'Windows only')
class WindowsSmoke(unittest.TestCase):
    """The live calls answer without raising (what they answer needs a desktop: [VM])."""

    def test_they_answer(self):
        c = win.in_call()
        self.assertTrue(c is None or isinstance(c, str), c)
        self.assertIsInstance(win.dnd_on(), bool)
        self.assertIsInstance(win.screen_locked(), bool)
        self.assertIn(win.session_locked(), (True, False, None))
        self.assertIn(win.sink_kind(), ('speakers', 'headphones'))
        self.assertTrue(win.node_missing('sources', 'capri-no-such-device'))

    def test_the_bindings(self):
        """With the bundle's packages installed (CI's wheel step), each call is shaped right. A box
        without audio or media may refuse (COM, PortAudio, no service); a wrong name may not."""
        import importlib.util
        if not all(importlib.util.find_spec(m) for m in ('pycaw', 'winrt', 'sounddevice')):
            self.skipTest('the bundle packages are not installed')
        for name, call in (('SMTC players', lambda: win.Smtc().players()), ('sessions', lambda: win.Sessions().list()),
                           ('mic', lambda: win.Mic(16000).terminate()), ('speaker', lambda: win.Player(24000).kill())):
            try:
                r = call()
            except (AttributeError, TypeError, NameError, ImportError):
                raise
            except Exception as e:
                print(f'  {name}: refused here ({type(e).__name__}: {e})', file=sys.stderr)
            else:
                print(f'  {name}: {r!r}', file=sys.stderr)
                if r is not None:
                    self.assertIsInstance(r, list)


if __name__ == '__main__':
    unittest.main(verbosity=2)
