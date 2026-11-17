# Voice on Windows: the capri-voice sidecar there

**Status:** decided: **A, the bundled Python sidecar** (Jawz, for Haseeb). **V1a** (VM-free, CI-checked)
built the link and the sidecar's Windows layer, and a script that builds the bundle; what it is, and what
is left, comes first below. The options as written before the decision follow, for the record. Sizes are
agent-days and guesses **[G]**. **[checked]** marks a fact read from a source (named). **[VM]** marks
what needs a real Windows desktop with audio (his machine: the capri-win VM has no real audio and no GPU).

## V1a: what is built

### The link: loopback TCP with two tokens
- **Linux: unchanged.** The Unix socket (`$CAPRI_VOICE_SOCK`, else `$XDG_RUNTIME_DIR/capri-voice.sock`),
  no hello. The Rust side moved behind `platform::voice_link()`; the log lines read the same.
- **Windows: loopback TCP, picked over a named pipe.** A pipe would carry his ACL by itself, but the
  sidecar's server is sockets and threads, and stock CPython has no pipe server without pywin32 (DLLs
  and a post-install step in the bundle) or an asyncio rewrite of its loop. TCP keeps one code path,
  so the link's tests run on Linux too.
- **The handshake:**
  - The sidecar binds `127.0.0.1` on a port the OS picks. It writes `endpoint.json` (the port, `token`,
    `reply`: fresh 256-bit hex each start) atomically to `%LOCALAPPDATA%\capri-voice`, which only he and
    the admins can read.
  - She sends `{"op":"hello","token"}` first. The sidecar answers `{"ev":"hello","token": reply}`, and
    only then takes her as its avatar, dropping the one before (as on Linux).
  - A connection without the right hello within 2 s is closed and never displaces her. She drops a peer
    whose reply is wrong or missing within 1 s, so a stale file whose port another program took leads
    nowhere: that program gets a dead token and can't send the reply.
  - The file is removed on a clean exit. `CAPRI_VOICE_ENDPOINT` overrides its path on both ends.
- **The trust boundary is Linux's:** anything running as him can read the file, as it can open the 0600
  socket there.
- **Tests:**
  - Rust (Windows CI): the handshake both ways; a wrong reply; a silent listener; no file; and against
    the real Python end (`capri_voice_win.py --test-serve`).
  - Python (both OSes): right, wrong, silent and oversized hellos; the file going with the sidecar.

### The sidecar's Windows layer: `tools/capri_voice_win.py`
Linux never imports it. On Windows `capri-voice` takes these names in place of its Linux ones, with
the same contracts:

| piece | Windows | CI | [VM] |
|---|---|---|---|
| capture | sounddevice (PortAudio) WASAPI shared mode, auto-converted to 16 kHz mono s16, 20 ms blocks, shaped like the `pw-record` process (`mic_chunk` is the read seam on both OSes) | the seam; the call's shape (no device on the runner) | his mic: open latency, the level, a hold end to end |
| source moved | none needed: a PortAudio stream never follows a new default; a removed device ends it (`mic lost`) | — | unplug a headset mid-hold |
| playback | `RawOutputStream` 24 kHz, shaped like `pw-play` (stdin write/close, `wait` with timeout, `kill`) | the call's shape | her voice; a hush mid-sentence |
| duck | WASAPI session volume (pycaw) × 0.125 (−18 dB, if session volume is a linear gain). Linux's rules: never her own process, system sounds, the `[music]` ignore list, or anything at all in a call; inactive sessions are ducked when they start (a 0.5 s poll while held); his change wins; the same state file and crash repair; a session that died ducked and comes back at our level gets its original | the rules and the crash repair on fake sessions | **by ear**; Spotify, a browser, Discord ignored |
| pause | SMTC (winrt): pause what plays; on release, play only what she paused that is still paused | the rules on a fake; the real SMTC call answers `[]` on the runner | Spotify, Edge, the Media Player app |
| "in a call" | the mic privacy store: `HKCU\…\CapabilityAccessManager\ConsentStore\microphone`, an app whose last use started and hasn't stopped (her own Python excluded); re-read every 0.5 s while a hands-off stream is open | the reading (packaged, desktop, hers, stopped) | Teams, Discord, the Sound Recorder |
| screen locked | the input desktop isn't `Default` (`OpenInputDesktop`; a UAC prompt counts too). No new protocol: the avatar's WTS events aren't needed | answers on the runner | lock with Win+L mid-hold |
| do not disturb | `SHQueryUserNotificationState`: busy, D3D full-screen, presentation | answers | **whether Windows 11's Do not disturb shows there** (no documented API) |
| headphones | the default output's name (`Headphones (…)`, `Headset Earphone (…)`). The form factor property is the better answer: a later refinement | the names | his devices |
| test devices | `CAPRI_VOICE_SOURCE` / `SINK` match device names; a missing one is never the default | `node_missing` | — |
| wake filter | `WakeForced` loads the wheel's `whisper-<hash>.dll` (pywhispercpp 1.5.1's file list) | — | the wake word end to end |

In `capri-voice` itself only seams changed:
- `WIN` and the Windows paths (`%LOCALAPPDATA%\capri-voice\{models,media.json}`);
- `mic_chunk` at the three read sites, and `_accepted` for the link;
- `WIN` branches where the capture and the player start, in the watchdog, and in the call watch.

On Linux every branch takes the old path, with the same commands in the same order (the `--selftest`
and the unit tests run on both OSes).

### CI: the `voice` job (ubuntu-24.04 and windows-latest, CPython 3.13)
- `tools/test_capri_voice.py` (18 tests) and `capri-voice --selftest` (no models: its rule checks).
- **Windows also:**
  - the bundle's whole wheel set, `--only-binary :all:`, imported;
  - the binding smoke checks;
  - `packaging/windows/voice-bundle.ps1` builds the bundle, and the bundled interpreter runs the imports,
    the selftest and the unit tests again.

## Packaging

### The bundle: `packaging/windows/voice-bundle.ps1` (built and run in CI; not in the installer yet)

```
voice\python\          CPython 3.13.17 embeddable (python.org, SHA-256 pinned), python313._pth → ..\site-packages
voice\site-packages\   tools\capri-voice-win-requirements.txt, wheels only (versions.txt: what was installed)
voice\capri-voice, voice\capri_voice_win.py
run: voice\python\pythonw.exe voice\capri-voice
```

The models are never in it. They go to `%LOCALAPPDATA%\capri-voice\models`, as `capri-voice-setup` fetches them on
Linux (≈ 320–370 MB: Kokoro int8 92 + voices 28 + Whisper small.en 190 or base.en 60 + Silero 2.3 +
the wake word's 2.4).

### Wheels for Windows (win_amd64, CPython 3.13) **[checked: PyPI, and CI installs them binary-only]**

| package | version (latest) | Windows wheel | notes |
|---|---|---|---|
| numpy | 2.5.5 | cp313 win_amd64 (12.6 MB) | |
| onnxruntime | 1.32.0 | cp313 win_amd64 (16.7 MB) | CPU provider. `onnxruntime-directml` 1.24.4 / `onnxruntime-windowsml` 1.30 exist as opt-ins |
| kokoro-onnx | 0.6.1 | py3-none-any | **Python < 3.14**: the bundle is 3.13 |
| phonemizer | 3.6.0 | py3-none-any | kokoro-onnx's (not `phonemizer-fork`) |
| espeakng-loader | 0.2.4 | py3-none-win_amd64 (9.4 MB) | carries espeak-ng's DLL and data (GPL-3.0: as on Linux, a separate library) |
| pywhispercpp | 1.5.1 | cp313 win_amd64 (1.4 MB) | whisper/ggml DLLs and msvcp140 inside |
| sounddevice | 0.5.6 | py3-none-win_amd64 (1.0 MB) | carries PortAudio |
| pycaw / comtypes / psutil | 20260927 / 1.5.0 / 7.2.2 | any / any / cp313 win_amd64 | |
| winrt-runtime + Windows.Foundation(.Collections) + Windows.Media.Control | 4.1.0 / 10.0.28000 | cp313 win_amd64 / any | |

**Nothing is missing:** every package has a Windows x64 wheel for CPython 3.13. **Missing for ARM64 Windows:**
pywhispercpp has no win_arm64 wheel; numpy, onnxruntime, espeakng-loader, sounddevice and winrt do.
**[VM]:** onnxruntime on a clean Windows may want the VC++ runtime (the runners have it installed): run
the bundle on the capri-win VM before shipping it.

### Into the installer: not wired (documented instead)
Two reasons it isn't cheap:
1. **Size.** An Inno component still sits inside `setup.exe` when unchecked, so every download would grow
   from 9 MB to about 9 + 45 MB: the bundle is 143 MB installed, and the wheels alone are ~42 MB compressed.
2. **Nothing starts the sidecar on Windows yet** (V1b).

The choices, for Haseeb:
- (1) a second, per-user `synaps-avatar-voice-setup.exe` into `{localappdata}\Programs\synaps-avatar\voice`;
  the main installer stays small;
- (2) a component in the one installer;
- (3) the installer downloads the bundle (its first download).

Wiring (2): `[Components] voice` (unchecked) → `[Files] Source: "build\voice\*"; DestDir: "{app}\voice";
Components: voice; Flags: recursesubdirs`, `build.ps1 -Voice` runs `voice-bundle.ps1` first, and
`test-install.ps1` checks `/COMPONENTS=voice`. About ½ day with (1) or (2).

## What is left for voice on Windows [G]

| | agent-days |
|---|---|
| V1b: she starts the sidecar (a job object with kill-on-close, following `[voice] enabled`; its log beside hers) | 1 |
| the models: a PowerShell `capri-voice-setup` (the same pinned URLs and SHA-256s) | ½ |
| the installer: (1), (2) or (3) above | ½ |
| the [VM] / his-machine pass: every [VM] cell above, plus Whisper and Kokoro speed on his CPU, and the fixes | 1–2 |
| hold-to-talk beyond tap and toggle (§6) | ½–1 |
| **total** | **3½–5**, on top of V1a's ~2 (inside the 6–9 of option A) |

## What there is today (Linux)

`tools/capri-voice` is one Python process (~3,200 lines) behind a Unix socket
(`$XDG_RUNTIME_DIR/capri-voice.sock`, newline JSON: `listen-start|stop|cancel`, `say`, `hush`,
`media-hold|release`…; the protocol is in its docstring). Everything runs on the CPU:

- **STT**: whisper.cpp through pywhispercpp (`small.en` q5_1, 190 MB; `base.en`, 60 MB).
- **TTS**: Kokoro v1.0 int8 on onnxruntime (92 MB + 28 MB of voices), with a patched ONNX graph for
  sentence flow (`Flow`, a protobuf rewrite in Python) and an optional rubberband pitch shift.
- **What counts as speech**: Silero VAD v5 (2.3 MB, onnxruntime), then junk filters.
- **The wake word**: openWakeWord's melspectrogram and embedding (2.4 MB), our `hey-capri` head, and a
  Whisper confirmation (`WakeForced`, which reaches into libwhisper through ctypes).
- **Caps**: a hold lasts at most 120 s, a toggle 65 s.

What is Linux-only, and what Windows has in its place:

| piece | Linux now | Windows |
|---|---|---|
| capture | `pw-record` (16 kHz s16 mono, pinned source) | WASAPI shared-mode capture (PortAudio, or direct) |
| playback | `pw-play` | WASAPI render |
| duck | `pactl` sink-input volumes, repairing WirePlumber's per-app memory | WASAPI session volume (§4); Windows also remembers per-app volume |
| pause | `playerctl` (MPRIS) | SMTC `TryPauseAsync`/`TryPlayAsync` (she already reads SMTC, P1b) |
| "he's in a call" | another app captures a real source (`pactl` source-outputs) | active sessions on the capture endpoints (`IAudioSessionManager2`, eCapture) from other processes |
| screen locked | `/proc` lockers + logind `LockedHint` | the avatar already gets WTS lock/unlock: tell the sidecar |
| do not disturb | dunst paused (`busctl`) | **[unverified]** no documented desktop API for Windows 11's Do not disturb; `SHQueryUserNotificationState` covers only full-screen and presentation |
| headphones or speakers | default sink's port and form factor | default render endpoint's `PKEY_AudioEndpoint_FormFactor` (Headphones, Headset) |
| pitch shift | rubberband CLI | rubberband has Windows builds **[unverified]**; a subprocess, as now |
| the link | AF_UNIX | the avatar has AF_UNIX on Windows (`uds_windows`). **Stock CPython on Windows doesn't** **[checked: cpython#77589, the PR still open]** |
| lifetime | a systemd user service | spawned by her, or its own Run key (§7) |

## 1. Bundled Python, or a Rust sidecar

### A. Port the Python sidecar; bundle CPython
- **What:** an audio seam in capri-voice (PipeWire or WASAPI). The Linux path stays as it is, and the
  Windows pieces come from the table above.
- **The link:** 127.0.0.1 TCP with a token file in `%LOCALAPPDATA%`, or a named pipe
  (`\\.\pipe\capri-voice-<user>`). On her side that is a second `LocalStream` kind in `src/platform`.
- **The bundle:**
  - CPython's embeddable zip (~10 MB).
  - Wheels: numpy, onnxruntime, kokoro-onnx, pywhispercpp and sounddevice.
    - kokoro-onnx brings phonemizer and espeakng-loader. espeak-ng is GPL-3.0, loaded as a library.
    - pywhispercpp ships pre-built wheels **[checked: PyPI]**; the win_amd64 one is **[unverified]**.
    - sounddevice bundles PortAudio.
  - A WASAPI session layer: pycaw/comtypes, or our own ctypes.
  - About 200–300 MB installed before models [G].
- **Models:** the same ~370 MB as Linux.
- **Risks:**
  - Two runtimes to ship and update.
  - Antivirus false positives on a bundled `python.exe`.
  - Seconds of cold start (Python plus models).
  - `WakeForced`'s ctypes reach into libwhisper (pybind11's holder layout, the DLL's exports) is the most
    fragile part.
  - pip wheels have to be laid out offline for an installer.
  - `onnxruntime` and `onnxruntime-directml` are separate wheels: one or the other.
- **Size:** 6–9 agent-days [G], one of them with a real mic and speakers.

### B. A Rust sidecar (`capri-voice-rs`, a workspace binary)
- **What:**
  - whisper-rs (whisper.cpp).
  - `ort` (onnxruntime: CPU, DirectML optional) for Kokoro, Silero and the wake word.
  - cpal (WASAPI) for audio.
  - The `windows` crate for sessions and SMTC.
  - It could later replace the Python one on Linux too: one sidecar, one CI.
- **The hard parts:**
  - Kokoro's front end (G2P). kokoro-onnx phonemizes through espeak-ng, so Rust either links espeak-ng
    (C, GPL-3.0, so that becomes the sidecar's licence) or ports a G2P.
  - `Flow`'s graph patch: ship the patched ONNX instead of patching at start.
  - About 3,200 lines of measured behaviour, each threshold re-tuned:
    - endpointing and VAD hysteresis;
    - echo correlation and barge-in;
    - the junk and loop filters;
    - the gate and the unprompted-reply budget;
    - hands-free and the two-stage wake word;
    - the voice clean-up DSP;
    - and the selftests.
- **Gains:**
  - One small binary: tens of MB plus onnxruntime's DLL [G].
  - A fast start.
  - AF_UNIX on both ends, so the link doesn't change.
  - No Python on his machine.
- **Risks:**
  - Behaviour drifts from the tuned Python.
  - The espeak-ng licence.
  - whisper.cpp and onnxruntime builds in CI (cmake + MSVC; `ort` downloads binaries).
- **Size:** 15–25 agent-days [G] for Windows parity. Switching Linux over is more on top.

### C. Rejected: WSL2
The Linux sidecar under WSLg's audio means RDP audio latency, and WSL installed and running on his
machine. That isn't an end-user setup.

## 2. Models: whisper.cpp and Kokoro, CPU or DirectML
- **whisper.cpp** builds with MSVC (AVX2). Its backends are CPU, CUDA and Vulkan; there's no DirectML.
  CPU works as on Linux. Vulkan would use his GPU, and that GPU may be running a game.
- **Kokoro on onnxruntime:** CPU as on Linux, or the DirectML execution provider. DirectML is **in
  sustained engineering**: still supported, but new work has moved to Windows ML, which picks an
  execution provider (NPU, GPU, CPU) itself **[checked: onnxruntime's DirectML EP page]**. Windows ML's
  minimum Windows version is **[unverified]**.
- **VAD and the wake front end** are tiny: CPU always.
- **Options:**
  - (i) CPU everywhere: the same numbers as Linux, no driver variance.
  - (ii) DirectML or Windows ML as an opt-in for Kokoro, once measured on his box.
  - (iii) Vulkan whisper.cpp as an opt-in.
- The VM can't measure any of this (WARP).

## 3. Capture and playback: PortAudio or WASAPI direct
- **PortAudio** (Python sounddevice): the WASAPI host API in shared mode, resampled to 16 kHz. It's
  simple, but Python only.
- **WASAPI direct** (cpal in Rust, or comtypes): event-driven shared mode in the device's mix format
  (often 48 kHz float), resampled to 16 kHz by us.
- **Either way, the stream category matters.** A *communications* stream triggers Windows' own
  "communications activity" ducking (default: other sounds −80 %), which would stack on hers. Her
  streams must be *not* communications, unless option 3b below.
- **Privacy:**
  - While a capture stream is open, Windows shows the mic icon in the taskbar and lists her under
    Settings → Privacy → Microphone. That matches the existing rule: the stream exists only between
    `listen-start` and `listen-stop`.
  - With the wake word on, the icon stays on. That's honest and visible.
  - With "Let desktop apps access your microphone" off, capture fails (or reads silence)
    **[unverified which]**. Her tag should then say so, as it does for the other refusals.
- **Echo:**
  - (a) Today's check: correlate the mic with her own playback; headphones or speakers comes from the
    endpoint's form factor.
  - (b) Windows' echo cancellation, offered to communications streams **[unverified per device]**. Real
    AEC would make barge-in work on speakers, at the price of the system ducking above.
  - Either needs real speakers and a mic.

## 4. Ducking per app: WASAPI session volume
- **Reading:** each app's audio on an endpoint is a session.
  - `IAudioSessionManager2::GetSessionEnumerator` → `IAudioSessionControl2` gives its process id,
    `IsSystemSoundsSession` and active/inactive.
  - `ISimpleAudioVolume` (or `IChannelAudioVolume`) sets it.
  - New sessions arrive through `IAudioSessionNotification`, state changes through
    `IAudioSessionEvents`.
- **Today's rules map one to one:**
  - never her own process;
  - never the system sounds session;
  - never an app on the `[music] ignore` list (by exe name: Discord, Teams, Zoom);
  - nothing at all while another app captures (a call);
  - inactive sessions are skipped, but ducked if they start mid-hold.
- **Restore:**
  - Exactly, and only if the volume is still what she set (his change wins).
  - Windows remembers each app's volume across restarts, so a crash mid-duck would leave an app ducked
    for good. The same repair as Linux is needed: originals saved atomically, mended at the next start.
- **Level:** −18 dB is a PipeWire raw (cubic) number. The session scalar's taper is **[unverified]**,
  so the level gets re-tuned by ear.
- **Pause mode:** SMTC on the sessions she paused that are still paused. The avatar already holds SMTC,
  so she could pause and resume herself and leave the sidecar out of it.
- **Size:** 1–1.5 agent-days [G], plus a real-audio check.

## 5. Wake word and hands-free
- **Portable as is:**
  - openWakeWord's front end, our head and Silero (onnxruntime, numpy).
  - The echo checks.
  - Hands-free's endpointing.
- **The Whisper confirmation** (`WakeForced`) is the fragile part in Python (ctypes into libwhisper). In
  Rust it is whisper-rs's own API.
- **The always-open mic** keeps the taskbar mic icon on. That is Windows' honest signal; CPU and battery
  on a laptop are to be measured.
- **The gate:**
  - lock: from the avatar (WTS);
  - a call: capture sessions;
  - his do-not-disturb: **[unverified]** (see the table). Without it, unprompted replies keep only the
    budget and the caps.

## 6. Hold-to-talk without a low-level hook
Tap and toggle are Windows' default today: Win+Alt+C taps, Win+Alt+Ctrl+C toggles (`docs/VOICE.md`).
`WH_KEYBOARD_LL` stays rejected: a hook in every keystroke's path is what anti-cheat dislikes.

| option | how | anti-cheat / antivirus | UX | size [G] |
|---|---|---|---|---|
| **a. hotkey + key-state poll** | `WM_HOTKEY` (`MOD_NOREPEAT`) starts the hold. Only while held, a thread polls the letter with `GetAsyncKeyState` every 15–25 ms; up → `talk release` | Reads key state only, installs nothing in another process, injects nothing. OBS's hotkeys on Windows poll `GetAsyncKeyState` every 25 ms, all the time (libobs `obs-windows.c` `vk_down`, `obs-hotkey.c` `obs_hotkey_thread`) **[checked: source]**, and OBS runs beside every anti-cheat. That's reputation, not proof | Holding Win+Alt+C while talking is awkward. A chord without Win, or a spare key (F13–F24, Pause), is kinder. On a secure desktop (UAC, the lock screen) the poll reads up, which ends the hold. The press is consumed by the hotkey; the release reaches the focused app as a harmless key-up | 0.5 + VM |
| **b. Raw Input sink** | `RegisterRawInputDevices` (keyboard, `RIDEV_INPUTSINK`) on a hidden window of hers: every key's down and up in the session, without a hook | Not a hook, no injection. But a keyboard input sink is also what keyloggers use: antivirus heuristics may flag it **[unverified]**. It sees all his keys; we'd keep one | An exact up event; mouse buttons too (a side-button push-to-talk) | 1 + VM |
| **c. hold her** | Press and hold on her body to talk; let go to send. Her own window's mouse events | Nothing global at all | Needs the pointer on her; no keyboard | 0.5 |
| **d. toggle (today)** | The press starts, his pause ends it | — | Already there | 0 |

These combine: (a) for the keyboard plus (c) for the mouse, with tap and toggle as the default.

## 7. Lifetime, install, the link
- **Start:**
  - (i) She spawns the sidecar as her child, in a job object with kill-on-close: it lives and dies with
    her, and follows `[voice] enabled`.
  - (ii) Its own Run key, as systemd does now.
- **Install:**
  - A "Voice" component in the installer, unchecked by default.
  - The models (~370 MB) come either from a setup step, like `capri-voice-setup` (the same pinned URLs,
    SHA-256 checked), or from a local folder, like her pack.
  - Today's installer downloads nothing, so voice would be its first download: his call.
- **The link:** A needs TCP or a named pipe (§1). B keeps AF_UNIX.

## Sizes, together [G]

| | agent-days |
|---|---|
| A: the Python sidecar ported, bundled | 6–9 |
| B: a Rust sidecar, Windows parity | 15–25 |
| ducking (inside A or B) | 1–1.5 |
| hold-to-talk: a / b / c | 0.5 / 1 / 0.5 (each + VM) |
| the installer's Voice component (+ model fetch) | 1 |

## For the decision (1 is decided: A)
1. A, B, or A now and B later?
2. Hold-to-talk beyond tap and toggle: none, (a), (b), (c), and which key?
3. Models: fetched by the installer, by a separate step, or from a folder?
4. CPU only, or DirectML / Windows ML / Vulkan as opt-ins?
5. Echo: today's correlation check, or Windows' AEC through a communications stream (and its system
   ducking)?
