//! The voice sidecar's link on Windows: loopback TCP and a two-way token handshake. capri-voice is
//! Python, and stock CPython has no AF_UNIX on Windows (it does on Linux, where the link stays the
//! Unix socket). A named pipe would carry his ACL by itself, but the sidecar's loop is built on
//! sockets, and a pipe server in stock Python needs pywin32 or an asyncio rewrite.
//!
//! The sidecar binds 127.0.0.1 on a port the OS picks and writes `endpoint.json` (the port, two
//! fresh 256-bit tokens) under `%LOCALAPPDATA%\capri-voice`, which only he and the admins can read
//! (`tools/capri_voice_win.py`). She connects and sends `{"op":"hello","token"}` before anything
//! else; the sidecar answers `{"ev":"hello","token": reply}` first. Any local process can reach the
//! port, but without the file it can neither drive the sidecar nor pose as one: a stale file's
//! port taken by another program gets a dead token, and she drops it for want of the reply.

use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Her end of the link.
pub type VoiceLink = TcpStream;

/// How long the connect and the sidecar's hello may take (it answers at once: it is local).
const HELLO_WAIT: Duration = Duration::from_secs(1);

/// `$CAPRI_VOICE_ENDPOINT`, else `%LOCALAPPDATA%\capri-voice\endpoint.json`.
fn endpoint_path() -> PathBuf {
    if let Some(p) = std::env::var_os("CAPRI_VOICE_ENDPOINT") {
        return p.into();
    }
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("capri-voice").join("endpoint.json")
}

/// Where the sidecar is (for her log) and the link, connected and proven.
pub fn voice_link() -> (String, io::Result<VoiceLink>) {
    let p = endpoint_path();
    let s = connect(&p);
    (p.display().to_string(), s)
}

#[derive(serde::Deserialize)]
struct Endpoint {
    port: u16,
    token: String,
    reply: String,
}

fn bad(why: &str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, why.to_string())
}

/// Equal in constant time (the reply is a secret).
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

/// One line, a byte at a time: nothing past it is taken off the socket (her reader thread gets
/// the stream from the next byte on).
fn read_line(mut s: &TcpStream) -> io::Result<String> {
    let mut b = Vec::new();
    let mut c = [0u8; 1];
    while b.len() < 512 {
        if s.read(&mut c)? == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "the link closed in the hello"));
        }
        if c[0] == b'\n' {
            return String::from_utf8(b).map_err(|_| bad("a hello that is not text"));
        }
        b.push(c[0]);
    }
    Err(bad("a hello that never ends"))
}

fn connect(path: &Path) -> io::Result<TcpStream> {
    let e: Endpoint = serde_json::from_slice(&std::fs::read(path)?).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let s = TcpStream::connect_timeout(&SocketAddr::from((Ipv4Addr::LOCALHOST, e.port)), HELLO_WAIT)?;
    s.set_nodelay(true)?;
    let mut hello = serde_json::json!({ "op": "hello", "token": e.token }).to_string();
    hello.push('\n');
    (&s).write_all(hello.as_bytes())?;
    s.set_read_timeout(Some(HELLO_WAIT))?;
    let line = read_line(&s)?;
    s.set_read_timeout(None)?;
    let v: serde_json::Value = serde_json::from_str(&line).map_err(|_| bad("not the sidecar (no hello)"))?;
    let ok = v["ev"] == "hello" && v["token"].as_str().is_some_and(|t| same(t.as_bytes(), e.reply.as_bytes()));
    if !ok {
        return Err(bad("not the sidecar (its hello did not match)"));
    }
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;

    /// A fake sidecar: one avatar, checked as `capri_voice_win.serve` does; `reply` is what it
    /// answers (the right one, or not), then an event she must still see whole.
    fn sidecar(dir: &Path, reply: Option<&'static str>) -> (PathBuf, std::thread::JoinHandle<Option<String>>) {
        let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = l.local_addr().unwrap().port();
        let path = dir.join("endpoint.json");
        std::fs::write(&path, serde_json::json!({ "v": 1, "port": port, "token": "t0k", "reply": "r3p", "pid": 1 }).to_string()).unwrap();
        let h = std::thread::spawn(move || {
            let (c, _) = l.accept().unwrap();
            let mut r = BufReader::new(c.try_clone().unwrap());
            let mut first = String::new();
            r.read_line(&mut first).unwrap();
            if let Some(reply) = reply {
                let hello = serde_json::json!({ "ev": "hello", "token": reply }).to_string();
                (&c).write_all(format!("{hello}\n{{\"ev\":\"ready\"}}\n").as_bytes()).unwrap();
                std::thread::sleep(Duration::from_millis(200));
            } else {
                std::thread::sleep(HELLO_WAIT * 2);
            }
            Some(first)
        });
        (path, h)
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("sa-voice-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_handshake_proves_both_ends() {
        let d = tmp("ok");
        let (p, h) = sidecar(&d, Some("r3p"));
        let s = connect(&p).expect("the right sidecar");
        let mut line = String::new();
        BufReader::new(s).read_line(&mut line).unwrap();
        assert_eq!(line.trim(), r#"{"ev":"ready"}"#, "the first event after the hello is hers, whole");
        let first: serde_json::Value = serde_json::from_str(&h.join().unwrap().unwrap()).unwrap();
        assert_eq!((first["op"].as_str(), first["token"].as_str()), (Some("hello"), Some("t0k")));
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn a_wrong_or_silent_sidecar_is_dropped() {
        let d = tmp("wrong");
        let (p, h) = sidecar(&d, Some("guess"));
        assert_eq!(connect(&p).unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        h.join().unwrap();
        let (p, h) = sidecar(&d, None);
        let e = connect(&p).unwrap_err().kind();
        assert!(matches!(e, io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock), "{e:?}");
        h.join().unwrap();
        std::fs::remove_dir_all(&d).ok();
        assert_eq!(connect(&d.join("endpoint.json")).unwrap_err().kind(), io::ErrorKind::NotFound, "no sidecar: no file");
    }

    /// Both real ends: `capri_voice_win.serve` (Python) and this. Runs when `CAPRI_VOICE_TEST_PY`
    /// names a Python (CI sets it); skipped otherwise.
    #[test]
    fn the_python_sidecar_end() {
        let Some(py) = std::env::var_os("CAPRI_VOICE_TEST_PY") else {
            eprintln!("skipped: CAPRI_VOICE_TEST_PY is not set");
            return;
        };
        let d = tmp("py");
        let p = d.join("endpoint.json");
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tools").join("capri_voice_win.py");
        let mut child = std::process::Command::new(py).arg(&script).arg("--test-serve").arg(&p).spawn().expect("python");
        let t0 = std::time::Instant::now();
        while !p.exists() && t0.elapsed() < Duration::from_secs(20) {
            std::thread::sleep(Duration::from_millis(50));
        }
        let got = connect(&p).map(|s| {
            let mut line = String::new();
            BufReader::new(s).read_line(&mut line).map(|_| line)
        });
        child.kill().ok();
        child.wait().ok();
        let line = got.expect("the handshake with capri_voice_win.serve").unwrap();
        assert_eq!(line.trim(), r#"{"ev":"ready"}"#);
        std::fs::remove_dir_all(d).ok();
    }
}
