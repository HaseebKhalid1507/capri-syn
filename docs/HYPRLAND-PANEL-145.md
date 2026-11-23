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

## Change (fixed, pending confirmation on Hyprland)

Only `Role::Line` (`capri-line`, including Settings) opts into live dimensions in
`LayerStage::place`. It converts device dimensions to positive logical pixels with
`(v / scale - 1e-3).ceil()` (the tolerance prevents float noise adding a pixel).
**Width is live too**, following placement instead of remaining frozen at opening width.
Reader-follow and effect-host `place` remain margin-only, as before this PR. Effect hosts'
existing `Window::resize` retains its original size/scale calculation. The pure scope test
covers all six namespaces; no Reader-follow behavior change is included.

The vendored `layer_ctl::set_bounds` queues GPUI's drawable resize without waiting for
configure. Like `PlatformWindow::resize`, the callback runs outside the active App/frame
borrow. Size and margins are queued with it, then a forced redraw presents the resized buffer
and geometry together. There is no geometry-only idle commit: Hyprland would stretch the old
120 px buffer to the new dimensions for a frame. The stale-configure guard covers the drawable
as well as the geometry echo, so an older reply cannot restore a stretched old buffer.
A subsequent `Window::resize` replaces the remembered request on this managed surface;
unmanaged surfaces are not opted in. Configures are still acknowledged.

No X11, Windows, xdg-toplevel/popup or other-compositor behavior changes. The shared Line height
helper is a verbatim extraction of the original truncate-before-clamp expression, not a new
policy on other backends. These panels stay on Overlay, outside tiling. No installer/config
change is needed: `install.sh` already confines its summon binds/autostart to `block_put`
markers, but a `windowrule` cannot size a layer surface or repair this bug. No rule is installed.

## Verification

- Thirteen pure Rust tests cover bootstrap → chat → Settings → shrink; 100%, 125%, 150%, 200%
  scale conversion and float noise; nonzero requests; Line-only scope; stale configure buffer
  and echo protection; later `Window::resize`; unchanged unmanaged policy; and the original
  truncate/clamp logic. The impossible `(0, 0)` **geometry echo** test is gone (the caller
  clamps geometry first). Zero *requested* sizes remain tested at the conversion boundary.
  The three pure modules can be tested with `rustc --edition 2024 --test` from a wrapper.
- `platform::hypr::panel_test::contract_hypr_panels` opens the **real Line** and Settings with
  deterministic transcript/draft, without art, daemon, login or audio. It uses the production
  height helper (`lineview` truncation and work-area/minimum clamp) and `beside::place`, not
  a raw `ceil(height * scale)` request. It covers fresh chat, fresh Settings and both swaps;
  layer/viewport height (chat ≥180 px, Settings ≥300 px); visible bottom controls; overlay
  membership, absent from xdg clients; delayed height stability; live width (780→620); and a
  subsequent `Window::resize` to 640×700 that stays there after another configure/frame.
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

On the **Intel-render-node Linux box, over real seatless SSH**, in the checked-out PR worktree:

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
    timeout --kill-after=5s 90s "$BIN" \
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
ten bounded settling loops of at most 5 seconds (six page checks, two widths, two explicit
resizes), plus ~1.4 seconds of delays and rendering/IPC time. The 90-second watchdog also
catches startup/event-loop hangs. Rig startup has its own 30-second readiness bound; teardown
normally takes <6 seconds. Allow about two minutes total, excluding artifact download.

This change was prepared without Cargo or running anything on bella. The earlier local rig
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
