# She on Windows (in progress)

Windows 11, x86_64-pc-windows-msvc. Everything Windows-only is in `src/platform/windows/`; GPUI's
Windows crate is vendored with small patches (`vendor/gpui-pre-windows/PATCHES.md`). CI builds,
tests and lints it on every push (the `windows` job). What CI can't see (a desktop, his input,
monitors) was checked in the capri-win VM on 2026-11-18: see "The VM day" below. Live audio (item
13) is still his machine's.

## Build and run

```powershell
cargo build --release --bin synaps-avatar
.\target\release\synaps-avatar.exe      # a console build: her log in the terminal
```

`capri-ctl` and the brain talk to her over the same local socket (AF_UNIX, Windows 10 1803+).

Her files (`platform::base_dir`; `$XDG_*_HOME` still wins, as on Linux):

| | where |
|---|---|
| settings | `%APPDATA%\synaps-avatar\settings.toml` (roams with him) |
| her pack (default `--pack`) | `%LOCALAPPDATA%\synaps-avatar\packs\capri` |
| state | `%LOCALAPPDATA%\synaps-avatar\state.json`, `sessions.json` |
| her log, without a console | `%LOCALAPPDATA%\synaps-avatar\synaps-avatar.log` (appended; over 4 MB at start, kept as `.old.log`) |

## The installer

`capri-syn-setup-<version>-<build>.exe` (`packaging/windows/synaps-avatar.iss`, Inno Setup 6), released
as `capri-syn-setup-x86_64.exe`. CI's `windows-installer` job builds it on every PR and push to main
and uploads it as an artifact (`capri-syn-setup-<sha>`, kept 14 days). Locally, on a machine with
Rust and Inno Setup: `.\packaging\windows\build.ps1`. In Apps & features she is
`capri-syn`; the AppId is the one every `synaps-avatar` install had, so an older install upgrades in
place. Her binary and folders keep the name `synaps-avatar`.

Double-clicked on a PC with nothing installed, it leaves a working Capri. After her files it runs the
install steps install.ps1 runs too: one copy of them, `packaging/windows/steps.ps1`, installed with
her in `{app}\steps` along with what they read (the manifest, `make-pack`, the pins, her moods and
persona; no art).

- **Per user, no admin**: she goes to `%LOCALAPPDATA%\Programs\synaps-avatar`.
- **Her brain** (page "Her brain", shown only when it's needed): Synaps is looked for as install.ps1
  looks (`synaps` on PATH, else `~\.cargo\bin\synaps.exe`, at least the manifest's `synaps_min`).
  Missing or too old: "Install Synaps (her brain, open source)", checked, runs the latest release's
  own `synaps-installer.ps1`, checked against GitHub's sha256 of the asset (the manifest's pin when
  GitHub's API doesn't answer).
- **Her pack** (page "Her pack"): "Download her art (CapriCore + lachy's desktop art, ~100 MB)", the
  default; "Use a pack folder I already have" (copied; a folder without `sprites.json` is refused);
  or "Skip, I'll add one later". The download is the one install.ps1 does: CPython's embeddable
  package and pip (pinned, SHA-256 checked) in a cache folder, fonttools into it, CapriCore at
  `capricore_commit` from codeload, then `tools/make-pack --lachy auto`, every file checked against
  `tools/*.sha256`, and her `--check` on the result. Nothing of his is used or changed: not his
  Python, not his PATH. lachy's PolyForm Noncommercial notices (`Required Notice:` lines) are shown
  on the finish page. Her art is never in the installer, and never mochi's (`--with-local-art` is
  install.ps1's alone).
- **Upgrades:** a pack the steps made (it holds `.capri-syn-pack`) is replaced whole, or left as it
  is when it was built from the same inputs; any other pack (his) is moved to
  `capri.pre-install-<date>`, never deleted.
- **The daemon**: `synaps daemon --detach` now, and a Run value for sign-in through
  `conhost --headless` (no console window; see the install.ps1 note below).
- **Her voice** (page "Her voice"): "Talk to her", on when Windows has a microphone (off and greyed,
  "no microphone found", when it doesn't); a microphone picker over Windows' capture endpoints
  ("Windows default" first); "Hey Capri" and "Read replies aloud", both on. Ticked, it downloads
  the voice installer below (the latest release's, its `.sha256` checked) and runs it, fetches
  the models (`voice-models.ps1`, each pinned), then saves his choices with her own settings writer
  (`synaps-avatar --set`: `[voice] enabled`, `wake_word`, `speak_unprompted`, `mic`). Her voice
  itself (`voice`, `speed`, `pace`, `flow`) is never written: she keeps her built-in default, so a
  new voice is a change to her alone. install.ps1's `--voice` runs the same step.
- **Progress**: a page of its own while the steps run, with each step's lines (downloads in MB).
- **A step that fails doesn't fail Setup.** Offline, a pin that doesn't match, a build that stops:
  she is installed without it, and the finish page says what happened in plain words, with the
  one command that tries it again (`powershell -NoProfile -ExecutionPolicy Bypass -File
  "…\steps\steps.ps1" pack`). Unattended, the setup log says the same.
- **The finish page**: "Start her now", and with no login yet "Sign in to Claude" (`steps.ps1 login`:
  Synaps' `synaps login --provider claude`, no window: his browser opens). Synaps 0.12 opens the
  page with `cmd /c start URL`, and cmd cuts the URL at its first `&` (no `client_id`: claude.ai
  can't sign him in), so the step opens the whole URL from Synaps' stderr itself (a broken tab from
  Synaps may open first, until Synaps quotes it). Its stdin is held open until `auth.json` changes:
  0.12 gives up at once on a closed one. Until her first question her tag reads ONLINE on Windows
  (the daemon's environment can't be read there, so no login on file is "unknown", `creds.rs`);
  her first question without a login turns it to NO BRAIN YET.
- **Her exe is the windowed build** (`cargo rustc … /SUBSYSTEM:WINDOWS`, her binary only): no
  console window from the Start menu or at sign-in. Without a console she logs to the file above.
  `--check` from a terminal therefore prints nothing there: read the log, or use a cargo build.
- **Her AUMID** `Synaps.Avatar`: registered under `HKCU\Software\Classes\AppUserModelId` with
  `DisplayName` and `IconUri`, and set on the Start-menu shortcut, so toasts show under her name
  and icon. The icon is capri-syn's, `assets/icon/capri-syn.ico` (256 to 16 px) and its 256 px PNG,
  used as they are (`assets/icon/NOTICE`): the setup's own icon, Apps & features, the shortcut, toasts.
- **Autostart** (a task, checked): the `HKCU\…\Run` value.
- **Uninstall** (Settings → Apps, or `unins000.exe /VERYSILENT`): stops her, then removes her exe,
  the steps, the shortcut, the AUMID key, her Run value and the daemon's, the pack the steps made,
  her voice, her log, state and settings, the download cache, and her folders. A pack he put there
  himself (and one moved aside) stays. Synaps: when Setup installed it, the wizard asks whether to
  remove it too (his login and sessions in `~\.synaps-cli` stay either way); unattended, it stays.
- **Upgrades** close her (Restart Manager) and don't restart her.
- **Unsigned:** downloaded with a browser (Mark of the Web), SmartScreen stops it ("Windows
  protected your PC"; More info → Run anyway). Defender lets it be. Both installers, in the VM.

Unattended:

```
capri-syn-setup-….exe /VERYSILENT /SUPPRESSMSGBOXES [/SYNAPS=install|skip]
                      [/PACK=download|skip|"C:\packs\capri"] [/VOICE=on|off] [/MIC="name"]
                      [/VOICEFROM="C:\...\capri-syn-voice-setup.exe" or a URL] [/TASKS=autostart]
```

The defaults are the wizard's: Synaps if it's needed, her art downloaded, autostart, her voice when
Windows has a microphone and there's a voice to install. Her voice comes from `/VOICEFROM` (a path
or a URL; a `.sha256` beside it is checked), else a `capri-syn-voice-setup*.exe` beside the setup
(CI downloads both artifacts of a build into one folder: that's how the voice job gets the voice CI
built), else the latest release's `capri-syn-voice-setup-x86_64.exe`, checked against the
`capri-syn-voice-setup-x86_64.exe.sha256` release.yml uploads beside it. With none of those (no
release yet), the Voice page says "Her voice comes with her first release" and stays off and greyed;
nothing fails.
install.ps1 runs it with `/SYNAPS=skip /PACK=skip /VOICE=off /BY=install.ps1`: Synaps, her pack, the
daemon and her voice are its own steps (the same steps.ps1), so nothing is downloaded twice, and her
settings and its cache stay its own to remove.

CI:
- `windows-installer` (`test-install.ps1`): offline (a hosts file that sends GitHub, python.org and
  PyPI nowhere: Setup exits 0, she's installed without Synaps or a pack, the log says why and how to
  retry), a CapriCore that doesn't match its pins (refused, deleted), `/PACK=download` over a pack of
  his (moved aside, kept; hers built, `--check` passes; lachy's notices), again (up to date), a pack
  folder over the one Setup built (replaced whole), the files, keys, shortcut AUMID, the windowed
  exe, her size in Settings → Apps, and that nothing is left after. `test-login.ps1`: `steps.ps1
  login` against a stand-in `synaps.exe` (compiled there): the whole URL, stdin held open, a failed
  login in its own words.
- `windows-setup-e2e` (`test-setup-e2e.ps1`, a clean runner): the defaults, unattended; `synaps
  --version`, the daemon (protocol 3, and its Run value), her pack, autostart, the Start menu; she
  starts, attaches (her control socket's `state`: `session.live`, the tag not OFFLINE), and answers
  for 60 s of clicks on her (every `state` from her UI thread, `WM_NULL` to her windows, her ticks,
  no Application Hang event); no microphone, so no voice files; the uninstall leaves nothing of hers.
- `windows-setup-voice` (`test-setup-voice.ps1`, a clean runner): `/VOICE=on`, the voice installer
  CI built beside the setup (as a release's comes from GitHub); the bundle and its selftest, the models, `settings.toml` (`[voice] enabled`,
  never her voice itself), her log's `voice: sidecar started`, and an uninstall that leaves no voice.
- `windows-setup-wizard` (`test-setup-wizard.ps1`, a clean runner): the wizard clicked through with
  its defaults (UI Automation), a screenshot of every page (`tools/shoot-win.ps1`), her on the desktop
  after "Start her now", and the uninstall's questions. The shots are the job's artifact.

## Her voice: a second installer

`capri-syn-voice-setup-….exe` (`packaging/windows/synaps-avatar-voice.iss`, built by
`build-voice.ps1`; CI's `windows-voice-installer` job, an artifact; released as
`capri-syn-voice-setup-x86_64.exe`; `capri-syn voice` in Apps & features, its AppId unchanged).
**Its own download, not a
component of hers:** an Inno component sits inside `setup.exe` even unchecked, so every download of
her would carry the 143 MB bundle (~45 MB compressed). As a second installer, her default download
stays ~9 MB (CI fails hers past 20 MB), and only he who wants voice gets the rest.

- **Per user, no admin**: the bundle goes to `voice\` in her install folder (read from her installer's
  record, else its default), where she looks for it.
- **The models are never in it.** The "models" task (checked) runs `voice-models.ps1`: Whisper small.en and
  base.en, Kokoro int8 and its voices, Silero, the wake word's two feature models (~320 MB, each SHA-256
  pinned; "hey capri" itself comes with the bundle) into `%LOCALAPPDATA%\capri-voice\models`. A cut
  download resumes (`curl.exe -C -`); a wrong hash is deleted and the run fails. A Start-menu entry,
  "synaps-avatar voice models", runs it again.
- She starts the sidecar once the models are there (her log says so until then).
- **Uninstall** stops the sidecar (only the `python.exe` in that folder), removes the bundle, Python's
  caches, and `%LOCALAPPDATA%\capri-voice` (models, log, link, media state).
- CI installs, checks and uninstalls it unattended without the models (`test-voice-install.ps1`: the
  files, the selftest run by the installed interpreter, the folder beside her in both cases, nothing
  left), and runs the model fetch on its small files (`test-voice-models.ps1`: resume, a bad hash, a
  kept file).

Unattended: `capri-syn-voice-setup-….exe /VERYSILENT /SUPPRESSMSGBOXES [/MERGETASKS="!models"]`.

## install.ps1: one script (Synaps too)

`irm https://raw.githubusercontent.com/HaseebKhalid1507/capri-syn/main/install.ps1 | iex` runs
`install.sh`'s seven steps the Windows way (`docs/INSTALL.md`):
1. Synaps through its own `synaps-installer.ps1`, sha256-checked;
2. her installer from the latest release, or a windowed source build (rustup, and Visual Studio's
   C++ build tools via winget behind a UAC prompt, asked first);
3. her pack through `tools/make-pack` in CPython's embeddable package (in her cache folder: no
   Python of his needed or touched), the same steps.ps1 her installer runs;
4. `synaps daemon --detach` now, and a Run value for sign-in (through `conhost --headless`:
   `synaps.exe` is a console program, and run as is it opened a terminal at sign-in; see the
   note below);
5. Win+Alt+C (she registers it herself);
6. a Run value for her (default on);
7. `--voice`: her voice installer and the models.

Per user. A receipt in `%LOCALAPPDATA%\synaps-avatar` drives `--uninstall`.

**`conhost --headless` and security software.** `--headless` is the switch Windows' pseudo-console
(ConPTY) starts conhost with, not a documented interface, and starting a program hidden behind it is
also a known malware pattern: an EDR (Defender for Endpoint, CrowdStrike, Elastic's rules) may flag
or block the Run value at sign-in. If it does, the daemon isn't running after sign-in and she has no
brain until `synaps daemon --detach` runs. Nothing documented is as cheap: `powershell -WindowStyle
Hidden` and `wscript` flash a window or are flagged as often, and a scheduled task runs a console
program in a visible console. The clean fix is a tiny windowed launcher (her own exe, which already
starts the voice sidecar with `CREATE_NO_WINDOW`, could start the daemon the same way): a follow-up,
as it needs a desktop to check that nothing shows. To accept the flash instead: a Run value of his
own (any other name) with `"…\synaps.exe" daemon --detach`, and `synaps-daemon` deleted. A rerun
leaves his alone and adds none (an edit to `synaps-daemon` itself would be put back).

CI (`windows-installer`, every PR) runs it end to end on windows-latest with the installer that
job built (`installer/test-windows.ps1`):
- a dry run (also fetched with `irm`) that changes nothing;
- `--yes --no-autostart`;
- `synaps --version`, her `--check`, the daemon and its Run value;
- a rerun (up to date);
- `--uninstall`, which leaves nothing of hers.

What CI can't show is item 15 below.

## What she does here

| | how | status |
|---|---|---|
| her body, the Line, the reader, the intro | GPUI pop-ups made bare topmost tool windows: no taskbar button, not in Alt+Tab, no DWM frame or rounding | P0 [VM] |
| click-through | layered + `WS_EX_TRANSPARENT`, wanted by a 10 ms pointer poller over her input region, set by her UI thread on the poller's wake-up (the poller never waits on it) | P0 [VM]; [M] the 2026-11-19 hang |
| on top | Windows sometimes leaves a topmost window under plain ones with its `WS_EX_TOPMOST` bit still set (on Haseeb's box after he opened an app from Explorer; the second taskbar too). The poller checks her place every 250 ms and when a window comes forward; under a plain window, her UI thread puts her windows back on top (`z: re-topmost (was below …)` in her log). Not while a fullscreen app is in front; never over another topmost window (his picture-in-picture, an always-on-top app). While her puts don't hold, the checks slow down to one every 8 s until one finds her on top. A window still over her right after a put is above the topmost band (Task Manager's "always on top", the touch keyboard): it and every plain window over it are left out for 60 s (`z: can't get above …`, once a window) | [M] 2026-11-19 |
| fullscreen | a window covering its monitor (not a tool window, not the shell's: Start, Search, Alt+Tab and the flyouts are none, nor his focused window). The shell's UI: a window of the shell's processes (explorer, StartMenuExperienceHost, SearchHost, ShellExperienceHost, ShellHost, TextInputHost, LockApp) of a known class or any tool window; another process's window is an app whatever its class (a full-screen UWP app's `Windows.UI.Core.CoreWindow`) | [M] 2026-11-19 |
| perching | `EnumWindows`, DWM frame bounds, WinEvent hooks for moves, shows, hides, minimise, cloak, foreground | P0 [VM] |
| virtual desktops | windows on another desktop are cloaked (left out); `IVirtualDesktopManager` confirms; a switch (the shell's registry value) is `Hint::Desktop` | [VM] item 8 (after a fix) |
| monitors | `EnumDisplayMonitors`; `WM_DISPLAYCHANGE`, `WM_DPICHANGED` and a work-area change (taskbar moved) are `Hint::Layout`, logged (`windows: layout: …`) | [VM] item 4 |
| DPI | per-monitor aware (v2), set before GPUI starts. Her windows keep the primary monitor's scale everywhere (PW-5): her art is laid out at one scale. A display change reaches GPUI from the message loop (PW-6) | [VM] item 3 (100 % and 150 %; PW-6 fixed a crash) |
| effect layers | the bolt and anything reaching outside her window: a plain Win32 window per layer, solid colour, shaped by a region, click-through, never activated (`fx.rs`). The rest is painted in her window | [VM] item 9 |
| her keys | `RegisterHotKey`: **Win+Alt+C** summons (a tap), **Win+Alt+Ctrl+C** toggles talk. Win alone is the shell's | [VM] item 5 |
| the Line's keyboard | see below | [VM] items 2, 6 |
| music | SMTC (the media flyout's sessions): playing or quiet, and whether every playing session is a video. Edge reports no playback type: a video in Edge counts as music | [VM] item 10 |
| notifications | toasts, see below | [VM] item 11 (after a fix) |
| lock, sleep, idle | WTS session notifications, power broadcasts, `GetLastInputInfo` | P0 [VM] |
| voice: the link | her end of the sidecar's link: loopback TCP after a two-token handshake (`endpoint.json` in `%LOCALAPPDATA%\capri-voice`); Linux keeps the Unix socket | CI (both ends, Rust and Python) |
| voice: the sidecar | `tools/capri_voice_win.py`: PortAudio (WASAPI) capture and playback, SMTC pause, session-volume duck with Linux's crash repair, lock, calls (the mic privacy store), DND. The voice installer (below) puts its bundle (embeddable CPython 3.13 + wheels) beside her: `docs/design/windows-voice.md`. The lock is the session's own state (WTS): Windows 11's lock screen runs on the Default desktop | CI; [VM] item 14 (lock: after a fix); live audio: item 13 (his machine) |
| voice: she runs it | while `[voice] enabled`: `python.exe capri-voice` from `voice\` beside her exe (or `CAPRI_VOICE_BUNDLE`), no console window, in a job object that kills it with her (however she ends); restarted after 1, 2, 4 … 60 s; its log `%LOCALAPPDATA%\capri-voice\capri-voice.log`. Not before the models are there. `CAPRI_VOICE_SPAWN=0`: he runs it himself. `[voice] enabled` is read at start and on `capri-ctl settings reload` | CI (job, backoff, bundle lookup); [VM] item 14 |
| voice: connecting | on a thread (a stale endpoint can't freeze her: up to ~2 s); Linux connects inline, as before | CI (a silent listener) |
| hold-to-talk | opt-in, `[voice] hold_to_talk = true`: the summon key's press is `talk press`; her key thread then reads the key every 25 ms (`GetAsyncKeyState`, as OBS does) and its release is `talk release`. No keyboard hook. Off: the summon key is a tap, as before | CI (the state machine); [VM] item 14 |

## The Line's keyboard

Windows hands a process the foreground only right after the user's own input to it. So
(`docs/design/windows-line-keyboard.md`):

- her key and a click on her take the keyboard at once;
- any other opening (a summon from the brain, `capri-ctl`, voice) shows the Line **without** the
  keyboard, and her tag says `WIN+ALT+C TO TYPE`; his key or a click on the Line then takes it;
- she never takes the keyboard from the window he works in, never flashes, never injects input.

## Toasts

Without the installer she registers her AppUserModelID herself, once, under
`HKCU\Software\Classes\AppUserModelId\Synaps.Avatar` (`DisplayName`): no admin, no shortcut.
Windows then shows her toasts in the Action Center under that name, with the image as the logo.
The installer adds the icon (`IconUri`) and the shortcut's AUMID. The id has to stay
`Synaps.Avatar` (`notifications.rs` and `synaps-avatar.iss` share it).

**A click on a toast does nothing**, as on Linux (her notifications there carry no action). Making
it open the Line was left out: it needs a COM activator, a `ToastActivatorCLSID` on the shortcut
(Inno: `AppUserModelToastActivatorCLSID`), the CLSID's `LocalServer32` under HKCU, and in her process
a class factory and `INotificationActivationCallback` registered at start (`CoRegisterClassObject`),
plus a cold start when she isn't running. About half a day plus a VM check; not cheap without a
desktop to try it on.

## Shots on a runner

`.github/workflows/shots.yml` shoots her on a GitHub Windows runner: Actions → shots → Run workflow
(a ref and the scene sets), or the `shots` label on a PR (its head; `shots:settings` or
`shots:header` for one set). It builds the release binary of that ref, assembles her pack with
`tools/make-pack` (CapriCore v1.0.4 + lachy, `--lachy auto`; never mochi), starts the fake daemon
(`tools/fakedaemon.py`: the daemon's named pipe, its control on 127.0.0.1:47811) and, for voice
takes, the fake sidecar (`tools/fakevoice.py`, on the real link), then runs `tools/shots/scenes.ps1`
at 1920x1080 and 100%, and a few takes again at 150%. She is driven through her control socket;
keys and clicks only where it can't (the Line's keyboard, a menu row). Each shot is cropped to her
windows, with her `state` beside it; the lot is an artifact kept 3 days (public meanwhile, like
the run). No GPU there: GPUI draws with WARP, fine for layout, not for colour or timing. The job
fails on any problem in a take (she never loaded or exited, OFFLINE, a dialog of hers, a shot
missed), and with no shots at all; the shots it did get are uploaded anyway.

`tools/shoot-win.ps1` is the reusable part (dot-source it): the display pinned
(`Set-ShotDisplay -Width 1920 -Height 1080 -Scale 150`, live, no sign-out), a process's or a
title's windows shot cropped (`Save-WindowShot -Path x.png -ProcessId $p.Id` or `-Title 'Setup*'`),
and SendInput keys and clicks (`Send-ShotKeys 'ctrl+comma'`, `Send-ShotClick`). Never the whole
desktop. `Set-ShotDisplay` and `Clear-ShotDesktop` change the machine (scaling, wallpaper, every
dialog closed, a cover window left up), so they refuse to run off a GitHub-hosted runner
(`RUNNER_ENVIRONMENT`) unless `SHOOT_WIN_ANYWAY` is set.

## The VM day (2026-11-18)

The capri-win VM on bella: Windows 11 Enterprise Evaluation 26H2 (build 26300), QEMU/KVM, two
VirtIO GPU heads (1280×800 and 1920×1200), D3D on WARP (software), an HDA sound card with a silent
input, 8 GB. Builds: main's CI installers (3713ff0) and this branch built in the VM by
`install.ps1`'s source path. Driven from inside his session (`SendInput`, the QEMU keyboard for his
keys and UAC); screenshots on avante in `~/Jawz/workspace/synaps-avatar/winvm/<item>/`, not in the
repo. **[VM]**: true of the VM (software rendering, no real mic), to check again on his machine.

| # | what | result |
|---|---|---|
| 1 | PW-1/2/3: no frame; a click on her doesn't take focus; no stale frame or extra CPU while dragged | **pass**. No frame, border or shadow. A drag and a headpat leave the foreground where it was. While dragged her renders follow her ragdoll animation, not the moves (240 moves: 373 renders in 7 s; 78 moves: 298), no stale frame after. CPU [VM]: 7 % of a core idle, 13 % dragged (her animation on WARP) |
| 2 | PW-4: no stray Alt when the Line opens | **pass**. A low-level keyboard hook while the Line opened on her key and on a click on her: his keys only, nothing injected |
| 3 | PW-5 + DPI: 100 % and 150 % | **pass after a fix**. Her window stays 500×695 and her body the same size on both monitors, crisp. Her first move onto the second monitor **aborted her** (`RefCell already borrowed`, `async_context.rs:178`): fixed by **PW-6** (`vendor/gpui-pre-windows/PATCHES.md`) |
| 4 | hotplug, the taskbar | **pass**. `windows: layout: monitors changed` and `… the work area changed` (new log lines) on a monitor detached with her on it (`her monitor went away: to 848,78`), attached again, a DPI change, the taskbar's auto-hide on and off (Windows 11's taskbar has no other edge) |
| 5 | her keys | **pass**. Both register (none taken); Win+Alt+C opens the Line with the keyboard; Win+Alt+Ctrl+C toggles talk (`toggle: listening`) |
| 6 | the Line from `capri-ctl` | **pass**. `talk tap`: the Line opens parked, `ONLINE · WIN+ALT+C TO TYPE` on her tag and on the Line, the terminal keeps the keyboard; his Win+Alt+C, or his click on the Line, takes it; Esc gives it back |
| 7 | the contract suite | **11 of 13**, `keyboard` and `body-and-teleport-fx` among them. `perch-space` and `monitors-and-areas` need the driver to make a second monitor and a panel that reserves space (`split_monitors`, `dock_top`), not written for Windows (follow-up); items 3 and 4 checked the same by hand |
| 8 | virtual desktops | **pass after a fix**. With one desktop Windows 11 writes no `CurrentVirtualDesktop` until the first switch, so the watcher never started (`no virtual desktop in the registry`); it now watches both keys (Windows 11's and the per-session one) and logs which holds the current desktop. A window on desktop 2 is refused as a perch (cloaked: `not visible`); a switch logs `windows: virtual desktop switch` and her perch on desktop 1 is left (`left with the workspace`) |
| 9 | effect layers | **pass**. The launch and teleport bolts draw outside her window (captured mid-flight); the layer windows are layered, click-through (`WS_EX_TRANSPARENT`) and `WS_EX_NOACTIVATE`; under/over her: the suite |
| 10 | SMTC | **pass, one gap**. Media Player with a song: `smtc playing (Microsoft.ZuneMusic…=Playing)`, LISTENING, her headphones on; with a video: `(video)`. **Edge reports no playback type** for a video (a file or a page's `<video>`): to her it is music. A Discord call: not tried (no account in the VM) |
| 11 | toasts | **pass after a fix**. `CAPRI // NEEDS YOU` under "synaps-avatar", the key there. It came without her face: `fs::canonicalize`'s `\\?\` path made a `file://///?/…` URI, which the shell drops silently |
| 12 | the installer, interactively | **pass after a fix**. SmartScreen first (unsigned). The pack page: a non-pack refused, Browse (a folder picker), empty allowed; the shortcut starts her with no console; a toast shows her icon and face; autostart starts her 3.6 s after sign-in; uninstall from Settings → Apps, with her running, leaves none of hers (his settings stay). **Settings → Apps said 44.6 GB**: Inno sized the pack's line as `\*` (the root of C:) before the page had a value; fixed, and CI checks the size |
| 13 | voice, live audio | not in the VM: his machine (mic, ears, Whisper and Kokoro speed). The bundle's onnxruntime loaded (the wake word ran), but on a VM with Build Tools, so whether a clean Windows wants the VC++ runtime is still open |
| 14 | voice, her side | **pass after a fix**. The voice installer (SmartScreen too) and its models window (~358 MB in ~40 s); `voice: sidecar started`, killed with her; `[voice] enabled = false` + `settings reload`: `sidecar stopped (voice off)`, on: started again; the voice uninstaller under her removes everything, she backs off (1, 2, 4, 8 s, `no models`); `hold_to_talk`: a hold listens and its release sends (`key up: transcribing`), a tap opens the Line, a release with Notepad in front ends it and Notepad keeps the keyboard, Win+L mid-hold ends it; the sidecar listens on 127.0.0.1 only: no firewall prompt. **The sidecar missed Windows 11's lock screen** (its curtain runs on the Default desktop): the wake word's mic stayed open on it for 14 s. It now asks the session (WTS) |
| 15 | install.ps1, interactively | **pass after fixes**, below |

### install.ps1 in the VM

`irm …/install.ps1 | iex` in Windows PowerShell 5.1, no Python, no release (so a source build), on
bella's line:

- Synaps 0.12.0 through its own installer; the daemon up.
- The source path: the Build Tools prompt, then UAC (Visual Studio Installer, verified publisher
  Microsoft); Build Tools in ~1.5 min. The VM's Build Tools from P0 were half gone (winget listed
  them, Visual Studio didn't know them): it now says so and asks again before reinstalling
  (`--force`, a second UAC prompt; `--yes` answers both).
- Her build: ~6.5 min (release, thin LTO, 8 vCPUs). The winget prompt for Python 3.13 (per user,
  ~1 min), found at once without a new terminal; the pack in ~1.5 min. **~12 min end to end**,
  his answers included.
- `synaps login` (y at "pick one now"): its provider picker draws in the console.
- After a sign-out and in (a restart, autologon): the Run values start the daemon and her (her
  window 3.4 s after sign-in) and she finds her session. **`synaps.exe daemon --detach` opened a
  terminal** that took the foreground for a moment: the Run value now goes through
  `conhost --headless` (checked: nothing shows).
- `--voice` against a real release: no release yet. The voice installer itself: item 14.
- SmartScreen stops both installers when they come with the Mark of the Web (a browser download);
  `install.ps1` fetches with `Invoke-WebRequest` (no mark) and builds hers from source. Defender
  flagged nothing. Signing them is for the release.

Fixed in install.ps1 on the way: native programs' output shows as it comes (inside the module it
was held to the end, and lost on a stop); a stop without an error record; the build time from a
stopwatch (a clock sync mid-build printed "built in -474 min").
