//! Hyprland's two sockets (`$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/`): requests on
//! `.socket.sock` (one connection per request, never kept open: Hyprland answers them
//! synchronously), events on `.socket2.sock` (`EVENT>>DATA` lines).

use anyhow::{Context as _, Result, bail};
use serde::de::DeserializeOwned;
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

/// Requests answer within this, or fail (the caller falls back).
const TIMEOUT: Duration = Duration::from_millis(250);

#[derive(Clone)]
pub(super) struct Ipc {
    dir: PathBuf,
}

impl Ipc {
    /// The running instance named by the environment, if its request socket answers.
    pub(super) fn from_env() -> Result<Ipc> {
        let sig = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").context("HYPRLAND_INSTANCE_SIGNATURE unset")?;
        let run = std::env::var("XDG_RUNTIME_DIR").context("XDG_RUNTIME_DIR unset")?;
        let ipc = Ipc { dir: PathBuf::from(run).join("hypr").join(sig) };
        let v = ipc.request("j/version")?;
        if !v.trim_start().starts_with('{') {
            bail!("hyprland: unexpected version reply {v:?}");
        }
        Ok(ipc)
    }

    pub(super) fn request(&self, req: &str) -> Result<String> {
        let mut s = UnixStream::connect(self.dir.join(".socket.sock")).context("hyprland: request socket")?;
        s.set_read_timeout(Some(TIMEOUT))?;
        s.set_write_timeout(Some(TIMEOUT))?;
        s.write_all(req.as_bytes())?;
        let mut out = String::new();
        s.read_to_string(&mut out).with_context(|| format!("hyprland: reply to {req}"))?;
        Ok(out)
    }

    pub(super) fn json<T: DeserializeOwned>(&self, what: &str) -> Result<T> {
        let r = self.request(&format!("j/{what}"))?;
        serde_json::from_str(&r).with_context(|| format!("hyprland: j/{what}"))
    }

    /// One `[[BATCH]]` request; the replies in order (Hyprland joins them with blank lines).
    pub(super) fn batch(&self, reqs: &[&str]) -> Result<Vec<serde_json::Value>> {
        let r = self.request(&format!("[[BATCH]]{}", reqs.join(";")))?;
        let vals: Vec<serde_json::Value> = serde_json::Deserializer::from_str(&r).into_iter().collect::<Result<_, _>>()?;
        if vals.len() != reqs.len() {
            bail!("hyprland: batch of {} gave {} replies", reqs.len(), vals.len());
        }
        Ok(vals)
    }

    /// A Lua expression's value (`hyprctl repl`), as text.
    pub(super) fn lua(&self, code: &str) -> Result<String> {
        Ok(self.request(&format!("/repl {code}"))?.trim().to_string())
    }

    /// Read event lines (`name`, `data`) until the socket closes; reconnects with backoff.
    pub(super) fn events(&self, mut on: impl FnMut(&str, &str)) {
        let mut wait = Duration::from_millis(200);
        loop {
            if let Ok(s) = UnixStream::connect(self.dir.join(".socket2.sock")) {
                wait = Duration::from_millis(200);
                for line in BufReader::new(s).lines() {
                    let Ok(line) = line else { break };
                    if let Some((name, data)) = line.split_once(">>") {
                        on(name, data);
                    }
                }
            }
            std::thread::sleep(wait);
            wait = (wait * 2).min(Duration::from_secs(5));
        }
    }
}
