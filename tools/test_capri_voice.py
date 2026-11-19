#!/usr/bin/env python3
"""Tests for capri-voice that need no mic, speaker or model, on Linux and on Windows (CI runs both):
the sidecar's own --selftest rules (no models: the graph checks skip), the capture seam, and the
Windows platform layer (capri_voice_win): the link's handshake, the duck and pause rules with their
crash repair on fake sessions, the call check's reading of the privacy store, and on Windows that
the sidecar wires it all in. What needs a real desktop is [VM] (docs/WINDOWS.md).

python3 tools/test_capri_voice.py   (needs numpy; on Windows also sounddevice, for the smoke checks)"""
import atexit, collections, importlib.machinery, importlib.util, io, json, os, shutil, socket, sys, tempfile, threading, time, types, unittest
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
        for part in ('parsing ok', 'plain Kokoro fallback ok', 'unprompted gate ok', 'junk filter ok', 'vocab ok', 'endpointer ok', 'hands-free helpers ok',
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


class Vocab(unittest.TestCase):
    """`[voice] vocab` in the config op: appended to Whisper's prompt, kept across a model change."""

    def voice(self):
        v = object.__new__(CV.Voice)
        v.cfg = {'stt_model': 'small.en', 'voice': CV.VOICE, 'speed': CV.SPEED, 'flow': CV.FLOW, 'pace': CV.PACE, 'vocab': []}
        v.stt = types.SimpleNamespace(name='small.en', prompt=CV.VOCAB)
        v.vocab = CV.VOCAB_WORDS
        v.events = []
        v.emit = lambda **ev: v.events.append(ev)
        return v

    def test_config_sets_the_prompt(self):
        v = self.voice()
        with redirect_stderr(io.StringIO()):
            v.handle({'op': 'config', 'vocab': ['Lynx', 'polish-e', 'Capri']})
        self.assertEqual(v.stt.prompt, CV.VOCAB + ', Lynx, polish-e')
        self.assertIn('lynx', v.vocab)
        self.assertEqual(v.events[-1]['ev'], 'ready')
        self.assertEqual(v.events[-1]['cfg']['vocab'], ['Lynx', 'polish-e', 'Capri'])
        with redirect_stderr(io.StringIO()):
            v.handle({'op': 'config', 'voice': CV.VOICE})       # no vocab key: the words stay
        self.assertEqual(v.stt.prompt, CV.VOCAB + ', Lynx, polish-e')
        with redirect_stderr(io.StringIO()):
            v.handle({'op': 'config', 'vocab': []})              # an empty list: back to the default
        self.assertEqual((v.stt.prompt, v.vocab), (CV.VOCAB, CV.VOCAB_WORDS))

    def test_a_new_model_gets_the_words(self):
        v = self.voice()
        made = []

        def fake_stt(name):
            made.append(name)
            return types.SimpleNamespace(name=name, prompt=CV.VOCAB)
        with mock.patch.object(CV, 'Stt', fake_stt), redirect_stderr(io.StringIO()):
            v.handle({'op': 'config', 'stt_model': 'base.en', 'vocab': ['Lynx']})
        self.assertEqual((made, v.stt.name, v.stt.prompt), (['base.en'], 'base.en', CV.VOCAB + ', Lynx'))

    def test_ready_says_if_her_mic_is_named(self):
        """A named mic (CAPRI_VOICE_SOURCE; Windows: his `[voice] mic`) never falls back to the default,
        so the avatar asks again for a wake word whose stream lost it."""
        for source, named in ((None, False), ('Headset Microphone (WH-1000XM4 Hands-Free)', True)):
            v = self.voice()
            with mock.patch.object(CV, 'SOURCE', source), redirect_stderr(io.StringIO()):
                v.handle({'op': 'config'})
            self.assertEqual((v.events[-1]['ev'], v.events[-1]['named_mic']), ('ready', named), source)

    def test_a_bad_vocab_is_ignored(self):
        v = self.voice()
        with redirect_stderr(io.StringIO()):
            v.handle({'op': 'config', 'vocab': 'Lynx'})
        self.assertEqual(v.stt.prompt, CV.VOCAB)


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


class Refusals(unittest.TestCase):
    """A refused capture is logged; the wake word's only when its reason changes: the avatar asks for it
    every 30 s while his named mic is gone (every 3 s in a call), and one line each time was ~2,900 a day."""

    def voice(self):
        v = object.__new__(CV.Voice)
        v.rec, v.rec_stop_at, v.cur = None, None, {}
        v.gate = CV.Gate()
        v.gate.cache['hint'] = (time.monotonic() + 1e6, False)   # logind: not locked
        v.vad, v.wake, v.wake_paused_until, v.wake_refused = object(), object(), 0.0, None
        v.events = []
        v.emit = lambda **ev: v.events.append(ev)
        return v

    def test_the_wake_words_refusal_is_logged_once_per_reason(self):
        v = self.voice()
        gone, call = [True], ['']
        err = io.StringIO()
        with mock.patch.object(CV, 'TEST', None), mock.patch.object(CV, 'SOURCE', 'Headset'), \
                mock.patch.object(CV, 'node_missing', lambda kind, name: gone[0]), \
                mock.patch.object(CV, 'screen_locked', lambda: False), mock.patch.object(CV, 'in_call', lambda: call[0]), \
                redirect_stderr(err):
            def ask(mode='wake'):
                v.listen_start(mode)
                return v.events[-1]['why']
            for _ in range(5):
                self.assertEqual(ask(), 'NO MIC · ITS SOURCE IS GONE', 'still refused, still said to the avatar')
            self.assertEqual(ask('hold'), 'NO MIC · ITS SOURCE IS GONE', 'his press')
            gone[0], call[0] = False, 'Teams'
            for _ in range(3):
                self.assertEqual(ask(), 'WAKE PAUSED · IN A CALL')
            gone[0] = True
            ask()
            ask()
            v._log_refusal('wake', None)      # a wake stream opened
            ask()
        lines = [l.split(' ', 1)[1] for l in err.getvalue().splitlines()]
        self.assertEqual(lines, ['mic: refused (NO MIC · ITS SOURCE IS GONE)'] * 2 + ['mic: refused (WAKE PAUSED · IN A CALL)']
                         + ['mic: refused (NO MIC · ITS SOURCE IS GONE)'] * 2,
                         'once per reason; his press always; again after one got through')
        self.assertEqual(len(v.events), 2 * 12, 'every ask still answered (mic-closed, heard)')


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


class FakeSd:
    """sounddevice as the stock PortAudio shows a Windows PC: his USB mic three times (MME, its name
    cut at 31 characters; DirectSound; WASAPI), the built-in mic WASAPI's default, and a headset
    that is only a speaker. What gets opened is kept. `later`: devices PortAudio lists only once it
    starts again (`_terminate`, `_initialize`: a Bluetooth headset that connected since), which
    also closes every stream, as Pa_Terminate does. `gone`: devices that went away (`drop`): listed
    on till PortAudio starts again, and opening one fails."""
    MIC = 'Microphone (USB Audio Device Pro)'
    BT = 'Headset Microphone (WH-1000XM4 Hands-Free)'

    def __init__(self, later=()):
        self.opened, self.streams, self.later, self.extra, self.gone = [], [], list(later), [], set()
        self._initialized, self.restarts, self.fail_start, self.no_stream_start = 1, 0, False, False

    def drop(self, name):
        """A headset out of range: its streams end by themselves (finished_callback, as WASAPI's do)."""
        self.gone.add(name)
        for st in self.streams:
            if st.name == name:
                st.end()

    def back(self, name):
        self.gone.discard(name)
        if name not in self.extra:
            self.later.append(name)

    def _terminate(self):
        assert self._initialized > 0, 'Pa_Terminate: not initialized'
        self._initialized -= 1
        for st in self.streams:
            st.closed, st.active = True, False

    def _initialize(self):
        if self.fail_start:
            self.fail_start = False
            raise RuntimeError('Error initializing PortAudio')
        self._initialized += 1
        self.restarts += 1
        self.extra, self.later = [n for n in self.extra + self.later if n not in self.gone], []

    def query_hostapis(self):
        assert self._initialized > 0, 'PortAudio not initialized'
        return [{'name': 'MME', 'default_input_device': 0, 'default_output_device': -1},
                {'name': 'Windows DirectSound', 'default_input_device': 2, 'default_output_device': -1},
                {'name': 'Windows WASAPI', 'default_input_device': 4, 'default_output_device': 6}]

    def query_devices(self):
        assert self._initialized > 0, 'PortAudio not initialized'
        d = lambda i, name, api, cin, cout: {'index': i, 'name': name, 'hostapi': api, 'max_input_channels': cin, 'max_output_channels': cout}
        return [d(0, 'Microsoft Sound Mapper - Input', 0, 2, 0), d(1, self.MIC[:31], 0, 2, 0),
                d(2, 'Primary Sound Capture Driver', 1, 2, 0), d(3, self.MIC, 1, 2, 0),
                d(4, 'Microphone (Realtek(R) Audio)', 2, 2, 0), d(5, self.MIC, 2, 1, 0),
                d(6, 'Speakers (Realtek(R) Audio)', 2, 0, 2), d(7, 'Headset (USB)', 2, 0, 2)] + \
            [d(8 + i, name, 2, 1, 0) for i, name in enumerate(self.extra)]

    class WasapiSettings:
        def __init__(self, auto_convert):
            self.auto_convert = auto_convert

    class Stream:
        def __init__(self, sd, name=None, finished=None):
            self.sd, self.active, self.closed, self.name, self.finished = sd, True, False, name, finished

        def start(self):
            if self.sd.no_stream_start:
                raise RuntimeError('Error starting stream: Unanticipated host error')

        def end(self):
            """It goes inactive: its finished_callback runs (PortAudio's, on abort and close too)."""
            if self.active:
                self.active = False
                if self.finished:
                    self.finished()

        def abort(self):
            self.end()

        def close(self):
            self.end()
            self.closed = True

    def RawInputStream(self, **kw):
        name = {d['index']: d['name'] for d in self.query_devices()}.get(kw.get('device'))
        if name in self.gone:
            raise RuntimeError('Error opening RawInputStream: Unanticipated host error [PaErrorCode -9999]')
        self.opened.append(('input', kw))
        self.streams.append(self.Stream(self, name, kw.get('finished_callback')))
        return self.streams[-1]

    RawOutputStream = RawInputStream


class NamedMic(unittest.TestCase):
    """His [voice] mic is one device: the exact name, an input, under WASAPI (by its index, so
    sounddevice never sees an ambiguous name), for the open and the "is it there" check alike."""

    def setUp(self):
        self.sd = FakeSd()
        p = mock.patch.dict(sys.modules, {'sounddevice': self.sd})
        p.start()
        self.addCleanup(p.stop)
        self.addCleanup(lambda: self.assertEqual([s for s in win._STREAMS if not s.closed], [], 'every stream closed'))
        self.t = 1000.0
        for p in (mock.patch.object(win, '_clock', lambda: self.t), mock.patch.object(win, '_reloaded_at', None),
                  mock.patch.object(win, '_stale', False)):
            p.start()
            self.addCleanup(p.stop)

    def test_one_device_per_name(self):
        sd, mic = self.sd, FakeSd.MIC
        self.assertEqual(win._named(sd, mic, 'input'), 5, 'the WASAPI one of three')
        self.assertEqual(win._named(sd, f' {mic} ', 'input'), 5)
        for other in ('Microphone', mic[:31], 'microphone (usb audio device pro)', ''):
            self.assertIsNone(win._named(sd, other, 'input'), f'{other!r}: exact names only')
        self.assertIsNone(win._named(sd, 'Headset (USB)', 'input'), 'a speaker is no mic')
        self.assertEqual(win._named(sd, 'Headset (USB)', 'output'), 7)

    def test_the_check_and_the_open_agree(self):
        mic = FakeSd.MIC
        self.assertFalse(win.node_missing('sources', mic))
        self.assertFalse(win.node_missing('sources', ''), 'none named: the default')
        for gone in ('Microphone', mic[:31], 'Headset (USB)'):
            self.assertTrue(win.node_missing('sources', gone), gone)
        win.Mic(16000, mic).terminate()
        kind, kw = self.sd.opened.pop()
        self.assertEqual((kind, kw['device']), ('input', 5), 'opened by its WASAPI index, never by name')
        self.assertTrue(kw['extra_settings'].auto_convert)
        with self.assertRaises(LookupError):
            win.Mic(16000, 'Microphone')
        self.assertEqual(self.sd.opened, [], 'a mic that is not there: nothing opened, never the default instead')
        win.Mic(16000).terminate()
        self.assertEqual(self.sd.opened.pop()[1]['device'], 4, "none named: WASAPI's default")
        p = win.Player(24000, 'Headset (USB)')
        p.kill()
        self.assertEqual((p.wait(0), self.sd.opened.pop()[1]['device']), (1, 7))

    def use(self, sd):
        self.sd = sd
        p = mock.patch.dict(sys.modules, {'sounddevice': sd})
        p.start()
        self.addCleanup(p.stop)
        return sd

    def test_a_mic_that_came_later_is_found(self):
        """A mic PortAudio didn't list when it started (a Bluetooth headset connecting after
        sign-in): on a miss its list is read again, so it's found without a restart of hers."""
        sd = self.use(FakeSd(later=[FakeSd.BT]))
        self.assertFalse(win.node_missing('sources', FakeSd.BT), 'found once the list was read again')
        self.assertEqual((sd.restarts, sd._initialized), (1, 1))
        self.assertFalse(win.node_missing('sources', FakeSd.BT))
        self.assertEqual(sd.restarts, 1, 'a hit reads nothing again')
        win.Mic(16000, FakeSd.BT).terminate()
        self.assertEqual(sd.opened.pop()[1]['device'], 8)
        self.t += 9.9
        self.assertTrue(win.node_missing('sources', 'Microphone (Nothing Here)'))
        self.assertEqual(sd.restarts, 1, 'a miss reads it again at most once per 10 s')
        self.t += 0.1
        self.assertTrue(win.node_missing('sources', 'Microphone (Nothing Here)'))
        self.assertEqual((sd.restarts, sd._initialized), (2, 1), 'then again, and PortAudio stays up')
        for _ in range(30):                       # the wake word asks every 30 s while it's gone, a talk press in between
            self.t += 3
            win.node_missing('sources', 'Microphone (Nothing Here)')
        self.assertEqual(sd.restarts, 2 + 7, 'never more often than that: here every fourth look (12 s)')

    def test_never_while_a_stream_of_hers_is_open(self):
        """Starting PortAudio again ends every stream: while one of hers is open (her speech, the
        wake word's mic) a miss stays a miss; once it closes, the next look reads the list again."""
        sd = self.use(FakeSd(later=[FakeSd.BT]))
        p = win.Player(24000)
        self.assertTrue(win.node_missing('sources', FakeSd.BT), 'not while she speaks')
        self.assertEqual(sd.restarts, 0)
        self.assertFalse(p.stream.closed, 'her speech plays on')
        with self.assertRaises(LookupError):
            win.Mic(16000, FakeSd.BT)
        self.assertEqual(sd.restarts, 0)
        p.close()
        p.done.set()
        self.assertEqual(p.wait(0), 0)
        self.assertFalse(win.node_missing('sources', FakeSd.BT), 'her speech is over: read again, found')
        self.assertEqual(sd.restarts, 1)
        m = win.Mic(16000, FakeSd.BT)
        self.t += 60
        self.assertTrue(win.node_missing('sources', 'Microphone (Nothing Here)'))
        self.assertEqual(sd.restarts, 1, 'the mic is open')
        self.assertFalse(m.stream.closed)
        m.terminate()

    def test_a_failed_start_is_tried_again(self):
        sd = self.use(FakeSd(later=[FakeSd.BT]))
        sd.fail_start = True
        self.assertTrue(win.node_missing('sources', FakeSd.BT), 'PortAudio would not start again')
        self.assertEqual(sd._initialized, 0)
        self.assertFalse(win.node_missing('sources', FakeSd.BT), 'the next look starts it')
        self.assertEqual((sd.restarts, sd._initialized), (1, 1))
        sd._terminate()
        win.Mic(16000).terminate()
        self.assertEqual((sd.restarts, sd.opened.pop()[1]['device']), (2, 4), "down: started before an open, even of the default")

    def test_a_down_portaudio_is_started_whenever(self):
        """The 10 s cap is for misses: PortAudio down (a start again failed) is started on the next look."""
        sd = self.use(FakeSd(later=[FakeSd.BT]))
        sd.fail_start = True
        self.assertTrue(win.node_missing('sources', FakeSd.BT))
        self.t += 1
        self.assertFalse(win.node_missing('sources', FakeSd.BT), 'down: started at once, the cap or not')
        self.assertEqual((sd.restarts, sd._initialized), (1, 1))

    def test_a_named_mic_that_dropped_is_looked_for_again(self):
        """His named headset went out of range mid-listen: its stream ends by itself, and PortAudio still
        lists it (its list is a snapshot), so a hit on its name would prove nothing. The list is marked
        stale: the next look (the wake word asks again in 30 s) reads it again, finds it gone (ITS SOURCE
        IS GONE), and once it is back finds it again."""
        sd = self.use(FakeSd(later=[FakeSd.BT]))
        self.assertFalse(win.node_missing('sources', FakeSd.BT))
        m = win.Mic(16000, FakeSd.BT)
        self.t += 60
        sd.drop(FakeSd.BT)
        self.assertEqual(m.read(0), b'', 'the mic is lost')
        m.terminate()
        self.assertEqual(sd.restarts, 1)
        self.t += 30
        self.assertTrue(win.node_missing('sources', FakeSd.BT), 'read again: gone, not the old entry')
        self.assertEqual(sd.restarts, 2)
        self.t += 30
        self.assertTrue(win.node_missing('sources', FakeSd.BT), 'still gone: a miss')
        sd.back(FakeSd.BT)
        self.t += 30
        self.assertFalse(win.node_missing('sources', FakeSd.BT), 'back: found')
        self.assertEqual(sd.restarts, 4)
        win.Mic(16000, FakeSd.BT).terminate()
        self.assertEqual(sd.opened.pop()[1]['device'], 8)
        self.t += 60
        self.assertFalse(win.node_missing('sources', FakeSd.BT))
        self.assertEqual(sd.restarts, 4, 'closed by her, not lost: nothing is read again')

    def test_a_named_open_that_fails_marks_the_list_stale(self):
        """A device PortAudio still lists went away with nothing of hers on it: the check says it's there
        and the open fails (CAPTURE FAILED). The next look reads the list again, within the same cap."""
        sd = self.use(FakeSd(later=[FakeSd.BT]))
        self.assertFalse(win.node_missing('sources', FakeSd.BT))
        sd.gone.add(FakeSd.BT)
        self.t += 3
        self.assertFalse(win.node_missing('sources', FakeSd.BT), 'PortAudio still lists it')
        with self.assertRaises(RuntimeError):
            win.Mic(16000, FakeSd.BT)
        self.assertFalse(win.node_missing('sources', FakeSd.BT), 'stale, but 3 s since the last read: the cap holds')
        self.assertEqual(sd.restarts, 1)
        self.t += 10
        self.assertTrue(win.node_missing('sources', FakeSd.BT), 'read again: gone')
        self.assertEqual(sd.restarts, 2)
        with self.assertRaises(LookupError):
            win.Mic(16000, FakeSd.BT)          # ITS SOURCE IS GONE, not CAPTURE FAILED

    def test_a_stream_that_will_not_start_is_closed(self):
        """A stream whose start fails is closed at once: left open (till the garbage collector) it
        would keep the device list from being read again."""
        sd = self.use(FakeSd(later=[FakeSd.BT]))
        sd.no_stream_start = True
        with self.assertRaises(RuntimeError):
            win.Mic(16000, FakeSd.MIC)
        with self.assertRaises(RuntimeError):
            win.Player(24000)
        self.assertEqual(len(sd.streams), 2)
        self.assertTrue(all(s.closed for s in sd.streams), 'both closed')
        self.assertEqual([s for s in win._STREAMS if not s.closed], [])
        sd.no_stream_start = False
        self.assertFalse(win.node_missing('sources', FakeSd.BT), 'nothing of hers open: read again, found')
        self.assertEqual(sd.restarts, 1)


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
        try:
            import sounddevice as sd
        except OSError as e:
            print(f'  sounddevice: refused here ({e})', file=sys.stderr)
        else:
            for a in ('_initialize', '_terminate', '_initialized'):
                self.assertTrue(hasattr(sd, a), f'sounddevice.{a}: how its device list is read again (capri_voice_win._reload)')
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
