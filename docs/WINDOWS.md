# She on Windows

Windows 11, x64 (x86_64-pc-windows-msvc). Everything Windows-only is in `src/platform/windows/`;
GPUI's Windows crate is vendored with small patches (`vendor/gpui-pre-windows/PATCHES.md`). CI
builds, tests and lints it on every push (the `windows` job), and installs, starts and uninstalls
her on clean runners (below). What CI can't see (a desktop, his input, monitors) was checked in a
Windows 11 VM on 2026-11-18: see "The VM day" below. Live audio (item 13) needs real hardware, not
the VM.

**Windows 10** (1809, build 17763, or newer) is let through by both installers (`MinVersion` in
`packaging/windows/*.iss`), but she has never been run on it: untested, and not supported yet.
The same goes for Windows 11 on Arm, which the installers accept (`x64compatible`: x64 emulation).

## Build and run

```powershell
cargo build --release --bin synaps-avatar
.\target\release\synaps-avatar.exe      # a console build: her log in the terminal
```

Her control socket is there too (AF_UNIX, Windows 10 1803+), `synaps-avatar.sock` in your temp
folder; no control CLI for it ships here ([Control](#control-no-avatarctl-here)).

Her files (`platform::base_dir`; `$XDG_*_HOME` still wins, as on Linux):

| | where |
|---|---|
| settings | `%APPDATA%\synaps-avatar\settings.toml` (roams with him) |
| her pack (default `--pack`) | `%LOCALAPPDATA%\synaps-avatar\packs\capri` |
| state | `%LOCALAPPDATA%\synaps-avatar\state.json`, `sessions.json`, `tray-promoted` (the tray's marker) |
| her log, without a console | `%LOCALAPPDATA%\synaps-avatar\synaps-avatar.log` (appended; past 4 MB it starts over, at start or while she runs, and the earlier part is kept as `.old.log`). If she can't start (her pack is missing, say), a message box says so and names this file |
| her icon (tray), your own | `%APPDATA%\synaps-avatar\icon.ico` if it's there (up to 1024 px a side), else the capri-syn icon (`assets/icon`, in her exe; a pack's `icon` isn't used here); read at start and on a `settings reload` ([Control](#control-no-avatarctl-here)), when it changed |

Settings in the Line (the cog left of its field, Ctrl+, there, or SETTINGS in her menu or her
tray's) changes BRAIN (and its MODEL), ATTENTION and "HEY CAPRI", each applied at once; ABOUT shows
her versions ([REFERENCE.md](REFERENCE.md#what-she-shows)). Every other key is `settings.toml` by
hand.

## Control: no avatarctl here

`avatarctl`, Linux's control client (a Python script on her Unix socket), doesn't ship on Windows.
Her log's hotkey line and some older notes name a `capri-ctl` for Windows: it was never built.
What the client is for in daily use is on her instead:

| on Linux | on Windows |
|---|---|
| `avatarctl talk tap` (a summon key's binding) | Win+Alt+C, or a click on her tray icon |
| `avatarctl talk toggle` | Win+Alt+Ctrl+C (twice in quick succession: hands-free) |
| `avatarctl talk handsfree on` | HANDS-FREE TALK in her menu or her tray's, or "Hey Capri" |
| `avatarctl show` | SHOW in her tray's menu, or a click on her chip |
| `avatarctl settings open [brain]` | SETTINGS in her menu or her tray's (SET UP MY BRAIN while she has none), the cog in the Line |
| `avatarctl settings reload` | a change made in Settings applies at once. A hand edit of `settings.toml` applies when she starts again: QUIT (her menu, or her tray's), then start her from the Start menu (`synaps-avatar`); or at once, with the script below (PowerShell 7) |
| `avatarctl quit` | QUIT in her menu (it asks twice) or in her tray's (once) |

For scripts, the socket takes the same one-line commands as on Linux
([REFERENCE.md](REFERENCE.md#dev-control-and-test-tools)). It is `synaps-avatar.sock` in
`[IO.Path]::GetTempPath()` (or in `%XDG_RUNTIME_DIR%` when that's set). PowerShell 7 can send one
(Windows PowerShell 5.1's .NET has no Unix sockets; to get 7: `winget install Microsoft.PowerShell`,
then run `pwsh`). This is what CI's shots do (`tools/shots/scenes.ps1`): her reply is one line,
read until its newline, with a timeout.

```powershell
$s = [Net.Sockets.Socket]::new('Unix', 'Stream', 'Unspecified'); $s.ReceiveTimeout = 5000
try {
    $s.Connect([Net.Sockets.UnixDomainSocketEndPoint]::new((Join-Path ([IO.Path]::GetTempPath()) 'synaps-avatar.sock')))
    $null = $s.Send([Text.Encoding]::UTF8.GetBytes("settings reload`n"))
    $b = [byte[]]::new(65536); $m = [IO.MemoryStream]::new()
    do { $n = $s.Receive($b); $m.Write($b, 0, $n) } while ($n -gt 0 -and $b[$n - 1] -ne 10)
    [Text.Encoding]::UTF8.GetString($m.ToArray()).Trim()
} finally { $s.Dispose() }
```

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
persona; no art). With her exe: `LICENSE` and `licenses\` (GPUI's Apache-2.0 and `PATCHES.md`, the
icon's and synaps-client's notices, Lucide's ISC licence for the icons compiled in, and
`THIRD-PARTY-LICENSES.txt`, the licences of every crate in that exe: `build.ps1` generates it with
cargo-about, `installer/licenses/`).

- **Per user, no admin**: she goes to `%LOCALAPPDATA%\Programs\synaps-avatar`.
- **Her brain** (page "Her brain", shown only when it's needed): Synaps is looked for as install.ps1
  looks (`synaps` on PATH, else `~\.cargo\bin\synaps.exe`, at least the manifest's `synaps_min`).
  Missing or too old: "Install Synaps (her brain, open source)", checked, runs the `synaps-installer.ps1`
  of the release the manifest pins (`synaps_pin`, one she was run against), checked against its pinned
  SHA-256. Never the latest: her client needs the daemon's protocol exactly (`synaps_protocol`), and a
  daemon on another one, or one that doesn't say, fails the daemon step ("she can't use it", on the
  finish page, with the fix). Right after Setup installed or upgraded Synaps (`steps.ps1 daemon
  -Fresh`), a daemon on another protocol is an older one still running: "synaps 0.12.0 is installed,
  but the daemon still running is an older one (protocol 2; hers is 3): stop it, `synaps daemon stop`
  (--force if it won't), then run this again". An upgrade warns about a running daemon at once.
  A daemon on another protocol than the `synaps.exe` asking doesn't answer it: Synaps' `daemon
  status` says "running (not answering)", exits 1, and still gives its protocol. The step reads that
  as a daemon that runs and checks its protocol; one of hers that doesn't answer is "not running".
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
- **Run as administrator** (a right-click), Setup asks first, No the default: she and her daemon
  would run elevated too. Unattended, it's a line in the log.
- **Her voice** (page "Her voice"): "Talk to her", on when Windows has a microphone (off and greyed,
  "no microphone found", when it doesn't); a microphone picker over Windows' capture endpoints
  ("Windows default" first); "Hey Capri" and "Read replies aloud", both on. Ticked, it downloads
  the voice installer below (her own release's, its `.sha256` checked) and runs it, fetches
  the models (`voice-models.ps1`, each pinned), then saves his choices with her own settings writer
  (`synaps-avatar --set`: `[voice] enabled`, `wake_word`, `speak_unprompted`, `mic`). Her voice
  itself (`voice`, `speed`, `pace`, `flow`) is never written: she keeps her built-in default, so a
  new voice is a change to her alone. install.ps1's `--voice` runs the same step, writing only
  `[voice] enabled` (a re-run keeps his microphone, "hey Capri" and read-aloud); a voice that doesn't
  come is said in its summary, with the retry, and the rest of the install stands.
- **Upgrades never turn her voice on by themselves.** Over an install of hers (or with her voice
  installed), "Talk to her" starts unticked, unattended too: her voice and its settings (his
  microphone, "hey Capri", read-aloud) stay as he set them, and the finish page says "Her voice: as
  you set it". Only his tick (or `/VOICE=on`) sets it up again.
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
  0.12 gives up at once on a closed one. The same sign-in later, from PowerShell (her install
  folder's `steps`; the default folder shown): `powershell -NoProfile -ExecutionPolicy Bypass -File
  "$env:LOCALAPPDATA\Programs\synaps-avatar\steps\steps.ps1" login`. A plain `synaps login
  --provider claude` in a terminal (the line Settings ▸ Brain copies) opens the cut tab: copy the
  whole URL it prints into your browser instead. Until her first question her tag reads ONLINE on
  Windows for Claude, her default (the daemon's environment can't be read there, so no login on
  file is "unknown", `creds.rs`); her first question without a login turns it to NO BRAIN YET.
- **Her exe is the windowed build** (`cargo rustc … /SUBSYSTEM:WINDOWS`, her binary only): no
  console window from the Start menu or at sign-in. Without a console she logs to the file above.
  `--check`, `-V` and `--set` from a terminal therefore print nothing there (`--set` writes its
  line to the log): pipe them, `& "$env:LOCALAPPDATA\Programs\synaps-avatar\synaps-avatar.exe" -V
  2>&1 | Write-Output`. PowerShell then waits for her, shows what she printed, and `$LASTEXITCODE`
  holds `--set`'s exit code. Or use a cargo build.
- **Her AUMID** `Synaps.Avatar`: registered under `HKCU\Software\Classes\AppUserModelId` with
  `DisplayName` and `IconUri`, and set on the Start-menu shortcut, so toasts show under her name
  and icon. The icon is capri-syn's, `assets/icon/capri-syn.ico` (256 to 16 px) and its 256 px PNG,
  used as they are (`assets/icon/NOTICE`): the setup's own icon, Apps & features, the shortcut, toasts,
  her exe and her tray (#79).
- **Her tray menu** (a right click on her icon) is owner-drawn (`platform/windows/traymenu.rs`):
  her own menu's colours and marks (`platform/menulook.rs`, from `theme`), sized at the DPI of the
  monitor it opens on. The menu itself is Windows': its keys and its submenu. Each row keeps its
  words for a screen reader (MSAA reads them as it reads the stock menu's), and its item data is an
  `MSAAMENUINFO` with them too. Under high contrast it's the stock menu, in your theme.
- **Autostart** (a task, checked): the `HKCU\…\Run` value.
- **Uninstall** (Settings → Apps, or `unins000.exe /VERYSILENT`): stops her (this install of her, by
  its path: never a synaps-avatar.exe of his elsewhere), then removes her exe,
  the steps, the shortcut, the AUMID key, her Run value and the daemon's, the pack the steps made,
  her voice, her log, state and settings, the download cache, and her folders. A pack he put there
  himself (and one moved aside) stays. Synaps: when Setup installed it, the wizard asks whether to
  remove it too (his login and sessions in `~\.synaps-cli` stay either way); unattended, it stays. Yes
  removes the Synaps Setup recorded at install (never one found now): its daemon stopped, the
  binaries its installer's receipt lists (`synaps.exe`, `hidden.exe`), the receipt, and once nothing
  else is in it the folder and its Path entry.
- **Upgrades** close her (Restart Manager) and don't restart her. A process of hers that was killed and never went
  (seen once on a real desktop: one thread, 707 handles, for good) still maps her exe, and Windows won't
  let it be replaced ("DeleteFile failed; code 5"; Setup used to abort with exit 5). So just before
  her files go in, Setup renames the old exe aside (`synaps-avatar.exe.old-<time>`: a mapped exe
  can be renamed), and deletes it once the new one is in. Still mapped, it's left for the next
  Setup or the uninstall. If Setup stops before her files are in, it's renamed back. CI pins it as
  an image in another process and upgrades over it (`test-install.ps1`). Her quit itself is bounded
  (the quit row [below](#what-she-does-here)).
- **Unsigned:** downloaded with a browser (Mark of the Web), SmartScreen stops it ("Windows
  protected your PC"; More info → Run anyway). Defender lets it be. Both installers, in the VM.

Unattended:

```
capri-syn-setup-….exe /VERYSILENT /SUPPRESSMSGBOXES [/SYNAPS=install|skip]
                      [/PACK=download|skip|"C:\packs\capri"] [/VOICE=on|off] [/MIC="name"]
                      [/VOICEFROM="C:\...\capri-syn-voice-setup.exe" or a URL] [/TASKS=autostart]
```

The defaults are the wizard's: Synaps if it's needed, her art downloaded, autostart, her voice when
Windows has a microphone and there's a voice to install (a first install only: see Upgrades above).
Her voice comes from `/VOICEFROM` (a path, or an `https://` URL; a `.sha256` beside it is checked),
else the `capri-syn-voice-setup-x86_64.exe` of her own release, `v<the setup's version>` (never
`releases/latest`: a voice from a newer release could need a newer her), checked against the
`capri-syn-voice-setup-x86_64.exe.sha256` release.yml uploads beside it. install.ps1 `--voice` asks
for the release in her Apps entry, the setup it just ran. Never from a file found by
its name beside the setup: that folder is usually Downloads (CI's voice job passes its build with
`/VOICEFROM`). With none of those (no
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
- `windows-setup-voice` (`test-setup-voice.ps1`, a clean runner): `/VOICE=on`, `/VOICEFROM=` the
  voice installer CI built (as a release's comes from GitHub); the bundle and its selftest, the
  models, `settings.toml` (`[voice] enabled`, never her voice itself), her log's `voice: sidecar
  started`; an upgrade (the setup again, unattended, no `/VOICE`) that runs no voice step and leaves
  his settings byte for byte; and an uninstall that leaves no voice.
- `windows-setup-wizard` (`test-setup-wizard.ps1`, a clean runner): the wizard clicked through with
  its defaults (UI Automation; the runner is elevated, so first the administrator question, Yes), a
  screenshot of every page (`tools/shoot-win.ps1`), her on the desktop after "Start her now", and the
  uninstall's questions ("Remove Synaps too?": yes, then `synaps.exe`, `hidden.exe` and the receipt
  gone). The shots are the job's artifact.

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
- **Licences:** `LICENSE`, `wake\NOTICE` beside the wake-word model, and `licenses\`: every Python
  package with the licence texts its wheel carries, espeak-ng's licence files and sonic's (compiled
  into its DLL), and `SOURCE.txt` (where the source of espeak-ng 1.52.0's DLL and of phonemizer is:
  GPL-3.0; espeak-ng's, with sonic's, is attached to the same release), whisper.cpp's MIT licence (pywhispercpp's DLLs), PortAudio's (sounddevice's
  `libportaudio64bit.dll`, the only one of its builds kept: the ASIO ones and other platforms' are
  removed), Microsoft's end-user terms for the VC++ runtime in `python\` (shipped under the Visual
  Studio licence's Distributable Code), and CPython's in `python\LICENSE.txt`.
  `voice-licenses.py` writes them, and stops the build when espeakng-loader, pywhispercpp or
  sounddevice isn't the version its notes were written for, on a native binary nobody named, on a
  licence not accepted, and on a new or compiled GPL-family package.
- **Uninstall** stops the sidecar (only the `python.exe` in that folder), removes the bundle, Python's
  caches, and `%LOCALAPPDATA%\capri-voice` (models, log, link, media state).
- CI installs, checks and uninstalls it unattended without the models (`test-voice-install.ps1`: the
  files and licences, the selftest run by the installed interpreter, the folder beside her in both cases, nothing
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
| on top | Windows sometimes leaves a topmost window under plain ones with its `WS_EX_TOPMOST` bit still set (seen on a real desktop after an app was opened from Explorer; the second taskbar too). The poller checks her place every 250 ms and when a window comes forward; under a plain window, her UI thread puts her windows back on top (`z: re-topmost (was below …)` in her log). Not while a fullscreen app is in front; never over another topmost window (his picture-in-picture, an always-on-top app). While her puts don't hold, the checks slow down to one every 8 s until one finds her on top. A window still over her right after a put is above the topmost band (Task Manager's "always on top", the touch keyboard): it and every plain window over it are left out for 60 s, doubled each time it's found over her again, 10 min at most (`z: can't get above …`, once a window). The poller's fullscreen check never asks the shell (`SHQueryUserNotificationState` is answered by explorer: a hung one would stall the poller) | [M] 2026-11-19 |
| fullscreen | a window covering its monitor (not a tool window, not one clicks pass through, not the shell's: Start, Search, Alt+Tab and the flyouts are none, nor his focused window). The shell's UI: a window of the shell's processes (explorer, StartMenuExperienceHost, SearchHost, ShellExperienceHost, ShellHost, TextInputHost, TabTip, LockApp) of a known class or any tool window; another process's window is an app whatever its class (a full-screen UWP app's `Windows.UI.Core.CoreWindow`) | [M] 2026-11-19 |
| perching | `EnumWindows`, DWM frame bounds, WinEvent hooks for moves, shows, hides, minimise, cloak, foreground. A window clicks pass through (layered and `WS_EX_TRANSPARENT`, or layered at alpha 0: an overlay) is no window to sit on or be under, nor his window in front. The touch keyboard's handwriting canvas (TabTip.exe) is one, a topmost tool window over the whole primary monitor: taken for a window, it left her no seat on that monitor, and she fell to the floor there. A window over a whole monitor that isn't an app's (a tool window, one never activated, or a click-through one) is said in her log once, and once more if it turns click-through or back: `windows: <class> of <exe> (<id>, ex <style>) covers a whole monitor (<rect>): click-through: left out`, or `kept: …` for one that still hides the windows under it. The shell's own UI (Start's, Search's, Task View's and Alt+Tab's hosts) is said only when click-through | P0 [VM]; overlays: CI (unit test), the canvas seen on a real desktop 2026-11-19 |
| virtual desktops | windows on another desktop are cloaked (left out); `IVirtualDesktopManager` confirms; a switch (the shell's registry value) is `Hint::Desktop` | [VM] item 8 (after a fix) |
| quit | her bye, the daemon told (800 ms at most), her state saved, GPUI's teardown; 2 s after her bye a watchdog ends her (`quit: still here … ending now (exit N)`, her exit code: 2 after a fatal error), and after GPUI's run she ends with `TerminateProcess` on herself, not `ExitProcess` (no DLL's detach can hold her with her exe mapped); every exit once GPUI is up goes that way, her tray icon taken out first from the thread that ends her (300 ms at most; no ghost icon when her UI thread is the one stuck). Windows only: on Linux her teardown runs to its end. Once on a real desktop, locked: a quit stayed, a kill left her half-gone, her exe pinned, and the installer failed (code 5). `tests/windows/test-quit.ps1` (CI, last): gone within 5 s of her bye, exit code 0 and her exe free, unlocked, with her teardown held 10 s (`SA_TEST_QUIT_STALL_MS`, tests only: the watchdog's line, then gone), and with the session locked | [M] 2026-11-19 |
| monitors | `EnumDisplayMonitors`; `WM_DISPLAYCHANGE`, `WM_DPICHANGED` and a work-area change (taskbar moved) are `Hint::Layout`, logged (`windows: layout: …`) | [VM] item 4 |
| DPI | per-monitor aware (v2), set before GPUI starts. Her windows keep the primary monitor's scale everywhere (PW-5): her art is laid out at one scale. A display change reaches GPUI from the message loop (PW-6) | [VM] item 3 (100 % and 150 %; PW-6 fixed a crash) |
| effect layers | the bolt and anything reaching outside her window: a plain Win32 window per layer, solid colour, shaped by a region, click-through, never activated (`fx.rs`). The rest is painted in her window | [VM] item 9 |
| her keys | `RegisterHotKey`: **Win+Alt+C** summons (a tap), **Win+Alt+Ctrl+C** toggles talk. Win alone is the shell's | [VM] item 5 |
| her tray icon | `Shell_NotifyIconW` (`tray.rs`): her icon (her exe's, or your `icon.ico` above), the tooltip `capri-syn · ONLINE` (her tag's word; `HIDDEN` while hidden). A click opens the Line (her key's tap); a right-click opens her menu, the same rows as her right-click menu (`menu.rs`: RESET POSITION, DND, ATTENTION with its three choices, HANDS-FREE TALK, COPY LAST REPLY, HIDE or SHOW, SETTINGS, QUIT), and its QUIT quits at once. Back after Explorer restarts; gone when she quits, no ghost icon. Windows 11 puts a new tray icon in the `^` overflow, so the first time it's in she sets `IsPromoted` on her own `HKCU\Control Panel\NotifyIconSettings` entry (`promote.rs`, on a thread of its own, a few seconds after the add: Explorer takes no value written during it; matched by her exe's path and her icon's GUID), once: the marker `tray-promoted` in her state folder says it was done, and after that the setting is his (hide her and she stays hidden; Windows 10 has no such key, nothing is done). She then asks the shell whether her icon is on the taskbar (UI Automation, as the shots' rig does) and, if it isn't and her entry still reads 1, writes the same 1 once more; her log says what is true (`shown on the taskbar`, `shown on the taskbar (first run, written twice)`, or `set to show on the taskbar; Explorer hasn't moved her yet (next sign-in)`). Linux has none yet | CI (unit tests; the promote on a scratch registry key and on the runner's own tray; the `shots:tray` take fails if she doesn't promote herself) |
| the Line's keyboard | see below | [VM] items 2, 6 |
| music | SMTC (the media flyout's sessions): playing or quiet, and whether every playing session is a video. Edge reports no playback type: a video in Edge counts as music | [VM] item 10 |
| notifications | toasts, see below | [VM] item 11 (after a fix) |
| lock, sleep, idle | WTS session notifications, power broadcasts, `GetLastInputInfo` | P0 [VM] |
| voice: the link | her end of the sidecar's link: loopback TCP after a two-token handshake (`endpoint.json` in `%LOCALAPPDATA%\capri-voice`); Linux keeps the Unix socket | CI (both ends, Rust and Python) |
| voice: the sidecar | `tools/capri_voice_win.py`: PortAudio (WASAPI) capture and playback, SMTC pause, session-volume duck with Linux's crash repair, lock, calls (the mic privacy store), DND. The voice installer (below) puts its bundle (embeddable CPython 3.13 + wheels) beside her: `docs/design/windows-voice.md`. The lock is the session's own state (WTS): Windows 11's lock screen runs on the Default desktop | CI; [VM] item 14 (lock: after a fix); live audio: item 13 (real hardware) |
| voice: she runs it | while `[voice] enabled`: `python.exe capri-voice` from `voice\` beside her exe (or `CAPRI_VOICE_BUNDLE`), no console window, in a job object that kills it with her (however she ends); restarted after 1, 2, 4 … 60 s; its log `%LOCALAPPDATA%\capri-voice\capri-voice.log`. Not before the models are there. `CAPRI_VOICE_SPAWN=0`: he runs it himself. `[voice] enabled` is read at start and on a `settings reload` ([Control](#control-no-avatarctl-here)); a new `[voice] mic` there starts the sidecar again on it (the sidecar reads its mic only when it starts) | CI (job, backoff, bundle lookup); [VM] item 14 |
| voice: connecting | on a thread (a stale endpoint can't freeze her: up to ~2 s); Linux connects inline, as before | CI (a silent listener) |
| hold-to-talk | opt-in, `[voice] hold_to_talk = true`: the summon key's press is `talk press`; her key thread then reads the key every 25 ms (`GetAsyncKeyState`, as OBS does) and its release is `talk release`. No keyboard hook. Off: the summon key is a tap, as before | CI (the state machine); [VM] item 14 |

**Known issue (tray name):** on a first run, Explorer can name her taskbar button with her old tooltip followed by the current one (`capri-syn · SPEAKING capri-syn · ONLINE`), so a screen reader hears both. Her side is one tooltip string, sent whole with each `NIM_MODIFY`, and the shell's hover popup shows the current one alone; the name chains after a tooltip change (her `InitialTooltip` entry is empty, at 100% and 150%). A follow-up is open. The `shots:tray` take accepts the doubled name (her current tooltip last, only a tooltip of hers before it) and raises a warning in the run summary.

## The Line's keyboard

Windows hands a process the foreground only right after the user's own input to it. So
(`docs/design/windows-line-keyboard.md`):

- her key and a click on her take the keyboard at once;
- any other opening (a summon from the brain, a `talk tap` on her control socket, voice) shows the Line **without** the
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

<a name="vm-pending-one-vm-day"></a>

## The VM day (2026-11-18)

The VM: Windows 11 Enterprise Evaluation 26H2 (build 26300), QEMU/KVM, two
VirtIO GPU heads (1280×800 and 1920×1200), D3D on WARP (software), an HDA sound card with a silent
input, 8 GB. Builds: main's CI installers (3713ff0) and this branch built in the VM by
`install.ps1`'s source path. Driven from inside his session (`SendInput`, the QEMU keyboard for his
keys and UAC); screenshots kept outside the repo. **[VM]**: true of the VM (software rendering, no real mic), to check again on real hardware.

| # | what | result |
|---|---|---|
| 1 | PW-1/2/3: no frame; a click on her doesn't take focus; no stale frame or extra CPU while dragged | **pass**. No frame, border or shadow. A drag and a headpat leave the foreground where it was. While dragged her renders follow her ragdoll animation, not the moves (240 moves: 373 renders in 7 s; 78 moves: 298), no stale frame after. CPU [VM]: 7 % of a core idle, 13 % dragged (her animation on WARP) |
| 2 | PW-4: no stray Alt when the Line opens | **pass**. A low-level keyboard hook while the Line opened on her key and on a click on her: his keys only, nothing injected |
| 3 | PW-5 + DPI: 100 % and 150 % | **pass after a fix**. Her window stays 500×695 and her body the same size on both monitors, crisp. Her first move onto the second monitor **aborted her** (`RefCell already borrowed`, `async_context.rs:178`): fixed by **PW-6** (`vendor/gpui-pre-windows/PATCHES.md`) |
| 4 | hotplug, the taskbar | **pass**. `windows: layout: monitors changed` and `… the work area changed` (new log lines) on a monitor detached with her on it (`her monitor went away: to 848,78`), attached again, a DPI change, the taskbar's auto-hide on and off (Windows 11's taskbar has no other edge) |
| 5 | her keys | **pass**. Both register (none taken); Win+Alt+C opens the Line with the keyboard; Win+Alt+Ctrl+C toggles talk (`toggle: listening`) |
| 6 | the Line from her control socket | **pass**. `talk tap` (sent from a terminal): the Line opens parked, `ONLINE · WIN+ALT+C TO TYPE` on her tag and on the Line, the terminal keeps the keyboard; his Win+Alt+C, or his click on the Line, takes it; Esc gives it back |
| 7 | the contract suite | **11 of 13**, `keyboard` and `body-and-teleport-fx` among them. `perch-space` and `monitors-and-areas` need the driver to make a second monitor and a panel that reserves space (`split_monitors`, `dock_top`), not written for Windows (follow-up); items 3 and 4 checked the same by hand |
| 8 | virtual desktops | **pass after a fix**. With one desktop Windows 11 writes no `CurrentVirtualDesktop` until the first switch, so the watcher never started (`no virtual desktop in the registry`); it now watches both keys (Windows 11's and the per-session one) and logs which holds the current desktop. A window on desktop 2 is refused as a perch (cloaked: `not visible`); a switch logs `windows: virtual desktop switch` and her perch on desktop 1 is left (`left with the workspace`) |
| 9 | effect layers | **pass**. The launch and teleport bolts draw outside her window (captured mid-flight); the layer windows are layered, click-through (`WS_EX_TRANSPARENT`) and `WS_EX_NOACTIVATE`; under/over her: the suite |
| 10 | SMTC | **pass, one gap**. Media Player with a song: `smtc playing (Microsoft.ZuneMusic…=Playing)`, LISTENING, her headphones on; with a video: `(video)`. **Edge reports no playback type** for a video (a file or a page's `<video>`): to her it is music. A Discord call: not tried (no account in the VM) |
| 11 | toasts | **pass after a fix**. `CAPRI // NEEDS YOU` under "synaps-avatar", the key there. It came without her face: `fs::canonicalize`'s `\\?\` path made a `file://///?/…` URI, which the shell drops silently |
| 12 | the installer, interactively | **pass after a fix**. SmartScreen first (unsigned). The pack page: a non-pack refused, Browse (a folder picker), empty allowed; the shortcut starts her with no console; a toast shows her icon and face; autostart starts her 3.6 s after sign-in; uninstall from Settings → Apps, with her running, leaves none of hers (his settings stay). **Settings → Apps said 44.6 GB**: Inno sized the pack's line as `\*` (the root of C:) before the page had a value; fixed, and CI checks the size |
| 13 | voice, live audio | not in the VM: real hardware (mic, ears, Whisper and Kokoro speed). The bundle's onnxruntime loaded (the wake word ran), but on a VM with Build Tools. **A clean Windows wants the VC++ runtime here, so the bundle carries it**: onnxruntime 1.32.0's `onnxruntime.dll` and `onnxruntime_pybind11_state.pyd` import `MSVCP140.dll` and `MSVCP140_1.dll`, espeakng-loader 0.2.4's `espeak-ng.dll` imports `MSVCP140.dll` (CPython's embeddable package brings only `vcruntime140.dll` and `vcruntime140_1.dll`; numpy's and pywhispercpp's msvcp140 are renamed copies, which don't count). `voice-bundle.ps1` copies `msvcp140.dll` and `msvcp140_1.dll` from Visual Studio's `Microsoft.VC14x.CRT` into `python\`, and its `vcruntime140*.dll` only when they're newer than the embeddable package's (never vc_redist: the installers are per-user). `voice-imports.py` stops the build when a DLL imports what neither the bundle nor a clean Windows has, or a runtime older than the MSVC that linked it. CI also checks the installed bundle loads its own copies over the runner's System32 ones, espeak-ng's as phonemizer loads it (a copy in `%TEMP%`). Left for a Windows with no VC++ Redistributable (not this VM: its Build Tools put one in System32): the sidecar starting there. Her own exe needs nothing: the C runtime is linked in, and build.ps1 checks her imports |
| 14 | voice, her side | **pass after a fix**. The voice installer (SmartScreen too) and its models window (~358 MB in ~40 s); `voice: sidecar started`, killed with her; `[voice] enabled = false` + `settings reload`: `sidecar stopped (voice off)`, on: started again; the voice uninstaller under her removes everything, she backs off (1, 2, 4, 8 s, `no models`); `hold_to_talk`: a hold listens and its release sends (`key up: transcribing`), a tap opens the Line, a release with Notepad in front ends it and Notepad keeps the keyboard, Win+L mid-hold ends it; the sidecar listens on 127.0.0.1 only: no firewall prompt. **The sidecar missed Windows 11's lock screen** (its curtain runs on the Default desktop): the wake word's mic stayed open on it for 14 s. It now asks the session (WTS) |
| 15 | install.ps1, interactively | **pass after fixes**, below |

### install.ps1 in the VM

`irm …/install.ps1 | iex` in Windows PowerShell 5.1, no Python, no release (so a source build):

- Synaps 0.12.0 through its own installer; the daemon up.
- The source path: the Build Tools prompt, then UAC (Visual Studio Installer, verified publisher
  Microsoft); Build Tools in ~1.5 min. The VM's Build Tools from P0 were half gone (winget listed
  them, Visual Studio didn't know them): it now says so and asks again before reinstalling
  (`--force`, a second UAC prompt; `--yes` answers both).
- Her build: ~6.5 min (release, thin LTO, 8 vCPUs). The winget prompt for Python 3.13 (per user,
  ~1 min), found at once without a new terminal; the pack in ~1.5 min. **~12 min end to end**,
  his answers included.
- `synaps login` (y at "pick one now"): its provider picker draws in the console. (install.ps1
  offered a sign-in then; it no longer does: giving her a brain is hers, Settings ▸ Brain.)
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
