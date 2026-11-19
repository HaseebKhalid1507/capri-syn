//! A start that fails says so on screen too when no one reads her log (S5), from her real binary:
//! only a start (not `--check`), only where her stderr isn't read (Linux: not a terminal, as here,
//! a pipe; Windows: only her own log file, never a pipe), and here never for real
//! (`SA_NO_NOTICE=1`: her log says what it would have said, and nothing is shown).

use std::process::{Command, Output, Stdio};

fn her(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_synaps-avatar"))
        .args(args)
        .env_remove("SA_LOG")
        .env("SA_NO_NOTICE", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("her binary runs")
}

#[test]
fn a_start_without_her_pack_says_so_on_screen_too() {
    let missing = std::env::temp_dir().join(format!("sa-start-fail-no-pack-{}", std::process::id()));
    assert!(!missing.exists(), "{}", missing.display());
    let pack = missing.to_str().unwrap();

    let out = her(&["--pack", pack]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    let lines: Vec<&str> = stderr.lines().collect();
    let at = lines.iter().position(|l| l.starts_with("synaps-avatar: pack: cannot read")).unwrap_or_else(|| panic!("the pack's error, as before: {stderr}"));
    let said: Vec<&&str> = lines.iter().filter(|l| l.contains("said on screen")).collect();
    if cfg!(target_os = "linux") {
        assert_eq!(
            lines.get(at + 1).copied(),
            Some(
                "synaps-avatar: not said on screen (SA_NO_NOTICE=1): Capri couldn't start: her character pack is missing or damaged, \
                 and installing capri-syn again puts it back."
            ),
            "after the error, what it would say (a pipe: no one reads it): {stderr}"
        );
        assert_eq!(said.len(), 1, "once: {stderr}");
    } else {
        assert!(said.is_empty(), "a pipe on Windows is a script's: {stderr}");
    }

    // --check is a script's: her log only, on every OS.
    let out = her(&["--pack", pack, "--check"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(!stderr.contains("said on screen"), "{stderr}");
    // So is a bad command line.
    let out = her(&["--pack", pack, "--nope"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(!stderr.contains("said on screen"), "{stderr}");
}
