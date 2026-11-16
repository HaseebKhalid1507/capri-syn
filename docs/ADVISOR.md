# ADVISOR brief: synaps-avatar (Capri), the hard problems

You're a senior advisor. You read (avante: the repo `~/Projects/synaps-avatar` and the docs in
`~/Jawz/workspace/synaps-avatar/`), you don't build or run anything, and you answer with concrete,
ranked recommendations that the builder (Spike) implements and measures on bella.

**[LOCKED.md](~/Jawz/workspace/synaps-avatar/LOCKED.md) (`~/Jawz/workspace/synaps-avatar/LOCKED.md`, outside the repo) beats everything**: Haseeb's own picks (plain red teleport bolt, a subtle ragdoll
by default (`[ragdoll] strength = 0.5`), calm hanging legs, the "preem" hang bid tagged `baseline/hang-bid-preem`, etc.).
A recommendation that conflicts with a lock is out; say so if you see the conflict.

## 0. Orientation

**What it is.** Capri is a desktop companion: a hand-drawn anime cat-girl sprite (75 frames, ~542×631 device
px at sf 1.083) living on Haseeb's X11 desktop. She sits on windows, sulks, bids for attention, gets summoned
by AI sessions, and (round B) can be grabbed, dragged, shaken and flung like a ragdoll.

**The environment.**
- **bella**: Arch, **i3 on X11 with NO compositor** (picom is off on purpose). Monitors are eDP (laptop panel) and
  DP-3; polybar is 28 px at the top of each monitor.
- 24 cores, 30 GB. She renders on the **boot VGA device = Intel Arrow Lake-S iGPU (0x7d67)** through Vulkan
  (`main.rs:process_setup` pins it; the NVIDIA RTX 5070 Ti Mobile is ignored unless `SA_ANY_GPU=1`).
- **GPUI** from crates `gpui-pre = 0.3.12` + `gpui-kit = 0.9.1`, x11rb 0.13 (shape/randr/xfixes/screensaver),
  Rust 1.100 (the build lane), release `lto=thin, codegen-units=1, strip=true`.
- The avatar is one **override-redirect GPUI window**, shaped with **XShape** (input and bounding rect lists), plus
  small override-redirect "fx" windows (bolt, hearts, shadow) drawn with plain X11 fills.
- Without a compositor, any newly shaped-in region shows whatever the window had before the GPU present lands.

**Repo map** (`src/`, 18.6 k lines):

| module | role |
|---|---|
| `main.rs` | args, GPU pick (`process_setup`), window creation, the GPUI app run loop |
| `view.rs` (4.2 k) | `Avatar`: state, `tick`/`next_deadline`/`schedule` (exact wake-ups, no idle tick loop), `render` (the canvas paint closure that paints the body bands and builds and sends the XShape, ~l.3000–4120), teleports and glitches, HUD/bubble, `move_to`/`apply_offset` |
| `ragview.rs` | the ragdoll's view side: `Rag` state, `rag_held_step`/`rag_flight_step`/`rag_land_step`, `rag_plan_next` (commit-ahead cur/next plans), `rag_painted`, `tick_period` (1/60 s), `SA_RAG_LOCKSTEP`, `rag-prof` |
| `ragdoll.rs` | pure sim: verlet `Chain` (substeps, angle clamps, `bands_with` maps rows to per-band dx/dy), `BandHold` (offset hysteresis), `PumpedSwing` (bid swing), `Flight` (bounces off arenas/platforms), `Strength` |
| `drag.rs` | the held/flight state machine (grab → surprised → embarrassed …, swing, shake → dizzy, release/fling/land), scripted `PointerFeed` |
| `fx.rs` | `BodyPlan` (bands = horizontal slices with dx/dy, solid/split colours, ghosts, squash sx/sy), glitch patterns, `pose_bands` (shear "poses"), bolt geometry, `shape` (rects from a plan) |
| `frames.rs` | decode PNG → crop → resize to device px → `Arc<RenderImage>` + per-row profile/shape rects, face band; `rotate_rgba` (only for music notes) |
| `platform/x11.rs` | x11rb: `set_shape`, `move_to` (ConfigureWindow), fx windows, `stack()`, `work_areas()` (struts), focus/idle/fullscreen senses |
| `brain.rs` / `places.rs` / `moves.rs` / `settings.rs` | the cat brain (pure): modes, bids, sulks, summons, spot choice, settings.toml |
| `brainview.rs` | brain → view wiring: approaches (hang/cling/…), bid marker, `swing_tick`/`swing_bands` (the bid chain swing), `hang_swing` |
| `theme.rs` | the UI's look, one place: the SHARD tokens (`Pal`, a pack's `tones` may override any by name), the cuts (`Cut`: panel, chamfers, slants, tabs, the nameplate's slant/arrow/flag/square/check/split, the unread blade, the danger plate's teeth) as polygons, the HUD glyphs, `tracked` / `metered` labels. No other UI file holds a colour |
| `edges.rs` | exposed top-edge spans of windows (occlusion) for perches/platforms |
| `session.rs`, `control.rs`, `state.rs`, `anim.rs`, `pack.rs`, `shape.rs`, `ticker.rs` | daemon session client, `avatarctl` socket, persisted state, frame sequencing, pack schema, small shape helpers, timers |
| `tools/` | `abuild` (the only build path, on bella), `shoot.py` (stage + recorder + `cpu:` meter + taint guard), `sheet.py`/`onion.py` (contact sheets, onion composites), `slabcheck.py`, `lockcheck.py`, `rag/*.drag` scripts |

**How it's measured.** `shoot.py cpu:N` reads `/proc/<pid>/stat` for our process and for Xorg over N s (wall clock).
`rag-prof:` (stderr at the end of a drag) gives per-tick sim+plan time. Clips are lossless x11grab at 30 fps.
Briefs: `BRIEF-ragdoll.md` (rendering options §2, perf §, RSS), `BRIEF-brain.md`; the history is in `LOG.md`.

## 1. The hard problems (ranked)

### P1. Drag/fling CPU, tick rate and spikes (biggest gap)
**Symptom** (release build on bella, scripted 9 s figure-eight drag, `tools/rag/*` via `avatarctl drag`):
- **Ours:** chain drag **40–44 %** of one core, **Xorg 25–36 %**. Fling to rest: ours 29–33 %, Xorg 16–25 %.
- **The same drag rigid** (`SA_RAG=0`, today's pre-ragdoll drag): **ours 63 %, Xorg 23 %**. The rigid drag costs
  MORE than the chain.
- **Ticks:** only ~**40 ticks/s** (349 in 8.6 s), not 60. That's ~11 ms of CPU per tick, yet the sim+plan p50 is
  0.1–0.4 ms, so ~10 ms per tick is spent elsewhere.
- **Spikes:** sim+plan **p95 ~7 ms, max up to 44 ms**.
- **Idle and holding still are fine:** idle with the brain on is 0.65–0.91 %; holding still with the chain asleep
  is 0.65 % (no ticking at rest).
- **The target:** drag ≤ 20 % ours, Xorg as low as possible, a steady 60 ticks/s, p95 ≤ 2 ms.

**Why it's hard.**
- There's no compositor, so every window move makes Xorg revalidate the shaped window's clip and expose
  whatever is uncovered underneath, and those clients repaint.
- GPUI owns the render loop and the swapchain; we only `cx.notify()` and paint in a canvas closure.
- The shape has to change in lockstep with the pixels (P2), and a tick waits for the render that painted the
  committed-ahead state.
- perf: `perf record -p` is blocked (`kernel.yama.ptrace_scope=1`, no sudo). Running her as perf's child
  through a wrapper used as shoot.py's `--bin` produced a **0-byte perf.data** twice: with `-e cpu-clock`
  ("Failure to open any events"), and again with `-e cpu-clock:u` (`perf_event_paranoid=2`). The cause isn't
  diagnosed: perf may get killed by shoot.py's quit → SIGTERM → SIGKILL, or the event open may still fail.
  The release build is stripped; `--config profile.release.strip=false` works through abuild.

**Architecture.**
- **Per tick:** `view.rs:tick` → `ragview.rs:rag_held_step` (pointer feed → `Chain::step`) → `rag_plan_next` (bands via
  `Chain::bands_held`, a `BodyPlan`) → `move_to` → `apply_offset` → `x11.move_to` (one ConfigureWindow per changed
  origin) → `cx.notify()`.
- **The render:** `view.rs:render` paint closure (~l.3000–3290). It paints each band with
  `window.paint_image(clip, shifted, …)` (one draw per band, ~4–6 px bands, so ~100+ image draws for a full
  figure), plus the commit-ahead "next" plan painted underneath.
- **The shape:** it's built from the plan's rects (`fx.rs:shape`/`body_rects`, frame profile rects in `frames.rs`).
  `set_shape` is skipped when the rect lists are equal (`view.rs` ~l.4108 `last.0 != input || last.1 != visible`),
  but during a drag the bands change every tick, so in practice it's sent every tick, as hundreds of rects
  (per-row profile).
- `ragview.rs:rag_painted` gates the next tick on the paint of `next`.

**Tried so far.** Shape dedup (already there); held band offsets (cut 1 px churn, so fewer shape changes at rest);
per-tick bands computed once (the bid swing). The perf profile wasn't obtained.

**Hypotheses.**
- (a) GPUI does a full re-render + present for every notify, and the Vulkan present on the Intel iGPU without a
  compositor blocks or spins (FIFO). That ~10 ms/tick is render+present, and it also caps ticks at ~40/s.
- (b) The XShape with hundreds of rects per tick is costly in Xorg (region ops, ValidateTree) and in our
  rect building.
- (c) A ConfigureWindow per tick makes GPUI see bounds changes, and that may trigger extra layout/redraw, maybe
  swapchain work. The rigid drag (one image, moves only) costing more points at the move/expose path, not
  the bands.
- (d) The spikes are allocations (Vec per band per tick, `BTreeMap` in `BandHold`, plan clones in
  `rag_plan_next`/render) or atlas uploads when the frame changes.

**Questions.**
1. Given GPUI's X11 + Vulkan (blade) path in gpui-pre 0.3.12, what does a window move (ConfigureNotify on an
   override-redirect window) cost inside GPUI? Does it re-render or recreate anything? And how do we
   move the window without triggering a repaint when the pixels didn't change?
2. Is it better to keep the window still and big (the drag area) and move the sprite *inside* it (paint
   offset + shape offset), trading moves for a bigger shape region? Or to move it and never repaint for pure
   translations?
3. How do we cut the XShape cost: fewer rects (coarser row quantisation, run-merging), `ShapeOffset` for
   translation-only changes, the bounding shape only (no separate input shape), or a 1-bit pixmap mask
   (`shape_mask`) instead of rect lists? Which is cheapest for Xorg without a compositor?
4. How do we profile reliably here without ptrace (a perf child that survives shoot.py's shutdown, or
   `perf stat`/`perf record` with `--` and a clean quit), and which counters matter most (our CPU vs Xorg vs
   the Mesa/Vulkan driver threads)?

### P2. Present-lag black slabs and the commit-ahead design
**Symptom.** With no compositor, when the shape grows before the GPU present of the new pixels lands, the
newly shaped-in pixels show black (un-painted) for one frame: "black slabs". `slabcheck.py` finds them in
clips. Round B: 0 slabs in 21 clips and the reel after fixes. The real one it caught was a teleport glitch
laid at the wrong time: 7c99861 now lays each glitch step at the time it was laid underneath.
4 remaining hits were classified as Capri's dark ink cut by glitch bands (`--ink 6,1,2`).
The cost: everything that changes the silhouette has to be planned one frame ahead.

**Why it's hard.** The shape is applied synchronously by Xorg, but the pixels arrive asynchronously (Vulkan present,
1+ frame later), and GPUI gives us no present-complete callback. We can't set a window background that
matches the desktop, and there's no ARGB without a compositor.

**Architecture.** In the `view.rs:render` comments (~l.3060): "a frame paints, but its present lands up to a frame
later, so wherever the next shape grows, this frame's pixels show for a moment". The answer is commit-ahead:
- the next state's plan and frame are painted *underneath* the current one (`next`, `next_img`, `under`
  smear of `POSE_REACH`);
- the shape is the union, and the rag's `cur`/`next` advance only after `rag_painted`;
- glitches lay step n+1 underneath (`glitch_at`);
- HUD parts join the shape only once painted (31cd3bb).

**Tried so far.** All of the above works, but it doubles paint work during motion and couples the tick rate
to the render cadence (P1).

**Questions.**
1. Is there a robust way to learn when a present has actually hit the screen with GPUI on X11
   (Present extension `CompleteNotify` on our window, `XSync` fences, a Vulkan present-wait extension through
   GPUI's renderer), so the shape is applied *after* the pixels land instead of painting ahead?
2. Alternatively: shape = the union of the last two frames' silhouettes (never shrink until the next present), and
   paint fully on top. Would that remove the need to paint the next state underneath? What are the risks
   (stale pixels in shrinking areas)?
3. Does a window background (`CWBackPixmap` None vs ParentRelative vs a colour) change what un-painted pixels look
   like enough to hide a 1-frame lag on a varied desktop?

### P3. Readable rotation/tumble without image transforms
**Symptom.** The fling's mid-flight tumble frame reads as "an unreadable red blob" (r8 critic). Hanging and
dangling read as shear, not rotation. Critics accept a small amount; LOCKED #5 wants it subtle anyway.

**Why it's hard.** As far as we know, GPUI's `paint_image` has no rotation (affine transforms exist for SVG/paths,
not raster images). Our body is drawn as horizontal bands, each a clipped `paint_image` with dx/dy (a shear
plus vertical stretch). Rotation by more than ~15° shears the art into smears. Pre-rotated frames cost RSS
(P4): 9 angles × ~0.55 MB × each frame. `frames.rs:rotate_rgba` already rotates the small music-note images
on the CPU at load.

**Architecture.**
- `ragdoll.rs:Chain::bands_with` (row → band dx/dy from node positions)
- `ragdoll.rs:clamp_angles` (bend limits scaled by strength: swing ~16°, joint ~7° at 0.5)
- `fx.rs:pose_bands`/`pose_leg` (pose shear, column split for the legs)
- `ragview.rs:rag_plan_next` (flip-card `sy` for the legs-grab upside-down)
- `fx.rs:squash`

**Questions.**
1. Can gpui-pre 0.3.12 rotate a raster in any supported way (an image as an SVG pattern, `paint_svg` with a
   transformation on an embedded image, a custom `PaintSurface`/sprite path, a texture via a path fill)?
   Which file/API?
2. If not: how about a tile mosaic (paint 2-D tiles each translated by the rotation map, i.e. piecewise rotation
   by translation, maybe with Paeth's three-shear idea applied per tile), versus lazily CPU-rotated, quantised
   (e.g. ±8°, ±16°) copies of only the flight frames, cached and evicted? What's the readability/cost
   trade-off?
3. For the flight specifically: should we just cap the tilt (e.g. ≤ 12°) and swap to a dedicated "tumble"
   pose frame from the pack instead of rotating?

### P4. RSS ~160 MB over the 150 MB budget
**Symptom.** Steady RSS is 160–164 MB, both in a drag and at rest (it was 153 at round A, over a 150 budget).
The ragdoll added ~2 extra frames and the chain/plans.

**Why it's hard.** 75 frames decoded to device-size RGBA ≈ 542×631×4 ≈ 1.37 MB each ≈ ~100 MB of pixels, held as
`Arc<RenderImage>` (`frames.rs:decode_one` l.329). GPUI also uploads them to its texture atlas, and on an
iGPU, mapped GPU memory may count in our RSS. All frames are decoded at start
(`frames.rs:decode` on 8 threads, 117 ms) so any frame is instant.

**Questions.**
1. Does GPUI keep the CPU-side `RenderImage` bytes after upload, and must it (atlas eviction → re-upload)?
   Can we drop our copy, or decode lazily (a per-mood frame set) with a small LRU, without first-frame hitches?
2. Which counts more in RSS here: our decoded buffers, GPUI's atlas staging, or Mesa's mapped buffers? How
   would you split it with `/proc/<pid>/smaps_rollup` / `smaps` (anon vs file vs `/dev/dri` mappings)?
3. Is storing frames trimmed to their opaque bbox (they're tall and narrow, with large transparent margins) a
   cheap win for both RAM and atlas space?

### P5. Lockstep pixel determinism vs x11grab, and mixed captures
**Symptom.** `SA_RAG_LOCKSTEP=1` (fixed 1/60 s sim steps, one per tick, `SA_RAG_TICK_MS` slows ticks) gives an
**identical sim trace** across two recordings (368 committed states). But the pixels aren't identical:
285 common states in the same order, plus 2–9 isolated single extra captures per clip, where x11grab caught
the window moved but not yet repainted. In fling-bounce strips, "mixed capture" frames interleave two
positions. The brief asked for pixel-identical lockstep recordings.

**Why it's hard.** x11grab samples the root asynchronously (30 fps), and with no compositor the move (Xorg,
instant) and the repaint (present, later) are separate events.

**Questions.**
1. What's the cleanest deterministic capture here: the app grabs its own frame after each committed state (XShm
   GetImage of the root region once its present is known done) and writes PNGs, or shoot.py drives "step →
   wait for an ack → grab"? Which ack is trustworthy without a compositor?
2. Would moving the sprite inside a static window (P1 Q2) remove the move/repaint tearing as a side effect?

### P6. Xorg expose/repaint cost of a moving shaped window without a compositor
**Symptom.** Xorg spends 16–36 % during motion, and 2–9 % even at rest with the brain on (the bob, swing, HUD).
Each move exposes what was under her: the desktop root and other clients (the test stage, terminals),
which then repaint.

**Why it's hard.** We can't enable a compositor (Haseeb's choice), and we don't control the other clients.

**Questions.**
1. Which X11 knobs reduce the expose work: `backing_store`/`save_under` on our override-redirect window, fewer
   shape rects, moving in larger, rarer steps (coalescing to the display refresh), or the static big window
   (P1 Q2)?
2. Is `save_under` honoured by modern Xorg (modesetting/glamor) for override-redirect windows, and does it help
   here?

## 2. What a good answer looks like
A **ranked list of recommendations**. For each one:
- **The change:** exact code locations (file:function, approximate line) and the shape of the change in a few lines
  (pseudo-code or a diff sketch).
- **Expected win:** numbers or a direction for our CPU, Xorg CPU, ticks/s, p95/max, RSS, or slabs.
- **Risk:** what could break (black slabs, a lag between shape and pixels, determinism, LOCKED feel), and how to
  guard against it.
- **How to verify:** the exact measurement, e.g. `shoot.py cpu:8.5` on the scripted drag (ours + Xorg),
  `rag-prof` p50/p95/max, ticks/s from `rag-prof` ticks ÷ duration, `slabcheck.py --ink 6,1,2` = 0 on the
  affected clips, `/proc/<pid>/smaps_rollup`, a lockstep `lockcheck.py` pair.
- **Where you're unsure** about GPUI internals, say so and give the cheapest experiment that settles it, e.g.
  "log GPUI frame callbacks per move" or "compare 100 moves with and without notify".

Prefer a few high-leverage changes (e.g. "don't repaint on pure translation" or "move the sprite inside a
static window") over many micro-optimisations. Don't propose enabling a compositor, changing GPUI versions
mid-round, or anything that touches LOCKED picks.
