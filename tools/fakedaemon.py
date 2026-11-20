#!/usr/bin/env python3
"""fakedaemon.py [FIXTURES_DIR]: a FAKE Synaps daemon on /tmp/sa-fake/d.sock for what the sandbox
can't raise safely (a Secret prompt) or cheaply (compaction). Modelled on synaps-client's
tests/fake_daemon.rs: Hello -> Welcome (one live session named `capri`), Sessions -> list,
Attach -> the attached_history.json fixture, then events on demand.

Control: one line per connection on /tmp/sa-fake/ctl.sock:
  secret | confirm | confirm2 | benign | compact | compacted | idle | refuse | relay | say TEXT | trig T TEXT
  | resolve | nobrain | fail TEXT | brain | model M | quit
A submit or steer streams a canned reply (FAKE: no model): a turn, text deltas every
FAKE_DELTA_MS (default 30), done, idle. `refuse`: the next submit is refused instead.
`relay`: a turn another session woke ([comms] ...). `say TEXT`: a turn with that reply. Both come as
trigger `event_auto`, as the real daemon's event turns do; a submit's turn is `user`. `trig T TEXT`: a
turn with trigger T. `resolve`: the pending prompts are answered elsewhere (they go).
`nobrain`: every submit fails as a Synaps with no model credentials does (its exact error text);
`fail TEXT`: every submit fails with TEXT; `brain`: replies again.
FAKE_DAEMON_VERSION and FAKE_MODEL set what the Welcome and the attach say (default: the Synaps her
installer pins, installer/manifest's synaps_pin, so no FAKE string reaches a shot; and fake).
A `set` (her persona, `[session] model`) is applied to the session's view and answered as the daemon
does (`setting_changed`, ok, the new view): her model shows as set. Her `engine_command` `context auto`
or `context off` (`[session] rollover`) is answered as Synaps 0.12 answers it (`query_result`: the
mode, or on Windows auto's Unix-only refusal); any other name `unhandled`. Her `context_report` query
(the session's mode, asked first) is answered as 0.12 does (`{text}`, its `context management:` line).
FAKE_CONTEXT=auto: the session's mode starts auto (Synaps' config has it on), not off. `model M`: another client's
`/model M` (his TUI's): the view changed, broadcast as the daemon does an engine command's change
(`setting_changed` with its reserved id, `VIEW_CHANGED_SETTING_ID`), a reply to no one's Set.
FAKE_HISTORY=FILE: the attached chat's messages instead of the fixture's (a JSON list of api messages).
The Synaps TUI attaches too (`synaps --attach capri`, SYNAPS_RUNTIME_DIR=FAKE_DIR with daemon.sock
linked to d.sock, SYNAPS_DAEMON_AUTOSPAWN=0): FAKE_LOCK=FILE holds a flock there as a daemon does.
Commands the avatar sends are printed; answer values are never printed (only their length).

Windows (stock CPython has no AF_UNIX there; the shots workflow runs it): the daemon end is the
named pipe synaps-client derives from %TEMP%\\sa-fake\\d.sock (run her with `--socket` that path), and
control is 127.0.0.1:FAKE_CTL_PORT (default 47811), one line per connection as above. FAKE_DIR moves
the folder on either OS."""
import hashlib, json, os, socket, sys, tempfile, threading, time

WIN = os.name == "nt"
DIR = os.environ.get("FAKE_DIR") or (os.path.join(tempfile.gettempdir(), "sa-fake") if WIN else "/tmp/sa-fake")
CTL_PORT = int(os.environ.get("FAKE_CTL_PORT", "47811"))
SID = "20261113-073427-5fde"
fixtures = sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "tests/fixtures")
os.makedirs(DIR, mode=0o700, exist_ok=True)
ATTACHED = json.loads(open(os.path.join(fixtures, "attached_history.json"), encoding="utf-8").read())
if os.environ.get("FAKE_HISTORY"):
    # Her chat as a shot wants it: these messages ([] for an empty chat), no replayed turn.
    _msgs = json.loads(open(os.environ["FAKE_HISTORY"], encoding="utf-8").read())
    ATTACHED["conversation"]["api_messages"] = _msgs
    ATTACHED["conversation"]["messages_len"] = len(_msgs)
    ATTACHED["replay"] = []
MODEL = os.environ.get("FAKE_MODEL", "fake")
VIEW = dict(ATTACHED.get("view") or {}, **({"model": MODEL} if "FAKE_MODEL" in os.environ else {}))
# The Synaps her installer pins (installer/manifest): what she'd really meet.
def _synaps_pin():
    try:
        with open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "installer", "manifest"), encoding="utf-8") as f:
            return next((l.split("=", 1)[1].strip() for l in f if l.startswith("synaps_pin=")), "0.0.0-fake")
    except OSError:
        return "0.0.0-fake"


SYNAPS_PIN = _synaps_pin()
WELCOME = {"type": "welcome", "protocol_version": 3, "daemon_version": os.environ.get("FAKE_DAEMON_VERSION") or SYNAPS_PIN, "pid": os.getpid(), "profile": None,
           # The session entry carries the attach's meta (`created_at`, cwd, ...) as the daemon's does:
           # the Synaps TUI reads it from the Welcome before it attaches.
           "sessions": [dict(ATTACHED["meta"], id=SID, model=MODEL, lifecycle="live", name="capri")],
           "progressive_tool_disclosure": False, "generation": 1, "features": []}
attached = []          # live session connections (socket, lock)
pending = {}           # prompt id -> request (in every Attached snapshot until answered)
seq = [100]
client_id = [10]
owner = [None]         # the input owner: the last Mirror attach, as the daemon hands it
lock = threading.Lock()
refuse_next = [False]
fail_with = [None]     # every submit fails with this error text (`nobrain`, `fail TEXT`)
context = [os.environ.get("FAKE_CONTEXT") or "off"]  # the session's context management mode


def context_status():
    """0.12 `context_management_status`'s first line: the mode, then the window's numbers (a 200k
    window's bands, `ContextThresholds::bands`: pressure 70%, rollover 90%)."""
    return f"context management: {context[0]} | window 1 | capacity 200000 | pressure 140000 | rollover 180000 | archive none"


NO_CREDENTIALS = "No Anthropic credentials. Run `synaps login`, or switch to a provider model with `/model groq/llama-3.3-70b-versatile`."
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


def fail_turn(user_text, message):
    """A turn that fails before the model says a word (as a daemon with no credentials does)."""
    event({"ev": "turn_started", "turn_baseline": 1, "trigger": "user", "user_text": user_text})
    time.sleep(0.25)
    event({"ev": "stream", "event": {"kind": "session", "session": "error", "message": message}})
    remember([{"role": "user", "content": user_text}])
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
            send(conn, wl, {"type": "event", "session_id": SID, "seq": n, "ts": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "event": ev})
        except OSError:
            pass


def serve(conn):
    wl = threading.Lock()
    mine = set()           # this connection's client ids (its ownership goes with it)
    f = conn.makefile("r", encoding="utf-8")
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
                    mine.add(me)
                log(f"attach {a.get('attach')} {a.get('session_id', '')} mode={mode} -> client {me}")
                if os.environ.get("FAKE_DEBUG"):
                    log("frame keys", sorted(fr.keys()))
                v = dict(ATTACHED)
                v["client"] = me
                v["meta"] = dict(v["meta"], name="capri")
                with lock:
                    if owner[0] in mine:            # this connection's last client is gone
                        owner[0] = None
                    if mode == "mirror":
                        owner[0] = me
                    v["input_owner"] = owner[0]
                    v["view"] = dict(VIEW)
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
                elif c.get("cmd") == "set":
                    st = c.get("setting") or {}
                    name = st.get("setting", "")
                    log(f"cmd set {name}" + (f" {st.get('model')}" if name == "model" else ""))
                    with lock:
                        if name == "model":
                            VIEW["model"] = st.get("model")
                        elif name == "system_prompt":
                            VIEW["system_prompt"] = st.get("text")
                        elif name == "reasoning_level":
                            VIEW["reasoning_level"] = st.get("level")
                        view = dict(VIEW)
                    event({"ev": "setting_changed", "applied": {"id": c.get("id"), "setting": name, "ok": True, "view": view}})
                elif c.get("cmd") == "engine_command":
                    # Her `/context auto` or `off` ([session] rollover): answered as Synaps 0.12 does, on this OS.
                    name, arg = c.get("name", ""), c.get("arg", "")
                    log(f"cmd engine_command {name} {arg}")
                    if name != "context":
                        value = {"kind": "unhandled"}
                    elif WIN and arg.startswith("auto"):
                        value = {"kind": "error", "text": "context archive is currently supported on Unix only; mode unchanged"}
                    else:
                        with lock:
                            context[0] = "off" if arg == "off" else "auto"
                        value = {"kind": "output", "text": context_status() + "\nSession-only setting. (FAKE)"}
                    event({"ev": "query_result", "id": c.get("id"), "value": value})
                elif c.get("cmd") == "query" and (c.get("query") or {}).get("query") == "context_report":
                    # `/context`'s report, as 0.12's `ContextReport` (`context_command`): sizes, then the mode.
                    log("cmd query context_report")
                    text = f"context \u2014 model {VIEW.get('model')}\n  system prompt: 0 bytes\n  tools: 0 (FAKE)\n" + context_status()
                    event({"ev": "query_result", "id": c.get("id"), "value": {"text": text}})
                elif c.get("cmd") in ("submit", "steer"):
                    log(f"cmd {c.get('cmd')} <{len(c.get('text', ''))} chars>")
                    if refuse_next[0]:
                        refuse_next[0] = False
                        send(conn, wl, {"type": "error", "session_id": SID, "message": "refused: not the input owner (FAKE)"})
                    elif fail_with[0]:
                        threading.Thread(target=fail_turn, args=(c.get("text", ""), fail_with[0]), daemon=True).start()
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
            if owner[0] in mine:
                owner[0] = None
        conn.close()


def control():
    if WIN:
        s = socket.socket(socket.AF_INET)
        s.bind(("127.0.0.1", CTL_PORT))
    else:
        path = f"{DIR}/ctl.sock"
        if os.path.exists(path):
            os.unlink(path)
        s = socket.socket(socket.AF_UNIX)
        s.bind(path)
        os.chmod(path, 0o600)
    s.listen()
    while True:
        c, _ = s.accept()
        cmd = c.makefile("r", encoding="utf-8").readline().strip()
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
        elif cmd == "nobrain":
            fail_with[0] = NO_CREDENTIALS
        elif cmd.startswith("fail "):
            fail_with[0] = cmd[5:]
        elif cmd == "brain":
            fail_with[0] = None
        elif cmd.startswith("model "):
            with lock:
                VIEW["model"] = cmd[6:].strip()
                view = dict(VIEW)
            event({"ev": "setting_changed", "applied": {"id": (1 << 63) + 3, "setting": "model", "ok": True, "message": None, "view": view, "clamp": None}})
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


# ---- Windows: the daemon's named pipe (overlapped, as multiprocessing's PipeConnection) --------

def pipe_name(path):
    """synaps-client's `ipc::pipe_name` (Synaps' own): the path in one spelling (absolute, its folder
    resolved, `\\` separators, no `\\?\\` prefix, lowercase), hashed into the flat pipe namespace."""
    d, n = os.path.split(os.path.abspath(path))
    if os.path.isdir(d):
        d = os.path.realpath(d)
    s = os.path.join(d, n).replace("/", "\\")
    if s.startswith("\\\\?\\UNC\\"):
        s = "\\\\" + s[8:]
    elif s.startswith("\\\\?\\"):
        s = s[4:]
    return "\\\\.\\pipe\\synaps-daemon-" + hashlib.sha256(s.lower().encode()).hexdigest()[:32]


class Pipe:
    """One connected pipe instance: as much of a socket as `serve` uses. Overlapped, so an event
    goes out while a read waits."""

    def __init__(self, h):
        self.h, self.shut = h, False

    def recv(self, n):
        import _winapi
        try:
            ov, _ = _winapi.ReadFile(self.h, n, overlapped=True)
            ov.GetOverlappedResult(True)
            return bytes(ov.getbuffer())
        except OSError:
            return b""

    def sendall(self, b):
        import _winapi
        ov, _ = _winapi.WriteFile(self.h, b, overlapped=True)
        n, _ = ov.GetOverlappedResult(True)
        if n != len(b):
            raise OSError("short pipe write")

    def makefile(self, mode="r", encoding="utf-8"):
        import io
        pipe = self

        class Raw(io.RawIOBase):
            def readable(self):
                return True

            def readinto(self, buf):
                d = pipe.recv(len(buf))
                buf[:len(d)] = d
                return len(d)

        return io.TextIOWrapper(io.BufferedReader(Raw()), encoding=encoding)

    def close(self):
        import _winapi
        if not self.shut:
            self.shut = True
            _winapi.CloseHandle(self.h)


def pipe_accept(name):
    """Connected instances, one after another; the next one is listening before this one is
    handed on (her client retries a busy pipe, not a missing one)."""
    import _winapi
    first = [True]

    def new():
        flags = _winapi.PIPE_ACCESS_DUPLEX | _winapi.FILE_FLAG_OVERLAPPED
        if first[0]:
            flags |= _winapi.FILE_FLAG_FIRST_PIPE_INSTANCE
            first[0] = False
        return _winapi.CreateNamedPipe(name, flags, _winapi.PIPE_WAIT, _winapi.PIPE_UNLIMITED_INSTANCES, 1 << 16, 1 << 16,
                                       _winapi.NMPWAIT_WAIT_FOREVER, _winapi.NULL)

    h = new()
    while True:
        try:
            ov = _winapi.ConnectNamedPipe(h, overlapped=True)
        except OSError as e:
            if e.winerror != _winapi.ERROR_NO_DATA:
                raise
        else:
            _winapi.WaitForMultipleObjects([ov.event], False, _winapi.INFINITE)
            ov.GetOverlappedResult(True)
        nxt = new()
        yield Pipe(h)
        h = nxt


def main():
    if os.environ.get("FAKE_LOCK"):
        # Stand in for a Synaps daemon's run dir (the TUI checks the daemon's flock): synthetic only.
        import fcntl
        main.lockf = open(os.environ["FAKE_LOCK"], "a+")
        fcntl.flock(main.lockf, fcntl.LOCK_EX | fcntl.LOCK_NB)
    path = f"{DIR}/d.sock"
    os.makedirs(DIR, exist_ok=True)
    if WIN:
        threading.Thread(target=control, daemon=True).start()
        name = pipe_name(path)
        log(f"fake daemon on {path} = {name}, control 127.0.0.1:{CTL_PORT} (FAKE: no model, no real prompts)")
        for c in pipe_accept(name):
            threading.Thread(target=serve, args=(c,), daemon=True).start()
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
