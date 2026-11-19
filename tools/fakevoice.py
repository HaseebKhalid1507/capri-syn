#!/usr/bin/env python3
"""fakevoice.py SOCK: a FAKE capri-voice sidecar for tests. No audio, no mic, no models: it speaks
the protocol and prints every op it gets, one JSON line each, so a test reads what she would
have said and when.

  config            -> ready (features: unprompted)
  say id text       -> say-start now, say-end after FAKE_WORD_MS (default 60) per word
                       (FAKE_CAP_S: an unprompted turn as capri-voice's gate: a first sentence over it plays
                       that long and ends {"cut": true}; a later one past it is say-skipped "reply cap")
  drop id           -> that turn's queued sentences dropped (its playing one cut)
  hush              -> hushed (queued sentences dropped)
  listen-start      -> mic-open      listen-stop -> mic-closed, heard (FAKE_HEARD, default below)
  listen-cancel     -> mic-closed    media-hold / media-release -> printed only
  listen-start mode toggle -> mic-open {mode}, then as FAKE_TOGGLE (a JSON file, read at each
                       start) says: {"speech_ms": 300, "end_ms": 1500, "why": "silence|cap|nothing"}
                       -> speech, endpoint, mic-closed, heard; no file or {} -> open until stop/cancel
  listen-start mode handsfree -> mic-open {mode}, then the steps of FAKE_HF (a JSON list, read at each
                       start): {"at": ms, "heard": "text"} (speech, endpoint, heard), {"at": ms, "ev": "...", ...}
                       (any event), {"at": ms, "die": true} (the stream dies: mic-closed)
  listen-start mode wake -> (FAKE_WAKE_REFUSE, a file: if it holds a reason, refused: mic-closed, heard "" why)
                       mic-open {mode}, then the steps of FAKE_WAKE (a JSON list, read at each start):
                       {"at": ms, "wake": "text"} (wake, speech, endpoint, heard), {"at": ms, "wake": "", "handsfree": true}
                       (wake, wake-handsfree), {"at": ms, "end": "call|lock|false starts|mic lost"} (mic-closed, wake-end).
                       With "handsfree": true on the listen-start (wake_handsfree): a "wake" step is {"ev":"wake","handsfree":true}
                       and the stream goes on as hands-free: its text (if any) the first utterance ("let's talk": heard ""
                       PHRASE ONLY), then the steps of FAKE_HF, timed from the wake.
                       Any other listen-start closes an open wake stream first (mic-closed {mode: wake}), as the real one.
  listen-drop       -> printed only
  stats             -> stats {mic_open}
Run the avatar with CAPRI_VOICE_SOCK=SOCK. Windows (no AF_UNIX in stock CPython): SOCK is the
endpoint.json she reads (CAPRI_VOICE_ENDPOINT=SOCK for her), served as the real sidecar's link is:
loopback TCP and the token handshake (tools/capri_voice_win.py `serve`)."""
import json, os, socket, sys, threading, time

SOCK = sys.argv[1] if len(sys.argv) > 1 else "/tmp/sa-fake/v.sock"
WORD = float(os.environ.get("FAKE_WORD_MS", "60")) / 1000.0
CAP = float(os.environ.get("FAKE_CAP_S", "0"))   # 0: no cap (an older sidecar)
spent = {}          # unprompted turn id -> seconds of it played
HEARD = os.environ.get("FAKE_HEARD", "what's spike doing right now")
lock = threading.Condition()
queue = []          # (gen, id, text, unprompted)
dropped = set()
gen = [0]
conn = [None]
mic = {"open": False, "n": 0}
TOGGLE = os.environ.get("FAKE_TOGGLE", "")
HF = os.environ.get("FAKE_HF", "")
WAKE = os.environ.get("FAKE_WAKE", "")
WAKE_REFUSE = os.environ.get("FAKE_WAKE_REFUSE", "")


def readf(path, default):
    try:
        return json.load(open(path)) if path else default
    except (OSError, ValueError):
        return default


def log(**v):
    print(json.dumps({"t": round(time.time(), 3), **v}), flush=True)


def emit(**ev):
    c = conn[0]
    if c is not None:
        try:
            c.sendall((json.dumps(ev) + "\n").encode())
        except OSError:
            pass


def player():
    while True:
        with lock:
            while not queue:
                lock.wait()
            g, sid, text, unprompted = queue.pop(0)
            if g != gen[0] or sid in dropped:
                continue
        dur = max(0.2, WORD * len(text.split()))
        trimmed = False
        if CAP and unprompted:
            if sid in spent and spent[sid] + dur > CAP:
                emit(ev="say-skipped", id=sid, why="reply cap")
                log(ev="say-skipped", id=sid, why="reply cap")
                continue
            if sid not in spent and dur > CAP:
                dur, trimmed = CAP, True
            spent[sid] = spent.get(sid, 0.0) + dur
        emit(ev="say-start", id=sid, text=text, dur_ms=round(dur * 1000), step_ms=90, shapes=[1, 2, 3, 2, 1, 0])
        log(ev="say-start", id=sid, text=text)
        time.sleep(dur)
        with lock:
            if g != gen[0] or sid in dropped:
                continue
        if trimmed:
            emit(ev="say-end", id=sid, cut=True)
            log(ev="say-end", id=sid, cut=True, why="trimmed to the cap")
        else:
            emit(ev="say-end", id=sid)


def handle(m):
    op = m.get("op")
    log(op=op, **{k: v for k, v in m.items() if k != "op"})
    if op == "config":
        emit(ev="ready", cfg=m, features=["unprompted"])
    elif op == "say":
        with lock:
            queue.append((gen[0], m.get("id", 0), m.get("text", ""), bool(m.get("unprompted"))))
            lock.notify_all()
    elif op == "drop":
        with lock:
            dropped.add(m.get("id"))
            queue[:] = [q for q in queue if q[1] != m.get("id")]
    elif op == "hush":
        with lock:
            gen[0] += 1
            queue.clear()
        emit(ev="hushed")
    elif op == "listen-start":
        if mic["open"] and mic.get("mode") == "wake" and m.get("mode", "hold") != "wake":
            mic["open"] = False
            emit(ev="mic-closed", mode="wake")
        if m.get("mode") == "wake" and (why := readf(WAKE_REFUSE, "")):
            emit(ev="mic-closed", mode="wake")
            emit(ev="heard", text="", why=why, ms={})
            log(ev="refused", why=why)
            return
        mic["open"], mic["n"], mic["mode"] = True, mic["n"] + 1, m.get("mode", "hold")
        if m.get("mode") == "wake":
            emit(ev="mic-open", mode="wake", echo="headphones")
            threading.Thread(target=wake_run, args=(mic["n"], readf(WAKE, []), bool(m.get("handsfree"))), daemon=True).start()
            return
        emit(ev="mic-open", mode=m.get("mode", "hold"))
        if m.get("mode") == "handsfree":
            try:
                steps = json.load(open(HF)) if HF else []
            except (OSError, ValueError):
                steps = []
            threading.Thread(target=hf_run, args=(mic["n"], steps), daemon=True).start()
        if m.get("mode") == "toggle":
            try:
                plan = json.load(open(TOGGLE)) if TOGGLE else {}
            except (OSError, ValueError):
                plan = {}
            if plan:
                threading.Thread(target=toggle_run, args=(mic["n"], plan), daemon=True).start()
    elif op == "listen-stop":
        if mic["open"]:
            mic["open"] = False
            emit(ev="mic-closed", mode=mic.get("mode", "hold"))
            emit(ev="heard", text=HEARD, why="", ms={"stt": 1})
    elif op == "listen-cancel":
        if mic["open"]:
            mic["open"] = False
            emit(ev="mic-closed", mode=mic.get("mode", "hold"))
    elif op == "stats":
        emit(ev="stats", mic_open=mic["open"])


def hf_run(n, steps):
    """A hands-free session as scripted: utterances heard, events, the stream dying."""
    t0 = time.time()
    for st in steps:
        time.sleep(max(0, st.get("at", 0) / 1000 - (time.time() - t0)))
        if mic["n"] != n or not mic["open"]:
            return
        if "heard" in st:
            emit(ev="speech")
            time.sleep(0.3)
            emit(ev="endpoint", why="silence", speech_ms=900, silence_ms=1000)
            emit(ev="heard", text=st["heard"], why=st.get("why", ""), ms={"stt": 1})
            log(ev="heard", text=st["heard"])
        elif st.get("die"):
            mic["open"] = False
            emit(ev="mic-closed", mode="handsfree")
            log(ev="died")
            return
        else:
            ev = {k: v for k, v in st.items() if k != "at"}
            if ev.get("ev") == "handsfree-end":      # as the real one: the stream closes, then why
                emit(ev="mic-closed", mode="handsfree")
            emit(**ev)
            log(**ev)
            if ev.get("ev") == "handsfree-end":
                mic["open"] = False
                return


def wake_run(n, steps, hf=False):
    """The wake stream as scripted: "hey capri" turns, hands-free asked for, the stream ending."""
    t0 = time.time()
    for st in steps:
        time.sleep(max(0, st.get("at", 0) / 1000 - (time.time() - t0)))
        if mic["n"] != n or not mic["open"]:
            return
        if "wake" in st and hf:
            # wake_handsfree: the same stream goes on as a hands-free one
            emit(ev="wake", handsfree=True, ms={"fire_to_wake": 300, "verify": 280})
            mic["mode"] = "handsfree"
            log(ev="wake", handsfree=True)
            if st["wake"] or st.get("handsfree"):
                emit(ev="speech")
                time.sleep(st.get("speak_ms", 600) / 1000)
                emit(ev="endpoint", why="silence", speech_ms=900, silence_ms=1000)
                if st.get("handsfree"):
                    emit(ev="heard", text="", why="PHRASE ONLY", ms={"stt": 1})
                else:
                    emit(ev="heard", text=st["wake"], why="", ms={"stt": 1})
                log(ev="heard", text=st["wake"], first=True)
            hf_run(n, readf(HF, []))
            return
        if "wake" in st:
            emit(ev="wake", ms={"fire_to_wake": 300, "verify": 280})
            log(ev="wake")
            if st.get("handsfree"):
                time.sleep(0.4)
                emit(ev="wake-handsfree", ms={})
                log(ev="wake-handsfree")
                continue
            time.sleep(0.3)
            emit(ev="speech")
            time.sleep(st.get("speak_ms", 600) / 1000)
            emit(ev="endpoint", why="silence" if st["wake"] else "nothing", speech_ms=900, silence_ms=1000)
            emit(ev="heard", text=st["wake"], why="" if st["wake"] else "NOTHING HEARD · MIC CLOSED", ms={"stt": 1})
            log(ev="heard", text=st["wake"])
        elif "end" in st:
            mic["open"] = False
            emit(ev="mic-closed", mode="wake")
            emit(ev="wake-end", why=st["end"], app="")
            log(ev="wake-end", why=st["end"])
            return


def toggle_run(n, plan):
    """A toggled turn as the real sidecar plays it: speech, then the endpoint, unless stopped."""
    t0 = time.time()
    if plan.get("speech_ms") is not None:
        time.sleep(plan["speech_ms"] / 1000)
        if mic["n"] != n or not mic["open"]:
            return
        emit(ev="speech")
        log(ev="speech")
    time.sleep(max(0, plan.get("end_ms", 1500) / 1000 - (time.time() - t0)))
    if mic["n"] != n or not mic["open"]:
        return
    why = plan.get("why", "silence")
    mic["open"] = False
    emit(ev="endpoint", why=why, speech_ms=800, silence_ms=1000)
    emit(ev="mic-closed")
    log(ev="endpoint", why=why)
    if why == "nothing":
        emit(ev="heard", text="", why="NOTHING HEARD · MIC CLOSED", ms={})
    else:
        emit(ev="heard", text=HEARD, why="", ms={"stt": 1})


def link(c):
    """One avatar's connection, until it goes."""
    conn[0] = c
    log(ev="connected")
    buf = b""
    while True:
        try:
            d = c.recv(4096)
        except OSError:
            d = b""
        if not d:
            break
        buf += d
        while b"\n" in buf:
            line, buf = buf.split(b"\n", 1)
            try:
                handle(json.loads(line))
            except ValueError:
                pass
    if conn[0] is c:
        conn[0] = None
    log(ev="gone")


def main():
    if os.name == "nt":
        sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
        import capri_voice_win
        threading.Thread(target=player, daemon=True).start()
        log(fake=f"capri-voice at {SOCK} (FAKE: no audio)")
        capri_voice_win.serve(link, path=SOCK)
        return
    if os.path.exists(SOCK):
        os.unlink(SOCK)
    s = socket.socket(socket.AF_UNIX)
    s.bind(SOCK)
    os.chmod(SOCK, 0o600)
    s.listen(1)
    threading.Thread(target=player, daemon=True).start()
    log(fake=f"capri-voice on {SOCK} (FAKE: no audio)")
    while True:
        c, _ = s.accept()
        conn[0] = c
        log(ev="connected")
        buf = b""
        while True:
            try:
                d = c.recv(4096)
            except OSError:
                d = b""
            if not d:
                break
            buf += d
            while b"\n" in buf:
                line, buf = buf.split(b"\n", 1)
                try:
                    handle(json.loads(line))
                except ValueError:
                    pass
        conn[0] = None
        log(ev="gone")


if __name__ == "__main__":
    main()
