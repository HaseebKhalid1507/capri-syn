"""capri_voice_win: capri-voice's Windows platform layer. Linux never imports it: there the sidecar's
own code talks to PipeWire, pactl, playerctl, logind, dunst and /proc. On Windows, capri-voice
imports this module (it sits next to the script) and uses these names in their place, with the
same contracts:

  serve           the avatar's link: loopback TCP and a token handshake (stock CPython has no
                  AF_UNIX on Windows), instead of the Unix socket
  Mic, Player     capture and playback on PortAudio (sounddevice, WASAPI), shaped like the
                  pw-record / pw-play processes they stand for
  Media           pause (SMTC) or duck (WASAPI per-app session volume), with the Linux rules and
                  the same crash repair (the state file, the vanished list)
  screen_locked, locked_hint, in_call, dnd_on, sink_kind, node_missing

The Windows-only packages (sounddevice, pycaw/comtypes, winrt) are imported where they are used,
so the module imports on any OS and its pure parts are tested on Linux too
(tools/test_capri_voice.py). What only a real desktop can show is marked [VM] in docs/WINDOWS.md."""
import hmac, json, ntpath, os, queue, re, secrets, socket, subprocess, sys, threading, time

STATE = None                  # capri-voice sets these (setup)
PLAYERS, DUCK_MATCH = [], None
VANISHED_S = 21600


def log(*a):
    print(time.strftime('%H:%M:%S'), *a, file=sys.stderr, flush=True)


def setup(**kw):
    globals().update(kw)


def write_json(path, v):
    """Atomic: a crash leaves the old file or the new one, never half of one."""
    os.makedirs(os.path.dirname(path), exist_ok=True)
    tmp = f'{path}.tmp'
    with open(tmp, 'w') as f:
        json.dump(v, f)
    os.replace(tmp, path)


def data_dir():
    """%LOCALAPPDATA%\\capri-voice (his own: other users can't read it)."""
    base = os.environ.get('LOCALAPPDATA') or os.path.expanduser('~')
    return os.path.join(base, 'capri-voice')


# ---- the link: loopback TCP + endpoint.json ----------------------------------------------------
#
# The sidecar binds 127.0.0.1 on a port the OS picks and writes endpoint.json (the port and two
# fresh 256-bit tokens) under %LOCALAPPDATA%\capri-voice, which only he and the admins can read.
# The avatar connects and sends {"op":"hello","token"} first; the sidecar answers
# {"ev":"hello","token": reply} and only then takes her as its avatar (the one before is dropped,
# as on Linux). A wrong or missing hello within HELLO_S is closed, and never displaces her. Any
# local process can reach the port, but without the file it can neither drive the sidecar nor pose
# as one: a stale file's port, taken by another program, gets a dead token and never the reply.
# Why not a named pipe (which would carry his ACL): the sidecar's loop is built on sockets, and a
# pipe server in stock Python needs pywin32 or an asyncio rewrite.

HELLO_S = 2.0


def endpoint_path():
    return os.environ.get('CAPRI_VOICE_ENDPOINT') or os.path.join(data_dir(), 'endpoint.json')


def read_line(c, limit=512):
    """One line, a byte at a time (nothing past it is taken off the socket)."""
    b = bytearray()
    while len(b) < limit:
        x = c.recv(1)
        if not x:
            return None
        if x == b'\n':
            return bytes(b)
        b += x
    return None


def hello_ok(line, token):
    try:
        m = json.loads(line)
    except (ValueError, TypeError):
        return False
    return isinstance(m, dict) and m.get('op') == 'hello' and isinstance(m.get('token'), str) and hmac.compare_digest(
        m['token'].encode(), token.encode())


def serve(accepted, path=None, ready=None, stop=None):
    """Accept avatars until `stop` is set; each one that passes the handshake goes to accepted(c).
    ready(port) is called once the endpoint file is written (tests)."""
    path = path or endpoint_path()
    srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    srv.bind(('127.0.0.1', 0))
    srv.listen(4)
    port = srv.getsockname()[1]
    token, reply = secrets.token_hex(32), secrets.token_hex(32)
    write_json(path, {'v': 1, 'port': port, 'token': token, 'reply': reply, 'pid': os.getpid()})
    log('listening on', f'127.0.0.1:{port}', f'(endpoint {path})')
    if ready:
        ready(port)

    def shake(c):
        try:
            c.settimeout(HELLO_S)
            line = read_line(c)
            if line is None or not hello_ok(line, token):
                log('link: a connection without the right hello: closed')
                c.close()
                return
            c.sendall((json.dumps({'ev': 'hello', 'token': reply}) + '\n').encode())
            c.settimeout(None)
        except OSError:
            c.close()
            return
        accepted(c)

    try:
        srv.settimeout(0.5)
        while stop is None or not stop.is_set():
            try:
                c, _ = srv.accept()
            except socket.timeout:
                continue
            c.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
            threading.Thread(target=shake, args=(c,), daemon=True).start()
    finally:
        srv.close()
        try:
            with open(path) as f:
                ours = json.load(f).get('token') == token
            if ours:
                os.remove(path)
        except (OSError, ValueError):
            pass


# ---- audio: PortAudio (sounddevice) ------------------------------------------------------------

def _wasapi(sd, kind):
    """WASAPI's host API index and its default device of `kind` ('input'/'output'), or None."""
    try:
        for i, h in enumerate(sd.query_hostapis()):
            if 'WASAPI' in h['name']:
                return i, h[f'default_{kind}_device']
    except Exception:
        pass
    return None


def _open(sd, cls, rate, device, kind, **kw):
    """A WASAPI stream (shared mode, Windows resampling to our rate) on `device` (a name, as the
    Linux test hooks name a node) or WASAPI's default; else PortAudio's default host API."""
    w = _wasapi(sd, kind)
    if w is not None:
        try:
            dev = device if device is not None else w[1]
            return cls(samplerate=rate, channels=1, dtype='int16', device=dev,
                       extra_settings=sd.WasapiSettings(auto_convert=True), **kw)
        except Exception as e:
            log(f'audio: WASAPI {kind} failed ({e}); the default host API')
    return cls(samplerate=rate, channels=1, dtype='int16', device=device, **kw)


class _Out:
    def fileno(self):
        return -1


class Mic:
    """The capture, shaped like the pw-record process it stands for: poll/terminate/kill/wait, and
    read(wait) for capri-voice's mic_chunk. 20 ms blocks. The stream stays on the device it opened:
    a new default never moves it, and a device removed ends it (b'': the mic is lost)."""

    def __init__(self, rate, device=None):
        import sounddevice as sd
        self.q = queue.Queue()
        self.stdout = _Out()
        self.closed = False
        self.stream = _open(sd, sd.RawInputStream, rate, device, 'input', blocksize=rate // 50,
                            callback=lambda data, n, t, st: self.q.put(bytes(data)), finished_callback=lambda: self.q.put(b''))
        self.stream.start()

    def read(self, wait):
        try:
            return self.q.get(timeout=wait)
        except queue.Empty:
            return None if self.poll() is None else b''

    def poll(self):
        return 0 if self.closed or not self.stream.active else None

    def terminate(self):
        if not self.closed:
            self.closed = True
            try:
                self.stream.abort()
                self.stream.close()
            except Exception:
                pass
            self.q.put(b'')

    kill = terminate

    def wait(self, timeout=None):
        self.terminate()
        return 0


class Player:
    """Her speech out, shaped like the pw-play process it stands for: stdin.write/close, wait(timeout)
    (subprocess.TimeoutExpired while it plays), kill. The stream stays on the device it opened."""

    def __init__(self, rate, device=None):
        import sounddevice as sd
        self.sd, self.buf, self.pos, self.eof = sd, bytearray(), 0, False
        self.lk, self.done = threading.Lock(), threading.Event()
        self.stdin = self
        self.stream = _open(sd, sd.RawOutputStream, rate, device, 'output', callback=self._fill, finished_callback=self.done.set)
        self.stream.start()

    def write(self, data):
        with self.lk:
            self.buf += data

    def close(self):
        with self.lk:
            self.eof = True

    def _fill(self, out, frames, t, status):
        n = len(out)
        with self.lk:
            chunk = bytes(self.buf[self.pos:self.pos + n])
            self.pos += len(chunk)
            end = self.eof and self.pos >= len(self.buf)
        out[:len(chunk)] = chunk
        if len(chunk) < n:
            out[len(chunk):] = b'\x00' * (n - len(chunk))
        if end:
            raise self.sd.CallbackStop

    def wait(self, timeout=None):
        """0 when all it was given played (its input closed), else 1 (killed, the device gone): as
        pw-play's exit, so a sentence cut short says so in its say-end."""
        if not self.done.wait(timeout):
            raise subprocess.TimeoutExpired('capri-voice speech', timeout)
        try:
            self.stream.close()
        except Exception:
            pass
        with self.lk:
            return 0 if self.eof and self.pos >= len(self.buf) else 1

    def kill(self):
        try:
            self.stream.abort()
        except Exception:
            pass
        self.done.set()


def _devices(kind):
    import sounddevice as sd
    ch = 'max_input_channels' if kind == 'sources' else 'max_output_channels'
    return [d['name'] for d in sd.query_devices() if d.get(ch, 0) > 0]


def node_missing(kind, name):
    """A named device (the CAPRI_VOICE_SOURCE/SINK test hooks) that isn't there: never the default
    instead."""
    if not name:
        return False
    try:
        return not any(name in d for d in _devices(kind))
    except Exception:
        return True


HEADPHONES = re.compile(r'headphone|headset|earphone|earbud|hands-?free|airpods', re.I)


def sink_kind(with_id=False):
    """'headphones' when the default output's name says so (Windows names endpoints "Headphones
    (...)", "Headset Earphone (...)"); else 'speakers'. with_id: (kind, (name, None))."""
    name = ''
    try:
        import sounddevice as sd
        w = _wasapi(sd, 'output')
        idx = w[1] if w and w[1] is not None and w[1] >= 0 else sd.default.device[1]
        name = sd.query_devices(idx)['name']
    except Exception:
        pass
    kind = 'headphones' if HEADPHONES.search(name) else 'speakers'
    return (kind, (name, None)) if with_id else kind


# ---- the session: lock, calls, do-not-disturb --------------------------------------------------

_lock_unknown = set()


def lock_unknown(why):
    """session_locked() can't tell: logged once per reason (it is asked every second), then None:
    the input desktop is the whole answer again."""
    if why not in _lock_unknown:
        _lock_unknown.add(why)
        log(f"lock: the session's lock state is unknown ({why}): only a secure desktop counts as locked")
    return None


def session_locked():
    """His session's own lock state (WTSSessionInfoEx's SessionFlags): True locked, False not, None
    unknown (logged once). Windows 11's lock screen (LockApp's curtain) runs on the "Default"
    desktop, so the input desktop alone missed it: the wake word's mic stayed open on the lock
    screen until she hid (14 s, seen in the VM, build 26300)."""
    import ctypes
    from ctypes import wintypes
    wts, k32 = ctypes.WinDLL('wtsapi32'), ctypes.WinDLL('kernel32')
    query = wts.WTSQuerySessionInformationW
    query.argtypes = [wintypes.HANDLE, wintypes.DWORD, ctypes.c_int, ctypes.POINTER(ctypes.c_void_p), ctypes.POINTER(wintypes.DWORD)]
    query.restype = wintypes.BOOL
    wts.WTSFreeMemory.argtypes = [ctypes.c_void_p]
    sid = wintypes.DWORD()
    if not k32.ProcessIdToSessionId(k32.GetCurrentProcessId(), ctypes.byref(sid)):
        return lock_unknown(f'ProcessIdToSessionId failed: {ctypes.GetLastError()}')
    buf, n = ctypes.c_void_p(), wintypes.DWORD()
    if not query(None, sid, 25, ctypes.byref(buf), ctypes.byref(n)):     # WTSSessionInfoEx
        return lock_unknown(f'WTSQuerySessionInformation failed: {ctypes.GetLastError()}')
    try:
        # WTSINFOEXW: DWORD Level (1), then WTSINFOEX_LEVEL1_W, 8-aligned (it holds LARGE_INTEGERs):
        # SessionId @8, SessionState @12, SessionFlags @16 (0 WTS_SESSIONSTATE_LOCK, 1 UNLOCK).
        if not buf.value or n.value < 20 or ctypes.c_uint32.from_address(buf.value).value != 1:
            return lock_unknown(f'an unexpected WTSINFOEX ({n.value} bytes)')
        flags = ctypes.c_int32.from_address(buf.value + 16).value
        return {0: True, 1: False}.get(flags) if flags in (0, 1) else lock_unknown(f'SessionFlags {flags}')
    finally:
        wts.WTSFreeMemory(buf)


def screen_locked():
    """His session is locked (`session_locked`), or the input desktop isn't his ("Default"): a UAC
    prompt (the secure desktop can't be opened from his session at all)."""
    if session_locked():
        return True
    import ctypes
    from ctypes import wintypes
    u = ctypes.WinDLL('user32', use_last_error=True)
    u.OpenInputDesktop.restype = wintypes.HANDLE
    h = u.OpenInputDesktop(0, False, 0x0001)      # DESKTOP_READOBJECTS
    if not h:
        return True
    try:
        buf = ctypes.create_unicode_buffer(256)
        need = wintypes.DWORD()
        if not u.GetUserObjectInformationW(wintypes.HANDLE(h), 2, buf, ctypes.sizeof(buf), ctypes.byref(need)):  # UOI_NAME
            return True
        return buf.value.lower() != 'default'
    finally:
        u.CloseDesktop(wintypes.HANDLE(h))


def locked_hint():
    return False              # screen_locked is the whole answer on Windows


CONSENT = r'Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone'


def consent_entries():
    """Every app the mic privacy store knows: (name, packaged, start, stop). NonPackaged names are
    exe paths with '#' for '\\'. None when the store can't be read."""
    import winreg
    out = []

    def walk(key, packaged):
        i = 0
        while True:
            try:
                name = winreg.EnumKey(key, i)
            except OSError:
                break
            i += 1
            if name == 'NonPackaged' and packaged:
                with winreg.OpenKey(key, name) as k:
                    walk(k, False)
                continue
            try:
                with winreg.OpenKey(key, name) as k:
                    start = winreg.QueryValueEx(k, 'LastUsedTimeStart')[0]
                    stop = winreg.QueryValueEx(k, 'LastUsedTimeStop')[0]
            except OSError:
                continue
            out.append((name, packaged, start, stop))
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, CONSENT) as root:
            walk(root, True)
    except OSError:
        return None
    return out


def capturing_app(entries, own_exe):
    """The app using the mic now (a call, a recorder), from the privacy store: its last use started
    and hasn't stopped (stop == 0). Ours (own_exe, this Python) doesn't count."""
    own = ntpath.normcase(own_exe) if own_exe else None      # (Windows paths, on any OS: the tests)
    for name, packaged, start, stop in entries:
        if not start or stop:
            continue
        if packaged:
            return name.split('_')[0]
        path = name.replace('#', '\\')
        if own and ntpath.normcase(path) == own:
            continue
        return ntpath.splitext(ntpath.basename(path))[0] or 'an app'
    return None


def in_call():
    """The app capturing the mic (a call), '' if none, None if it can't be told."""
    try:
        e = consent_entries()
    except Exception:
        e = None
    if e is None:
        return None
    return capturing_app(e, sys.executable) or ''


class CallWatch:
    """While a hands-off stream is open, a call that starts ends it: check() every 0.5 s (the
    privacy store is a registry read). terminate() stops it, as pactl subscribe's process."""

    def __init__(self, check, every=0.5):
        self.stop = threading.Event()

        def run():
            while not self.stop.wait(every):
                if check():
                    return
        threading.Thread(target=run, daemon=True).start()

    def terminate(self):
        self.stop.set()


QUNS = {1: 'not present', 2: 'busy', 3: 'D3D full screen', 4: 'presentation', 5: 'accepts', 6: 'quiet time', 7: 'app'}


def dnd_on():
    """His do-not-disturb, as far as Windows tells an app: a full-screen app, presentation mode
    (SHQueryUserNotificationState). Whether Windows 11's own Do not disturb shows here is [VM]."""
    import ctypes
    s = ctypes.c_int(0)
    if ctypes.windll.shell32.SHQueryUserNotificationState(ctypes.byref(s)) != 0:
        return False
    return s.value in (2, 3, 4)


# ---- other audio: pause (SMTC) or duck (session volume) ----------------------------------------

DUCK = 0.125      # a session's volume is a linear gain: x0.125 is -18 dB, Linux's duck [VM: by ear]
PLAYING, PAUSED = 4, 5   # GlobalSystemMediaTransportControlsSessionPlaybackStatus
NEVER_DUCK_BINS = {'audiodg.exe'}


def close_to(a, b):
    return abs(a - b) <= 0.002


class Sessions:
    """The WASAPI audio sessions on the default output (pycaw): what Media ducks. Each one:
    {key, app, bin, pid, vol, active, system}. key: the session instance id (unique per session)."""

    def _all(self):
        from pycaw.pycaw import AudioUtilities
        return AudioUtilities.GetAllSessions()

    def list(self):
        out = []
        for s in self._all():
            try:
                ctl2 = s._ctl
                key = ctl2.GetSessionInstanceIdentifier()
                p = s.Process
                b = p.name() if p else ''
                # The system sounds session is the one with no process (pid 0).
                out.append({'key': key, 'app': s.DisplayName or (os.path.splitext(b)[0] if b else ''), 'bin': b,
                            'pid': s.ProcessId, 'vol': float(s.SimpleAudioVolume.GetMasterVolume()),
                            'active': s.State == 1, 'system': not s.ProcessId})
            except Exception:
                continue
        return out

    def set(self, key, vol):
        for s in self._all():
            try:
                if s._ctl.GetSessionInstanceIdentifier() == key:
                    s.SimpleAudioVolume.SetMasterVolume(float(vol), None)
                    return True
            except Exception:
                continue
        return False


class Smtc:
    """Media players through the System Media Transport Controls (winrt): (aumid, status) and
    pause/play by AUMID."""

    def _run(self, fn):
        import asyncio
        from winrt.windows.media.control import GlobalSystemMediaTransportControlsSessionManager as M

        async def go():
            mgr = await M.request_async()
            return await fn(list(mgr.get_sessions()))
        return asyncio.run(go())

    def players(self):
        async def f(ss):
            return [(s.source_app_user_model_id, int(s.get_playback_info().playback_status)) for s in ss]
        return self._run(f)

    def _act(self, aumid, play):
        async def f(ss):
            for s in ss:
                if s.source_app_user_model_id == aumid:
                    return await (s.try_play_async() if play else s.try_pause_async())
            return False
        return self._run(f)

    def pause(self, aumid):
        return self._act(aumid, False)

    def play(self, aumid):
        return self._act(aumid, True)


class Media:
    """Other audio while she listens or speaks, Linux's rules on Windows' parts.
    pause: every SMTC player that plays is paused; on release, only the ones she paused that are
    still paused play again. duck: every eligible session's volume x DUCK, put back exactly.
    Saved to STATE (atomically, on every change) so a crash mid-hold is repaired at the next start.

    Duck rules: never her own process, the system sounds session, the [music] ignore list (calls),
    or anything at all while another app captures the mic (a call). Inactive sessions are skipped,
    and ducked if they start mid-hold; new sessions mid-hold are ducked too (a poll every 0.5 s:
    sessions have no change feed that needs no COM callbacks). Restore only a session with the same
    instance id whose volume is still what we set (a change he made wins). Windows keeps an app's
    session volume: one that dies ducked would come back ducked, so its originals are kept
    (VANISHED_S) and a session of the same app that shows up at exactly our ducked level gets them
    back."""

    def __init__(self, sessions=None, smtc=None, state=None, call=None, every=0.5):
        self.sessions, self.smtc = sessions or Sessions(), smtc or Smtc()
        self.state = state or STATE
        self.call = call or in_call
        self.every = every
        self.held = None
        self.vanished = []       # {'app', 'bin', 'orig', 'set', 't'}
        self.ignore = []
        self.lk = threading.RLock()
        self.watching = False
        self.restore_saved()

    def _save(self):
        if self.held or self.vanished:
            write_json(self.state, {**(self.held or {}), 'vanished': self.vanished})
        elif os.path.exists(self.state):
            os.remove(self.state)

    def restore_saved(self):
        try:
            with open(self.state) as f:
                st = json.load(f)
        except FileNotFoundError:
            return
        except Exception as e:
            log('media: bad state file', e)
            return
        self.vanished = [v for v in st.pop('vanished', []) if time.time() - v.get('t', 0) < VANISHED_S]
        self.held = st if st.get('mode') else None
        log('media: repairing a hold left by a crash:', json.dumps(self.held), f'({len(self.vanished)} vanished)')
        self.release()
        self._watch()

    def eligible(self, s):
        app, b = s.get('app') or '', s.get('bin') or ''
        if not (app or b) or s.get('system') or s.get('pid') == os.getpid() or 'capri-voice' in app:
            return False
        if b.lower() in NEVER_DUCK_BINS:
            return False
        low = f'{app} {b}'.lower()
        if any(i and i in low for i in self.ignore):
            return False
        return not DUCK_MATCH or DUCK_MATCH in app or DUCK_MATCH in b

    def _duck(self, s, orig=None):
        orig = s['vol'] if orig is None else orig
        want = round(orig * DUCK, 4)
        if not close_to(s['vol'], want):
            self.sessions.set(s['key'], want)
        self.held['streams'][s['key']] = {'app': s.get('app') or '', 'bin': s.get('bin') or '', 'orig': orig, 'set': want}
        self._save()

    def hold(self, mode):
        with self.lk:
            if self.held or mode == 'off':
                return
            if mode == 'pause':
                paused = []
                try:
                    for aumid, st in self.smtc.players():
                        if st == PLAYING and (not PLAYERS or any(p in aumid for p in PLAYERS)):
                            self.smtc.pause(aumid)
                            paused.append(aumid)
                except Exception as e:
                    log('media: SMTC:', e)
                self.held = {'mode': 'pause', 'players': paused}
            elif mode == 'duck':
                app = self.call()
                app = 'unknown' if app is None else app     # can't tell: touch nothing
                self.held = {'mode': 'duck', 'streams': {}, 'call': app or None}
                if not app:
                    for s in self.sessions.list():
                        if self.eligible(s) and s.get('active'):
                            self._duck(s)
            self._save()
            log('media: hold', json.dumps(self.held))
        self._watch()

    def release(self):
        with self.lk:
            h, self.held = self.held, None
            if not h:
                return
            if h.get('mode') == 'pause':
                try:
                    now = dict(self.smtc.players())
                    for aumid in h.get('players', []):
                        if now.get(aumid) == PAUSED:
                            self.smtc.play(aumid)
                except Exception as e:
                    log('media: SMTC:', e)
            elif h.get('mode') == 'duck':
                now = {s['key']: s for s in self.sessions.list()}
                for key, e in h.get('streams', {}).items():
                    s = now.get(key)
                    if s is None:
                        self.vanished.append({**{k: e[k] for k in ('app', 'bin', 'orig', 'set')}, 't': time.time()})
                    elif close_to(s['vol'], e['set']):
                        self.sessions.set(key, e['orig'])
                    else:
                        log(f"media: session {e['app'] or e['bin']} was changed while ducked: left as he set it")
            self._save()
            log('media: released', json.dumps(h))

    # -- the watcher: sessions that start or die while ducked; ones that come back after
    def _watch(self):
        with self.lk:
            want = (self.held or {}).get('mode') == 'duck' or bool(self.vanished)
            if want and not self.watching:
                self.watching = True
                threading.Thread(target=self._watch_loop, daemon=True).start()

    def _watch_loop(self):
        seen = set()
        while True:
            time.sleep(self.every)
            try:
                seen = self.poll(seen)
            except Exception as e:
                log('media: watch:', e)
            with self.lk:
                self.vanished = [v for v in self.vanished if time.time() - v['t'] < VANISHED_S]
                if (self.held or {}).get('mode') != 'duck' and not self.vanished:
                    self.watching = False
                    return

    def poll(self, seen):
        """One look at the sessions (the watcher's step; `seen`: the keys active at the last look).
        Returns the keys active now."""
        now = self.sessions.list()
        with self.lk:
            held = self.held if (self.held or {}).get('mode') == 'duck' and not self.held.get('call') else None
            keys = {s['key'] for s in now}
            if held:
                for key in [k for k in held['streams'] if k not in keys]:
                    e = held['streams'].pop(key)
                    self.vanished.append({**{k: e[k] for k in ('app', 'bin', 'orig', 'set')}, 't': time.time()})
                    self._save()
            for s in now:
                if not s.get('active') or s['key'] in seen:
                    continue
                if held and s['key'] in held['streams']:
                    continue
                if not self.eligible(s):
                    continue
                same = lambda v: v['app'] == (s.get('app') or '') and v['bin'] == (s.get('bin') or '') and close_to(s['vol'], v['set'])
                back = next((v for v in self.vanished if same(v)), None)
                sib = next((v for v in (held or {}).get('streams', {}).values() if same(v)), None) if not back else None
                if back:
                    self.vanished.remove(back)
                back = back or sib
                if held:
                    self._duck(s, back['orig'] if back else None)
                elif back:
                    self.sessions.set(s['key'], back['orig'])
                    log(f"media: session {back['app'] or back['bin']} came back ducked: put back")
                self._save()
            return {s['key'] for s in now if s.get('active')}


if __name__ == '__main__':
    # --test-serve ENDPOINT: the link alone, for the Rust end's test (src/platform/windows/voice.rs):
    # each avatar that passes the handshake gets {"ev":"ready"}.
    if sys.argv[1:2] == ['--test-serve']:
        serve(lambda c: c.sendall(b'{"ev":"ready"}\n'), path=sys.argv[2])
