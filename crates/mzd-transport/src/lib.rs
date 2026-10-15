//! Ways to get a bundle onto a CMU, behind one small trait.
//!
//! * [`usb::UsbBundle`] writes the USB stick layout (no exec).
//! * [`serial::SerialSession`] talks to the CMU console over a 3.3 V UART at 115200 8N1.
//!   Generic over any `Read + Write`, so it is tested against a mock CMU; opening a real
//!   device needs the `serial` feature. Verify on hardware.
//! * [`power::PowerControl`] is only an interface: power and GPIO stay outside this
//!   crate (a Jetson service or the XIAO supervisor provides them).

use std::collections::BTreeMap;

pub mod power;
pub mod serial;
pub mod usb;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0} is not supported by the {1} transport")]
    Unsupported(&'static str, &'static str),
    #[error("unsafe path {0:?}")]
    UnsafePath(String),
    #[error("{0}")]
    Refused(String),
    #[error("timed out waiting for {0}")]
    Timeout(String),
    #[error("login failed with every credential")]
    LoginFailed,
    #[error("command exited {code}: {command}")]
    CommandFailed { command: String, code: i32 },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecOutput {
    pub code: i32,
    pub stdout: String,
}

/// A way to reach a CMU (or the medium it reads from).
pub trait Transport {
    fn kind(&self) -> &'static str;
    /// Whether [`Transport::exec`] can run commands.
    fn can_exec(&self) -> bool;
    fn connect(&mut self) -> Result<()>;
    fn login(&mut self) -> Result<()>;
    fn exec(&mut self, command: &str) -> Result<ExecOutput>;
    /// Write `data` to `path` (relative for USB, absolute on the CMU).
    fn upload(&mut self, path: &str, data: &[u8]) -> Result<()>;
    fn close(&mut self) -> Result<()>;
}

/// Upload every bundle file under `base` ("" for a USB root). Returns the paths written.
pub fn deliver(
    t: &mut dyn Transport,
    base: &str,
    files: &BTreeMap<String, Vec<u8>>,
) -> Result<Vec<String>> {
    let mut written = Vec::new();
    for (rel, data) in files {
        let path = if base.is_empty() {
            rel.clone()
        } else {
            format!("{}/{rel}", base.trim_end_matches('/'))
        };
        t.upload(&path, data)?;
        written.push(path);
    }
    Ok(written)
}

/// Relative path made of plain segments (no `..`, no leading `/`).
pub(crate) fn safe_relative(p: &str) -> bool {
    !p.is_empty()
        && !p.starts_with('/')
        && p.split('/').all(|s| {
            !s.is_empty()
                && s != "."
                && s != ".."
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        })
}
