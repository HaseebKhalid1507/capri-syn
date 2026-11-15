# capri-watch: events that wake Capri

Capri reacts to her own session. `tools/capri-watch` gives her things to react to: a failing CI run, a session
stuck on an approval, a failed unit, a nearly full disk. It's one small stdlib-Python poller per machine
(a systemd `--user` unit). For each new event it writes ONE short line into her session:

```
[watch] ci_failed@bella: capri-syn main: ci failed on 09b138d (https://github.com/HaseebKhalid1507/capri-syn/actions/runs/43671709347)
```

She answers like she answers anything else, in a line, and offers the obvious next step. Her persona has a
paragraph for `[watch]` lines (`packs/capri/persona.md`): one line, no acting on anything risky without him.
With voice on, the voice PR speaks replies to events (never over music).

## Machines

| | bella (`role = "bella"`) | avante (`role = "avante"`) |
|---|---|---|
| watches | GitHub, bella's daemon, units, disk (+ battery, GPU, calendar when on) | avante's daemon (`synaps-dev`), units, disk, the Jawz backup |
| delivers | `synaps send --session capri --source watch:<event> -- <line>` | the same command on bella, over ssh (BatchMode, ConnectTimeout 5) |

The ssh transport ships the argv base64-encoded in a Python script on stdin (`ssh bella python3 -`). No
remote shell parses the line, so quotes, `$( )` and the like in a PR title arrive unchanged. bella's login
shell is fish. `[deliver] transport = "command"` takes any argv with `{source}` `{text}` `{session}`, so
agent-comms can replace ssh later without code changes.

The role comes from the hostname when it isn't set (bella/parabellum → bella, else avante).

## Events

| event | default | source | fires when |
|---|---|---|---|
| `ci_failed` | **on** | `gh run list` (each branch in `branches`, default main) + `gh pr list` (his open PRs) on `[github] repos` | a workflow's last completed run on main failed, or a check on one of his open PRs failed |
| `approval_waiting` | **on** | `synaps daemon sessions --json` (`awaiting_input`) | a session has waited on an approval for 30 s (`after_s`) |
| `session_error` | **on** | the listing + the session's journal + the daemon log | a turn ended without a reply: an error, or an abort |
| `unit_failed` | **on** | `systemctl [--user] list-units --state=failed` | a unit failed. Noise filter (`ignore`, globs): dunst, xdg portals, `run-*`, `probe-*`, and every transient (systemd-run) unit |
| `disk_low` | **on** | statvfs on `/` and `/home` (one line per filesystem) | under 10 % OR under 20 GB free |
| `backup_failed` | **on** | `systemctl --user show jawz-backup-local` + its log | the last run exited non-zero, or logged ERROR lines (avante; nothing on a machine without the unit) |
| `pr_opened` | off | `gh pr list` | a new open PR in a watched repo (any author) |
| `pr_merged` | off | `gh pr list --state merged` | a PR merged |
| `review_requested` | off | `gh search prs --review-requested=@me` | someone asks for his review |
| `long_turn` | off | the listing (`busy`, `state_since`) + her avatar | a turn of 10 min or more (`min_s`) finished while he's away (her avatar says away, or idle for 5 min) |
| `release` | off | `gh release list` | a release published |
| `battery_low` | off | `/sys/class/power_supply/BAT*` | discharging below 15 % |
| `gpu_hot` | off | `nvidia-smi` | 85 °C or more |
| `calendar` | off | `~/Jawz/data/calendar.db` (read-only) | a timed event starts within 10 min |

A one-shot event (opened, merged, release, a turn's end, calendar) learns what already exists on its first
pass and fires only for what's new. A condition (CI failing, unit failed, disk low, approval waiting, backup)
fires when it starts holding, and again only after it cleared. capri-syn's main CI, which fails on every
push while its secret is missing, therefore fires once.

### Sessions: what 0.11.1 exposes

`daemon sessions --json` carries `lifecycle`, `busy`, `state_since`, `awaiting_input` and `messages`, but no
error field. A turn's end is a `busy` → idle edge (or a new `state_since` while idle, for a turn shorter than
one poll). Then the watcher reads that session's journal (`~/.synaps-cli/sessions/<journal_id>.json`,
read-only, only the last message). After a failure the daemon repairs the history, so a turn that succeeded
ends on an assistant text message and one that failed or was aborted ends on the prompt or a tool result. The
daemon log within 20 s of the end supplies the reason when there is one ("HTTP 503", "turn budget:
wall_clock"). Without one, the line says "error or abort". Parking and compaction aren't turns. A journal
that isn't saved yet is retried on the next pass. Sessions are never attached to.

Her own session (`ignore_sessions = ["capri"]`, by name, id or id prefix) is never reported on any machine,
so her answering an event can't make another event.

## Safeguards

- **Dedupe:** once per state change, persisted in `~/.local/state/capri-watch/state.json`, so a restart
  doesn't repeat itself. A source that can't be reached (gh down, a daemon restarting) clears nothing.
- **Bundling:** the first event opens a `bundle_s` window (15 s). Everything that arrives inside it goes
  out as one digest (`--source watch:digest`, a header line plus up to 10 event lines).
- **Wake cap:** `max_per_hour = 6` deliveries per rolling hour. The rest wait and arrive together.
- **Her DND:** while it's on (DND 1 HOUR in her menu), events wait. The watcher reads it from the avatar's
  control socket (`brain state`, the socket `capri-ctl` uses). On avante that check runs on bella over ssh.
- **Music:** `music = "defer"` waits until it stops, `"drop"` drops, `"ignore"` sends anyway. "Playing" is
  her music mode (MPRIS, `[music] ignore` honoured). Without her avatar running it falls back to `playerctl`.
- **Quiet hours:** `quiet_hours = "23:00-08:00"` holds events. Off by default.
- **Stale:** anything held longer than `stale_s` (1 h) is dropped. A condition that still holds is found again
  and queued fresh, and a late line says how late it is (`[2.0 h ago]`).
- **Cleared before delivery:** an approval answered inside the window, or a unit restarted while music played,
  never reaches her.
- **Lines:** built only from structured fields (repo, PR number and title, unit name, numbers). Titles are cut
  at 50 characters and a line at 400. Control characters are stripped and anything shaped like a token is
  redacted. No log text, no environment.
- **Delivery failure** (bella asleep, ssh down): kept, retried every 60 s, dropped when stale.

## Running it

```
capri-watch --dry-run --once -v   # one pass: what each source saw, and what would be sent (or why it would wait)
capri-watch --dry-run -v          # the loop, printing instead of sending; never writes state
capri-watch --print-config        # the merged config
python3 tools/capri-watch/test_capri_watch.py   # unit tests, fake sources only
```

Config: `~/.config/synaps-avatar/watch.toml`. `tools/capri-watch/watch.toml` documents every key with its
default, and a test keeps the two identical. Unknown keys are an error. Turn an event on with
`[event.<name>] enabled = true`.

Unit: `tools/systemd/capri-watch.service` (install steps at its top). `Restart=on-failure`, with at most 5
starts in 10 min.
