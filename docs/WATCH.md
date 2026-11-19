# capri-watch: events that wake Capri

Capri reacts to her own session. `tools/capri-watch` gives her things to react to: a failing CI run, a session
stuck on an approval, a failed unit, a nearly full disk. It's one small stdlib-Python poller per machine
(a systemd `--user` unit; Linux only: `install.sh --watch`). For each new event it writes ONE short line
into her session:

```
[watch] ci_failed@desk: capri-syn main: ci failed on 09b138d (https://github.com/you/capri-syn/actions/runs/43671709347)
```

She answers like she answers anything else, in a line, and offers the obvious next step. Her persona has a
paragraph for `[watch]` lines (`packs/capri/persona.md`): one line, no acting on anything risky without your yes.
With voice on, she speaks her replies to events too (`[voice] speak_unprompted`, never over music:
[VOICE.md](VOICE.md#which-replies-are-spoken)).

## Machines

| | `role = "local"` (the default): the machine Capri lives on | `role = "remote"`: any other machine |
|---|---|---|
| watches | GitHub (`[github] repos`), this daemon, units, disk (+ battery, GPU, calendar, a backup unit when set) | this daemon (`synaps`, its CLI name here), units, disk, a backup unit when set |
| delivers | `synaps send --session capri --source watch:<event> -- <line>` | the same command on her machine (`[deliver] ssh_host`, required), over ssh (BatchMode, ConnectTimeout 5) |

The role and the transport come from `watch.toml` only. The hostname never picks them. It's only the
default label in event lines (`host = ""`: `ci_failed@<short hostname>`); set `host` for a nicer name.
With no file at all, the watcher runs as `local`: it delivers on this machine and watches no GitHub repos
until you list some.

Two settings stop it with an error instead of delivering to the wrong place:

- A role from a copy of `watch.toml` from before v1: `role = ""`, where it meant "pick by hostname", or one
  of the two machine names it took then. The error says the file predates v1. Set `role = "local"`, or
  `role = "remote"` with `[deliver] ssh_host`.
- `[deliver] ssh_host` set while delivery is local (no `role = "remote"`): that's a forgotten role. On her
  machine, leave `ssh_host` empty.

Its first log line names the events it really watches: `ci_failed` without `[github] repos`, `backup_failed`
without a `unit` and `calendar` without a `db` are listed as on but not configured. With GitHub off (a
`remote` machine's default, or `[github] enabled = false`) its events are simply not listed.

`install.sh --watch` checks the file with `capri-watch --print-config` and warns if it won't load.

The ssh transport ships the argv base64-encoded in a Python script on stdin (`ssh <ssh_host> python3 -`). No
remote shell parses the line, so quotes, `$( )` and the like in a PR title arrive unchanged, whatever the
login shell (fish too). `[deliver] transport = "command"` takes any argv with `{source}` `{text}` `{session}`,
so another channel can replace ssh without code changes.

Two machines, for example: her desktop runs it with the defaults plus your repos,

```toml
# her machine: ~/.config/synaps-avatar/watch.toml
host = "desk"
[github]
repos = ["you/capri-syn", "you/other-repo"]
```

and a build box sends its own events to her over ssh:

```toml
# the other machine: ~/.config/synaps-avatar/watch.toml
role = "remote"
host = "buildbox"
[deliver]
ssh_host = "desk"          # an ssh host or ~/.ssh/config alias that reaches her machine without a prompt
```

## Events

| event | default | source | fires when |
|---|---|---|---|
| `ci_failed` | **on** | `gh run list` (each branch in `branches`, default main) + `gh pr list` (your open PRs) on `[github] repos` | a workflow's last completed run on main failed, or a check on one of your open PRs failed |
| `approval_waiting` | **on** | `synaps daemon sessions --json` (`awaiting_input`) | a session has waited on an approval for 30 s (`after_s`) |
| `session_error` | **on** | the listing + the session's journal + the daemon log | a turn ended without a reply: an error, or an abort |
| `unit_failed` | **on** | `systemctl [--user] list-units --state=failed` | a unit failed. Noise filter (`ignore`, globs): dunst, xdg portals, `run-*`, `probe-*`, and every transient (systemd-run) unit |
| `disk_low` | **on** | statvfs on `/` and `/home` (one line per filesystem) | under 10 % OR under 20 GB free |
| `backup_failed` | **on**, once `unit` is set | `systemctl --user show <unit>` + its `log` (optional) | the last run exited non-zero, or logged ERROR lines since the last `log_start` line (nothing until a unit is named, or on a machine without it) |
| `pr_opened` | off | `gh pr list` | a new open PR in a watched repo (any author) |
| `pr_merged` | off | `gh pr list --state merged` | a PR merged |
| `review_requested` | off | `gh search prs --review-requested=@me` | someone asks for your review |
| `long_turn` | off | the listing (`busy`, `state_since`) + her avatar | a turn of 10 min or more (`min_s`) finished while you're away (her avatar says away, or idle for 5 min) |
| `release` | off | `gh release list` | a release published |
| `battery_low` | off | `/sys/class/power_supply/BAT*` | discharging below 15 % |
| `gpu_hot` | off | `nvidia-smi` | 85 °C or more |
| `calendar` | off | `db`: a SQLite calendar, read-only (below) | a timed event starts within 10 min |

A one-shot event (opened, merged, release, a turn's end, calendar) learns what already exists on its first
pass and fires only for what's new. A condition (CI failing, unit failed, disk low, approval waiting, backup)
fires when it starts holding, and again only after it cleared. A CI that fails on push after push
therefore fires once, and again only after a green run.

The calendar is read from two tables, dates and times in UTC: `events` (`id`, `title`, `date`, `time`,
`recurring_pattern`) for one-off events and `event_instances` (`event_id`, `override_title`,
`occurrence_date`, `occurrence_time`, `status = 'active'`) for recurring ones.

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
  control socket (`brain state`, the socket `avatarctl` uses). On a `remote` machine that check runs on her
  machine over ssh.
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
- **Delivery failure** (her machine asleep, ssh down): kept, retried every 60 s, dropped when stale.

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
