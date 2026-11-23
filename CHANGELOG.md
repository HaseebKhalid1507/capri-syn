# Changelog

## Unreleased

- **Hyprland Lua summon bindings**: the installer prefers `hyprland.lua`, writes Lua press/release
  bindings and tags them so live collision checks keep the same key on reinstall. The optional
  mic toggle is checked too; occupied chords fall back together to Super+Alt+C / Super+Alt+Ctrl+C.
  Offline helpers and aliases require manual setup rather than guessing that a key is free.

- **Hyprland: she sits on windows again under shells that draw on one full-screen layer** (caelestia,
  some quickshell setups). `perch sit` / `perch stand`, her flights and drag snaps used to end in
  `edge hidden … exposed []`: the shell's transparent full-output layer counted as covering every
  window. A layer that covers its output's whole work area, or one 2 px thin or less, no longer counts
  as a cover; bars, docks, side panels and popups still do. A modal overlay that big (a launcher's or
  logout screen's dimmer) doesn't count either, so she may sit on what it hides: `hyprctl layers`
  doesn't say which layers take the keyboard.
- **Hyprland/NVIDIA:** select the compositor's GPU before Vulkan starts, fixing black boxes
  when NVIDIA is primary. The XWayland fallback selects its X screen's GPU, so its window should
  open instead of reporting no compatible adapter (not yet run on XWayland with NVIDIA). Single-GPU Wayland is also pinned to the
  compositor's GPU, but its Vulkan driver list is left unchanged; vendor narrowing is only
  for multi-GPU machines. Native X11 keeps its boot-VGA workaround.
- **Hyprland: the Line and Settings grow to fit their contents** instead of staying at their
  initial 120 px height with the input field or Settings rows cut off (#145). They remain overlay
  layer surfaces, never tiled; no Hyprland config changes are needed. The Line's width also
  follows placement instead of freezing at its opening width. **Fixed, pending confirmation
  on Hyprland.** Reader/effect-host placement is unchanged.

## 1.2.0 (2026-11-23)

She's restless, turns to another loop while she sits or stands, and changes outfit now and then. Her
bubble fits what she says. Capri's pack takes CapriCore v1.2.2's wave, hanging cels, keytar and
no-jacket look.

### Her life
- **She's restless:** an idle beat every few seconds from whatever the pack fills (blinks, looks, a wink,
  a lean in, a sit on the floor, her gestures), never the same one twice running; now and then a quick
  glance at the pointer or at a window that came forward, dropped for something else; steady while her
  session works; more gestures the longer nobody engages her, up to her attention bids. Asleep, held (the
  Line, a prompt, the menu, a drag, a reply bubble, the voice) or moving: still. No new settings.
- **She doesn't hold one loop:** perched, perched with music or standing idle, she turns to another of
  that state's loops every 8–25 s, as a pass of the one on screen ends: its exit, now and then a short
  beat of hers, then the next one's entry. Never a move, so `max_moves_per_hour` doesn't see it.
- **She changes outfit,** her jacket or the new no-jacket look, about every 45–120 min, where it can't be
  seen: in a bolt (a teleport's or her launch's) or as she wakes; the one she wears is kept across
  restarts. In the no-jacket look her greeting starts with a new wave, and hanging from a bar she now and
  then does a pull-up, a catch or a wrist scan. In both looks she sometimes plays a keytar on a window top
  while music plays. In her jacket look a summon, a teleport or a landing on a window top can open a
  second way, never the same one twice running.

### The bubble
- **It fits what she says.** A short reply gets a small bubble, a longer one grows with its lines, and a
  reply past a page (or with code) goes to READ at about the full width; prompts and the text field fit
  their words too. Her status shortens before it is cut, and the keys under a reply or a prompt (ABORT,
  READ · N LINES, DENY, ALLOW, REVIEW ▸, the collapse key, the pager) are never cut; where they can't all
  fit, the pager goes first, then READ's count.
- **A streaming reply grows in steps,** at most four a second, and doesn't shrink until the turn ends,
  then settles once to its fit; the text field grows with what you type, in steps, and doesn't flip at a
  step's edge. A reply shown all at once, or a spoken caption, sizes to its text at once. A line waits for
  the box to grow around it, so a word or a command isn't cut while it grows, and the widest line isn't
  clipped as the box narrows to settle. With no compositor (she cuts her window to her outline there) a
  streaming reply and the field open at their widest and stay there.

### Packs
- **New optional slots any character pack can fill** (docs/REFERENCE.md, "Slots any pack can fill"): cel
  sheets and animated WebP strips, which play as clips wherever steps go; a second outfit (a strip may be
  one outfit's drawing of a clip); hang fidgets; a glance; a perched music loop; idle face and pose pools;
  arrival alternatives; hearts drawn from the pack's own effect sprite. A pack without them behaves as
  before.
- **Loop ranges:** a looped list or strip splits into an `entry` that plays once, a `loop` and an `exit`
  that plays once as the loop ends; a one-shot beat adds `repeat: [min, max]`. `--check` names the key of
  any mistake.
- **Capri's pack takes CapriCore v1.2.2's files,** pinned by `make-pack` at `828f272` and checked against
  `tools/capricore-v1.2.2.sha256`. The installers fetch them with the rest of her art (~120 MB to
  download, ~115 MB built); an upgrade rebuilds her pack once.

### Fixes
- **Music after a carry.** Carried while a player's music played, she used to land still listening, and
  pausing it no longer took her out. Now music still playing when she lands stops with the player, and
  music that stopped while she was carried lands her out of music mode.

### Known issues
- A dark slab on X11 without a compositor (#72): now and then her whole window's rectangle paints solid
  black behind her and her bubble, covering what's under it, more often while a prompt hangs below her. So
  far only seen with no compositor, where she cuts her window to her outline with the X shape. Workaround
  (likely; not yet confirmed): run a compositor (picom: `picom -b`); with `[display] composited = "auto"`,
  the default, she switches to alpha when it starts, no restart.
- GNOME, KDE and sway run her through XWayland, with its limits (docs/INSTALL.md). None of them has been
  tried on a real session yet: CI and the test containers stand in for them.
- Hyprland has no idle time for her yet: your presence is the pointer moving, so typing alone for 5 min
  reads as away. Hyprland before 0.40 isn't supported: she takes XWayland there.
- Hyprland: the Line and Settings can open cut short, with no room to type (#145). Being looked at.
- On Windows, Synaps 0.12's own `synaps login` opens a sign-in tab cut at the URL's first `&`: copy the
  whole URL it prints into your browser, or sign in from her (Settings ▸ Brain) or through her installers'
  step, which open it whole (docs/INSTALL.md has the command). Grok's sign-in never prints its URL, so
  Grok can't be signed in on Windows yet.
- Windows: the installers are unsigned, so a browser download meets SmartScreen ("More info", then "Run
  anyway"). Windows 10 and Windows 11 on Arm are untested. A click on one of her notifications does
  nothing (as on Linux). Her voice brings its own VC++ runtime but hasn't yet run on a PC without the
  Visual C++ Redistributable. Live audio is still to check on real hardware: her desktop checks ran in a
  Windows 11 VM, with no real audio.
- Linux has no tray icon yet: HIDE's chip is the way back there.
- Her taskbar button's name, for a screen reader (Windows): on a first run, Explorer can name her button
  with her old tooltip followed by the current one, so a screen reader hears both
  (`capri-syn · SPEAKING capri-syn · ONLINE`). Her tooltip is one string, and the hover popup shows the
  current one alone: the name chains after a tooltip change.
- Windows: her context doesn't roll over yet. Synaps 0.12 keeps its context archive on Linux and macOS
  only, so `[session] rollover` (docs/REFERENCE.md) is skipped there (her log says so). A long chat fills
  her model's context until a turn is refused as too long; then compact or clear it from Synaps
  (`/compact` or `/clear` in `synaps --attach capri`).
- A ChatGPT account without Codex (coding) access reads as AT THE LIMIT: Synaps 0.12 words its refusal as
  the same 429 Too Many Requests a rate limit gets, so she can't tell them apart. If it never clears, pick
  another brain in Settings ▸ Brain.
- With `XDG_RUNTIME_DIR` unset, she and her voice sidecar look for the Linux socket in different places:
  she in the temp dir (`$TMPDIR`, else `/tmp`), the sidecar in `/run/user/<uid>`. Set it, or set
  `CAPRI_VOICE_SOCK` for both. It isn't aligned yet because that would change how she behaves.
- A word streamed in pieces can leave the bubble a step too wide until the reply ends, and the half-typed
  word drops a line. Nothing is cut (#146).

## 1.1.1 (2026-11-20)

She runs free on a ChatGPT account, a free one too: no API key, no card, no GPU. On Linux her context
rolls over by itself, so an always-on chat doesn't fill her model's context.

### Her brain
- **Free on a free ChatGPT account.** Its row says "free account works" in Settings ▸ Brain, her setup's
  "Her model" page and the installers' lists. Free accounts have usage limits: when yours is used up she
  says she's hit her model's limit (AT THE LIMIT). Wait for it to reset, or pick another brain.
- **ChatGPT's default is GPT-5.6 Terra,** which a free account runs (GPT-5.6 Luna too): a ChatGPT sign-in
  puts her on it. GPT-5.6 Sol, GPT-6 Astra and GPT-6.1 Sol need a paid plan; on a plan that doesn't run her
  model she says NOT ON YOUR PLAN and to pick another in Settings ▸ Brain ▸ MODEL, instead of Synaps'
  error (only while her model is on ChatGPT). A model you picked stays yours.
- **GPT-5.5, GPT-5.4, GPT-5.4 Mini and GPT-5.3 Codex Spark are gone from ChatGPT's list:** OpenAI retired
  them from ChatGPT sign-ins. A `[session] model` set to one of them moves to GPT-5.6 Terra once, as she
  starts (written to settings.toml, said in her log); `--model` with one of them runs Terra.
- **A provider's trouble reads in her words.** Once Synaps' retries are spent, a 429 (ChatGPT, a key
  service, Gemini; Claude's `rate_limit_error`) is AT THE LIMIT, a 500, 502, 503, 504 or 529 (or a reply
  stopped at capacity) is MODEL BUSY, and a 408 is NO CONNECTION. ChatGPT's 401 is LOGGED OUT, with
  `SIGN IN AGAIN ▸` (only while her model is on that account).
- **Settings ▸ Brain's account and key-service rows fit** at the Line's narrowest (480 px).

### Her session
- **Her context rolls over by itself (Linux).** She turns on Synaps' automatic context rollover for her
  own session only (`/context auto`; Synaps' config and your other sessions stay as they are): when the
  context gets long, Synaps archives the older messages and carries on in a fresh window with the
  recent ones, and the Line shows one quiet row where it happened. It's for ChatGPT, Claude and other
  brains with room for it (most have 200k): she skips it for a local model and for a window under 128k, and
  when Synaps has it on already (its config, or another client's `/context auto`) she leaves it to
  Synaps. Her log says which. `[session] rollover = false` leaves it to Synaps. Not on Windows yet
  (Known issues).
- **Privacy:** the archive (`~/.synaps-cli/context-archives/`) holds both sides of the chat word for
  word, and Synaps never prunes it: it stays after `rollover = false` and `/clear`. It's shared by every
  session in the same Synaps project. Her session's folder is your home by default, so if your home has
  a `.git` or a `.synaps-project`, sessions in folders under it without their own can search her
  archived messages (docs/REFERENCE.md, The session rules).

### For developers
- `synaps-avatar --providers` has a seventh field, a hint beside a brain's name (ChatGPT's alone); her
  installers show it as given, and an older her's six fields still read.
- The vendored synaps-client sends engine commands and queries (`Client::engine_command`, `Client::query`
  and their events and errors), to port back (crates/synaps-client/VENDORED.md). The fake daemon
  answers `context auto`, `context off` and the context report as Synaps 0.12 does.

### Known issues
- Windows: her context doesn't roll over yet. Synaps 0.12 keeps its context archive on Linux and macOS
  only, so `[session] rollover` (docs/REFERENCE.md) is skipped there (her log says so). A long chat
  fills her model's context until a turn is refused as too long; then compact or clear it from Synaps
  (`/compact` or `/clear` in `synaps --attach capri`).
- A ChatGPT account without Codex (coding) access reads as AT THE LIMIT: Synaps 0.12 words its refusal
  as the same 429 Too Many Requests a rate limit gets, so she can't tell them apart. If it never
  clears, pick another brain in Settings ▸ Brain.
- The rest are in docs/INSTALL.md's Known issues.

## 1.1.0 (2026-11-19)

Her brain is yours to pick: an account you have, an API key or a model on your own PC, from Settings ▸
Brain and from her installers, with nothing picked for you. On Windows 11 her tray icon starts on the
taskbar, and on Linux her voice installs where `python3` is 3.14.

### Her brain (Settings ▸ Brain and her installers)
- **Three ways in, none first.** Settings ▸ Brain offers SIGN IN, API KEY and RUN LOCAL, each opened in
  place, then MODEL. SIGN IN lists the accounts by name: ChatGPT, Claude, Copilot, Gemini (when Synaps
  has its Google client), Grok, Kimi and Muse. API KEY lists Synaps' 19 key services. RUN LOCAL lists
  the models of the server on your PC when it's up, and otherwise opens the local-model steps.
- **She signs you in herself.** A SIGN IN row runs Synaps' sign-in with no window, and your browser
  opens on the provider's sign-in page. Copilot's code is shown large and copied, to type there: the
  one thing she copies, since it is short-lived and single-use. While it runs the row reads SIGNING
  IN…, ⏎ stops it, and NO BROWSER? under it (Claude, ChatGPT, Gemini and Copilot only) opens the page
  again. Signed in, she switches to that account's model and says so. If it fails she says why, with
  the terminal's line in her bubble. A sign-in not finished within 10 minutes is stopped. An API key
  goes in a hidden field and to Synaps on its stdin. Neither the page's address nor the code goes to
  her log.
- **Only the provider's own sign-in page is opened.** She reads the page off the line Synaps prints and
  opens it only when it is that provider's own. On Windows she opens the whole page herself, since
  Synaps' `cmd /c start` cuts it at its first `&`. Grok there says NEEDS A NEWER SYNAPS: its page is
  never printed.
- **LOGGED OUT signs in again from her.** `SIGN IN AGAIN ▸` in her bubble opens Settings ▸ Brain on her
  account's row (LOGGED OUT · ⏎ SIGN IN AGAIN), with no terminal.
- **MODEL is a menu.** It lists every model of every account signed in on this PC, every key service
  with a key, and the local server's, grouped by provider with hers checked, and it scrolls. Only a
  confirmed login counts, so a fresh install lists no Claude models until you sign in; with nothing to
  list it says `sign in first`.
- **NO BRAIN YET asks for a pick.** With no brain picked anywhere, a failed message asks you to pick
  one, with `SET ME UP ▸` to Settings ▸ Brain. A brain you picked whose login is missing names that
  provider and its login line.
- **Her installers ask which brain.** When Synaps has no brain yet, her setup's "Her model" page, and
  install.sh and install.ps1 in your terminal, offer four choices: sign in to an account (the same
  list as Settings, Copilot's code shown to you), a model on this PC (Ollama or LM Studio, whichever
  is running), an API key (never on a command line or in a log), or later, the default: nothing is
  picked for you. Her model then becomes the account's default (once you're signed in), the key
  service's default, or the local model you picked. On Windows the finish page offers the sign-in for
  the account you picked, ticked; unticked or stopped, her model stays unset. Grok can't be signed in
  on Windows yet. Unattended: `--brain=` (`/BRAIN=` for her setup); `--yes`, `--dry-run` or no
  terminal mean later unless `--brain` says otherwise. `synaps-avatar --set session.model=…` sets her
  model.

### Windows
- **Her tray icon starts on the taskbar.** Windows 11 hides a new tray icon in the `^` overflow. She
  moves hers out once; after that it's your setting, an icon you placed is left alone, and Windows 10
  gets nothing.
- **Setup's window drags** on its "Setting her up" page (its buttons are hidden there and the close
  box is greyed). While its system menu's About box is open it holds off, so a second box can't open
  on top. Setup's log has a `step NAME: exit N` for every step (`-1` when there is no exit code), and
  names any other window of Setup's that keeps the wizard disabled.

### Linux voice
- **Her voice installs where `python3` is 3.14 (Arch now).** kokoro-onnx takes Python 3.10 to 3.13: she
  uses your `python3` when it is one of them, else a `python3.13` to `python3.10` on your PATH; with
  none, uv fetches 3.13 (105 MB) into `~/.local/share/capri-voice/python` and nowhere else; with no uv
  she says what to install.
- **`pw-record` and `pw-play` are checked first,** before anything is installed. A missing one names
  its package: `pipewire-bin` (Debian, Ubuntu), `pipewire-utils` (Fedora), `pipewire-audio` (Arch) or
  `pipewire-tools` (openSUSE).
- **`capri-voice` started by hand (no systemd) runs on the voice's venv,** not the system Python,
  which has no kokoro-onnx.

### Docs
- The repository has a code of conduct, contributing notes, a security policy, a support page, issue
  forms and a pull request template; the bug form's Version asks for what Settings ▸ ABOUT ▸ COPY
  copies. The README has a Requirements section: OS, graphics, CPU, RAM and disk, her brain, voice,
  network, and its Brain picture shows the current Settings ▸ Brain. docs/ describe it and the
  installers' picker. REFERENCE.md's trouble table has her reply when no model is picked, and INSTALL.md
  and VOICE.md say what the Linux voice needs.

### For developers
- `synaps-avatar --providers` prints her accounts, key services and local server, a line each, in her
  own table's order: six tab-separated fields (id, login, kind, default model, name, how it's signed in
  to). Her installers read it, so they keep no copy.
- `tools/fakesynaps.py` and `tools/fakelocal.py` stand in for Synaps' sign-in and a local model server.

### Known issues
- A dark slab on X11 without a compositor (#72): now and then her whole window's rectangle paints solid
  black behind her and her bubble, covering what's under it, more often while a prompt hangs below her.
  So far it has only been seen with no compositor running, where she cuts her window to her outline
  with the X shape. Workaround (likely; not yet confirmed): run a compositor (picom: `picom -b`). With
  `[display] composited = "auto"`, the default, she switches to alpha when it starts, no restart.
- GNOME, KDE and sway run her through XWayland, with its limits (docs/INSTALL.md). None of them has been
  tried on a real session yet: CI and the test containers stand in for them.
- Hyprland has no idle time for her yet: your presence is the pointer moving, so typing alone for 5 min
  reads as away. Hyprland before 0.40 isn't supported: she takes XWayland there.
- On Windows, Synaps 0.12's own `synaps login` opens a sign-in tab cut at the URL's first `&`: copy the
  whole URL it prints into your browser, or sign in from her (Settings ▸ Brain) or through her
  installers' step, which open it whole (install.ps1 puts it there too; the default folder shown;
  `-Provider` for another account than Claude):
  `powershell -NoProfile -ExecutionPolicy Bypass -File "$env:LOCALAPPDATA\Programs\synaps-avatar\steps\steps.ps1" login`
  Grok's sign-in never prints its URL, so Grok can't be signed in on Windows yet.
- Windows: the installers are unsigned, so a browser download meets SmartScreen ("More info", then "Run
  anyway"). Windows 10 and Windows 11 on Arm are untested. A click on one of her notifications does
  nothing (as on Linux). Her voice brings its own VC++ runtime but hasn't yet run on a PC without the
  Visual C++ Redistributable. Live audio is still to check on real hardware: her desktop checks ran in
  a Windows 11 VM, with no real audio.
- Linux has no tray icon yet: HIDE's chip is the way back there.
- Her taskbar button's name, for a screen reader (Windows): on a first run, Explorer can name her button
  with her old tooltip followed by the current one, so a screen reader hears both
  (`capri-syn · SPEAKING capri-syn · ONLINE`). Her tooltip is one string, and the hover popup shows the
  current one alone: the name chains after a tooltip change.
- With `XDG_RUNTIME_DIR` unset, she and her voice sidecar look for the Linux socket in different places:
  she in the temp dir (`$TMPDIR`, else `/tmp`), the sidecar in `/run/user/<uid>`. Set it, or set
  `CAPRI_VOICE_SOCK` for both. It isn't aligned yet because that would change how she behaves.


## 1.0.0 (2026-11-19)

capri-syn's first release: Capri on your desktop, her brain in Synaps. Linux (X11, Hyprland, and
GNOME, KDE and other Wayland desktops through XWayland) and Windows 11 (x64). One line installs her:
`install.sh` on Linux, `install.ps1` or `capri-syn-setup-x86_64.exe` on Windows. Her voice is its own
download: `--voice` on Linux; on Windows her setup's Voice page (on when Windows has a microphone, on a
first install only; an upgrade never turns it on by itself).

### Install and releases
- `curl -fsSL …/install.sh | bash`, `irm …/install.ps1 | iex`, or either from a checkout. Seven steps, the
  same on both, each one checks first, so a rerun is an upgrade:
  1. Synaps, when it's missing or older than 0.11.0: the manifest's `synaps_pin` (0.12.0), through
     SynapsCLI's own release installer, checked against the manifest's sha256. Never its latest release:
     her client speaks protocol 3 exactly, so a Synaps with a newer protocol would come in the day one
     shipped;
  2. her binary (a release asset, else a source build: rustup, the pinned toolchain, the distro's
     packages printed and asked for, never a silent sudo);
  3. her art pack (CapriCore v1.0.4 + lachy through `tools/make-pack`, in a venv of its own;
     lachy's Required Notice shown);
  4. the Synaps daemon at login and her session;
  5. the summon key (i3/sway/Hyprland, with consent, backed up, between markers; Windows: Win+Alt+C);
  6. autostart (default on);
  7. `--voice` / `--watch`.
- Flags: `--yes`, `--dry-run`, `--uninstall [--purge]`, `--voice`, `--watch`, `--no-autostart`,
  `--from-source`, `--prefix`, `--with-local-art` (mochi, local personal use only), `--artifact`,
  `--allow-root`. Pins in `installer/manifest`. `docs/INSTALL.md` has the steps and flags.
- Run it as yourself: it refuses root unless `--allow-root` (Linux, root with its own `HOME`), and always
  when `HOME` is someone else's, as `sudo` leaves it. It never runs `sudo` on its own.
- `--uninstall` walks a receipt of everything it added, and leaves Synaps, settings and state.
- A daemon on another protocol, or one that doesn't say which, stops the install (exit 1), with the fix.
  When this run installed or upgraded Synaps and an older daemon is still up, it says to restart that
  one (`synaps daemon stop`, then run it again); an upgrade says so in step 1 too, while the rest
  installs.
- Windows: `capri-syn-setup-x86_64.exe` leaves a working Capri on a PC with nothing installed: Synaps
  (the pinned release), its daemon (one on another protocol is said, with the fix), her art downloaded
  and built on the PC (CapriCore and lachy's, never in the installer; `/PACK="C:\…"` takes a pack from
  his disk instead), her voice from her own release, and the sign-in. A step that can't finish says why
  on the finish page, with the one command to retry. install.ps1 runs the same steps
  (`packaging/windows/steps.ps1`). It's per-user (Inno Setup, no admin): her windowed exe (no console
  window), her AUMID with an icon and a Start-menu shortcut carrying it, autostart at sign-in, a clean
  uninstall. An install from `synaps-avatar-setup-x86_64.exe` (the same AppId) upgrades in place.
  `docs/WINDOWS.md` → The installer.
- Her voice on Windows is a download of its own, `capri-syn-voice-setup-x86_64.exe`; her own installer
  stays small. Its "models" task fetches Whisper, Kokoro, Silero and the wake word's models (~320 MB,
  SHA-256 pinned, resumable) with `voice-models.ps1`.
- A `v*` tag (`.github/workflows/release.yml`) builds the Linux tarball (on Ubuntu 22.04: glibc 2.35 and
  newer; older systems build from source) and the two Windows installers, each with a `.sha256`,
  smoke-tests them through the installers, and attaches them to a draft release. Each release also
  carries `install.sh`, `install.ps1`, `SHA256SUMS`, her Windows exe's symbols, espeak-ng's source and a
  build-provenance attestation. From 1.0.0 the asset names are frozen: the installers fetch them by
  name.
- Release notes come from this file (a version's own `## X.Y.Z` section), not a list of merged PRs.

### Her
- **A new personality, written from scratch.** Her persona's first paragraph is rewritten clean-room
  from the show's canon: a construct stitched together from many human minds, gleeful and fun-seeking,
  cocky, teasing, with a quiet side that watches the sky. She's firmly on her user's side, never
  possessive, and still keeps to one or two plain-text sentences. The Synaps and capri-watch paragraphs
  are unchanged.
- **Her persona is everyone's.** `packs/capri/persona.md` names no one and no machine ("your user",
  they/them), and it says what runs her only when asked: one plain sentence, no pitch, no links; for a
  job too big for a bubble she mentions once that `synaps --attach capri` opens the same conversation
  in a terminal. A personal persona goes in `[session] persona_file`.
- **Her first words**, once in her life: "Hey. I'm Capri. Tap $MOD+C when you want me. Headpats also
  accepted." Nothing about what runs her. Not for anyone who ran her before (`state.json` `met`).
- **Her tag shows her state only:** `ONLINE`, `SLEEPING`, `THINKING`, `NEEDS YOU`… instead of
  `CAPRI // ONLINE`. The unread pill drops her name too (`1 NEW  <first words>  READ ▸`). WORKING keeps
  its red block, with the label on it. Notifications, the Line and the reader keep her name.
- **The HUD moves as one system.** One curve (ease-out, 2 % past the target at most, no wobble) and one
  set of durations, named in `src/hud.rs`: a size change 200 ms, the bubble opening 240 ms (180 ms out
  of the tag), into the tag 150 ms, a close in place 120 ms, a label swap 120 ms (cut at its middle), a
  glitch 2 frames.
  - Never an empty shell, never faint or doubled words. A plate or bubble stays opaque while it
    reshapes; its outline morphs into the new one (shared edges stay straight). Words cut, never fade
    through each other: on one frame the old go and the new are there, readable (60 % or more, whole a
    frame later). The glitch is a 2-frame RGB split (whole, then half) along the border's steep sides on
    that cut frame, never on text.
  - The tag (and the pill, and the unread chip): the plate's width eases to the new text from wherever
    it is (it leads a longer label, waits for a shorter one); the label and WORKING's red block cut
    together, every change with its split. A label shows at least 350 ms; changes meanwhile wait, and
    only the latest shows (no strobe). Its place is pinned on whole pixels while she stays put.
  - The bubble grows out of whatever tag is showing (its label at the centre until the shell covers the
    reply, which cuts in then), or from her (90 % at her tail) when none is; grows and shrinks to its
    reply, a new line showing only once there is room for it. A new reply cuts in whole at once
    (streaming text never glitches). It goes back into the tag that will show (closing or collapsing to
    the pill): its words on its first frame, the tag's label from the next as its box eases in.
  - `[hud] glitch` and `[hud] motion` (both default on). Alpha mode only: under the X shape (no
    compositor) the HUD changes at once. The Line and the menu don't glitch.
- **Asleep, she stays put.** While Sleepy (no turn for `--sleepy-after`) or in the away nap, nothing
  moves her on her own: no roam, explore, micro-hop, bid, focus hop, workspace follow, dodge, startle,
  look or lean, and her drives (attention, the roam clock) stop. A move already due between two polls
  is held too.
  - He can still carry or throw her: she sleeps where she lands (lying if it fits, else sitting; on the
    floor Sleepy's entry plays again, the nap's pose comes back). A drop on his window is never "left"
    for covering it; a throw in her sleep is no offence (no sulk, no far move). A pat, a drag or the
    menu don't wake her; while she naps, input within 10 s of handling her isn't "he is back".
  - She wakes for a new turn or a prompt (the mood leaves Sleepy), $mod+c, the Line, the field or
    typing to her, a summon, or him being back after the nap. Waking restarts the session's quiet
    clock, plays the stand-up (`sit_floor-neutral` → `floor_stand`; on a window top a 0.6 s sit-up)
    before any move ($mod+c's teleport to the Line waits for it), then 20 s of grace before her own
    moves resume.
  - Still allowed asleep: her window closing (rescue or fall), her perch leaving with the workspace,
    riding a moved window, falling off a covered one.
- **The new art about half the time.** Window perch: CapriCore's sit and lachy's perch come up 1 : 1,
  and the pose shifts in place every 2–6 min. Floor sleep: CapriCore's lie and lachy's doze 1 : 1.
  Window sleep is CapriCore's (laydown is at weight 0). Weights only, in `packs/capri/moods.json`.
- **Her right-click menu** stays in her work area and keeps off her face, placed once before it shows;
  if her pose then puts her face under it, it moves once, across her face (never off-screen). Her
  maker's mark is one dim row under QUIT, her Synaps and her model as Settings names them
  (`SYNAPS 0.12.0 · SONNET 4.6`). Not a button.
- **When she can't start, she says so.** A start that fails where no one reads her log (the Start menu,
  the app menu, autostart) says so on screen too, once, as she exits. On Windows it's a message box
  titled capri-syn: one sentence (a missing or broken pack: install capri-syn again) and where her log
  is. It goes away when she exits, after 2 minutes at the most. On Linux it's a critical desktop
  notification (she waits up to 2 s for it). Nothing more for `--check`, `--set`, `-h`, `-V`, a bad
  command line, or one that's already running. On Linux a terminal doesn't get it either (a virtual
  console under a desktop does). On Windows her exe has no console even from a terminal, so it shows
  there too, unless her stderr is redirected (a script's). The installers start her with
  `SA_NO_NOTICE=1` (it keeps it to her log): they say a failed start themselves.
- **Her logs stay small.** Her log starts over past 4 MB while she runs, not only at start, the earlier
  part kept as `.old.log` (one kept). A thread that isn't her UI thread checks it every 30 s. That
  covers Windows' `synaps-avatar.log` and Linux's install.sh `launch.log`; a terminal, the journal or
  your own redirect are left alone. The Windows voice sidecar's output goes through a pipe, so
  `capri-voice.log` starts over as it's written.
- **Her icon** (`assets/icon/`: `capri-syn.ico`, 256 to 16 px, and 512 and 256 px PNGs), in order: your
  own beside settings.toml (`icon.ico` on Windows, `icon.png` on Linux), then her pack's `icon` (Linux
  only), then capri-syn's. Read at start and on a settings reload. Her Windows exe carries the .ico as
  its icon (`build.rs`), and her tray loads its size from there.
- After a failed turn the Line keeps her answer below his question: `synaps-client`'s `Transcript`
  anchors the error note to its turn, so a refold never puts it ahead of the turn's prompt. The Line's
  footer wraps.

### Her brain (Settings ▸ Brain)
- Settings' first row is BRAIN: her model and its login (`✓ CLAUDE · SONNET 4.6`, or `● NO BRAIN YET`
  with a red `SET UP ▸`). It opens Settings ▸ Brain: SIGN IN WITH CLAUDE first, RUN LOCAL, and OTHER
  (ChatGPT, Copilot, Gemini, Grok, Kimi, Muse, API KEY) a step away (Gemini only with
  `SYNAPS_GOOGLE_GEMINI_CLIENT_ID` set or its login on file: Synaps 0.12 can't sign in to it without);
  MODEL once she has a brain, written to `settings.toml` as `[session] model` and set on her session at
  once. The sign-ins copy their `synaps login --provider …` for now. A login made in a terminal shows
  on the open page.
- **No brain yet, in her words.** With no brain her first words say so, with `SET ME UP ▸`; her summon
  key, a click on her or on her tag open the Line on Settings ▸ Brain (Esc: chat); her menu's SETTINGS
  reads `SET UP MY BRAIN`. A turn that fails for want of model credentials shows "No brain yet, choom.
  Synaps has no Claude login. Log in, or set another model ([session] model in my settings):" and
  `$ synaps login` as a copyable chip (tag `NO BRAIN YET`, calm) instead of the daemon's text cut at 90
  characters. A lapsed login, a rate limit, an overloaded provider and an unreachable model get their
  own lines; the Line's footer adds `↑ resends your message`. Anything else is shown raw. A turn that
  fails at once never shows THINKING or ABORT first (a turn shows as started after 400 ms or its first
  word).
- NO BRAIN YET from the files for every Synaps provider, not just Claude: an account's login in
  `auth.json`, an API key there, in Synaps' `config` or in its variable. One table (`provider.rs`) for
  the detection, the bubble's login command and the Brain screen.
- NO BRAIN YET goes when he sets up a remote broker (`auth.remote_endpoint`, `SYNAPS_AUTH_ENDPOINT`)
  instead of logging in: a read that can't tell any more takes back what a read said. The daemon's own
  no-brain error goes too once the files read otherwise than when it spoke. A file caught mid-write
  changes neither: she reads again. Any sign of a working brain clears it at once, whatever the files
  say: a word, a thought, a tool call or her model's token count (a subagent's on another model doesn't
  count). After that no read puts it back, a re-attach to the same daemon neither; only the daemon's
  own error, a model change or a new daemon process (a restart: she reads again) does.
- MODEL follows her session when his TUI changes it (`/model`: the daemon's `setting_changed` to every
  client), so picking her model back sends it; a Set's reply lost for 10 s doesn't leave MODEL on the
  asked model. MODEL is refused under `--model` / `--session` whoever asks, and the page goes back on
  the model in force; with `[brain]` off its write reads `[session]` off her UI thread.
- The open Brain screen looks again every 5 s: a sign-in to any provider in a terminal gets its ✓
  without reopening it (and a login landing mid-look looks again); the row picked stays on its provider
  when a look adds Gemini or drops a row. A look reads Synaps' `config`, `auth.json` and the daemon's
  environment once, not once per provider.
- The keys under the Brain screen say what ⏎ does on the row (copy the sign-in, use it, close), and
  nothing under `--socket`.
- API KEY shows `IN USE · <service>` / `KEY SET · …`, and acts on what it says: with keys set here, ⏎
  switches her to one (`⏎ use it`: the first, also on a key service with no key while another is set;
  on one of several, `IN USE · GROQ +1` and `⏎ the next key`); on the only one, or none, it copies
  `synaps login` to set one. A service's key counts as Synaps counts it: `<id>` with `type: api_key`
  and a key in `auth.json`, the config's last line, and a config key only once the daemon has started
  since it was written (by 2 s or more: closer, no file time or clock tells them apart, so it's unknown
  until a restart (or, for her own model, her first answer), not a ✓). A key counts where the daemon
  has it: in its environment when that can be read (Linux), not one only in hers.
- RUN LOCAL's server: the config's last `provider.local.url`, the daemon's `LOCAL_ENDPOINT` (Linux), one
  deadline for the name lookup, the connect and the reply. `provider.local.url` counts as Synaps reads
  it, at the daemon's start: the one she saw there for this start stands when his TUI rewrites the
  config (`/model`, `/thinking`, `/theme`); one she never saw for it is taken as written. A config she
  can't read keeps what she knew.
- A local server that answers (with her model, if she's on one of its) is asked again after 5 s, then
  less often while it answers the same (to every 40 s); one that's off, or without her model yet, every
  5 s (he may be starting it, or pulling it). A Settings page opened (even mid-look) or a new URL asks
  at once. At most two name lookups left hanging at a time; an address isn't looked up.
- A local URL changed while the daemon runs: Synaps reads it at its start, so it keeps trying the old
  one. When its error names another URL than the one she shows, Settings ▸ Brain drops the ✓ and says
  `Synaps is still on its old local URL. Run synaps daemon reload, then ask me again.` (until she
  answers or the daemon reconnects).
- No credentials in her log or bubble: a URL is shown without anything up to its last `@` (a password
  with `/`, `?` or `#` in it too), a `?query` or a `#fragment`, in what she logs and in Synaps' errors
  (it quotes its local URL whole). A URL she shows stops at a query's first `?#&=;` after its last `@`.
  The probe never sends them, and never sends a `user:pa` (an unencoded `/`, `?` or `#` in a password)
  to DNS; on `localhost` the connect's time is split between 127.0.0.1 and ::1.
- `avatarctl settings open brain`.

### Voice
- **The default voice:** W3, a blend of stock Kokoro voices (`timbre|prosody` split, pushed away from
  af_sarah, `+clean`), at `@+7.00` (about 306 Hz median), 23 % faster (speed 1/0.77, pace "stretch"),
  flow "emphasis". Stock Kokoro voices and our own DSP only. A `voice` set in `settings.toml` is kept.
- Pace "stretch": Kokoro speaks at its own speed, and the time goes ×0.77 in the same rubberband pass as
  the pitch shift. This keeps Kokoro's speed-1 pitch shape: Kokoro's own speed 1.32 drops the pitch floor
  ~1.4 st and the sentence ends ~2 st deeper. `[voice] pace = "model"` gives that instead; there
  Kokoro's speed scales time linearly, unrounded durations rounded once (no 25 ms steps).
- Flow: Kokoro's graph is patched in memory (weights and voices untouched, guarded by SHA-256, plain
  Kokoro as the fallback) to read and steer its own duration, pitch and energy predictions.
  - A `*word*` in her reply is stressed: a longer vowel, more energy, and a closed-loop pitch accent.
    Its local target is 4–6 st over the speech around the word, capped at 400 Hz. At her pitch it
    rides close to that cap: any planned peak over ~267 Hz in the model leaves no room, so most pitch
    bumps are cut short and the stress is mostly length and energy (raising `emph_st` doesn't change
    it).
  - The stretch after the word is lowered.
  - A yes/no question keeps its rise.
  - The `pauses` and `full` presets exist, but are off.
- New settings: `[voice] flow` = `emphasis` (default), `pauses`, `full` or `off`; `[voice] pace` =
  `stretch` (default) or `model`. `avatarctl settings reload` re-sends changed voice settings to the
  sidecar.
- `*word*` reaches the sidecar; `**bold**` and stray asterisks don't. The bubble and the caption never
  show the asterisks; the Line shows the word in italics. With voice on, her persona gains a few lines
  on writing for the ear. They allow one stressed word in a reply, and say most replies need none.
- With no flow graph (another model file, or the patch didn't load) and no rubberband (always so on
  Windows), a voice with an `@` shift speaks plain Kokoro at its own pitch, as docs/VOICE.md says, and
  the log says once that the shift is off.
- **"Hey Capri" starts a conversation** (`[voice] wake_handsfree`, default true, with `handsfree` on):
  the wake stream goes on as the hands-free one in the same capture (no gap), so "Hey Capri, what time
  is it?" is the conversation's first turn (the phrase cut) and "Hey Capri" alone just listens.
  Every hands-free rule holds: each pause sends, spoken replies, half-duplex/barge-in, the end phrases.
  Quiet for `handsfree_timeout` (60 s, counted down on her tag for the last 10), it ends and the wake
  word listens again at once. `wake_handsfree = false` (or `handsfree = false`): one turn per "Hey
  Capri".
- **"Hey Capri" from across the desk.** The check decodes 2.8 s of audio, the window its thresholds were
  measured on. The second look (his whole turn, decoded once he pauses) gets every rejected candidate
  whose margin is over −8, and it passes at its own thresholds: the words say "hey capri", `T > −10` and
  `margin > −3`. With the request after the phrase, Whisper reads far and casual speech better. The log
  gets the second look's two scores (never words), and says when its words held no "hey capri".
- **"Hey Capri" is never sent as his words.** Whisper decoding the phrase again in the first hands-free
  utterance can write "Take a breath", "Aye, capri", "And capri", "Ee capri". The phrase is cut as
  Whisper spells it, doubled or misheard as the first word or two; words with no trace of it are the
  phrase misheard, and the audio after the check's window decides (no speech there: nothing is sent,
  hands-free goes on; speech: that audio is decoded on its own).
- **Her tag says ONLINE while the wake word listens.** The idle wake stream doesn't replace her tag with
  `MIC ON · SAY "HEY CAPRI"`. Her usual tag stays (`ONLINE`, her mood, `LISTENING` for music), and while
  the wake word's mic is open its dot is a mic glyph, so the always-open mic is still visible. `SPEAKING`
  and `HEARD` show while the wake word listens.
- **Toggle-to-talk:** `avatarctl talk toggle` (proposed i3: `$mod+Ctrl+c`; Windows: Win+Alt+Ctrl+C):
  one tap opens the mic, his pause sends it (`[voice] silence_s`, 1.0 s; ×1.5 while he has said under
  1 s). A second tap sends now (cancels before he spoke); Esc or `talk cancel` cancel; no speech in 8 s
  closes it; 60 s sends. Holding the chord is one toggle. Tag `● MIC 0:03 ▮▮▯ · AUTO`.
- The mic refuses on a secret (not an approval), the lock, and for a toggle her hidden; a secret
  appearing or the lock closes any open mic, hiding a toggled one. Hard caps in the sidecar: hold 120 s,
  toggle 65 s. A test sidecar (`CAPRI_VOICE_TEST`) refuses a real or unset source.
- **The talk key's fine print.** A `talk tap` doesn't wait out a press that lost its `talk release`. A
  tap is ignored only while a hold is his: the press is under 1 s old (its mic may still be opening), or
  its mic is asked for or open (up to 120 s, the sidecar's hold cap). Any other unreleased press (voice
  off, no sidecar, a binding that never sent `talk release`) is cleared by the tap, which then opens or
  closes the Line as usual. A press or a tap within 60 ms of a release is taken for an X auto-repeat
  (one key-down); a `talk press` while one waits for its release still waits out 120 s.
- **Duck, don't pause.** `[voice] duck = "duck"` is the default: browser video and music go down −18 dB
  (raw ×0.5 per channel) while he talks and while she answers, then back to exactly their old
  per-channel levels. Never her own streams, module loopbacks, calls (role or `[music] ignore`),
  notification sounds; nothing at all while another app captures a mic. Streams that start or unpause
  mid-duck are ducked too; his own change wins; same stream only (`object.serial`); WirePlumber's
  per-app restore of a ducked level is undone (for a stream that comes back, and for a sibling stream of
  an app still ducked); a crash mid-duck is repaired at the next start (state written atomically).
- **No junk from near-silence.** Silero VAD v5.1.2 gates Whisper: at least 6 speech frames (192 ms)
  peaking at p ≥ 0.70, else nothing is transcribed or sent (DIDN'T CATCH THAT). The clip is trimmed to
  the speech (+400 ms / +200 ms). `tools/capri-voice-setup` fetches `silero_vad.onnx` (SHA-256 checked).
- Junk transcripts are dropped, in every mode: no letters (`-2,` `,` `2.`), ≤ 2 letters unless a real
  short word (`nd,` `Q.`), one vowel-less word (`, chr,` `hmm`), stock hallucinations and fillers,
  subtitle credits (at the line's start), the prompt echoed; `*cough*` tags cut.
  `thank you`/`bye`/`take a breath` count as filler under 300 ms of speech, sign-offs
  (`see you next time`, `thanks for listening`) under 600 ms. Heard/dropped lines carry Whisper's
  confidence.
- Whisper's window is never under 10 s: on trimmed clips the smaller formula window looped ("b, b, b,
  b") and garbled most short sentences (56 % word errors on 30 test sentences; 3 % with the floor).
  Short words don't take 2–7 s or come back as loops; no temperature retries; a token cap. A lone "stop"
  is heard in ~0.7 s.
- **Her replies to what arrives on its own are spoken** (`[voice] speak_unprompted = true`, default): a
  reply she makes without him typing first: events (hooks, `synaps send`, comms, summons) and the
  session driver (`event_auto`, `driver_auto`, live turns only). Turns he typed stay silent. Unknown
  triggers stay silent.
  - First lines only: two sentences / 30 words / up to `---`, then "The rest is on the line." once.
  - Silent over an approval or a secret, while music plays (skipped, not queued), with his mic open,
    when hidden for fullscreen; the sidecar adds calls, the screen lock, do-not-disturb, 10 s per
    reply, 20 s per 60 s and 15 s staleness (`say-skipped`). Any of these mid-reply stops her for that
    turn.
  - Such a reply waits behind her last sentences instead of cutting them off; one waits at most (newest
    wins). No duck or pause for these. Events never open the mic.
  - With voice on, she connects to the sidecar at start and every 10 s after it went away.
- **A reply she says whole is read.** A spoken reply heard to its last word counts as read: no unread
  strip, no `N NEW` (the Line and `state`'s `unread` agree: they are the one strip). His spoken turns'
  replies and events' alike, hands-free or not.
  - Cut off (a tap on her, Esc, music, a new turn, his voice over hers, the sidecar's gate, the sidecar
    going or restarting mid-say) or said in part (past the cap or 60 words, after `---`, code, a table,
    a link) it stays unread. `$mod+c` cuts her too, but opens the Line, so the reply is read there as
    seen. `docs/VOICE.md`, "Heard whole is read".
  - The strip counts each missed reply and shows the newest; one heard whole after its bubble went drops
    only itself.
- **Speech-to-text vocabulary:** Whisper's recognition prompt is generic
  (`Capri, choom, preem, Night City, Synaps, avatarctl`; on Windows the word `capri-ctl` instead of
  `avatarctl`: only a word in the prompt, no such tool ships). Add your own with `[voice] vocab = ["…"]`
  in `settings.toml`: the words are appended to the prompt and count for the prompt-echo filter too (so
  no everyday words). Up to 16 entries of ≤ 32 characters, 200 characters in all; past that, the first
  that fit are kept and the rest are logged, not an error (a word already in her prompt still takes a
  slot). An empty entry, a comma or a control character is a settings error naming the entry.
- Her pronunciation fixes are built into the sidecar (an update replaces them); docs/VOICE.md lists
  them.
- Linux: she records with `pw-record --latency 20ms`. A `CAPRI_VOICE_SOURCE` that's gone doesn't keep
  the wake word off till a settings reload.

### Windows
- **She runs on Windows 11 (x64):** `src/platform/windows`, GPUI's Windows crate vendored with small
  patches (`vendor/gpui-pre-windows/PATCHES.md`). Per-monitor DPI aware (v2); her windows keep one scale
  on every monitor. Monitor, DPI and work-area changes are noticed at once.
- **Her keys:** Win+Alt+C is her summon key (a tap opens the Line), Win+Alt+Ctrl+C is toggle-to-talk.
  `[voice] hold_to_talk` (opt-in): the summon key talks while held. The release is read every 25 ms from
  her key thread, with no keyboard hook. A tap opens the Line as before.
- **The Line's keyboard follows Windows' rule:** her key (Win+Alt+C) or a click on her takes it at once;
  any other opening (a summon, her control socket, voice) shows the Line without it, and her tag says
  `WIN+ALT+C TO TYPE` until he presses her key or clicks the Line. She never takes the keyboard from his
  window, never flashes, never injects input (GPUI's Alt trick patched out).
- Music from the media flyout (SMTC): playing or quiet, and whether he's watching a video.
- Notifications are toasts (her AppUserModelID registered under HKCU; `docs/WINDOWS.md` lists what a
  package must provide).
- Perch targets on another virtual desktop are left out; a desktop switch is noticed at once.
- Effect layers reaching outside her window get windows of their own (click-through, never activated).
- **An overlay doesn't take her seats.** The touch keyboard's handwriting canvas (`TabTip.exe`) keeps a
  topmost tool window the size of the primary monitor, invisible and click-through, whenever its service
  runs. She took it for a window over every other one and fell to the floor there. A window clicks pass
  through (layered and `WS_EX_TRANSPARENT`, or layered at alpha 0: an overlay, the canvas, a game or GPU
  overlay, a recorder's frame) is left out of the windows she sits on or hides behind. It isn't a
  fullscreen app either, nor the window he's in when it has the foreground. `TabTip.exe` is also counted
  with the shell's processes. A window that isn't his and covers a whole monitor goes in her log once
  (its class, exe, style and rect, and whether it was left out), and once more if it turns
  click-through or back. The shell's own hosts (Start, Search, Task View, Alt+Tab) are said only when
  click-through.
- **Her icon in the tray.** It sits in the notification area while she runs. Its tooltip is
  `capri-syn · <her status>` (ONLINE, SLEEPING, NO BRAIN YET, HIDDEN, ...). It changes when her status
  does; her mic's clock and level meter stay off it, and hands-free's last seconds read ENDING.
  - A click opens the Line, as Win+Alt+C does. A right click opens her menu: the same rows as her
    right-click menu, from one model (`src/menu.rs`), with the same greying of what can't act now.
    Hidden, the tray is the way back (SHOW). An ATTENTION pick that didn't save says so under its
    choices (CAN'T SAVE / NOT SAVED YET), as her menu does under its row.
  - Explorer restarting puts it back. QUIT takes it out, with no ghost left, a quit cut short by the
    watchdog too. An icon the shell won't take at sign-in is tried again every 2 s for a minute.
  - Its menu is drawn as her own menu is: her red words on jet black, the row under the pointer ink on
    her red blade (its corner cut), what can't act now dim (a dim line around it when the arrow keys stop
    on it), a dot on ATTENTION's choice in force, her dark red frame (square on Windows 11). DND OFF and
    END HANDS-FREE have no tick: the words say what a click does. Sized at the DPI of the monitor it
    opens on, in whole pixels at 100–200 %. High contrast on: the stock menu in your theme's colours.
  - Keyboard: arrows, Enter, Esc, → into ATTENTION; a typed letter goes to the row it starts (a second
    row with that letter: it lights, a second press moves on). MSAA (what NVDA and JAWS read Win32 menus
    through) reads each row's words, as it reads the stock menu's; Narrator is still to check by hand.
    Each row's item data is an `MSAAMENUINFO` with its words too, for a reader that goes by that.
- **Her files:** settings in `%APPDATA%\synaps-avatar`, packs and state in `%LOCALAPPDATA%\synaps-avatar`.
  `$XDG_*_HOME` still wins. Without a console she logs to
  `%LOCALAPPDATA%\synaps-avatar\synaps-avatar.log`.
- **No VC++ runtime needed.** Her exe links the C runtime in (`+crt-static`, `.cargo/config.toml`), so a
  PC without the Visual C++ Redistributable runs her; `build.ps1` lists her exe's DLL imports and stops
  on a C runtime one. Her voice bundle carries the runtime beside its Python (`msvcp140.dll`,
  `msvcp140_1.dll`, `vcruntime140*.dll`, from Visual Studio, unmodified, under the Visual Studio
  licence's Distributable Code) with Microsoft's end-user terms for it
  (`licenses\vc-runtime\LICENSE.txt`): onnxruntime's and espeak-ng's DLLs import `MSVCP140.dll`
  (onnxruntime `MSVCP140_1.dll` too), and the sidecar can't load them without it. Its build stops when a
  DLL imports what neither the bundle nor a clean Windows has (a pinned list of System32's DLLs), or a
  runtime older than the MSVC that linked it (`voice-imports.py`), and CI checks the installed bundle
  loads its own copies, espeak-ng's as phonemizer loads it.
- **Her voice on Windows.** She starts the sidecar while `[voice] enabled` is on: the bundle beside her,
  in a job object that kills it when she ends however she ends, restarted after 1, 2, 4 … 60 s, its log
  in `%LOCALAPPDATA%\capri-voice`. Connecting to it runs on a thread (a stale endpoint on a silent port
  doesn't freeze her).
  - The link: loopback TCP after a two-token handshake (`endpoint.json` in `%LOCALAPPDATA%\capri-voice`),
    since stock CPython has no AF_UNIX there. `CAPRI_VOICE_ENDPOINT` overrides the file on both ends.
  - `capri-voice` runs through `tools/capri_voice_win.py` (Linux never loads it): PortAudio (WASAPI)
    capture and playback; SMTC pause, and a session-volume duck with Linux's rules and crash repair; the
    lock, calls (the mic privacy store) and do-not-disturb.
  - `packaging/windows/voice-bundle.ps1` builds the 143 MB folder the voice installer carries (CPython
    3.13 embeddable + wheels, no models): the sidecar runs with no Python installed.
  - The pitch matches Linux's within 0.1 st (no rubberband there: the shift is in-model, the pace is
    Kokoro's). The floor is 2.3 st lower, the sound brighter, and the name/number test set slips more
    (7.5 % vs 1.7 % WER). Numbers in docs/VOICE.md. Bundling rubberband there is a follow-up.
- **Her mic by name** (`[voice] mic`): the microphone she listens on, by its WASAPI name
  (`Microphone (USB Audio Device Pro)`, not the two lines Sound settings shows; `docs/VOICE.md` says how
  to see the names). Her installer's Voice page writes it. A new one on a settings reload starts her
  voice sidecar again on it. A control character or a name over 256 characters is a settings error
  (`--set` exits 2).
  - A mic that connects after she started (a Bluetooth headset after sign-in) is found without a
    restart. On a miss the sidecar reads the device list again (at most every 10 s, never while a stream
    of hers is open). "Hey Capri", refused for a mic that's gone (`WAKE PAUSED · ITS SOURCE IS GONE`), is
    asked for again every 30 s, so it comes back by itself; a talk press that finds the mic brings it
    back as soon as the press ends.
  - A named mic that drops while "Hey Capri" listens (a headset out of range): `WAKE PAUSED · MIC LOST`,
    asked for again every 30 s (it can't fall back to the default, so asking is safe). The stream that
    ended marks the device list stale, so the next look reads it again, once per streak of failures (a
    mic held by another app doesn't restart PortAudio every 10 s). The default mic keeps its rule:
    `WAKE OFF · MIC LOST` till a reload. The sidecar logs the wake word's refusal once per reason.
- No `avatarctl` on Windows: the summon key, her menu and her tray do what it's for (`docs/WINDOWS.md`,
  Control). Scripts can send the same one-line commands to her socket from PowerShell 7.

### Linux
- **GNOME, KDE and other Wayland desktops, through XWayland.** A bare `synaps-avatar` on a Wayland
  desktop that isn't Hyprland, with XWayland running (`DISPLAY` set), runs as `SA_BACKEND=x11` does and
  says so in one log line. With no XWayland it stops with an error that says so. `SA_BACKEND` always
  wins: `x11` is XWayland, `wayland` is Hyprland or nothing (with no Wayland session, an error). Another
  value is ignored, with a line in her log.
- Hyprland is a `HYPRLAND_INSTANCE_SIGNATURE` naming one that runs on this display, or
  `XDG_CURRENT_DESKTOP` / `XDG_SESSION_DESKTOP` saying Hyprland while one does, for her binary and
  `install.sh` alike. One runs while the PID in its `hyprland.lock` is a Hyprland that runs (a crash
  leaves the lock and its sockets, and its PID may be reused), on the display the lock names; with no
  lock, her binary asks its socket. Hyprland before 0.40 (in `/tmp/hypr`) isn't read. A signature or
  desktop variable left over from an earlier Hyprland login (GNOME's environment, a tmux server's)
  doesn't read as Hyprland (on GNOME she takes XWayland); a Hyprland whose signature got lost doesn't
  fall back to XWayland unasked. What she picked and ignored goes in her log after its header.
- The XWayland limits (docs/INSTALL.md): she perches only on X apps, sees the pointer only over X
  windows, and her presence counts only input X apps get, so she may nap while you type in a native
  Wayland app.
- `install.sh` puts her in your app menu as Capri (`~/.local/share/applications/synaps-avatar.desktop`,
  with her icon, `--no-autostart` or not), so once she's quit she starts from there, as from Windows'
  Start menu. In the receipt: `--uninstall` removes it, and her autostart entry, while they carry the
  header it wrote (KDE's menu editor drops it: an entry edited there stays, and her autostart entry's
  `TryExec` keeps it from starting anything once she's gone). An entry there that isn't hers is backed
  up first. Her autostart entry is named Capri too.
- Summon key: on GNOME it offers a custom keyboard shortcut, Super+Alt+C running `avatarctl talk tap`,
  when that's free and only when you say yes on the terminal (`--yes` doesn't answer it). It's added to
  your custom shortcuts (yours stay), recorded as `gkey PATH`, and `--uninstall` takes it out (with no
  gsettings, it keeps the line for later). Both read GNOME's settings back: when they don't hold the
  change, a warning and the `gsettings` commands to run yourself (and `--uninstall` keeps the line for
  next time). On Hyprland, when Super+C is taken (the stock config's killactive) it offers Super+Alt+C
  instead.
- `avatarctl state` → `line.waiting` (`false` unless the Line opened without the keyboard and is waiting
  for his key or click; Windows only today), beside `line.parked`. The summon key on a parked Line
  counts as `waiting` only when the desktop refused the keyboard; any other failure (she is gone) is
  logged as itself (`line: summoned, but she is gone`).
- The MPRIS error line says `mpris:` again, and the Line's hint names the summon letter you set.
- No tray yet: HIDE's chip is the way back. A StatusNotifierItem would plug in at
  `platform::linux::tray`, for X11 and Hyprland both.

### capri-watch
- **No hostname magic:** the role comes only from `watch.toml`.
  - `role = "local"` (the default) is her machine. It delivers here and watches GitHub.
  - `role = "remote"` is any other machine. It delivers over ssh to `[deliver] ssh_host`, which it
    requires (there is no default host).
  - `[github] repos` is empty by default.
  - `backup_failed` watches nothing until `unit` names one. Its `log` is optional, and `log_start` marks
    a run's start in it.
  - The calendar's `db` has no default path.
  - `install.sh --watch` delivers locally unless `watch.toml` says `remote`; it never guesses the role
    from the hostname.
  - A two-machine setup sets `role`, `host` and `[deliver] ssh_host` in each machine's `watch.toml`
    (docs/WATCH.md has an example). A copy from before v1 (`role = ""`, or one of the two old machine
    roles) stops with an error that says the file predates v1, instead of guessing; so does `ssh_host`
    with local delivery (a forgotten `role = "remote"`). `install.sh --watch` checks the file first and
    warns, rather than enabling a watcher that won't start.
  - Its first log line lists only the events that really run: `ci_failed` with no repos, `backup_failed`
    with no unit and `calendar` with no db show as on but not configured. With GitHub off (a remote
    machine, or `enabled = false`) its events aren't listed at all.
- `tools/synaps-avatar-remote` needs its host (`synaps-avatar-remote <host>`).

### Licences
- capri-syn is MIT (`LICENSE`, © 2026 Haseeb Khalid), except the paths README's Credits & legal names
  (GPUI, parts of synaps-client, `docs/media`, the icon, the wake-word model), which keep their own
  terms.
- **Every download carries its licences.** The Linux tarball and both Windows installers hold
  capri-syn's `LICENSE` and the notices that go with what's inside them: GPUI's Apache-2.0 licence and
  our patches to it, synaps-client's notice, the icon's, and a `THIRD-PARTY-LICENSES.txt` listing every
  Rust crate compiled into that build with its licence text, and Lucide's licence for the icons built
  in. The wake-word model's `NOTICE` sits beside it. The voice installer also lists every Python package
  it bundles with its licence, carries espeak-ng's, whisper.cpp's and PortAudio's licences, and says
  where the source of its GPL parts (espeak-ng 1.52.0, phonemizer) is: espeak-ng's is attached to every
  release, with that of sonic, which espeak-ng's build compiles into its DLL.
- The app icon (`assets/icon/`) is not covered by the MIT licence, and no licence is granted for it: a
  crop of a frame of Cyberpunk: Edgerunners 2, which belongs to its rights holders. capri-syn is an
  unofficial, non-commercial fan project, not affiliated with or endorsed by them (`assets/icon/NOTICE`).
- Her persona's first paragraph is clean-room, not a paraphrase of CapriCore's GPL-3.0 `PERSONALITY.md`.
- Smaller downloads: of the PortAudio builds sounddevice ships, the voice installer keeps only the one
  she loads (no ASIO builds, no other platforms'); her binary doesn't compile in syntect and two-face's
  syntax definitions (she never highlighted code with them).

### Docs
- The README is new for v1.
- docs/ says what she does at v1: her Settings page, menu and Windows tray; Settings ▸ Brain as the way
  to a brain; what Windows has instead of `avatarctl` (WINDOWS.md's Control, with a PowerShell 7
  script); Known issues in INSTALL.md; which Whisper models setup fetches; `--set`'s exit codes; no
  one's names or machines.
- `docs/design/windows-voice.md`: the options for voice on Windows, sized.

### For developers
- CI: Linux and Windows (build, unit tests, the fake contract suite, clippy); a `voice` job on both (the
  unit tests and the selftest; on Windows every wheel installed binary-only, the bundle built, the tests
  run again with the bundled interpreter); the installers (shellcheck, PSScriptAnalyzer, the two scripts
  kept in step, dry runs, `install.ps1` end to end on windows-latest); shots on a Windows runner.
- The contract has an `overlay` scenario (the fake and the Windows rig make one); a Windows unit test
  puts the canvas, as found, over a window of his.
- `SA_HUD_PROF=1` logs frame intervals and main-thread frame times while the HUD moves.
- `avatarctl talk tap`: a press and its release at once, for a key that only reports its press.
- Sidecar protocol: `say … "unprompted": true`, `drop`, `say-skipped`, `ready.features`; a `say-end`
  carries `"cut": true` when its player ended early (nonzero exit: killed from outside, its sink gone) or
  the gate trimmed it to the 10 s cap (an event's long first sentence), and she counts it short (an
  older sidecar's plain `say-end` is whole). Windows: the player's `wait` reports a stream that didn't
  play all it was given.
- `tools/fakevoice.py` (a fake sidecar, no audio); the fake daemon's event turns are `event_auto`, plus
  `trig T TEXT` and `resolve`; it runs on Windows too (the daemon's named pipe).
- `capri-voice --render` (`|||` separates replies) and `--selftest` cover the voice rules. Details are in
  `docs/VOICE.md`.
- `tools/test_capri_voice.py` doesn't read your `CAPRI_VOICE_SOCK` / `CAPRI_VOICE_ENDPOINT`, and
  removes its temp folder.
- `tools/make-pack` copies `persona.md` into the pack. `capri-voice-setup` takes PyPI's pywhispercpp
  wheel where the distro has none.

### Known issues
- A dark slab on X11 without a compositor (#72): now and then her whole window's rectangle paints solid
  black behind her and her bubble, covering what's under it, more often while a prompt hangs below her.
  So far it has only been seen with no compositor running, where she cuts her window to her outline
  with the X shape. Workaround (likely; not yet confirmed): run a compositor (picom: `picom -b`). With
  `[display] composited = "auto"`, the default, she switches to alpha when it starts, no restart.
- GNOME, KDE and sway run her through XWayland, with its limits (docs/INSTALL.md). None of them has been
  tried on a real session yet: CI and the test containers stand in for them.
- Hyprland has no idle time for her yet: your presence is the pointer moving, so typing alone for 5 min
  reads as away. Hyprland before 0.40 isn't supported: she takes XWayland there.
- Signing in is a terminal step: Settings ▸ Brain copies the `synaps login` line; she doesn't run it
  herself yet. On Windows, Synaps 0.12's `synaps login` opens a sign-in tab cut at the URL's first `&`:
  copy the whole URL it prints into your browser, or run her installer's Claude sign-in (install.ps1
  puts it there too; the default folder shown):
  `powershell -NoProfile -ExecutionPolicy Bypass -File "$env:LOCALAPPDATA\Programs\synaps-avatar\steps\steps.ps1" login`
- Windows: the installers are unsigned, so a browser download meets SmartScreen ("More info", then "Run
  anyway"). Windows 10 and Windows 11 on Arm are untested. A click on one of her notifications does
  nothing (as on Linux). Her voice brings its own VC++ runtime but hasn't yet run on a PC without the
  Visual C++ Redistributable. Live audio is still to check on real hardware: her desktop checks ran in
  a Windows 11 VM, with no real audio.
- Linux has no tray icon yet: HIDE's chip is the way back there.
- With `XDG_RUNTIME_DIR` unset, she and her voice sidecar look for the Linux socket in different places:
  she in the temp dir (`$TMPDIR`, else `/tmp`), the sidecar in `/run/user/<uid>`. Set it, or set
  `CAPRI_VOICE_SOCK` for both. It isn't aligned yet because that would change how she behaves.
