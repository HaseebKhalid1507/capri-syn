//! `-V` / `--version`, and her log's header, from her real binary: the flag exits before anything
//! starts (no pack, no window, no daemon), and the header comes after the command line is read.

use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Her binary with these arguments. No GPU choice and no backend swap on Linux (`process_setup`
/// says nothing of its own), no GPUI log.
fn her(args: &[&str]) -> (u32, Output) {
    let child = Command::new(env!("CARGO_BIN_EXE_synaps-avatar"))
        .args(args)
        .env("SA_ANY_GPU", "1")
        .env_remove("SA_BACKEND")
        .env_remove("SA_LOG")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("her binary runs");
    let pid = child.id();
    (pid, child.wait_with_output().expect("her output"))
}

/// A pack folder that isn't there.
fn no_pack() -> PathBuf {
    let p = std::env::temp_dir().join(format!("sa-version-no-pack-{}", std::process::id()));
    assert!(!p.exists(), "{}", p.display());
    p
}

#[test]
fn version_is_one_line_on_stdout_and_nothing_starts() {
    let pack = no_pack();
    // A pack that isn't there before it, too: she never reads it.
    let cases: [&[&str]; 3] = [&["-V"], &["--version"], &["--pack", pack.to_str().unwrap(), "-V"]];
    for args in cases {
        let (_, out) = her(args);
        let (stdout, stderr) = (String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        assert!(out.status.success(), "{args:?}: {:?}\n{stderr}", out.status);
        assert_eq!(stdout, format!("synaps-avatar {VERSION}\n"), "{args:?}");
        // No log header: `$(synaps-avatar -V 2>&1)` in a script is her version and nothing else.
        assert_eq!(stderr, "", "{args:?}");
    }
    // An unknown flag before it is still refused.
    let (_, out) = her(&["--nope", "-V"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown argument `--nope`"));
}

/// Linux: her log (stderr: install.sh's launch.log, a journal) starts with her version and pid,
/// before the pack loads, so a pack that fails is still said by a known build. Windows writes its
/// own header only into the log file she keeps without a console (`dirs::log_without_console`):
/// stderr here is a pipe, so none.
#[test]
fn her_log_starts_with_her_version() {
    let missing = no_pack();
    let (pid, out) = her(&["--pack", missing.to_str().unwrap(), "--check"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "no pack there: {stderr}");
    assert!(out.stdout.is_empty(), "{}", String::from_utf8_lossy(&out.stdout));
    let lines: Vec<&str> = stderr.lines().collect();
    assert!(lines.last().is_some_and(|l| l.starts_with("synaps-avatar: pack: cannot read")), "{stderr}");
    if cfg!(target_os = "linux") {
        assert_eq!(lines.first().copied().unwrap_or(""), format!("--- synaps-avatar {VERSION} (pid {pid})"), "{stderr}");
        assert_eq!(lines.len(), 2, "the header, then the error: {stderr}");
    } else {
        assert!(!stderr.contains("--- synaps-avatar"), "{stderr}");
    }
}
