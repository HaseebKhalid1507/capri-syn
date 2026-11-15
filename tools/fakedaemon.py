#!/usr/bin/env python3
"""fakedaemon.py [FIXTURES_DIR]: a FAKE Synaps daemon on /tmp/sa-fake/d.sock for what the sandbox
can't raise safely (a Secret prompt) or cheaply (compaction). Modelled on synaps-client's
tests/fake_daemon.rs: Hello -> Welcome (one live session named `capri`), Sessions -> list,
Attach -> the attached_history.json fixture, then events on demand.

Control: one line per connection on /tmp/sa-fake/ctl.sock:
  secret | confirm | confirm2 | benign | compact | compacted | idle | refuse | relay | say TEXT | trig T TEXT
  | resolve | quit
A submit or steer streams a canned reply (FAKE: no model): a turn, text deltas every
FAKE_DELTA_MS (default 30), done, idle. `refuse`: the next submit is refused instead.
`relay`: a turn another session woke ([comms] ...). `say TEXT`: a turn with that reply. Both come as
trigger `event_auto`, as the real daemon's event turns do; a submit's turn is `user`. `trig T TEXT`: a
turn with trigger T. `resolve`: the pending prompts are answered elsewhere (they go).
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
refuse_next = [False]
replies = [
    "Build's green, choom. Spike pushed the reader fix eleven minutes ago.",
    "Here's the build command:\n\n```bash\ntools/abuild build --release\n```\n\nDocs: [gpui-component](https://github.com/longbridge/gpui-component)",
    "Rebasing polish-e onto main. Two conflicts in view.rs; he's on it.",
    "Checking. CI on main is green as of 21:05; I'll ping you when polish-e lands.",
]
n_reply = [0]
DELTA = float(os.environ.get("FAKE_DELTA_MS", "30")) / 1000.0


def remember(msgs):
    """The turn goes into the history a later attach gets (as a real daemon keeps it)."""
    with lock:
        c = ATTACHED["conversation"]
        c["api_messages"] = list(c["api_messages"]) + msgs
        c["messages_len"] = len(c["api_messages"])


def stream_turn(user_text, reply, trigger="user", tool=None):
    event({"ev": "turn_started", "turn_baseline": 1, "trigger": trigger, "user_text": user_text})
    event({"ev": "stream", "event": {"kind": "llm", "llm": "response_start"}})
    if tool:
        event({"ev": "stream", "event": {"kind": "llm", "llm": "tool_use_start", "tool_name": tool[0], "tool_id": "toolu_fake1"}})
        event({"ev": "stream", "event": {"kind": "llm", "llm": "tool_use", "tool_name": tool[0], "tool_id": "toolu_fake1", "input": tool[1]}})
        time.sleep(0.3)
        event({"ev": "stream", "event": {"kind": "llm", "llm": "tool_result", "tool_id": "toolu_fake1", "result": "ok"}})
        event({"ev": "stream", "event": {"kind": "llm", "llm": "response_start"}})
    words = reply.split(" ")
    for i, w in enumerate(words):
        event({"ev": "stream", "event": {"kind": "llm", "llm": "text", "text": (w if i == 0 else " " + w)}})
        time.sleep(DELTA)
    msgs = [{"role": "user", "content": user_text}] if user_text else []
    if tool:
        msgs += [{"role": "assistant", "content": [{"id": "toolu_fake1", "input": tool[1], "name": tool[0], "type": "tool_use"}]},
                 {"role": "user", "content": [{"content": "ok", "tool_use_id": "toolu_fake1", "type": "tool_result"}]}]
    msgs.append({"role": "assistant", "content": [{"text": reply, "type": "text"}]})
    remember(msgs)
    event({"ev": "stream", "event": {"kind": "session", "session": "done"}})
    event({"ev": "idle"})


VOICE_REPLY = ("On it, choom. Spike pushed the reader fix eleven minutes ago, and CI went green right after. "
               "Want me to nudge lynx about the rebase?\n---\nRun 9f8e7d6 · https://github.com/HaseebKhalid1507/capri-syn/actions")


def reply_to(text):
    if text.startswith("[voice"):
        # A spoken turn (synthetic): a short, speakable reply, the details after the rule.
        threading.Thread(target=stream_turn, args=(text, VOICE_REPLY), daemon=True).start()
        return
    r = replies[n_reply[0] % len(replies)]
    n_reply[0] += 1
    tool = ("bash", {"command": "gh run list --branch main --limit 1"}) if "CI" in text else None
    threading.Thread(target=stream_turn, args=(text, r), kwargs={"tool": tool}, daemon=True).start()


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
                    if refuse_next[0]:
                        refuse_next[0] = False
                        send(conn, wl, {"type": "error", "session_id": SID, "message": "refused: not the input owner (FAKE)"})
                    else:
                        reply_to(c.get("text", ""))
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
        elif cmd == "confirm2":
            req = {"id": 43, "kind": "confirm", "title": "Confirm tool activation",
                   "prompt": "[FAKE DAEMON] The model requests session-scoped activation of 3 exact tool id(s):\n"
                             "  builtin:shell_end\n  builtin:shell_send\n  builtin:shell_start\n"
                             "They can run any command in a shell you started, read its output and type into it "
                             "until the session ends. Nothing outside this session is affected.\n"
                             "Allow exactly these tools?"}
        elif cmd == "benign":
            req = {"id": 44, "kind": "confirm", "title": "Confirm tool call",
                   "prompt": "[FAKE DAEMON] read: README.md (lines 1-40)"}
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
        elif cmd == "refuse":
            refuse_next[0] = True
        elif cmd == "relay":
            threading.Thread(target=stream_turn, args=("[comms] #12 ask from spike: CI is red on main after the rebase. Rebase polish-e again or merge main in?",
                                                        "Spike's asking: rebase or merge for polish-e? Your call."), kwargs={"trigger": "event_auto"}, daemon=True).start()
        elif cmd.startswith("say "):
            threading.Thread(target=stream_turn, args=(None, cmd[4:].replace("\\n", "\n")), kwargs={"trigger": "event_auto"}, daemon=True).start()
        elif cmd.startswith("trig "):
            _, trig, text = cmd.split(" ", 2)
            threading.Thread(target=stream_turn, args=(None, text.replace("\\n", "\n")), kwargs={"trigger": trig}, daemon=True).start()
        elif cmd == "resolve":
            with lock:
                ids = list(pending)
                pending.clear()
            for i in ids:
                event({"ev": "prompt_resolved", "prompt_id": i})
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
