#!/usr/bin/env python3
"""fakedaemon.py [FIXTURES_DIR]: a FAKE Synaps daemon on /tmp/sa-fake/d.sock for what the sandbox
can't raise safely (a Secret prompt) or cheaply (compaction). Modelled on synaps-client's
tests/fake_daemon.rs: Hello -> Welcome (one live session named `capri`), Sessions -> list,
Attach -> the attached_history.json fixture, then events on demand.

Control: one line per connection on /tmp/sa-fake/ctl.sock:
  secret | confirm | compact | compacted | idle | quit
Commands the avatar sends are printed; answer values are never printed (only their length)."""
import json, os, socket, sys, threading, time

DIR = "/tmp/sa-fake"
SID = "20261113-073427-5fde"
fixtures = sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "tests/fixtures")
os.makedirs(DIR, mode=0o700, exist_ok=True)
ATTACHED = json.loads(open(os.path.join(fixtures, "attached_history.json")).read())
WELCOME = {"type": "welcome", "protocol_version": 3, "daemon_version": "0.0.0-fake", "pid": os.getpid(), "profile": None,
           "sessions": [{"id": SID, "model": "fake", "lifecycle": "live", "name": "capri"}],
           "progressive_tool_disclosure": False, "generation": 1, "features": []}
attached = []          # live session connections (socket, lock)
pending = {}           # prompt id -> request (in every Attached snapshot until answered)
seq = [100]
client_id = [10]
lock = threading.Lock()


def log(*a):
    print(time.strftime("%H:%M:%S"), *a, flush=True)


def send(conn, wl, v):
    with wl:
        conn.sendall((json.dumps(v) + "\n").encode())


def event(ev):
    with lock:
        seq[0] += 1
        n = seq[0]
        targets = list(attached)
    for conn, wl in targets:
        try:
            send(conn, wl, {"type": "event", "session_id": SID, "seq": n, "ts": "t", "event": ev})
        except OSError:
            pass


def serve(conn):
    wl = threading.Lock()
    f = conn.makefile("r")
    try:
        hello = f.readline()
        if not hello:
            return
        send(conn, wl, WELCOME)
        for line in f:
            fr = json.loads(line)
            t = fr.get("type")
            if t == "sessions":
                send(conn, wl, {"type": "session_list", "sessions": WELCOME["sessions"]})
            elif t == "attach":
                a = fr if isinstance(fr.get("attach"), str) else fr.get("attach", {})
                mode = a.get("mode")
                with lock:
                    client_id[0] += 1
                    me = client_id[0]
                log(f"attach {a.get('attach')} {a.get('session_id', '')} mode={mode} -> client {me}")
                if os.environ.get("FAKE_DEBUG"):
                    log("frame keys", sorted(fr.keys()))
                v = dict(ATTACHED)
                v["client"] = me
                v["meta"] = dict(v["meta"], name="capri")
                with lock:
                    v["pending_prompts"] = list(pending.values())
                send(conn, wl, v)
                with lock:
                    attached.append((conn, wl))
            elif t == "cmd":
                c = dict(fr.get("cmd", {}))
                if c.get("cmd") == "answer":
                    val = c.get("value")
                    log(f"cmd answer prompt_id={c.get('prompt_id')} value={'None' if val is None else f'<{len(val)} chars, not shown>'}")
                    with lock:
                        pending.pop(c.get("prompt_id"), None)
                    event({"ev": "prompt_resolved", "prompt_id": c.get("prompt_id")})
                elif c.get("cmd") in ("submit", "steer"):
                    log(f"cmd {c.get('cmd')} <{len(c.get('text', ''))} chars>")
                else:
                    log(f"cmd {c.get('cmd')}")
                    if c.get("cmd") == "detach":
                        send(conn, wl, {"type": "bye", "bye": "detached"})
                        return
            elif t == "bye":
                return
            elif t == "ping":
                send(conn, wl, {"type": "pong"})
    except (OSError, ValueError):
        pass
    finally:
        with lock:
            attached[:] = [(c, w) for c, w in attached if c is not conn]
        conn.close()


def control():
    path = f"{DIR}/ctl.sock"
    if os.path.exists(path):
        os.unlink(path)
    s = socket.socket(socket.AF_UNIX)
    s.bind(path)
    os.chmod(path, 0o600)
    s.listen()
    while True:
        c, _ = s.accept()
        cmd = c.makefile("r").readline().strip()
        log(f"ctl {cmd}")
        req = None
        if cmd == "secret":
            req = {"id": 41, "kind": "secret", "title": "password required",
                   "prompt": "[FAKE DAEMON] test password for the sandbox (never a real one):"}
        elif cmd == "confirm":
            req = {"id": 42, "kind": "confirm", "title": "Confirm tool call",
                   "prompt": "[FAKE DAEMON] bash: rm -rf /tmp/sa-fake/nothing"}
        if req:
            with lock:
                pending[req["id"]] = req
            event({"ev": "prompt", "request": req})
        elif cmd == "compact":
            event({"ev": "compaction_started"})
        elif cmd == "compacted":
            event({"ev": "compaction_applied", "previous_session_id": SID, "session_id": SID, "msg_count": 4})
        elif cmd == "idle":
            event({"ev": "idle"})
        elif cmd == "quit":
            c.sendall(b"ok\n")
            os._exit(0)
        c.sendall(b"ok\n")
        c.close()


def main():
    path = f"{DIR}/d.sock"
    if os.path.exists(path):
        os.unlink(path)
    s = socket.socket(socket.AF_UNIX)
    s.bind(path)
    os.chmod(path, 0o600)
    s.listen()
    threading.Thread(target=control, daemon=True).start()
    log(f"fake daemon on {path} (FAKE: no model, no real prompts)")
    while True:
        c, _ = s.accept()
        threading.Thread(target=serve, args=(c,), daemon=True).start()


if __name__ == "__main__":
    main()
