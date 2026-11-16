//! synaps-avatar: a desktop companion overlay. A transparent, click-through,
//! always-on-top window animates a character pack; one Synaps daemon session drives it
//! (moods from turn events, the reply in the bubble, a text field that submits to the
//! session). A dev control socket takes commands for testing and overrides.

/// stderr that never panics. `eprintln!` panics when stderr is gone (a closed pipe:
/// `synaps-avatar … 2>&1 | head -1`), and release builds abort on panic. A lost log line
/// is not worth the process.
macro_rules! elog {
    ($($t:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stderr(), $($t)*);
    }};
}

/// stdout, the same way (`--help`, `--check`).
macro_rules! olog {
    ($($t:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stdout(), $($t)*);
    }};
}

mod anim;
mod beside;
mod brain;
mod control;
mod drag;
mod edges;
mod facing;
mod frames;
mod fx;
mod pack;
mod art;
mod variants;
mod places;
mod moves;
mod platform;
mod ragdoll;
mod reader;
mod line;
mod chip;
mod mpris;
mod session;
mod settings;
mod shape;
mod ticker;
mod state;
mod theme;
mod view;
mod voice;

use anyhow::{Context as _, Result, bail};
use gpui_kit::*;
use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

struct Args {
    pack: PathBuf,
    height: f32,
    verify: bool,
    focus_probe: bool,
    name: Option<String>,
    threads: usize,
    check: bool,
    /// With --check: write every decoded frame here as PNG (what she draws at that size).
    dump: Option<PathBuf>,
    /// Daemon profile (`None`: `$SYNAPS_PROFILE`, else the default daemon).
    profile: Option<String>,
    /// A daemon socket path instead of a profile (fake daemons in tests).
    socket: Option<PathBuf>,
    target: session::Target,
    cwd: Option<PathBuf>,
    model: Option<String>,
    no_session: bool,
    sleepy_after: std::time::Duration,
}

const USAGE: &str = "usage: synaps-avatar [--profile P | --socket PATH] [--session NAME|ID | --new [--cwd DIR] [--model M]] [--no-session]
                     [--sleepy-after MIN] [--pack DIR] [--height PX] [--name NAME] [--threads N] [--no-verify]
                     [--focus-probe] [--check [--dump DIR]]

Session (one Synaps daemon session drives the body):
  --profile P     daemon profile (default $SYNAPS_PROFILE, else the default daemon)
  --socket PATH   this daemon socket instead (e.g. an ssh-forwarded remote daemon:
                  synaps-avatar-remote)
  --session S     ride this live session (name or id; never `subconscious`)
  --new           create a new session (--cwd: its working directory, absolute, on the
                  daemon's machine; --model: its model); afterwards follow it
                  with neither: her own session, named after the pack's character
                  (`capri`): attached if live, else resumed, else created
  --no-session    the body alone (dev control only)
  She watches in Observe and attaches Mirror only to send, answer or abort.
  --sleepy-after MIN   sleepy after this many minutes without a turn (default 10)

  --pack DIR      character pack (default $XDG_DATA_HOME/synaps-avatar/packs/capri)
  --height PX     standing body height in logical pixels (default 340)
  --name NAME     name shown in the HUD (default: the pack's character name)
  --threads N     decode threads (default min(8, cores/2))
  --no-verify     skip the sha256 pass over the pack's frames
  --focus-probe   dev: show a text field in the bubble to test keyboard focus
  --check         validate and decode the pack, print stats, exit (no window)
  --dump DIR      with --check: every decoded frame as a PNG in DIR (her pixels at --height)

Dev control: newline commands on $XDG_RUNTIME_DIR/synaps-avatar.sock
  mood <idle|thinking|working|talking|needs_you|done|annoyed|sleepy|away>
  clip <clip, action or frame id>   say <text>   page next|prev   collapse   expand
  pos <x> <y> [now]   perch [sit|stand] [window id]   perch off   drop
  music on|off   glitch   state   hide   quit
  art <perch|sleepFloor|sleepWindow> <variant id|next>   (a variant pool she is in)
  submit <text>   abort   prompt confirm|secret [text]   (session / dev)";

fn parse_args() -> Result<Args> {
    parse_args_from(std::env::args().skip(1))
}

fn parse_args_from(argv: impl IntoIterator<Item = String>) -> Result<Args> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_default();
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    let mut a = Args {
        pack: data.join("synaps-avatar/packs/capri"),
        height: 340.0,
        verify: true,
        focus_probe: false,
        name: None,
        threads: (cores / 2).clamp(1, 8),
        check: false,
        dump: None,
        profile: None,
        socket: None,
        target: session::Target::Default,
        cwd: None,
        model: None,
        no_session: false,
        sleepy_after: std::time::Duration::from_secs(600),
    };
    let (mut session, mut new) = (None, false);
    let mut it = argv.into_iter();
    while let Some(arg) = it.next() {
        let mut val = |what: &str| it.next().with_context(|| format!("{arg} needs {what}"));
        match arg.as_str() {
            "--pack" => a.pack = PathBuf::from(val("a directory")?),
            "--height" => a.height = val("pixels")?.parse().context("--height: not a number")?,
            "--name" => a.name = Some(val("a name")?),
            "--threads" => a.threads = val("a count")?.parse().context("--threads: not a number")?,
            "--no-verify" => a.verify = false,
            "--focus-probe" => a.focus_probe = true,
            "--check" => a.check = true,
            "--dump" => a.dump = Some(PathBuf::from(val("a directory")?)),
            "--profile" => a.profile = Some(val("a profile name")?),
            "--socket" => a.socket = Some(PathBuf::from(val("a socket path")?)),
            "--session" => session = Some(val("a session name or id")?),
            "--new" => new = true,
            "--cwd" => a.cwd = Some(PathBuf::from(val("a directory")?)),
            "--model" => a.model = Some(val("a model")?),
            "--no-session" => a.no_session = true,
            "--sleepy-after" => {
                let m: f64 = val("minutes")?.parse().context("--sleepy-after: not a number")?;
                if !(m > 0.0 && m < 1e6) {
                    bail!("--sleepy-after must be a positive number of minutes");
                }
                a.sleepy_after = std::time::Duration::from_secs_f64(m * 60.0);
            }
            "-h" | "--help" => {
                olog!("{USAGE}");
                std::process::exit(0);
            }
            _ => bail!("unknown argument `{arg}`\n{USAGE}"),
        }
    }
    if !(120.0..=1200.0).contains(&a.height) {
        bail!("--height {} is out of range (120..1200)", a.height);
    }
    if a.socket.is_some() && a.profile.is_some() {
        bail!("--socket and --profile exclude each other");
    }
    a.target = match (session, new) {
        (Some(_), true) => bail!("--session and --new exclude each other"),
        (Some(s), false) => {
            if session::guarded(&s) {
                bail!("--session {s}: refusing, `{}` is the memory keeper's session (pinned); the avatar never attaches to it", session::GUARDED);
            }
            if a.cwd.is_some() || a.model.is_some() {
                bail!("--cwd and --model apply to a session the avatar creates, not to --session");
            }
            session::Target::Session(s)
        }
        (None, true) => session::Target::New,
        (None, false) => session::Target::Default,
    };
    if a.cwd.as_ref().is_some_and(|c| !c.is_absolute()) {
        bail!("--cwd must be an absolute path (it is resolved on the daemon's machine)");
    }
    Ok(a)
}

/// `SA_LOG=debug|info|warn` prints GPUI's own log lines to stderr.
struct StderrLog;
impl log::Log for StderrLog {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }
    fn log(&self, r: &log::Record) {
        elog!("[{} {}] {}", r.level(), r.target(), r.args());
    }
    fn flush(&self) {}
}

/// `SA_COMPOSITED=1` (alpha) or `0` (the X shape) forces the mode over settings.toml;
/// `auto` or unset: None (the settings key decides, then the compositor).
pub fn env_composited() -> Option<Option<bool>> {
    match std::env::var("SA_COMPOSITED").ok()?.trim() {
        "1" | "on" | "true" => Some(Some(true)),
        "0" | "off" | "false" => Some(Some(false)),
        _ => None,
    }
}

fn main() {
    let t_start = Instant::now();
    platform::process_setup();
    if let Ok(level) = std::env::var("SA_LOG")
        && let Ok(level) = level.parse::<log::LevelFilter>()
    {
        log::set_logger(&StderrLog).ok();
        log::set_max_level(level);
    }
    if let Err(e) = run(t_start) {
        elog!("synaps-avatar: {e:#}");
        std::process::exit(1);
    }
}

fn run(t_start: Instant) -> Result<()> {
    let args = parse_args()?;
    let pack = Arc::new(pack::Pack::load(&args.pack)?);
    if args.verify {
        frames::verify_poses(&pack)?;
    }
    let t_pack = t_start.elapsed().as_millis();

    if args.check {
        let layout = view::Layout::new(&pack, 1.0, args.height);
        let all: Vec<usize> = (0..pack.frames.len()).collect();
        let d = frames::decode(&pack, &layout.geo, &all, args.verify, args.threads)?;
        let bytes: usize = d.frames.iter().map(|f| f.bytes()).sum();
        let cells = pack.frames.iter().filter(|f| f.cell.is_some()).count();
        let rects: Vec<usize> = d.frames.iter().map(|f| f.rects.len()).collect();
        if let Some(dir) = &args.dump {
            std::fs::create_dir_all(dir).with_context(|| format!("--dump: cannot create {}", dir.display()))?;
            for f in &d.frames {
                let (w, h) = f.image_size();
                // BGRA (GPUI's atlas order) back to RGBA, in the whole avatar rect.
                let mut px = image::RgbaImage::new(layout.geo.width, layout.geo.height);
                let src = f.image.as_bytes(0).unwrap_or(&[]);
                for (i, p) in src.as_chunks::<4>().0.iter().enumerate() {
                    let (x, y) = (f.at.0 + i as i32 % w.max(1), f.at.1 + i as i32 / w.max(1));
                    if x >= 0 && y >= 0 && (x as u32) < px.width() && (y as u32) < px.height() && y - f.at.1 < h {
                        px.put_pixel(x as u32, y as u32, image::Rgba([p[2], p[1], p[0], p[3]]));
                    }
                }
                let name: String = f.id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '+' }).collect();
                px.save(dir.join(format!("{name}.png"))).with_context(|| format!("--dump: cannot write {name}.png"))?;
            }
        }
        olog!(
            "pack `{}` ok: {} frames ({} strip cells) + {} poses, crop {:?}, frame {}x{} px, {:.1} MB scaled RGBA, \
             shape rects/frame {}..{}, load {t_pack} ms, decode {} ms ({} threads, verify={})",
            pack.name,
            pack.frames.len(),
            cells,
            pack.poses.len(),
            pack.crop_rect(),
            layout.geo.width,
            layout.geo.height,
            bytes as f64 / 1e6,
            rects.iter().min().unwrap_or(&0),
            rects.iter().max().unwrap_or(&0),
            d.decode_ms,
            args.threads,
            args.verify
        );
        return Ok(());
    }

    let sock = control::socket_path();
    let listener = control::bind(&sock)?;
    let os = std::rc::Rc::new(platform::connect()?);
    let fonts: Vec<Cow<'static, [u8]>> = pack
        .behaviour
        .font_files
        .iter()
        .filter_map(|f| std::fs::read(pack.dir.join(f)).ok().map(Cow::Owned))
        .collect();
    let name = args.name.clone().unwrap_or_else(|| pack.name.clone());

    gpui_kit::application().run(move |cx: &mut App| {
        if !fonts.is_empty()
            && let Err(e) = cx.text_system().add_fonts(fonts)
        {
            elog!("synaps-avatar: pack fonts not loaded: {e:#}");
        }
        // GPUI's displays are the root size divided by its scale factor.
        let display_w = cx.primary_display().map(|d| f32::from(d.bounds().size.width)).unwrap_or(0.0);
        let sf = if display_w > 0.0 { os.root_size().0 as f32 / display_w } else { 1.0 };
        let layout = view::Layout::new(&pack, sf, args.height);
        let origin = state::load_ground()
            .map(|(gx, gy)| view::Avatar::clamp(&os, &layout, (gx - layout.ground_x, gy - layout.ground_y)))
            .unwrap_or_else(|| view::Avatar::default_origin(&os, &layout));

        let bounds = Bounds::new(point(px(origin.0 as f32 / sf), px(origin.1 as f32 / sf)), size(px(layout.win_w as f32 / sf), px(layout.win_h as f32 / sf)));
        let options = os.window_options(platform::Role::Body, bounds, false);

        let (tx, rx) = async_channel::unbounded::<(control::Command, control::Reply)>();
        let mut built = None;
        let pack2 = pack.clone();
        let tx2 = tx.clone();
        let opened = cx.open_window(options, |window, cx| {
            let win = match os.adopt(platform::Role::Body, window) {
                Ok(w) => w,
                Err(e) => {
                    elog!("synaps-avatar: {e:#}");
                    std::process::exit(1);
                }
            };
            os.force_composited(env_composited().unwrap_or_else(|| settings::Settings::load().ok().and_then(|s| s.display.composited)));
            // Nothing clickable until the first frame is decoded. Keep one pixel visible:
            // an empty bounding shape makes X report the window fully obscured, and GPUI
            // stops drawing a window it thinks is hidden.
            let px1 = (layout.ground_x as i16, (layout.ground_y - 1) as i16, 1, 1);
            os.set_shapes(Some(&[]), Some(&[px1])).ok();
            os.move_to(origin.0, origin.1).ok();
            os.raise().ok();
            let tx = tx2.clone();
            os.spawn_watcher(move |ev| {
                let (r, _) = std::sync::mpsc::channel();
                tx.send_blocking((control::Command::Watch(ev), r)).ok();
            })
            .map_err(|e| elog!("synaps-avatar: watcher: {e:#}"))
            .ok();
            // A click outside the Line parks it (her window keeps the keyboard when he clicks
            // back into the window the WM already thinks is focused).
            let tx = tx2.clone();
            os.watch_clicks(move |x, y| {
                let (r, _) = std::sync::mpsc::channel();
                tx.send_blocking((control::Command::Watch(platform::WatchEvent::Click(x, y)), r)).ok();
            })
            .map_err(|e| elog!("synaps-avatar: click watcher: {e:#}; a click elsewhere won't park the Line"))
            .ok();
            // The pointer near her, swiping past, resting on her (her lively reactions).
            let tx = tx2.clone();
            let sf = layout.sf;
            let cfg = platform::PointerCfg { near_px: 150.0 * sf, swipe_px: 120.0 * sf, swipe_speed: 1500.0 * sf, hover: std::time::Duration::from_millis(1500) };
            os.watch_pointer(cfg, move |p| {
                let (r, _) = std::sync::mpsc::channel();
                tx.send_blocking((control::Command::Watch(platform::WatchEvent::Pointer(p)), r)).ok();
            })
            .map_err(|e| elog!("synaps-avatar: pointer watcher: {e:#}"))
            .ok();
            // Real LISTENING: media players on the session bus (signals only). Tests see only
            // their own fake player (SA_MPRIS_ONLY), never his.
            let music = settings::Settings::load().map(|s| s.music).unwrap_or_default();
            let only = std::env::var("SA_MPRIS_ONLY").ok().filter(|s| !s.is_empty());
            let synth = std::env::var_os("SA_BRAIN_SYNTH").is_some();
            if music.detect && (only.is_some() || !synth) {
                let tx = tx2.clone();
                let filter = mpris::Filter { ignore: music.ignore, only };
                if let Err(e) = mpris::spawn(filter, move |p| {
                    let (r, _) = std::sync::mpsc::channel();
                    tx.send_blocking((control::Command::Playback(p), r)).ok();
                }) {
                    elog!("synaps-avatar: mpris: {e:#} (no music detection)");
                }
            }
            elog!(
                "synaps-avatar: pack `{}` ({} frames) sf {sf:.3} window 0x{win:x} {}x{} at {},{} composited={} (pack load {t_pack} ms)",
                pack2.name,
                pack2.frames.len(),
                layout.win_w,
                layout.win_h,
                origin.0,
                origin.1,
                os.composited()
            );
            let init = view::Init { pack: pack2.clone(), layout, os: os.clone(), name, origin, t_start, focus_probe: args.focus_probe };
            let view = cx.new(|cx| view::Avatar::new(init, window, cx));
            built = Some(view.clone());
            view
        });
        if let Err(e) = opened {
            elog!("synaps-avatar: cannot open the window: {e:#}");
            std::fs::remove_file(control::socket_path()).ok();
            std::process::exit(1);
        }
        let view = built.expect("window built");

        // Decode: the default frame first so something shows quickly, then the rest.
        let geo = layout.geo;
        let (verify, threads) = (args.verify, args.threads);
        let decode_view = view.clone();
        let p = pack.clone();
        cx.spawn(async move |cx| {
            let default = p.frame(&p.catalog.default_sprite).unwrap_or(0);
            let p1 = p.clone();
            let first = off_thread(move || frames::decode(&p1, &geo, &[default], verify, 1)).await;
            let first = match first {
                Ok(d) => d,
                Err(e) => fatal(e),
            };
            decode_view.update(cx, |v, cx| v.add_frames(vec![(default, first.frames.into_iter().next().unwrap())], false, cx));
            // A strip's cells are decoded when a pool shows them (the view asks).
            let rest: Vec<usize> = (0..p.frames.len()).filter(|&i| i != default && p.frames[i].cell.is_none()).collect();
            let p2 = p.clone();
            let rest2 = rest.clone();
            let all = off_thread(move || frames::decode(&p2, &geo, &rest2, verify, threads)).await;
            let all = match all {
                Ok(d) => d,
                Err(e) => fatal(e),
            };
            elog!(
                "synaps-avatar: decoded {} frames in {} ms on {threads} threads; all frames ready {} ms after start",
                all.frames.len(),
                all.decode_ms,
                t_start.elapsed().as_millis()
            );
            decode_view.update(cx, |v, cx| v.add_frames(rest.into_iter().zip(all.frames).collect(), true, cx));
            if let Some(m) = p.behaviour.music.clone() {
                let p3 = p.clone();
                let size = crate::view::note_px(&geo);
                match off_thread(move || frames::decode_notes(&p3, &m.notes, size)).await {
                    Ok(notes) => decode_view.update(cx, |v, _| v.set_notes(notes)),
                    Err(e) => elog!("synaps-avatar: music notes unavailable: {e:#}"),
                }
            }
            // Hands freed decode buffers back to the OS.
            platform::trim_heap();
        })
        .detach();

        if !args.no_session {
            if session::guarded(&who(&pack.name)) {
                fatal(anyhow::anyhow!("this pack's character is called `{}`: that is the guarded session's name", pack.name));
            }
            let persona = persona_for(&args, &pack.dir);
            connect_session(&args, &pack.name, persona, &view, cx);
        }

        control::serve(listener, tx);
        cx.spawn(async move |cx| {
            while let Ok((cmd, reply)) = rx.recv().await {
                let r = view.update(cx, |v, cx| v.apply(cmd, cx));
                reply.send(r).ok();
            }
        })
        .detach();
    });
    Ok(())
}

/// Her name as a client and as her own session: the pack's character, lowercased, in
/// the daemon's client-name alphabet (1-32 letters, digits, `-`, `_`).
pub fn who(character: &str) -> String {
    let mut w: String = character
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '-' })
        .collect();
    w = w.trim_matches('-').chars().take(32).collect();
    if synaps_client::Identity::new(&w).is_err() { "avatar".into() } else { w }
}

/// Her own session's persona: settings.toml `[session]` and the pack's persona.md (a CLI
/// `--model` wins). None at all for `--session` (someone else's session).
fn persona_for(args: &Args, pack_dir: &std::path::Path) -> session::Persona {
    if matches!(args.target, session::Target::Session(_)) {
        return session::Persona::default();
    }
    let (s, voice) = settings::Settings::load().map(|s| (s.session, s.voice)).unwrap_or_else(|e| {
        elog!("synaps-avatar: {e:#}; the session gets no persona settings");
        (settings::SessionSettings { persona: false, ..Default::default() }, Default::default())
    });
    session_persona(&s, &voice, args.model.clone(), pack_dir)
}

/// The persona from `[session]` (and a model that overrides it); with voice on, the lines on
/// writing for the ear (voice::PERSONA) follow it.
pub(crate) fn session_persona(s: &settings::SessionSettings, voice: &settings::VoiceSettings, model: Option<String>, pack_dir: &std::path::Path) -> session::Persona {
    let system = s.persona_text(pack_dir).unwrap_or_else(|e| {
        elog!("synaps-avatar: persona: {e:#}; none sent");
        None
    });
    session::Persona { system: voice::persona(system, voice.enabled, voice.speak_unprompted), model: model.or_else(|| s.model.clone()), thinking: s.thinking.clone() }
}

/// Start the session link: the client and its pump live in the view.
fn connect_session(args: &Args, character: &str, persona: session::Persona, view: &Entity<view::Avatar>, cx: &mut App) {
    let me = who(character);
    let identity = match synaps_client::Identity::new(&me) {
        Ok(i) => i,
        Err(e) => fatal(anyhow::anyhow!("identity: {e}")),
    };
    let (endpoint, key) = match &args.socket {
        Some(p) => (synaps_client::Endpoint::Socket(p.clone()), format!("socket:{}", p.display())),
        None => {
            let profile = args.profile.clone().or_else(|| std::env::var("SYNAPS_PROFILE").ok());
            let key = format!("profile:{}", profile.as_deref().unwrap_or("default"));
            (synaps_client::Endpoint::profile(profile.as_deref()), key)
        }
    };
    let remembered = state::remembered_session(&key);
    elog!(
        "synaps-avatar: session: {key}, client `{me}`, target {:?}{}",
        args.target,
        if args.target == session::Target::Default { format!(" (her own: `{me}`{})", remembered.as_deref().map(|r| format!(", last {r}")).unwrap_or_default()) } else { String::new() }
    );
    let opts = session::Options {
        target: args.target.clone(),
        name: me,
        remembered,
        cwd: args.cwd.clone(),
        model: args.model.clone(),
        sleepy_after: args.sleepy_after,
        persona,
    };
    let link = session::Link::new(opts, Instant::now());
    let options = synaps_client::ClientOptions::new(endpoint, identity);
    view.update(cx, |v, cx| v.connect_session(options, link, key, cx));
}

/// Run `f` on a plain thread and await its result. GPUI's background executor can be
/// slow to hand a finished task back to an idle main loop (see ticker.rs).
async fn off_thread<T: Send + 'static>(f: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> anyhow::Result<T> {
    let (tx, rx) = async_channel::bounded(1);
    std::thread::spawn(move || {
        tx.send_blocking(f()).ok();
    });
    rx.recv().await.map_err(|_| anyhow::anyhow!("decode thread died"))?
}

fn fatal(e: anyhow::Error) -> ! {
    elog!("synaps-avatar: {e:#}");
    std::fs::remove_file(control::socket_path()).ok();
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    // Not `super::*`: gpui's glob exports a `test` attribute that would shadow `#[test]`.
    use super::{Args, PathBuf, Result, parse_args_from, session};

    fn parse(args: &[&str]) -> Result<Args> {
        parse_args_from(args.iter().map(|s| s.to_string()))
    }

    /// stderr a closed pipe (the reader of `2>&1 | head -1` gone): `elog!` carries on where
    /// `eprintln!` panicked (and the release build aborted).
    #[test]
    fn logging_survives_a_closed_stderr() {
        use std::io::Write as _;
        let mut fds = [0; 2];
        // SAFETY: plain fd juggling on this process's own descriptors; fd 2 is restored.
        unsafe {
            assert_eq!(libc::pipe(fds.as_mut_ptr()), 0);
            libc::close(fds[0]);
            let saved = libc::dup(2);
            libc::dup2(fds[1], 2);
            let broken = std::io::stderr().write_all(b"x\n").is_err();
            elog!("synaps-avatar: into a closed pipe {}", 42);
            elog!("and again");
            libc::dup2(saved, 2);
            libc::close(saved);
            libc::close(fds[1]);
            assert!(broken, "the pipe was not closed: the test proves nothing");
        }
    }

    #[test]
    fn socket_and_profile_exclude_each_other() {
        let a = parse(&["--socket", "/run/user/1000/synaps-avante.sock", "--session", "work"]).unwrap();
        assert_eq!(a.socket, Some(PathBuf::from("/run/user/1000/synaps-avante.sock")));
        assert_eq!(a.profile, None);
        assert_eq!(a.target, session::Target::Session("work".into()));
        let a = parse(&["--profile", "desktopproto"]).unwrap();
        assert_eq!((a.profile.as_deref(), a.socket), (Some("desktopproto"), None));
        assert_eq!(a.target, session::Target::Default);
        for args in [&["--socket", "/tmp/x.sock", "--profile", "p"][..], &["--profile", "p", "--socket", "/tmp/x.sock"][..]] {
            let e = parse(args).err().expect("both given").to_string();
            assert!(e.contains("exclude each other"), "{e}");
        }
        assert!(parse(&["--socket"]).err().expect("no path").to_string().contains("needs a socket path"));
    }

    #[test]
    fn subconscious_is_refused_and_names_are_slugs() {
        for s in ["subconscious", "Subconscious", " SUBCONSCIOUS "] {
            let e = parse(&["--session", s]).err().expect("refused").to_string();
            assert!(e.contains("refusing") && e.contains("subconscious"), "{e}");
        }
        assert!(parse(&["--session", "subconscious-notes"]).is_ok());
        assert_eq!(super::who("Capri"), "capri");
        assert_eq!(super::who("  Mister Byte! "), "mister-byte");
        assert_eq!(super::who("!!!"), "avatar");
        assert_eq!(super::who(&"x".repeat(40)).len(), 32);
    }
}

