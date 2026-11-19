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
mod creds;
mod provider;
mod drag;
mod edges;
mod facing;
mod frames;
mod fx;
mod host;
mod intro;
mod life;
mod pack;
mod art;
mod brand;
mod variants;
mod places;
mod moves;
mod motion;
mod hud;
mod platform;
mod ragdoll;
mod reader;
mod line;
mod logcap;
mod chip;
mod media;
mod menu;
mod notify;
mod session;
mod settings;
mod shape;
mod startfail;
mod ticker;
mod state;
mod theme;
mod cutin;
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
    /// `--set KEY=VALUE`: these settings written (her installer), then exit; nothing else runs.
    set: Vec<settings::Edit>,
}

const USAGE: &str = "usage: synaps-avatar [--profile P | --socket PATH] [--session NAME|ID | --new [--cwd DIR] [--model M]] [--no-session]
                     [--sleepy-after MIN] [--pack DIR] [--height PX] [--name NAME] [--threads N] [--no-verify]
                     [--focus-probe] [--check [--dump DIR]]
       synaps-avatar -V | --version
       synaps-avatar --set KEY=VALUE [--set KEY=VALUE ...]
       synaps-avatar --providers

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

  --pack DIR      character pack (default $XDG_DATA_HOME/synaps-avatar/packs/capri;
                  Windows: %LOCALAPPDATA%\\synaps-avatar\\packs\\capri)
  --height PX     standing body height in logical pixels (default 340)
  --name NAME     name shown in the HUD (default: the pack's character name)
  --threads N     decode threads (default min(8, cores/2))
  --no-verify     skip the sha256 pass over the pack's frames
  --focus-probe   dev: show a text field in the bubble to test keyboard focus
  -V, --version   print `synaps-avatar <version>`, exit
  --check         validate and decode the pack, print stats, exit (no window)
  --dump DIR      with --check: every decoded frame as a PNG in DIR (her pixels at --height)

  --set KEY=VALUE  write it to settings.toml through her settings writer (the rest of the file as
                  it was; one with a typo is refused), then exit. Her installer's keys only:
                  voice.enabled, voice.wake_word, voice.speak_unprompted (true|false), voice.mic,
                  session.model (a model Synaps routes: claude-..., <provider>/<model>, local/<name>).
                  A running her reads them again (settings reload). Exit 0 saved, 2 a bad --set,
                  3 settings.toml refused (a typo; can't read it), 4 not saved
  --providers     her brains, one a line, then exit: id, login, kind (account|key|local), default
                  model (none for local), name, how it's signed in to (browser|code|key|none);
                  tab-separated, in her Settings' order (her installers' list)

Dev control: newline commands on $XDG_RUNTIME_DIR/synaps-avatar.sock
  mood <idle|thinking|working|talking|needs_you|done|annoyed|sleepy|away>
  clip <clip, action or frame id>   say <text>   page next|prev   collapse   expand
  pos <x> <y> [now]   perch [sit|stand] [window id]   perch off   drop
  music on|off   glitch   state   hide   quit
  art <stand|perch|sleepFloor|sleepWindow> <variant id|next>   (a variant pool she is in)
  submit <text>   abort   prompt confirm|secret [text]   (session / dev)";

/// An error with its own exit code: `--set`'s, for her installer (any other error exits 1).
#[derive(Debug)]
struct Coded(i32, String);

impl std::fmt::Display for Coded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.1)
    }
}

impl std::error::Error for Coded {}

/// `--set`'s exit codes: 0 written (or already so), 2 a bad `--set` (nothing written), 3 his
/// settings file refused (a typo in it, or it can't be read: never written over), 4 not saved
/// (it can't be written, or the result wouldn't load).
const SET_USAGE: i32 = 2;
const SET_REFUSED: i32 = 3;
const SET_FAILED: i32 = 4;

fn set_exit(e: &settings::WriteError) -> i32 {
    use settings::WriteError::*;
    match e {
        Typo { .. } | Unreadable(_) => SET_REFUSED,
        Invalid(_) | Failed(_) => SET_FAILED,
    }
}

/// One `--set KEY=VALUE`: the keys her installers write (the Voice page's, and the brain picked),
/// and only those.
fn set_edit(pair: &str) -> Result<settings::Edit> {
    use settings::Edit;
    let (key, value) = pair.split_once('=').with_context(|| format!("--set {pair}: KEY=VALUE"))?;
    let flag = || match value.trim() {
        "true" | "on" => Ok(true),
        "false" | "off" => Ok(false),
        v => Err(anyhow::anyhow!("--set {key}: true or false, not `{v}`")),
    };
    Ok(match key.trim() {
        "voice.enabled" => Edit::Set("voice", "enabled", flag()?.into()),
        "voice.wake_word" => Edit::Set("voice", "wake_word", flag()?.into()),
        "voice.speak_unprompted" => Edit::Set("voice", "speak_unprompted", flag()?.into()),
        "voice.mic" => Edit::Set("voice", "mic", settings::mic_name(value)?.into()),
        "session.model" => Edit::Set("session", "model", session_model(value)?.into()),
        k => bail!("--set {k}: not a key she sets this way (voice.enabled, voice.wake_word, voice.speak_unprompted, voice.mic, session.model)"),
    })
}

/// `--set session.model=`: one word (as `[session] model` takes it) that Synaps routes to a
/// provider it has (`provider::of`): `claude-…`, `<provider>/<model>`, `local/<name>`.
fn session_model(value: &str) -> Result<String> {
    let m = value.trim();
    anyhow::ensure!(!m.is_empty() && !m.chars().any(|c| c.is_whitespace() || c.is_control()), "--set session.model: a model, one word, not `{}`", m.escape_debug());
    anyhow::ensure!(provider::of(m).is_some(), "--set session.model: Synaps has no provider for `{m}` (claude-..., <provider>/<model> or local/<name>)");
    Ok(m.to_string())
}

/// `--providers`: provider.rs' `ALL`, in its order, a line each: id, login, kind, default model
/// (none for a local server: its models are its own), name, how. Her installers' brains are this
/// table (install.sh and steps.ps1 read it from her: no copy of their own), so a provider added to
/// `ALL` is in them too; `the_installers_know_every_account` holds what they add to it.
fn providers_table() -> String {
    provider::ALL
        .iter()
        .map(|p| {
            let (kind, how) = match p.kind {
                provider::Kind::OAuth => ("account", sign_in(p)),
                provider::Kind::Key => ("key", "key"),
                provider::Kind::Local => ("local", "none"),
            };
            let model = p.models.first().map_or("", |&(m, _)| m);
            format!("{}\t{}\t{kind}\t{model}\t{}\t{how}", p.id, p.login, p.name)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// How Synaps 0.12's `login` signs in to an account: "browser", his browser opens on a page that
/// needs nothing typed (a PKCE link and a localhost callback; Kimi's and Muse's device pages come
/// with their code filled in), or "code", the page opens without its code, and he types the one
/// Synaps prints (Copilot: github.com/login/device).
fn sign_in(p: &provider::Provider) -> &'static str {
    if p.id == "github-copilot" { "code" } else { "browser" }
}

fn parse_args() -> Result<Args> {
    parse_args_from(std::env::args().skip(1))
}

fn parse_args_from(argv: impl IntoIterator<Item = String>) -> Result<Args> {
    let data = platform::base_dir(platform::BaseDir::Data).unwrap_or_default();
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
        set: Vec::new(),
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
            "--set" => a.set.push(val("KEY=VALUE").and_then(|v| set_edit(&v)).map_err(|e| Coded(SET_USAGE, format!("{e:#}")))?),
            "--sleepy-after" => {
                let m: f64 = val("minutes")?.parse().context("--sleepy-after: not a number")?;
                if !(m > 0.0 && m < 1e6) {
                    bail!("--sleepy-after must be a positive number of minutes");
                }
                a.sleepy_after = std::time::Duration::from_secs_f64(m * 60.0);
            }
            "-V" | "--version" => {
                olog!("synaps-avatar {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "--providers" => {
                olog!("{}", providers_table());
                std::process::exit(0);
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
    // -V alone: before process_setup, whose note (the GPU pick) is a stderr line a version query
    // doesn't want (its display server's wait for connect). With other arguments it's parse_args'
    // (in order, as -h is).
    let mut argv = std::env::args_os().skip(1);
    if let (Some(a), None) = (argv.next(), argv.next())
        && (a == "-V" || a == "--version" || a == "--providers")
    {
        if a == "--providers" {
            olog!("{}", providers_table());
        } else {
            olog!("synaps-avatar {}", env!("CARGO_PKG_VERSION"));
        }
        return;
    }
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
        // A start's failure, with no one reading her log: said on screen too (S5).
        startfail::say(startfail::why_of(&e), e.downcast_ref::<control::Running>().is_some());
        std::process::exit(e.downcast_ref::<Coded>().map_or(1, |c| c.0));
    }
}

fn run(t_start: Instant) -> Result<()> {
    let args = parse_args()?;
    if !args.set.is_empty() {
        let path = settings::Settings::path();
        return match settings::Settings::write(&args.set) {
            Ok(written) => {
                olog!("settings: {} ({})", if written { "written" } else { "already so" }, path.display());
                // She reads settings.toml as she starts: a running her is told to read it again (her
                // installer's sign-in sets her model while she runs).
                if written && let Some(r) = control::tell(&control::socket_path(), "settings reload") {
                    olog!("settings: she runs, and read them again ({r})");
                }
                Ok(())
            }
            Err(e) => Err(Coded(set_exit(&e), format!("settings: {} ({e}; {})", e.says(), path.display())).into()),
        };
    }
    // A start of hers (not --check): from here on a failure is said on screen too, when no one
    // reads her log (`startfail`: the Start menu, autostart).
    if !args.check {
        startfail::starting();
    }
    // Her log's header on Linux too (Windows: `dirs::log_without_console`). After the command line,
    // so -V, -h and --set print their own line only; before the pack loads.
    #[cfg(target_os = "linux")]
    elog!("--- synaps-avatar {} (pid {})", env!("CARGO_PKG_VERSION"), std::process::id());
    let pack = Arc::new(pack::Pack::load(&args.pack).map_err(startfail::PackError)?);
    if args.verify {
        frames::verify_poses(&pack).map_err(startfail::PackError)?;
    }
    let t_pack = t_start.elapsed().as_millis();
    if !args.check {
        // A write of hers that never finished (a crash, a kill) left its temp file: hers only.
        std::thread::spawn(|| {
            let n = settings::sweep_temps(&settings::Settings::path());
            if n > 0 {
                elog!("synaps-avatar: settings: {n} temp file(s) left by an unfinished write, removed");
            }
        });
    }

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
        if let Some(files) = pack.anim.as_ref().map(|a| &a.intro).filter(|f| !f.is_empty()) {
            // The size she shows them at on a 1440 px monitor.
            let t0 = std::time::Instant::now();
            let c = view::introview::decode(files, (1587, 893), args.threads)?;
            olog!("intro: {} close-ups ok (sha256 checked), 1587x893, decode {} ms", c.len(), t0.elapsed().as_millis());
        }
        return Ok(());
    }

    let sock = control::socket_path();
    let listener = control::bind(&sock)?;
    // Her own log (no console on Windows, install.sh's launch.log on Linux) kept under its cap
    // while she runs, from a thread of its own.
    platform::keep_log_under();
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
        // GPUI's primary display is its device px divided by its scale factor.
        let display_w = cx.primary_display().map(|d| f32::from(d.bounds().size.width)).unwrap_or(0.0);
        let sf = if display_w > 0.0 { os.primary_display_px() as f32 / display_w } else { 1.0 };
        let layout = view::Layout::new(&pack, sf, args.height);
        let origin = state::load_ground()
            .map(|(gx, gy)| view::Avatar::clamp(&os, &layout, (gx - layout.ground_x, gy - layout.ground_y)))
            .unwrap_or_else(|| view::Avatar::default_origin(&os, &layout));

        let bounds = Bounds::new(point(px(origin.0 as f32 / sf), px(origin.1 as f32 / sf)), size(px(layout.win_w as f32 / sf), px(layout.win_h as f32 / sf)));
        let mut options = os.window_options(platform::Role::Body, bounds, false);
        let hosts = os.host_per_output();
        if hosts {
            options.display_id = host::display_for(cx, bounds);
        }

        let (tx, rx) = async_channel::unbounded::<(control::Command, control::Reply)>();
        let mut built = None;
        let pack2 = pack.clone();
        let tx2 = tx.clone();
        let build = |window: &mut Window, cx: &mut App| {
            let win = match os.adopt(platform::Role::Body, window) {
                Ok(w) => w,
                Err(e) => fatal(startfail::Why::Other, e),
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
            // Escape while the intro plays skips it.
            let tx = tx2.clone();
            os.watch_escape(move || {
                let (r, _) = std::sync::mpsc::channel();
                tx.send_blocking((control::Command::Watch(platform::WatchEvent::Escape), r)).ok();
            });
            // The pointer near her, swiping past, resting on her (her lively reactions).
            let tx = tx2.clone();
            let sf = layout.sf;
            let cfg = platform::pointer::cfg(sf);
            os.watch_pointer(cfg, move |p| {
                let (r, _) = std::sync::mpsc::channel();
                tx.send_blocking((control::Command::Watch(platform::WatchEvent::Pointer(p)), r)).ok();
            })
            .map_err(|e| elog!("synaps-avatar: pointer watcher: {e:#}"))
            .ok();
            // Her global keys, where the backend grabs them itself (else his desktop binds them
            // to `capri-ctl talk ...`). A grabbed key reports its press only (the summon key is a
            // tap), or with `[voice] hold_to_talk` its press and release too. Never under test.
            if std::env::var_os("SA_BRAIN_SYNTH").is_none() {
                let tx = tx2.clone();
                let v = settings::Settings::load().map(|s| s.voice).unwrap_or_default();
                let keys = platform::Keys { summon: v.key, toggle: v.toggle_key, hold: v.hold_to_talk };
                os.watch_keys(keys, move |k| {
                    let cmd = match k {
                        platform::KeyEv::Summon => control::Command::TalkTap,
                        platform::KeyEv::TalkToggle => control::Command::TalkToggle,
                        platform::KeyEv::TalkPress => control::Command::Talk(true),
                        platform::KeyEv::TalkRelease => control::Command::Talk(false),
                    };
                    let (r, _) = std::sync::mpsc::channel();
                    tx.send_blocking((cmd, r)).ok();
                })
                .map_err(|e| elog!("synaps-avatar: global keys: {e:#}"))
                .ok();
            }
            // His presence: the screen locked, the machine asleep (logind; never under test).
            if std::env::var_os("SA_BRAIN_SYNTH").is_none() {
                let tx = tx2.clone();
                os.watch_session(move |ev| {
                    let (r, _) = std::sync::mpsc::channel();
                    tx.send_blocking((control::Command::Watch(ev), r)).ok();
                })
                .map_err(|e| elog!("synaps-avatar: session watcher: {e:#}; a lock won't put her to sleep at once"))
                .ok();
            }
            // Her icon in the tray (Windows' notification area; none on Linux yet): a click is
            // her summon key's tap, its menu is her menu (`menu`). The icon by its one rule
            // (`brand::icon`). Never under test. Another of her on another control socket
            // (`XDG_RUNTIME_DIR`) has an icon of her own.
            if std::env::var_os("SA_BRAIN_SYNTH").is_none() {
                let tx = tx2.clone();
                let icon = brand::icon(brand::pack_icon(&pack2).as_deref());
                if icon.file.is_some() {
                    elog!("synaps-avatar: her icon: {}", icon.source());
                }
                let instance = std::env::var_os("XDG_RUNTIME_DIR").map(|_| control::socket_path().display().to_string()).unwrap_or_default();
                if let Err(e) = os.start_tray(icon.tray(), &instance, move |ev| {
                    if let Some(cmd) = menu::command_for(ev) {
                        let (r, _) = std::sync::mpsc::channel();
                        // Unbounded: never waits (this runs on the tray's thread).
                        tx.try_send((cmd, r)).ok();
                    }
                }) {
                    elog!("synaps-avatar: tray: {e:#} (HIDE's chip still brings her back)");
                }
            }
            // Real LISTENING: his media players, as the OS reports them. Tests see only their
            // own fake player (SA_MPRIS_ONLY), never his.
            let music = settings::Settings::load().map(|s| s.music).unwrap_or_default();
            let only = std::env::var("SA_MPRIS_ONLY").ok().filter(|s| !s.is_empty());
            let synth = std::env::var_os("SA_BRAIN_SYNTH").is_some();
            if music.detect && (only.is_some() || !synth) {
                let tx = tx2.clone();
                let filter = media::Filter { ignore: music.ignore, only };
                if let Err(e) = platform::watch_media(filter, move |p| {
                    let (r, _) = std::sync::mpsc::channel();
                    tx.send_blocking((control::Command::Playback(p), r)).ok();
                }) {
                    // The platform's watcher names itself (`mpris: ...`, `smtc: ...`).
                    elog!("synaps-avatar: {e:#} (no music detection)");
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
        };
        // One window, or (`Caps::host_per_output`) her host on this output, the others dormant.
        let mut first = None;
        let opened: Result<AnyWindowHandle> = if hosts {
            cx.open_window(options, |window, cx| {
                let v = build(window, cx);
                let h = cx.new(|_| host::HostView { avatar: v, active: true });
                first = Some(h.downgrade());
                h
            })
            .map(Into::into)
        } else {
            cx.open_window(options, build).map(Into::into)
        };
        if let Err(e) = opened {
            fatal(startfail::Why::Other, e.context("cannot open the window"));
        }
        let view = built.expect("window built");
        if let (Ok(w), Some(h)) = (&opened, first) {
            let size = (layout.win_w, layout.win_h);
            let mut all = vec![(os.body(), h, *w)];
            for m in os.hosts_wanted() {
                all.extend(host::open_dormant(cx, &os, &view, &m, size, sf));
            }
            let fx = host::open_fx_hosts(cx, &os, sf);
            elog!("synaps-avatar: {} body hosts, {} effect hosts (one each per output)", all.len(), fx.len());
            view.update(cx, |v, _| {
                all.into_iter().for_each(|h| v.add_host(h));
                v.set_fx_hosts(fx);
            });
        }

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
                Err(e) => fatal(startfail::Why::Pack, e),
            };
            decode_view.update(cx, |v, cx| v.add_frames(vec![(default, first.frames.into_iter().next().unwrap())], false, cx));
            // A strip's cells are decoded when a pool shows them (the view asks).
            let rest: Vec<usize> = (0..p.frames.len()).filter(|&i| i != default && p.frames[i].cell.is_none()).collect();
            let p2 = p.clone();
            let rest2 = rest.clone();
            let all = off_thread(move || frames::decode(&p2, &geo, &rest2, verify, threads)).await;
            let all = match all {
                Ok(d) => d,
                Err(e) => fatal(startfail::Why::Pack, e),
            };
            elog!(
                "synaps-avatar: decoded {} frames ({:.1} MB) in {} ms on {threads} threads; all frames ready {} ms after start",
                all.frames.len(),
                all.frames.iter().map(|f| f.bytes()).sum::<usize>() as f64 / 1e6,
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
                fatal(startfail::Why::Pack, anyhow::anyhow!("this pack's character is called `{}`: that is the guarded session's name", pack.name));
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
    // Windows: nothing past GPUI's run (her state is saved: `finish_quit`). ExitProcess would run
    // every DLL's detach, and a GPU driver's can hold her there, her exe still mapped.
    if cfg!(windows) {
        platform::exit_now(0);
    }
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
        Err(e) => fatal(startfail::Why::Other, anyhow::anyhow!("identity: {e}")),
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
        // `--socket`: maybe another machine's daemon (`tools/synaps-avatar-remote`).
        local: args.socket.is_none(),
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

/// A failure on her way up, once GPUI runs: in her log, her socket gone, said on screen too when no
/// one reads her log (`startfail`, as `why`), then her end.
fn fatal(why: startfail::Why, e: anyhow::Error) -> ! {
    elog!("synaps-avatar: {e:#}");
    std::fs::remove_file(control::socket_path()).ok();
    startfail::say(why, false);
    platform::exit_now(1);
}

#[cfg(test)]
mod tests {
    // Not `super::*`: gpui's glob exports a `test` attribute that would shadow `#[test]`.
    use super::{Args, Coded, PathBuf, Result, SET_FAILED, SET_REFUSED, SET_USAGE, parse_args_from, provider, providers_table, session, session_model, set_exit, settings};

    fn parse(args: &[&str]) -> Result<Args> {
        parse_args_from(args.iter().map(|s| s.to_string()))
    }

    /// stderr a closed pipe (the reader of `2>&1 | head -1` gone): `elog!` carries on where
    /// `eprintln!` panicked (and the release build aborted).
    #[test]
    fn logging_survives_a_closed_stderr() {
        use std::io::Write as _;
        let broken = crate::platform::stderr_into_closed_pipe(|| {
            let broken = std::io::stderr().write_all(b"x\n").is_err();
            elog!("synaps-avatar: into a closed pipe {}", 42);
            elog!("and again");
            broken
        });
        assert!(broken, "the pipe was not closed: the test proves nothing");
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

    /// Her installer's Voice page: `--set` takes its keys (and nothing else) to her settings writer.
    #[test]
    fn set_takes_the_installers_keys_only() {
        let a = parse(&["--set", "voice.enabled=true", "--set", "voice.wake_word=off", "--set", "voice.mic=Microphone (USB Audio)"]).unwrap();
        assert_eq!(a.set.len(), 3);
        assert!(parse(&["--set", "voice.mic="]).is_ok(), "empty: Windows' default");
        for bad in ["voice.enabled=maybe", "brain.attention=off", "voice.enabled", "voice.voice=af_heart", "voice.mic=Mic\u{1b}[31m", "voice.mic=Mic\trophone"] {
            let e = parse(&["--set", bad]).err().expect(bad);
            assert_eq!(e.downcast_ref::<Coded>().map(|c| c.0), Some(SET_USAGE), "{bad}: a bad --set exits {SET_USAGE}");
        }
        let e = parse(&["--set"]).err().expect("no KEY=VALUE");
        assert_eq!(e.downcast_ref::<Coded>().map(|c| c.0), Some(SET_USAGE));
        let dir = std::env::temp_dir().join(format!("sa-set-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("settings.toml");
        std::fs::write(&p, "# his\n[voice]\nspeed = 1.1 # mine\n").unwrap();
        assert!(settings::write_at(&p, &a.set).unwrap());
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.starts_with("# his\n[voice]\nspeed = 1.1 # mine\n"), "{text}");
        let v = settings::Settings::parse(&text).unwrap().voice;
        assert!(v.enabled && !v.wake_word && v.mic == "Microphone (USB Audio)");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The brain picked in her installers: `--set session.model=` takes a model Synaps routes
    /// (`provider::of`), one word, and her settings writer puts it in `[session]`.
    #[test]
    fn set_takes_a_model_synaps_routes() {
        let default = provider::CLAUDE.models[0].0;
        for ok in [default, "openai-codex/gpt-5.5", "local/qwen3:8b", "local/qwen/qwen3-8b", " anthropic/claude-opus-5-5 "] {
            assert!(parse(&["--set", &format!("session.model={ok}")]).is_ok(), "{ok}");
        }
        for bad in ["", "gpt-5.5", "nope/x", "local/", "/x", "claude sonnet", "local/q\u{1b}[31m", "openai-codex/gpt\t5"] {
            let e = parse(&["--set", &format!("session.model={bad}")]).err().unwrap_or_else(|| panic!("{bad:?} taken"));
            assert_eq!(e.downcast_ref::<Coded>().map(|c| c.0), Some(SET_USAGE), "{bad:?}: a bad --set exits {SET_USAGE}");
        }
        let a = parse(&["--set", "session.model= local/qwen3:8b "]).unwrap();
        let dir = std::env::temp_dir().join(format!("sa-set-model-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("settings.toml");
        std::fs::write(&p, "# his\n[session]\nthinking = \"high\"\nmodel = \"claude-opus-5-5\"\n").unwrap();
        assert!(settings::write_at(&p, &a.set).unwrap());
        let text = std::fs::read_to_string(&p).unwrap();
        let s = settings::Settings::parse(&text).unwrap().session;
        assert_eq!((s.model.as_deref(), s.thinking.as_deref()), (Some("local/qwen3:8b"), Some("high")), "{text}");
        assert!(text.starts_with("# his\n[session]\n"), "{text}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// `--providers`: provider.rs' table as her installers read it, in `ALL`'s order, each
    /// provider's default model one she'd take.
    #[test]
    fn providers_table_gives_each_default() {
        let t = providers_table();
        let rows: Vec<Vec<&str>> = t.lines().map(|l| l.split('\t').collect()).collect();
        assert_eq!(rows.iter().map(|r| r[0]).collect::<Vec<_>>(), provider::ALL.iter().map(|p| p.id).collect::<Vec<_>>(), "{t}");
        assert!(rows.iter().all(|r| r.len() == 6 && !r[0].is_empty() && !r[4].is_empty()), "{t}");
        let claude = rows.iter().find(|r| r[0] == "anthropic").expect("Claude");
        assert_eq!(*claude, ["anthropic", "claude", "account", provider::CLAUDE.models[0].0, "Claude", "browser"]);
        assert!(rows.contains(&vec!["local", "local", "local", "", "Local", "none"]), "{t}");
        for r in rows.iter().filter(|r| r[2] != "local") {
            assert!(session_model(r[3]).is_ok(), "{r:?}: a default she takes");
            assert_eq!(provider::of(r[3]).map(|p| p.id), Some(r[0]), "{r:?}: its default is its own");
            assert_eq!(r[5] == "key", r[2] == "key", "{r:?}: a key service's is a key, an account's a sign-in");
        }
        // A code to type in: Copilot's alone (github.com/login/device opens without it; Kimi's and
        // Muse's pages come with theirs).
        let code: Vec<&str> = rows.iter().filter(|r| r[5] == "code").map(|r| r[0]).collect();
        assert_eq!(code, ["github-copilot"]);
    }

    /// Her installers have no list of brains: install.sh's picker and steps.ps1's (her Windows
    /// setup's, install.ps1's) read `--providers`, `ALL` in its order. What steps.ps1 adds is per
    /// account, one of three: the sign-in page it opens again whole (Synaps 0.12's `cmd /c start`
    /// cuts the URL at its first `&`), as `host/path`; none to reopen (a device sign-in: Copilot's,
    /// Kimi's, Muse's pages have no `&`); or why it isn't offered on Windows. An account added to
    /// `ALL` fails here until steps.ps1 says which; neither names one `ALL` hasn't.
    #[test]
    fn the_installers_know_every_account() {
        use crate::provider::Kind;
        let ps = include_str!("../packaging/windows/steps.ps1");
        let sh = include_str!("../install.sh");
        // `$script:Name = @{ 'id' = 'value'; 'id' = "value" ... }`: its ids and values.
        let table = |name: &str| -> Vec<(String, String)> {
            let start = format!("$script:{name} = @{{");
            let i = ps.find(&start).unwrap_or_else(|| panic!("steps.ps1 has no `{start}`")) + start.len();
            let body = ps[i..].split('}').next().unwrap_or_default();
            body.split(['\n', ';'])
                .filter_map(|e| {
                    let (id, rest) = e.trim().strip_prefix('\'')?.split_once('\'')?;
                    let v = rest.trim().strip_prefix('=')?.trim();
                    Some((id.to_string(), v.trim_matches(['\'', '"']).to_string()))
                })
                .collect()
        };
        let reopen = table("Reopen");
        let not_here: Vec<String> = table("NotHere").into_iter().map(|(id, _)| id).collect();
        let accounts: Vec<&str> = provider::ALL.iter().filter(|p| p.kind == Kind::OAuth).map(|p| p.id).collect();
        for a in &accounts {
            let n = reopen.iter().filter(|(id, _)| id == a).count() + not_here.iter().filter(|id| id == a).count();
            assert_eq!(n, 1, "steps.ps1: {a} needs one of $script:Reopen (its page, or '') or $script:NotHere: her Windows sign-in can't open its page");
        }
        for s in reopen.iter().map(|(id, _)| id).chain(&not_here) {
            assert!(accounts.contains(&s.as_str()), "steps.ps1 names {s}, not an account in provider.rs' ALL");
        }
        // Where a page is claimed: the one her own sign-in opens for that account
        // (`signin::allowed`: Synaps 0.12's AUTHORIZE_URLs, exact host and path), and no other
        // account's.
        let pages: Vec<(&str, &str)> = reopen.iter().filter(|(_, v)| !v.is_empty()).map(|(id, v)| (id.as_str(), v.as_str())).collect();
        assert_eq!(pages.iter().map(|(id, _)| *id).collect::<Vec<_>>(), ["anthropic", "openai-codex", "google-gemini"]);
        for (id, v) in &pages {
            let (host, path) = v.split_once('/').unwrap_or_else(|| panic!("{id}: `{v}` has no path"));
            assert!(host.contains('.') && !host.contains(['*', ':', '@', '\\']) && !path.is_empty() && !path.ends_with('/'), "{id}: `{v}` isn't host/path");
            let url = format!("https://{v}?a=1&b=2");
            for p in provider::ALL.iter().filter(|p| p.kind == Kind::OAuth) {
                let opens = crate::provider::signin::allowed(p, &url).is_some();
                assert_eq!(opens, p.id == *id, "steps.ps1's {id} page {url}: signin::allowed for {} says {opens}", p.id);
            }
            // Its path exactly: not one under it, not one beside it.
            for other in [format!("https://{v}/x?a=1&b=2"), format!("https://{v}x?a=1&b=2"), format!("https://{host}/?a=1&b=2")] {
                let p = crate::provider::by_id(id).unwrap_or_else(|| panic!("{id}"));
                assert_eq!(crate::provider::signin::allowed(p, &other), None, "{other}");
            }
        }
        let none: Vec<&str> = reopen.iter().filter(|(_, v)| v.is_empty()).map(|(id, _)| id.as_str()).collect();
        assert_eq!(none, ["github-copilot", "kimi-code", "muse"], "the device sign-ins: nothing to reopen");
        assert_eq!(not_here, ["xai-auth"], "Grok alone isn't offered on Windows (Synaps 0.12 never prints its page)");
        // No copy of the list: both ask her (`--providers`).
        assert!(ps.contains("Invoke-Her @('--providers')") && sh.contains("--providers"), "the installers read her table");
        for (file, text) in [("steps.ps1", ps), ("install.sh", sh)] {
            assert!(!text.contains("openai-codex|openai-codex|ChatGPT"), "{file} has a copy of the list of brains");
        }
    }

    /// `--set`'s exit codes, for her installer: a refused file (a typo, unreadable) is 3, a write
    /// that can't be done is 4; a bad `--set` is 2 (above).
    #[test]
    fn set_exit_codes_say_what_went_wrong() {
        use settings::WriteError::*;
        assert_eq!(set_exit(&Typo { line: Some(3), detail: "expected `=`".into() }), SET_REFUSED);
        assert_eq!(set_exit(&Unreadable("access denied".into())), SET_REFUSED);
        assert_eq!(set_exit(&Failed("disk full".into())), SET_FAILED);
        assert_eq!(set_exit(&Invalid("attention: sideways".into())), SET_FAILED);
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

