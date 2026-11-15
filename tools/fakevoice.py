#!/usr/bin/env python3
"""fakevoice.py SOCK: a FAKE capri-voice sidecar for tests. No audio, no mic, no models: it speaks
the protocol and prints every op it gets, one JSON line each, so a test reads what she would
have said and when.

  config            -> ready (features: unprompted)
  say id text       -> say-start now, say-end after FAKE_WORD_MS (default 60) per word
  drop id           -> that turn's queued sentences dropped (its playing one cut)
  hush              -> hushed (queued sentences dropped)
  listen-start      -> mic-open      listen-stop -> mic-closed, heard (FAKE_HEARD, default below)
  listen-cancel     -> mic-closed    media-hold / media-release / stats -> printed only
Run the avatar with CAPRI_VOICE_SOCK=SOCK."""
import json, os, socket, sys, threading, time

SOCK = sys.argv[1] if len(sys.argv) > 1 else "/tmp/sa-fake/v.sock"
WORD = float(os.environ.get("FAKE_WORD_MS", "60")) / 1000.0
HEARD = os.environ.get("FAKE_HEARD", "what's spike doing right now")
lock = threading.Condition()
queue = []          # (gen, id, text)
dropped = set()
gen = [0]
conn = [None]


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
            g, sid, text = queue.pop(0)
            if g != gen[0] or sid in dropped:
                continue
        dur = max(0.2, WORD * len(text.split()))
        emit(ev="say-start", id=sid, text=text, dur_ms=round(dur * 1000), step_ms=90, shapes=[1, 2, 3, 2, 1, 0])
        log(ev="say-start", id=sid, text=text)
        time.sleep(dur)
        with lock:
            if g != gen[0] or sid in dropped:
                continue
        emit(ev="say-end", id=sid)


def handle(m):
    op = m.get("op")
    log(op=op, **{k: v for k, v in m.items() if k != "op"})
    if op == "config":
        emit(ev="ready", cfg=m, features=["unprompted"])
    elif op == "say":
        with lock:
            queue.append((gen[0], m.get("id", 0), m.get("text", "")))
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
        emit(ev="mic-open")
    elif op == "listen-stop":
        emit(ev="mic-closed")
        emit(ev="heard", text=HEARD, why="", ms={"stt": 1})
    elif op == "listen-cancel":
        emit(ev="mic-closed")


def main():
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
