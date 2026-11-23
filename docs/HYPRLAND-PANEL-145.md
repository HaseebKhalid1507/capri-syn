# Hyprland panel height (#145)

## Cause (1.2.0)

Both screenshots show the **same Line window**, once on chat and once on Settings. It is
approximately 120 px high, with about 114 px inside its 3 px ring/frame inset on each edge.
This is the application's bootstrap size, not a 100% scaling error or a compositor minimum.

The path is:

1. `lineview::open_line_on` requests `LINE_H0 = 120` logical px in `WindowOptions`.
2. `Os::window_options` lets `LayerStage::dress` replace the normal window kind with a
   `capri-line` **overlay layer-shell** surface, anchored top-left, exclusive zone -1. Settings
   is a page of that same surface. There is no xdg-toplevel to tile, no app-id window rule to
   match, and no xdg min/max hint involved. GPUI sends these bounds as layer `set_size` before
   the initial empty commit; Hyprland's layer configure drives GPUI's viewport/buffer size.
3. `Line::render` lays out absolute, unconstrained content and defers `Ask::Height` when its
   measurement changes. `line_ask` clamps that height to the work area (and a 60 px floor),
   then `line_follow` calls `Os::place` with the new width and height.
4. **`LayerStage::place` discarded both dimensions (`_w`, `_h`) and only sent margins.**
   The core recorded the new size in `LineWin::at`, but neither `Window::resize` nor another
   layer `set_size` followed. There was no reason for Hyprland to configure a larger surface.
   X11's `place` sends width/height; Windows has its own sizing path.
5. The frame still clips children to the viewport. Chat's field and Settings' lower rows
   therefore exist below the visible 120 px surface. This looks like input/settings failing.

No trace from the reporter is available, so a particular configure sequence on their machine
has **not** been observed. The code explains both screenshots without a tiling or 0x0 configure.
GPUI treats a layer configure with either dimension zero as no size change; a positive configure
resizes its buffer. Initial configure commonly precedes the content measurement, which is why
sending the *post-layout* size matters. A hard-coded initial height or min-size hint would mask
rather than fix the missing resize.

The bubble is drawn inside the body's host, not this separately measured panel. Body hosts
are created at their output size; effect hosts start at 1×1 and `host::fx_hosts_sync` explicitly
calls `Window::resize` when their bounds change. The reader estimates its height before opening.
Those paths do not rely on a Line measurement reaching `place` to escape a 120 px bootstrap.

## Real run of 5c89162: a second cause (output selection)

The first real run supplied by the maintainer (Hyprland **0.57.2**, 100% scale) confirmed
that post-layout growth works, but the field still disappeared **off-screen**:

- Body on **eDP-1**, 2560×1600 at (0,0), body (760,914) size 500×708.
- App log: `line open at 1169,1455 780x120 (0x14) in 401 ms, right of her`.
- `hyprctl layers`: `capri-line` on **DP-2**, 2560×1440 at (2560,0),
  at **(3729,946), 820×669**. The terminal made DP-2 the focused monitor.
- **3729 − 2560 = 1169**: correct local offset, wrong output.
  **946 + 669 = 1615**, 175 px below DP-2's bottom, exactly where the field went.

`LayerStage::dress` computed an output origin for margins but did not set `display_id`.
Unlike body/effect hosts (`host::display_for`), per-moment windows never supplied it.
GPUI's `WaylandClient::open_window` resolves that id to a `wl_output`; with no id,
`WaylandSurfaceState::new` called `get_layer_surface(..., None, ...)`, leaving output selection
to Hyprland (the focused monitor). There was a second hazard: choosing an output from the
panel's centre can select the wrong one when the bootstrap bounds cross an edge.

A layer's output cannot change after creation. Previously an open Line followed by changing
margins only, so moving her to another host also left the panel on its original output.

## Change (height growth confirmed; output fix pending real-run confirmation)

Only `Role::Line` (`capri-line`, including Settings) opts into live dimensions in
`LayerStage::place`. It clamps requests to positive **logical** pixels. Hyprland's core
`layout.sf` is 1 (GPUI has no primary display); GPUI alone scales the drawable buffer.
Dividing dimensions, margins or input regions by the fractional output scale again was wrong.
`Os::adopt` now registers hosts from the stage's named monitor, not buffer-scaled window bounds.
**Width is live too**, following placement instead of remaining frozen at opening width.
Reader-follow and effect-host `place` remain margin-only, as before this PR. Effect hosts'
existing `Window::resize` retains its original size/scale calculation. The pure scope test
covers all six namespaces. Reader dimensions still use their existing path; output binding
and recreation now apply to Reader too.

The vendored `layer_ctl::set_bounds` queues GPUI's drawable resize without waiting for
configure. Like `PlatformWindow::resize`, the callback runs outside the active App/frame
borrow. Size and margins are queued with it, then a forced redraw presents the resized buffer
and geometry together. There is no geometry-only idle commit: Hyprland would stretch the old
120 px buffer to the new dimensions for a frame. The stale-configure guard covers the drawable
as well as the geometry echo, so an older reply cannot restore a stretched old buffer.
A subsequent `Window::resize` replaces the remembered request on this managed surface;
unmanaged surfaces are not opted in. Configures are still acknowledged.

### Explicit output and edge containment

- `Os::panel_options` invokes Hyprland's `Stage::pin_output` for every per-moment creation:
  Line/Settings, Reader, chip, cinematic intro and cut-in. Other backends use the default no-op.
- Startup, dormant/effect hosts and panels all select the output by **wl_output v4 name**,
  matching `hyprctl monitors`. `WaylandDisplay::uuid` already exposes UUIDv5(name); the vendor
  bridge resolves that identity to `DisplayId` without changing upstream display geometry.
  The earlier `output_id(body)` dependency is removed: an implicitly created body no longer
  disables every panel. `host::options` is shared by production and the contract. A genuinely
  unadvertised/removed output is still an error, not permission to use the focused output.
  At startup, `host::when_ready` retries the name lookup every 50 ms for up to 5 seconds,
  yielding to GPUI's event loop so wl_output.name/done can arrive. Successful retries log
  the elapsed wait. On timeout, `fatal(startfail::Why::Other, …)` logs the requested name
  (including an empty name) and GPUI's actual display list with names, removes the bound
  control socket and exits. The monitor cache is not refreshed before the watcher starts:
  an unplugged/unadvertised output fails startup rather than waiting invisibly forever.
  The application startup closure runs once after resolution; other backends remain immediate.
  `main.rs` explicitly sets `sf=1` for per-output hosts, independent of GPUI's primary display.
- Fractional-scale evidence from review: 2560×1600 @1.25 has Hyprland bounds 2048×1280, but
  GPUI divides by integer wl_output.scale=2 and reports 1280×800. Body centres beyond that
  smaller box used to get no display_id. Name identity is independent of this discrepancy.
  Surface records and host IDs use names, not origins; margins use the current named origin.
- Monitor geometry/reservations are cached and invalidated on socket2 layout events. Repeated
  `body_area`/`place` calls do not issue two synchronous IPC requests per follow. Work area is
  `HMon::work_area` from merged #151, not a second reserved-area implementation.
- `Stage::body_area` uses that host's monitor minus Hyprland's reserved areas, not the focused
  monitor. The Line's bootstrap bounds and every subsequent `place` are fitted to all four
  usable edges, reducing height if needed. Core measurement/placement uses the same area;
  a layout-change event rechecks an already-open Line even if her body has not moved.
- On a body-host change, panels are recreated **after landing**, never on each crossing
  during drag/flight. This keeps #152's drag-release keyboard holder alive. Line reopens at
  its previous `l.at.3` height (clamped on the new output), not the 120 px bootstrap.
- Draft, sent history, page, parked/waiting state and focus-return owner survive. If the old
  window is already gone, the retained Line entity supplies the draft. If another Line has
  opened during the defer, the old draft is saved in the avatar without replacing its field.
  Placement caches advance only after `Os::place` succeeds. Reader retains text/scroll/focus;
  chip reopens. Intro uses the current body host, as its comment now states.

### Keyboard sequencing dependency (#152)

Every #148 grab uses `Os::grab_keyboard`: initial Line open, landing reopen (including the
unparked restore), Reader open/reopen, and deferred-open race paths. `LayerStage::dress`
creates all layers with `KeyboardInteractivity::None`; no new direct Exclusive call is added.
The shared held-press guard belongs to #152, not a second guard here.

Pending until #152 merges: rebase and verify its choke point queues every grab while a
press is held, runs the pending grab at release, **and demotes any not-yet-mapped Exclusive
layer to None at press start**. A grab immediately after `open_window` can map up to about
400 ms later; a new press can start between the request and mapping. Deferring panels to
landing alone does not close that race. The #152 diff inspected for this update did not yet
contain that guard; no claim is made that the race is fixed at this head.

### Input handoff limits (explicit)

The ordinary chat field commits its currently displayed IME preedit via `unmark_text` before
reading it; no message is submitted. Its UTF-8 selection and direction/caret survive. With an
already-destroyed window, copying the displayed text into a fresh input commits the visible
preedit without carrying stale IME state. Undo history and scroll/selection in secret fields
are not transferred. **An unsent API key (Settings ▸ Brain) and a partly typed hidden prompt
answer are dropped on output migration**; they must be re-entered. On a live-window handoff
the old fields are cleared before removal; on window loss their entities are dropped. They
are never copied into the ordinary draft or persisted/logged. Preserving these secret inputs
**in memory only** across output handoff is a follow-up, not a capability claimed by this PR.
The follow-up must keep secret text out of ordinary drafts, logs and persistence.

No X11, Windows, xdg-toplevel/popup or other-compositor behavior changes. The shared Line height
helper is a verbatim extraction of the original truncate-before-clamp expression, not a new
policy on other backends. These panels stay on Overlay, outside tiling. No installer/config
change is needed: `install.sh` already confines its summon binds/autostart to `block_put`
markers, but a `windowrule` cannot size a layer surface or repair this bug. No rule is installed.

## Verification

- Fifteen pure Rust tests cover bootstrap → chat → Settings → shrink; fractional-output
  logical sizing/margins, negative origins; nonzero requests; Line-only scope; stale configure buffer
  and echo protection; later `Window::resize`; unchanged unmanaged policy; and the original
  truncate/clamp logic. The impossible `(0, 0)` **geometry echo** test is gone (the caller
  clamps geometry first). Zero *requested* sizes remain tested at the conversion boundary.
  Added the live report's coordinates, reserved bars, an over-height panel, negative output
  coordinates and output-change/recreation policy. The three pure modules can be tested with
  `rustc --edition 2024 --test` (or plain `clippy-driver`) from a wrapper.
- `platform::hypr::panel_test::contract_hypr_panels` opens the **real Line** and Settings with
  deterministic transcript/draft, without art, daemon, login or audio. It uses the production
  height helper (`lineview` truncation and work-area/minimum clamp) and `beside::place`, not
  a raw `ceil(height * scale)` request. The harness uses production `layout.sf=1`, not the
  fractional drawable `window.scale_factor()`. It covers fresh chat, fresh Settings and both swaps;
  layer/viewport height (chat ≥180 px, Settings ≥300 px); visible bottom controls; overlay
  membership, absent from xdg clients; delayed height stability; live width (780→620); and a
  subsequent `Window::resize` to 640×700 that stays there after another configure/frame.
  The rig asserts **three outputs**: 2560×1600 @1, 2560×1440 @1 and 3072×1920 @1.5 (logical
  2048×1280). Host creation uses `host::options`, exactly as production, including a centre
  outside GPUI's integer-scale bounds on the third output. Focus is opposite her body;
  Line must map inside the body's monitor block in `hyprctl -j layers`, never the focused one.
  It checks an over-height/off-screen request, moves her to the second host while Settings is
  open, recreates the Line with its draft/history/page, and checks Reader/chip/Intro binding.
  Reader/chip/Intro use stand-ins; chat and Settings are the real views. These output assertions
  fail on 5c89162, even though that head's height-only assertions can pass. The fractional
  output asserts viewport/layer dimensions agree. Its body is at (6620,500), bottom 1208,
  wholly inside HEADLESS-3 (logical bottom 1280); its centre still lies outside GPUI's
  integer-scale x bounds. A virtual-pointer probe checks the 1.5x body's logical input
  rectangle at its near/far inside edges and outside edges, plus empty-region pass-through.
  `contract_hypr` also creates its body via `host::options`. A handoff exercises composed text,
  reversed selection, then a closed-window draft read. **This contract is pending**: no rig
  or app build was run locally; the maintainer will schedule it after CI builds the harness.
- Atomic presentation is implemented client-side and stale-buffer policy is unit-tested;
  the contract checks settled geometry, **not a captured per-frame visual proof**. Use
  `WAYLAND_DEBUG=client` and/or a recording in the isolated rig if checking for transient stretch.

### Exact rig run (prebuilt test harness; no Cargo on the rig host)

Normal Linux CI now publishes **hypr-panel-contract-linux**, containing the **synaps-avatar
unit-test executable**, renamed `synaps-avatar-tests`. This is not `synaps-avatar`, the normal
application, and not the `synaps-client` harness. The archive preserves executable permissions,
records `SOURCE_HEAD` (PR head), `HEAD` (CI's possibly synthetic merge), `SHA256SUMS`, and `ldd.txt`.
Use the run ID for this PR's latest head, once its artifact is available; no waiting/polling is
part of this change. A separately prebuilt matching `target/debug/deps/synaps_avatar-HASH`
executable works too. Do not select a stale cached hash or the `.d`/`.rmeta` files.

For a separately authorized future run on the **Intel-render-node Linux box, over real
seatless SSH**, in the checked-out PR worktree (not run during this follow-up):

```bash
# RUN_ID is the compile/unit CI run for this PR head, reported with the push.
export RUN_ID=REPLACE_WITH_REPORTED_CI_RUN
export ART="$HOME/.cache/capri-hypr-145/$RUN_ID"
mkdir -p "$ART"
gh run download "$RUN_ID" -R HaseebKhalid1507/capri-syn \
  -n hypr-panel-contract-linux -D "$ART"
tar -xzf "$ART/hypr-panel-contract.tar.gz" -C "$ART"
export BIN="$ART/hypr-panel-contract/synaps-avatar-tests"
(cd "$(dirname "$BIN")" && sha256sum -c SHA256SUMS)
test "$(cat "$ART/hypr-panel-contract/SOURCE_HEAD")" = "$(git rev-parse HEAD)"
ldd "$BIN"   # Resolve any missing Linux runtime libraries before starting the sandbox.

# The rig guards SSH/seat, Intel PCI 0000:00:02.0, memory/load and no running game.
# Dependencies: bwrap, sway, Hyprland/hyprctl (Lua-config capable), Mesa Intel EGL/Vulkan,
# dbus-daemon, python3, jq, coreutils, DejaVu Sans Mono + the binary's ldd dependencies.
export RIG="$HOME/.cache/capri-hypr-panel-rig"  # dedicated path, removed by down.sh
export LOGS="$ART/rig-logs"
test ! -e "$RIG"  # don't tear down someone else's existing rig on an up.sh failure
(
  set -euo pipefail
  trap 'tools/wlrig/down.sh "$LOGS"' EXIT
  tools/wlrig/up.sh
  tools/wlrig/run.sh env SA_CONTRACT_HYPR=1 RUST_BACKTRACE=1 \
    timeout --kill-after=5s 150s "$BIN" \
    --ignored --exact --test-threads 1 --nocapture \
    platform::hypr::panel_test::contract_hypr_panels 2>&1 | tee "$RIG/out/panels.log"
)
```

The rig strips the outer environment, so pass `SA_CONTRACT_HYPR` (and optional
`WAYLAND_DEBUG=client`) **after `run.sh env`**, not before. The test refuses an ordinary desktop
runtime. Use an absolute binary path outside `/tmp` (private inside the sandbox). Do not
bypass the SSH/seat or GPU guards. No daemon credentials or real conversation are needed.
`down.sh` retains logs at `$LOGS` and removes the dedicated rig directory.

Expected contract time is a few seconds on a healthy rig, **not yet measured** here. There are
bounded settling loops of at most 5 seconds, plus a 5-second output-discovery bound,
delays and rendering/IPC time. The 150-second watchdog also catches startup/event-loop hangs.
Rig startup has its own 30-second readiness bound; teardown normally takes <6 seconds.
Allow about three minutes total, excluding artifact download.

This follow-up was prepared on avante without Cargo or building/testing/launching anything
on bella; the live evidence above was supplied by the maintainer, not collected here. The earlier local rig
attempt failed with `sway: command not found`; `Hyprland` is absent too. At base a31885e CI
compiled but did not run `tools/wlrig`; this remains a manual contract on the provisioned
seatless Intel host. The revised contract has **not** been claimed as a successful live run.

## Reporter confirmation if the rig remains unavailable

Use a build from this PR and provide, with both chat and Settings open in turn:

- `hyprctl version`, `hyprctl -j monitors`, `hyprctl -j layers`, `hyprctl -j clients`;
- `avatarctl state` (the `line` object suffices: redact transcript/draft-related data), and a
  screenshot showing the full panel including the field or last Settings row;
- whether `SA_BACKEND` is set and whether the process has `WAYLAND_DISPLAY` and
  `HYPRLAND_INSTANCE_SIGNATURE` (do not send the rest of the environment).

If it is still clipped, a short `WAYLAND_DEBUG=client` startup/open/close trace identifies the
`zwlr_layer_surface_v1.set_size`, `configure` and `ack_configure` sequence. Use no sensitive draft
or conversation and redact other protocol data. We need the requested/configured sizes and
ordering, not credentials or the contents of their config. Their existing window/layer rules
are useful only if these sizes diverge or they explicitly force the XWayland backend.
