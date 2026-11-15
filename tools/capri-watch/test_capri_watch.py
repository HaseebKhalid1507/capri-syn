#!/usr/bin/env python3
"""capri-watch tests: stdlib unittest, fake sources only (no gh, systemctl, daemon, avatar or ssh).

    python3 tools/capri-watch/test_capri_watch.py
"""
import atexit, base64, importlib.machinery, importlib.util, io, json, os, re, shutil, sys, tempfile, unittest

HERE = os.path.dirname(os.path.abspath(__file__))
_loader = importlib.machinery.SourceFileLoader("capri_watch", os.path.join(HERE, "capri-watch"))
_spec = importlib.util.spec_from_loader("capri_watch", _loader)
cw = importlib.util.module_from_spec(_spec)
_loader.exec_module(cw)
if not os.environ.get("CAPRI_WATCH_TEST_LOG"):
    cw.log = lambda msg: None   # the watcher's journal lines; set CAPRI_WATCH_TEST_LOG=1 to see them

T0 = 1_800_000_000.0   # 2027-01-15T08:00:00Z
TMP = tempfile.mkdtemp(prefix="capri-watch-test-")   # never the real ~/.local/state/capri-watch
atexit.register(shutil.rmtree, TMP, True)


def ts(t):
    return cw.dt.datetime.fromtimestamp(t, cw.dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.%fZ")


class FakeSys(cw.Sys):
    def __init__(self):
        self.t = T0
        self.calls = []
        self.files = {}
        self.disks = {"/": (1, 500e9, 1000e9), "/home": (1, 500e9, 1000e9)}
        self.minutes = 12 * 60
        self.rows = []
        self.db = False
        # argv-prefix → (rc, stdout, stderr); the first matching prefix wins
        self.routes = {}
        self.set("gh api user", out="haseeb\n")
        self.set("gh run list", out="[]")
        self.set("gh pr list -R", out="[]")
        self.set("gh release list", out="[]")
        self.set("gh search prs", out="[]")
        self.set("synaps daemon sessions --json", out="[]")
        self.set("systemctl list-units", out="[]")
        self.set("systemctl --user list-units", out="[]")
        self.set("systemctl --user show jawz-backup-local.service", out="LoadState=not-found\n")
        self.set("nvidia-smi", out="0, 50\n")

    def set(self, prefix, out="", rc=0, err=""):
        self.routes[prefix] = (rc, out, err)

    def run(self, argv, timeout=30, stdin=None):
        self.calls.append((argv, stdin))
        line = " ".join(argv)
        for prefix in sorted(self.routes, key=len, reverse=True):
            if line.startswith(prefix):
                return self.routes[prefix]
        return 0, "", ""

    def statvfs(self, path):
        return self.disks[path]

    def read(self, path, tail=None):
        if path not in self.files:
            raise OSError(path)
        return self.files[path]

    def glob(self, pattern):
        return sorted(p.rsplit("/", 1)[0] for p in self.files if p.startswith("/sys/class/power_supply/BAT") and p.endswith("/capacity"))

    def exists(self, path):
        return self.db

    def now(self):
        return self.t

    def local_minutes(self, t):
        return self.minutes

    def sqlite_rows(self, path, sql, args):
        return self.rows


class FakeTransport(cw.Transport):
    def __init__(self, cfg, sysx):
        super().__init__(cfg, sysx)
        self.sent, self.ok = [], True
        self.state = {"reachable": True, "dnd": False, "music": False, "present": True, "idle_s": 1.0}
        self.probes = 0

    def send(self, source, text):
        if not self.ok:
            return False, "ssh: connect to host bella port 22: No route to host"
        self.sent.append((source, text))
        return True, ""

    def avatar(self):
        self.probes += 1
        return dict(self.state)


def config(toml="", role="bella"):
    c = cw.load_config(text=f'role = "{role}"\n' + toml)
    c["state_file"] = os.path.join(TMP, "state.json")
    return c


class Base(unittest.TestCase):
    toml = ""
    role = "bella"

    def setUp(self):
        self.sys = FakeSys()
        self.cfg = config(self.toml, self.role)
        self.tr = FakeTransport(self.cfg, self.sys)
        self.w = cw.Watcher(self.cfg, self.sys, self.tr, state={})

    def tick(self, dt=0, force=True):
        self.sys.t += dt
        self.w.tick(self.sys.t, force=force)
        return self.tr.sent

    def texts(self):
        return [t for _, t in self.tr.sent]

    def lines(self):
        return "\n".join(self.texts())


def gh_runs(*runs):
    return json.dumps([{"databaseId": i, "workflowName": wf, "status": "completed", "conclusion": c,
                        "headSha": sha * 6, "url": f"https://github.com/x/runs/{i}", "event": "push"}
                       for i, (wf, c, sha) in enumerate(runs)])


def pr(n, author="haseeb", checks=(), title="feat: things"):
    return {"number": n, "title": title, "url": f"https://github.com/o/r/pull/{n}", "author": {"login": author},
            "isDraft": False, "statusCheckRollup": [{"name": a, "conclusion": b} for a, b in checks]}


# ─── config ──────────────────────────────────────────────────────────────────────────────


class ConfigTest(unittest.TestCase):
    def test_defaults_match_the_brief(self):
        c = cw.load_config(text="", hostname="parabellum")
        on = {k for k in cw.EVENTS if c["event"][k]["enabled"]}
        self.assertEqual(on, {"ci_failed", "approval_waiting", "session_error", "unit_failed", "disk_low", "backup_failed"})
        self.assertEqual(set(cw.EVENTS) - on, {"pr_opened", "pr_merged", "review_requested", "long_turn", "release",
                                               "battery_low", "gpu_hot", "calendar"})
        self.assertEqual(c["event"]["approval_waiting"]["after_s"], 30)
        self.assertEqual((c["event"]["disk_low"]["min_free_pct"], c["event"]["disk_low"]["min_free_gb"]), (10, 20))
        self.assertEqual(c["safeguards"]["quiet_hours"], "", "quiet hours default off")
        self.assertEqual(c["event"]["long_turn"]["min_s"], 600)

    def test_shipped_example_is_the_built_in_defaults(self):
        with open(os.path.join(HERE, "watch.toml"), "rb") as f:
            ex = cw.tomllib.load(f)
        self.assertEqual(cw.merge(cw.DEFAULTS, ex), cw.DEFAULTS)
        for k in cw.EVENTS:
            self.assertIn(k, ex["event"], f"watch.toml documents event.{k}")
            self.assertEqual(set(ex["event"][k]), set(cw.DEFAULTS["event"][k]), f"event.{k}: same keys")

    def test_role_by_host(self):
        b = cw.load_config(text="", hostname="parabellum")
        a = cw.load_config(text="", hostname="avante")
        self.assertEqual((b["role"], b["host"], b["synaps"], b["deliver"]["transport"], b["github"]["enabled"]),
                         ("bella", "bella", "synaps", "local", True))
        self.assertEqual((a["role"], a["host"], a["synaps"], a["deliver"]["transport"], a["github"]["enabled"]),
                         ("avante", "avante", "synaps-dev", "ssh", False), "GitHub runs on bella only")

    def test_overrides_and_errors(self):
        c = config('[event.pr_merged]\nenabled = true\n[safeguards]\nmax_per_hour = 2\n')
        self.assertTrue(c["event"]["pr_merged"]["enabled"])
        self.assertEqual(c["safeguards"]["max_per_hour"], 2)
        self.assertEqual(c["safeguards"]["bundle_s"], 15, "untouched keys keep their defaults")
        for bad in ('[event.nope]\nenabled = true\n', 'typo = 1\n', '[safeguards]\nmusic = "loud"\n',
                    '[safeguards]\nquiet_hours = "late"\n', '[deliver]\ntransport = "pigeon"\n'):
            with self.assertRaises(ValueError, msg=bad):
                config(bad)

    def test_quiet_hours_wrap_midnight(self):
        self.assertTrue(cw.in_quiet("23:00-08:00", 23 * 60 + 5))
        self.assertTrue(cw.in_quiet("23:00-08:00", 7 * 60 + 59))
        self.assertFalse(cw.in_quiet("23:00-08:00", 12 * 60))
        self.assertTrue(cw.in_quiet("13:00-14:00", 13 * 60 + 30))
        self.assertFalse(cw.in_quiet("", 0))


# ─── events ──────────────────────────────────────────────────────────────────────────────


class GitHubTest(Base):
    def test_ci_failed_on_main_fires_once_per_state_change(self):
        self.sys.set("gh run list -R HaseebKhalid1507/capri-syn", out=gh_runs(("ci", "failure", "09b138d")))
        self.tick()
        self.assertEqual(len(self.tr.sent), 1)
        src, text = self.tr.sent[0]
        self.assertEqual(src, "watch:ci_failed")
        self.assertEqual(text, "[watch] ci_failed@bella: capri-syn main: ci failed on 09b138d (https://github.com/x/runs/0)")
        self.tick(200)
        self.assertEqual(len(self.tr.sent), 1, "still failing: no second wake")
        self.sys.set("gh run list -R HaseebKhalid1507/capri-syn", out=gh_runs(("ci", "success", "aaaaaaa")))
        self.tick(200)
        self.sys.set("gh run list -R HaseebKhalid1507/capri-syn", out=gh_runs(("ci", "failure", "bbbbbbb")))
        self.tick(200)
        self.assertEqual(len(self.tr.sent), 2, "fixed, then broken again: fires again")
        self.assertIn("on bbbbbbb", self.tr.sent[1][1])

    def test_ci_last_completed_run_per_workflow_decides(self):
        runs = json.loads(gh_runs(("ci", "success", "c"), ("ci", "failure", "b"), ("Release", "cancelled", "a")))
        runs.insert(0, {"databaseId": 9, "workflowName": "ci", "status": "in_progress", "conclusion": "", "headSha": "d" * 7,
                        "url": "u", "event": "push"})
        runs.append({"databaseId": 8, "workflowName": "lint", "status": "completed", "conclusion": "failure",
                     "headSha": "e" * 7, "url": "u", "event": "pull_request"})
        self.sys.set("gh run list", out=json.dumps(runs))
        self.tick()
        self.assertEqual(self.tr.sent, [], "latest ci passed, cancelled isn't a failure, PR-event runs are ignored")

    def test_ci_failed_on_my_prs_only(self):
        self.sys.set("gh pr list -R HaseebKhalid1507/Axon --state open",
                     out=json.dumps([pr(26, checks=[("test", "SUCCESS"), ("windows", "FAILURE")]),
                                     pr(27, author="dependabot", checks=[("test", "FAILURE")])]))
        self.tick()
        self.assertEqual(self.texts(), ['[watch] ci_failed@bella: Axon #26 "feat: things": windows failed (https://github.com/o/r/pull/26)'])

    def test_pr_events_are_off_by_default(self):
        self.sys.set("gh pr list -R HaseebKhalid1507/Axon --state open", out=json.dumps([pr(1)]))
        self.tick()
        self.sys.set("gh pr list -R HaseebKhalid1507/Axon --state open", out=json.dumps([pr(1), pr(2)]))
        self.tick(200)
        self.assertEqual(self.tr.sent, [])
        self.assertFalse(any("--state merged" in " ".join(a) or "release" in a for a, _ in self.sys.calls),
                         "disabled events cost no API calls")

    def test_github_off_on_avante(self):
        cfg = config("", role="avante")
        w = cw.Watcher(cfg, self.sys, FakeTransport(cfg, self.sys), state={})
        w.tick(T0, force=True)
        self.assertFalse(any(a[0] == "gh" for a, _ in self.sys.calls))

    def test_a_failed_repo_keeps_its_conditions(self):
        self.sys.set("gh run list -R HaseebKhalid1507/capri-syn", out=gh_runs(("ci", "failure", "1234567")))
        self.tick()
        self.sys.set("gh run list -R HaseebKhalid1507/capri-syn", rc=1, err="HTTP 502")
        self.tick(200)
        self.sys.set("gh run list -R HaseebKhalid1507/capri-syn", out=gh_runs(("ci", "failure", "1234567")))
        self.tick(200)
        self.assertEqual(len(self.tr.sent), 1, "a GitHub hiccup is not a state change")


class GitHubOptInTest(Base):
    toml = "[event.pr_opened]\nenabled = true\n[event.pr_merged]\nenabled = true\n[event.release]\nenabled = true\n" \
           "[event.review_requested]\nenabled = true\n[github]\nrepos = [\"o/r\"]\n"

    def test_opened_merged_release_seed_then_fire(self):
        self.sys.set("gh pr list -R o/r --state open", out=json.dumps([pr(1)]))
        self.sys.set("gh pr list -R o/r --state merged", out=json.dumps([{"number": 5, "title": "old", "url": "u5", "mergedBy": {"login": "haseeb"}}]))
        self.sys.set("gh release list -R o/r", out=json.dumps([{"tagName": "v0.1.0", "isDraft": False}]))
        self.tick()
        self.assertEqual(self.tr.sent, [], "the first pass only learns what already exists")
        self.sys.set("gh pr list -R o/r --state open", out=json.dumps([pr(1), pr(2, author="spike", title="x" * 80)]))
        self.sys.set("gh pr list -R o/r --state merged", out=json.dumps([{"number": 6, "title": "fix", "url": "u6", "mergedBy": {"login": "haseeb"}},
                                                                         {"number": 5, "title": "old", "url": "u5"}]))
        self.sys.set("gh release list -R o/r", out=json.dumps([{"tagName": "v0.2.0", "isDraft": False, "isPrerelease": True},
                                                               {"tagName": "v0.1.0", "isDraft": False}]))
        self.sys.set("gh search prs", out=json.dumps([{"number": 9, "title": "review me", "url": "u9",
                                                       "repository": {"name": "r", "nameWithOwner": "o/r"}}]))
        self.tick(200)
        self.assertEqual(self.tr.sent[0][0], "watch:digest")
        text = self.tr.sent[0][1]
        self.assertTrue(text.startswith("[watch] digest@bella: 4 events\n"), text)
        self.assertIn(f'[watch] pr_opened@bella: r #2 "{"x" * 49}…" opened by spike (https://github.com/o/r/pull/2)', text)
        self.assertIn('[watch] pr_merged@bella: r #6 "fix" merged by haseeb (u6)', text)
        self.assertIn("[watch] release@bella: r v0.2.0 published (pre-release) (https://github.com/o/r/releases/tag/v0.2.0)", text)
        self.assertIn('[watch] review_requested@bella: r #9 "review me" wants your review (u9)', text)
        self.tick(200)
        self.assertEqual(len(self.tr.sent), 1, "one-shots fire once")


def session(sid="20261114-000000-aaaa", name="axon", busy=False, since=T0, awaiting=0, messages=10, life="live"):
    return {"id": sid, "name": name, "busy": busy, "state_since": ts(since), "awaiting_input": awaiting,
            "messages": messages, "lifecycle": life, "journal_id": sid, "title": "a long title " * 5}


class DaemonTest(Base):
    J = os.path.expanduser("~/.synaps-cli/sessions/20261114-000000-aaaa.json")

    def sessions(self, *ss):
        self.sys.set("synaps daemon sessions --json", out=json.dumps(list(ss)))

    def journal(self, *roles_last, n=10, env=True):
        msgs = [{"role": "user", "content": "hi"}] * (n - 1)
        role, content = roles_last
        msgs.append({"role": role, "content": content})
        d = {"api_messages": msgs, "env": {"ANTHROPIC_API_KEY": "sk-ant-secret-should-never-leak-123456"} if env else {}}
        self.sys.files[self.J] = json.dumps(d)

    def test_approval_waiting_after_30_s(self):
        self.sessions(session(awaiting=1, busy=True))
        self.tick()
        self.tick(20)
        self.assertEqual(self.tr.sent, [], "20 s: not yet")
        self.tick(15)
        self.tick(16)   # the bundle window
        self.assertEqual(self.texts(), ["[watch] approval_waiting@bella: session axon is waiting on an approval (20261114-000000-aaaa)"])
        self.tick(60)
        self.assertEqual(len(self.tr.sent), 1, "same prompt: once")
        self.sessions(session(awaiting=0, busy=True))
        self.tick(10)
        self.sessions(session(awaiting=1, busy=True))
        self.tick(10)
        self.tick(31)
        self.tick(16)
        self.assertEqual(len(self.tr.sent), 2, "answered, then a new prompt: fires again")

    def test_an_approval_answered_inside_the_window_never_wakes_her(self):
        w = self.w
        self.sessions(session(awaiting=1, busy=True))
        w.tick(T0, force=True)
        w.tick(T0 + 31)
        self.assertEqual(len(w.pending), 1)
        self.sessions(session(awaiting=0, busy=True))
        w.tick(T0 + 41)
        w.tick(T0 + 60)
        self.assertEqual((w.pending, self.tr.sent), ([], []))

    def test_session_error_from_the_journal_with_the_logs_reason(self):
        self.sessions(session(busy=True, since=T0 - 50, messages=9))
        self.tick()
        end = T0 + 5
        self.sessions(session(busy=False, since=end, messages=10))
        self.journal("user", "please fix it")
        day = cw.dt.datetime.fromtimestamp(end, cw.dt.timezone.utc).strftime("%Y-%m-%d")
        self.sys.files[os.path.expanduser(f"~/.synaps-cli/synaps.log.{day}")] = (
            f"{ts(end - 600)}  WARN ThreadId(05) API request failed (not retried) status=529 request_id=\"x\" error=old\n"
            f"{ts(end - 2)}  WARN ThreadId(77) API request failed (not retried) status=503 request_id=\"req\" error=Anthropic server error\n")
        self.tick(10)
        self.tick(16)
        self.assertEqual(self.texts(), ["[watch] session_error@bella: session axon: turn failed (HTTP 503) (20261114-000000-aaaa)"])
        self.assertNotIn("sk-ant", self.lines())

    def test_a_turn_that_replied_is_not_an_error(self):
        self.sessions(session(busy=True, since=T0 - 50, messages=8))
        self.tick()
        self.sessions(session(busy=False, since=T0 + 5, messages=10))
        self.journal("assistant", [{"type": "thinking", "thinking": "."}, {"type": "text", "text": "done"}])
        self.tick(10)
        self.tick(16)
        self.assertEqual(self.tr.sent, [])

    def test_an_abort_without_a_logged_reason(self):
        self.sessions(session(busy=True, since=T0 - 50, messages=9))
        self.tick()
        self.sessions(session(busy=False, since=T0 + 5, messages=10))
        self.journal("user", [{"type": "tool_result", "tool_use_id": "t", "content": "ok"}])
        self.tick(10)
        self.tick(16)
        self.assertEqual(self.texts(), ["[watch] session_error@bella: session axon: turn ended without a reply (error or abort) (20261114-000000-aaaa)"])

    def test_a_short_turn_between_two_polls_is_still_checked(self):
        self.sessions(session(busy=False, since=T0 - 50, messages=8))
        self.tick()
        self.sessions(session(busy=False, since=T0 + 5, messages=9))
        self.journal("user", "hello?", n=9)
        self.tick(10)
        self.tick(16)
        self.assertEqual(len(self.tr.sent), 1)

    def test_journal_not_saved_yet_waits_a_pass(self):
        self.sessions(session(busy=True, since=T0 - 50, messages=9))
        self.tick()
        self.sessions(session(busy=False, since=T0 + 5, messages=10))
        self.journal("assistant", "partial", n=8)   # stale save: 8 messages, the listing says 10
        self.tick(10)
        self.assertEqual(self.w.pending, [])
        self.journal("user", "hm", n=10)
        self.tick(10)
        self.tick(16)
        self.assertEqual(len(self.tr.sent), 1)

    def test_her_own_session_is_never_reported(self):
        self.sessions(session(name="capri", busy=True, since=T0 - 50, awaiting=1, messages=9))
        self.tick()
        self.sessions(session(name="capri", busy=False, since=T0 + 5, awaiting=1, messages=10))
        self.journal("user", "x")
        for _ in range(6):
            self.tick(20)
        self.assertEqual(self.tr.sent, [])
        self.assertEqual(self.w.state["src"]["daemon"]["sessions"], {})

    def test_an_error_whose_repair_dropped_a_message(self):
        self.sessions(session(busy=True, since=T0 - 50, messages=12))   # mid-turn: prompt, tool_use, result, tool_use
        self.tick()
        self.sessions(session(busy=False, since=T0 + 5, messages=11))   # the unmatched tool_use was popped
        self.journal("user", [{"type": "tool_result", "tool_use_id": "t", "content": "ok"}], n=11)
        self.tick(10)
        self.tick(16)
        self.assertEqual(len(self.tr.sent), 1)

    def test_a_new_idle_stamp_with_nothing_added_is_not_a_turn(self):
        self.sessions(session(busy=False, since=T0 - 50, messages=10))
        self.tick()
        self.sessions(session(busy=False, since=T0 + 5, messages=10))
        self.journal("user", "an old failure, already reported")
        self.tick(10)
        self.tick(16)
        self.assertEqual(self.tr.sent, [])

    def test_parking_and_compaction_are_not_turns(self):
        self.sessions(session(busy=False, since=T0 - 50))
        self.tick()
        self.sessions(session(busy=False, since=T0 + 5, life="parked"))
        self.journal("user", "x")
        self.tick(10)
        self.sessions(session(busy=True, since=T0 + 20, messages=40))
        self.tick(10)
        self.sessions(session(busy=False, since=T0 + 30, messages=12))   # compacted
        self.tick(10)
        self.tick(16)
        self.assertEqual(self.tr.sent, [])


class LongTurnTest(DaemonTest.__base__):
    toml = "[event.long_turn]\nenabled = true\n[event.session_error]\nenabled = false\n"

    def run_turn(self, minutes):
        self.sys.set("synaps daemon sessions --json", out=json.dumps([session(busy=True, since=T0 - minutes * 60)]))
        self.tick()
        self.sys.set("synaps daemon sessions --json", out=json.dumps([session(busy=False, since=T0 + 5)]))
        self.tick(10)
        self.tick(16)

    def test_a_long_turn_while_he_is_away(self):
        self.tr.state.update(present=False)
        self.run_turn(14)
        self.assertEqual(self.texts(), ["[watch] long_turn@bella: session axon finished a turn after 14 min (20261114-000000-aaaa)"])

    def test_not_when_he_is_here(self):
        self.run_turn(14)
        self.assertEqual(self.tr.sent, [])

    def test_idle_counts_as_away_and_short_turns_never(self):
        self.tr.state.update(idle_s=900)
        self.run_turn(5)
        self.assertEqual(self.tr.sent, [])
        self.run_turn(11)
        self.assertEqual(len(self.tr.sent), 1)


class UnitsTest(Base):
    def test_failed_units_with_the_noise_filter(self):
        self.sys.set("systemctl list-units", out=json.dumps([{"unit": "axon-nightly.service"}, {"unit": "xdg-document-portal.service"}]))
        self.sys.set("systemctl --user list-units", out=json.dumps([{"unit": u} for u in (
            "dunst.service", "jawz-backup-local.service", "probe-deletion-6.service", "proof-x.service",
            "run-u12.service", "wallpaper-rotate.service", "xdg-desktop-portal-gtk.service")]))
        self.sys.set("systemctl --user show -p Id", out="Id=jawz-backup-local.service\nTransient=no\n\nId=proof-x.service\nTransient=yes\n\n"
                                                         "Id=wallpaper-rotate.service\nTransient=no\n")
        self.tick()
        self.tick(16)
        self.assertEqual(self.texts(), ["[watch] digest@bella: 2 events\n"
                                        "[watch] unit_failed@bella: axon-nightly.service failed (system)\n"
                                        "[watch] unit_failed@bella: wallpaper-rotate.service failed (user)"])
        self.tick(100)
        self.assertEqual(len(self.tr.sent), 1)

    def test_the_filter_is_a_setting(self):
        cfg = config('[event.unit_failed]\nignore = ["axon-*"]\nuser = false\n')
        tr = FakeTransport(cfg, self.sys)
        self.sys.set("systemctl list-units", out=json.dumps([{"unit": "axon-nightly.service"}, {"unit": "dunst.service"}]))
        w = cw.Watcher(cfg, self.sys, tr, state={})
        w.tick(T0, force=True)
        self.assertEqual(tr.sent, [("watch:unit_failed", "[watch] unit_failed@bella: dunst.service failed (system)")])
        self.assertFalse(any(a[:3] == ["systemctl", "--user", "list-units"] for a, _ in self.sys.calls))


class DiskTest(Base):
    def test_low_by_percent_or_by_gb_on_one_device_once(self):
        self.sys.disks = {"/": (7, 90e9, 1000e9), "/home": (7, 90e9, 1000e9)}
        self.tick()
        self.assertEqual(self.texts(), ["[watch] disk_low@bella: / and /home low on space: 90.0 GB free (9 %)"])
        self.sys.disks = {"/": (1, 19e9, 100e9), "/home": (2, 500e9, 1000e9)}
        self.setUp()
        self.sys.disks = {"/": (1, 19e9, 100e9), "/home": (2, 500e9, 1000e9)}
        self.tick()
        self.assertEqual(self.texts(), ["[watch] disk_low@bella: / low on space: 19.0 GB free (19 %)"])

    def test_hysteresis(self):
        self.sys.disks = {"/": (1, 19e9, 100e9), "/home": (1, 19e9, 100e9)}
        self.tick()
        for free in (20.5e9, 19.5e9, 21e9, 18e9):   # wobbling around the 20 GB line
            self.sys.disks = {"/": (1, free, 100e9), "/home": (1, free, 100e9)}
            self.tick(30)
        self.assertEqual(len(self.tr.sent), 1)
        self.sys.disks = {"/": (1, 30e9, 100e9), "/home": (1, 30e9, 100e9)}
        self.tick(30)
        self.sys.disks = {"/": (1, 19e9, 100e9), "/home": (1, 19e9, 100e9)}
        self.tick(30)
        self.assertEqual(len(self.tr.sent), 2, "freed past the margin, then low again: fires again")


class BackupTest(Base):
    role = "avante"
    LOG = os.path.expanduser("~/Jawz/logs/backup-local.log")
    SHOW = "systemctl --user show jawz-backup-local.service"

    def test_exit_code_and_error_lines(self):
        self.sys.set(self.SHOW, out="LoadState=loaded\nActiveState=failed\nExecMainStatus=2\nExecMainExitTimestamp=Sat 2026-11-14 04:10:32 EST\n")
        self.sys.files[self.LOG] = ("[x] ===== Local backup started =====\n[x] ERROR: old run\n"
                                    "[y] ===== Local backup started =====\n[y] ERROR: could not snapshot brain.r8 from /secret/path\n"
                                    "[y] ERROR: snapshot of axel-v2-brain.r8 failed\n[y] ERROR: brains NOT in this backup\n")
        self.tick()
        self.assertEqual(self.texts(), ["[watch] backup_failed@avante: jawz-backup-local failed: exited 2, 3 ERROR lines in its last run "
                                        "(Sat 2026-11-14 04:10:32 EST)"])
        self.assertNotIn("/secret/path", self.lines(), "no log content in the line")

    def test_error_lines_alone_and_clean_runs(self):
        self.sys.set(self.SHOW, out="LoadState=loaded\nExecMainStatus=0\nExecMainExitTimestamp=Sun\n")
        self.sys.files[self.LOG] = "[y] ===== Local backup started =====\n[y] ERROR: tar hiccup\n"
        self.tick()
        self.assertEqual(len(self.tr.sent), 1)
        self.sys.files[self.LOG] += "[z] ===== Local backup started =====\n[z] Backup created\n"
        self.tick(30)
        self.assertEqual(self.w.state["fired"], {}, "a clean run clears it")

    def test_no_backup_on_this_machine(self):
        self.tick()
        self.assertEqual(self.tr.sent, [])

    def test_the_backup_unit_is_not_reported_twice(self):
        self.sys.set(self.SHOW, out="LoadState=loaded\nExecMainStatus=2\nExecMainExitTimestamp=Sat\n")
        self.sys.set("systemctl --user list-units", out=json.dumps([{"unit": "jawz-backup-local.service"}]))
        self.tick()
        self.assertEqual([s for s, _ in self.tr.sent], ["watch:backup_failed"])


class ExtrasTest(Base):
    toml = "[event.battery_low]\nenabled = true\n[event.gpu_hot]\nenabled = true\n[event.calendar]\nenabled = true\n"

    def test_battery_gpu_calendar(self):
        self.sys.files["/sys/class/power_supply/BAT1/capacity"] = "12\n"
        self.sys.files["/sys/class/power_supply/BAT1/status"] = "Discharging\n"
        self.sys.set("nvidia-smi", out="0, 91\n")
        self.sys.db = True
        start = cw.dt.datetime.fromtimestamp(T0 + 8 * 60, cw.dt.timezone.utc)
        later = cw.dt.datetime.fromtimestamp(T0 + 3600, cw.dt.timezone.utc)
        self.sys.rows = [(3, "Dentist", start.strftime("%Y-%m-%d"), start.strftime("%H:%M:%S")),
                         (4, "Later", later.strftime("%Y-%m-%d"), later.strftime("%H:%M:%S"))]
        self.tick()
        text = self.lines()
        self.assertIn("[watch] battery_low@bella: battery at 12 % and discharging", text)
        self.assertIn("[watch] gpu_hot@bella: GPU 0 at 91 °C", text)
        self.assertIn('[watch] calendar@bella: "Dentist" starts in 8 min', text)
        self.assertNotIn("Later", text)
        self.sys.files["/sys/class/power_supply/BAT1/status"] = "Charging\n"
        self.sys.set("nvidia-smi", out="0, 88\n")   # within 5 °C: still hot (hysteresis)
        self.tick(30)
        self.assertNotIn("battery_low:bella:BAT1", self.w.state["fired"])
        self.assertIn("gpu_hot:bella:0", self.w.state["fired"])
        self.assertEqual(len(self.tr.sent), 1)

    def test_all_off_costs_nothing(self):
        cfg = config("")
        w = cw.Watcher(cfg, self.sys, FakeTransport(cfg, self.sys), state={})
        self.sys.calls.clear()
        w.tick(T0, force=True)
        self.assertFalse(any(a[0] == "nvidia-smi" for a, _ in self.sys.calls))


# ─── safeguards ──────────────────────────────────────────────────────────────────────────


class SafeguardTest(Base):
    def fire(self, *units):
        self.sys.set("systemctl list-units", out=json.dumps([{"unit": u} for u in units]))

    def test_bundling_waits_for_the_window_then_sends_one_digest(self):
        self.fire("a.service")
        self.tick(0, force=False)
        self.fire("a.service", "b.service")
        self.tick(5, force=False)
        self.fire("a.service", "b.service", "c.service")
        self.tick(5, force=False)
        self.assertEqual(self.tr.sent, [], "inside the 15 s window")
        self.tick(6, force=False)
        self.assertEqual(len(self.tr.sent), 1)
        self.assertEqual(self.tr.sent[0][0], "watch:digest")
        self.assertTrue(self.tr.sent[0][1].startswith("[watch] digest@bella: 3 events"))

    def test_digest_caps_its_length(self):
        self.fire(*[f"u{i}.service" for i in range(14)])
        self.tick()
        lines = self.tr.sent[0][1].splitlines()
        self.assertEqual(len(lines), 12)
        self.assertEqual(lines[-1], "[watch] digest@bella: and 4 more")

    def test_wake_cap_per_hour(self):
        self.cfg["safeguards"]["max_per_hour"] = 2
        for i in range(4):
            self.fire(*[f"u{j}.service" for j in range(i + 1)])
            self.tick(60)
        self.assertEqual(len(self.tr.sent), 2)
        self.assertEqual(len(self.w.pending), 2, "the rest wait")
        self.sys.t += 3600
        self.tick(0)
        self.assertEqual(len(self.tr.sent), 3)
        self.assertEqual(self.tr.sent[2][0], "watch:digest", "held events arrive together")
        self.assertIn("ago]", self.tr.sent[2][1], "late events say how late")

    def test_dnd_holds_until_it_ends(self):
        self.tr.state["dnd"] = True
        self.fire("a.service")
        self.tick()
        self.tick(60)
        self.assertEqual(self.tr.sent, [])
        self.tr.state["dnd"] = False
        self.tick(60)
        self.assertEqual(len(self.tr.sent), 1)

    def test_dnd_can_be_ignored(self):
        self.cfg["safeguards"]["dnd"] = False
        self.cfg["safeguards"]["music"] = "ignore"
        self.tr.state.update(dnd=True, music=True)
        self.fire("a.service")
        self.tick()
        self.assertEqual(len(self.tr.sent), 1)
        self.assertEqual(self.tr.probes, 0, "nothing to ask the avatar")

    def test_music_defers_or_drops(self):
        self.tr.state["music"] = True
        self.fire("a.service")
        self.tick()
        self.assertEqual(self.tr.sent, [])
        self.tr.state["music"] = False
        self.tick(60)
        self.assertEqual(len(self.tr.sent), 1)
        self.setUp()
        self.cfg["safeguards"]["music"] = "drop"
        self.tr.state["music"] = True
        self.fire("b.service")
        self.tick()
        self.tr.state["music"] = False
        self.tick(60)
        self.assertEqual((self.tr.sent, self.w.pending), ([], []))

    def test_quiet_hours(self):
        self.cfg["safeguards"]["quiet_hours"] = "23:00-08:00"
        self.sys.minutes = 2 * 60
        self.fire("a.service")
        self.tick()
        self.assertEqual(self.tr.sent, [])
        self.sys.minutes = 8 * 60 + 1
        self.tick(60)
        self.assertEqual(len(self.tr.sent), 1)

    def test_stale_conditions_are_rechecked_and_stale_one_shots_dropped(self):
        self.tr.state["music"] = True
        self.fire("a.service")
        self.tick()
        self.tick(3601)       # held past stale_s: dropped, then found again on this same pass
        self.assertEqual(len(self.w.pending), 1)
        self.assertEqual(self.w.pending[0]["t"], self.sys.t, "a fresh detection, not the stale one")
        self.fire()
        self.tick(30)
        self.assertEqual(self.w.pending, [], "cleared while held: never delivered")

    def test_delivery_failure_retries(self):
        self.tr.ok = False
        self.fire("a.service")
        self.tick()
        self.assertEqual(len(self.w.pending), 1)
        self.tr.ok = True
        self.tick(30, force=False)
        self.assertEqual(self.tr.sent, [], "backs off 60 s")
        self.tick(31, force=False)
        self.assertEqual(len(self.tr.sent), 1)

    def test_state_survives_a_restart(self):
        path = os.path.join(TMP, "restart.json")
        self.cfg["state_file"] = path
        try:
            self.fire("a.service")
            w = cw.Watcher(self.cfg, self.sys, self.tr, state={})
            w.tick(T0, force=True)
            w2 = cw.Watcher(self.cfg, self.sys, self.tr)   # a restart reads the file
            w2.tick(T0 + 60, force=True)
            self.assertEqual(len(self.tr.sent), 1, "no repeat after a restart")
        finally:
            for p in (path, path + ".tmp"):
                if os.path.exists(p):
                    os.remove(p)

    def test_tests_never_touch_the_real_state(self):
        self.assertTrue(self.cfg["state_file"].startswith(TMP))

    def test_a_clock_jump_back_does_not_freeze_the_cap(self):
        self.w.state["sent"] = [T0 + 86400 * 365]   # a timestamp from the future
        self.fire("a.service")
        self.tick()
        self.assertEqual(len(self.tr.sent), 1)

    def test_dry_run_never_sends_or_writes(self):
        out = io.StringIO()
        self.cfg["state_file"] = "/nonexistent/dir/state.json"
        w = cw.Watcher(self.cfg, self.sys, self.tr, dry_run=True, out=out, state={})
        self.fire("a.service")
        w.tick(T0, force=True)
        self.assertEqual(self.tr.sent, [])
        self.assertIn("--- would send --source watch:unit_failed:\n[watch] unit_failed@bella: a.service failed (system)", out.getvalue())


class LineTest(unittest.TestCase):
    def test_lines_are_framed_cleaned_and_short(self):
        self.assertEqual(cw.clean("a\x1b[31m\nb\tc  d"), "a [31m b c d")
        self.assertEqual(cw.clean("token ghp_" + "A" * 36 + " here"), "token [redacted] here")
        self.assertEqual(cw.clean("key sk-ant-api03-" + "x" * 30), "key [redacted]")
        self.assertEqual(len(cw.clean("y" * 1000)), 300)
        self.assertEqual(cw.title("z" * 80), '"' + "z" * 49 + '…"')
        self.assertEqual((cw.ago(45), cw.ago(600), cw.ago(7200)), ("45 s", "10 min", "2.0 h"))


class TransportTest(unittest.TestCase):
    def test_local_argv(self):
        cfg = config("")
        sysx = FakeSys()
        cw.LocalTransport(cfg, sysx).send("watch:ci_failed", "[watch] x")
        self.assertEqual(sysx.calls[-1][0], ["synaps", "send", "--session", "capri", "--source", "watch:ci_failed", "--", "[watch] x"])
        cfg["deliver"]["profile"] = "sandbox"
        cfg["deliver"]["session"] = "watch-test"
        cw.LocalTransport(cfg, sysx).send("watch:x", "y")
        self.assertEqual(sysx.calls[-1][0][:5], ["synaps", "--profile", "sandbox", "send", "--session"])
        self.assertIn("watch-test", sysx.calls[-1][0])

    def test_ssh_ships_the_argv_without_a_remote_shell(self):
        cfg = config("", role="avante")
        sysx = FakeSys()
        text = "[watch] unit_failed@avante: it's \"quoted\" $(rm -rf ~) `x` \\ ok"
        ok, _ = cw.SshTransport(cfg, sysx).send("watch:unit_failed", text)
        self.assertTrue(ok)
        argv, stdin = sysx.calls[-1]
        self.assertEqual(argv, ["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "-o", "ServerAliveInterval=5", "bella", "python3", "-"])
        b64 = re.search(r"b64decode\('([^']+)'\)", stdin)[1]
        self.assertEqual(json.loads(base64.b64decode(b64)), ["synaps", "send", "--session", "capri", "--source", "watch:unit_failed", "--", text])
        compile(stdin, "remote", "exec")

    def test_ssh_avatar_probe_is_self_contained(self):
        cfg = config("", role="avante")
        sysx = FakeSys()
        sysx.set("ssh", out='{"reachable": true, "dnd": true, "music": false, "present": true, "idle_s": 3}\n')
        a = cw.SshTransport(cfg, sysx).avatar()
        self.assertEqual(a["dnd"], True)
        src = sysx.calls[-1][1]
        compile(src, "remote", "exec")
        self.assertIn("def probe_avatar", src)

    def test_probe_without_an_avatar(self):
        a = cw.probe_avatar("/nonexistent/capri-watch-test.sock")
        self.assertEqual((a["reachable"], a["dnd"]), (False, False))
        self.assertIn(a["music"], (True, False))

    def test_command_transport(self):
        cfg = config('[deliver]\ntransport = "command"\ncommand = ["comms", "send", "--to", "{session}", "--tag", "{source}", "{text}"]\n')
        sysx = FakeSys()
        t = cw.make_transport(cfg, sysx)
        t.send("watch:disk_low", "[watch] d")
        self.assertEqual(sysx.calls[-1][0], ["comms", "send", "--to", "capri", "--tag", "watch:disk_low", "[watch] d"])


if __name__ == "__main__":
    unittest.main(verbosity=1)
