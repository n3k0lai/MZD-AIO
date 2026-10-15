//! CMU serial console session (3.3 V UART, 115200 8N1).
//!
//! Verify on hardware: login prompts, the shell prompt, and how much kernel log noise
//! the console prints are taken from community notes (MZD-AIO's serial guide), not
//! from our own bench yet. Commands are framed with unique begin/end markers so stray
//! console output around them does not corrupt results.

use std::io::{ErrorKind, Read, Write};
use std::time::{Duration, Instant};

use crate::{Error, ExecOutput, Result, Transport};

pub const BAUD_RATE: u32 = 115_200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credentials {
    pub user: String,
    pub password: String,
}

/// Community-documented CMU console logins, tried in order: `cmu`/`jci` once the autorun
/// hook is installed, `user`/`jci` on a stock unit. Verify on hardware.
pub fn default_credentials() -> Vec<Credentials> {
    [("cmu", "jci"), ("user", "jci")]
        .into_iter()
        .map(|(u, p)| Credentials {
            user: u.into(),
            password: p.into(),
        })
        .collect()
}

/// Read-only commands the probe tools may run. Nothing here changes the CMU.
pub const PROBES: &[(&str, &str)] = &[
    ("version", "cat /jci/version.ini"),
    ("df", "df -k"),
    ("uptime", "cat /proc/uptime"),
];

pub fn probe_command(name: &str) -> Option<&'static str> {
    PROBES.iter().find(|(n, _)| *n == name).map(|(_, c)| *c)
}

/// What a session would send, for dry runs. Never opens the device.
pub fn dry_run_transcript(device: &str, probes: &[&str]) -> Vec<String> {
    let mut t = vec![
        format!("open {device} at {BAUD_RATE} 8N1, no flow control"),
        "send newline, wait for 'login:' (or a shell prompt)".into(),
    ];
    for c in default_credentials() {
        t.push(format!("try login {}/<password>", c.user));
    }
    for p in probes {
        match probe_command(p) {
            Some(cmd) => t.push(format!("run: {cmd}")),
            None => t.push(format!("refuse unknown probe {p:?}")),
        }
    }
    t.push("close".into());
    t
}

#[derive(Debug)]
pub struct SerialSession<P: Read + Write> {
    port: P,
    pub credentials: Vec<Credentials>,
    pub timeout: Duration,
    buf: String,
    seq: u32,
    logged_in: bool,
}

impl<P: Read + Write> SerialSession<P> {
    pub fn new(port: P) -> Self {
        Self {
            port,
            credentials: default_credentials(),
            timeout: Duration::from_secs(10),
            buf: String::new(),
            seq: 0,
            logged_in: false,
        }
    }

    fn send(&mut self, s: &str) -> Result<()> {
        self.port.write_all(s.as_bytes())?;
        self.port.flush()?;
        Ok(())
    }

    /// Read until `done(buffer)` returns Some, or time out.
    fn read_until<T>(&mut self, what: &str, done: impl Fn(&str) -> Option<T>) -> Result<T> {
        let deadline = Instant::now() + self.timeout;
        let mut chunk = [0u8; 512];
        loop {
            if let Some(v) = done(&self.buf) {
                return Ok(v);
            }
            if Instant::now() >= deadline {
                return Err(Error::Timeout(what.into()));
            }
            match self.port.read(&mut chunk) {
                Ok(0) => return Err(Error::Io(ErrorKind::UnexpectedEof.into())),
                Ok(n) => self
                    .buf
                    .push_str(&String::from_utf8_lossy(&chunk[..n]).replace('\r', "")),
                Err(e) if matches!(e.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(e) => return Err(e.into()),
            }
        }
    }
}

fn tail(buf: &str) -> &str {
    buf.trim_end_matches(' ')
}

fn at_prompt(buf: &str) -> bool {
    let t = tail(buf);
    t.ends_with('#') || t.ends_with('$')
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Seen {
    Login,
    Password,
    Prompt,
    Incorrect,
}

fn seen(buf: &str) -> Option<Seen> {
    let t = tail(buf);
    if t.ends_with("login:") {
        if buf.contains("incorrect") {
            Some(Seen::Incorrect)
        } else {
            Some(Seen::Login)
        }
    } else if t.ends_with("Password:") {
        Some(Seen::Password)
    } else if at_prompt(buf) {
        Some(Seen::Prompt)
    } else {
        None
    }
}

impl<P: Read + Write> Transport for SerialSession<P> {
    fn kind(&self) -> &'static str {
        "serial"
    }

    fn can_exec(&self) -> bool {
        true
    }

    fn connect(&mut self) -> Result<()> {
        self.send("\n")?;
        Ok(())
    }

    fn login(&mut self) -> Result<()> {
        if self.read_until("login prompt", seen)? == Seen::Prompt {
            self.logged_in = true;
            return Ok(());
        }
        for c in self.credentials.clone() {
            self.buf.clear();
            self.send(&format!("{}\n", c.user))?;
            if self.read_until("password prompt", seen)? != Seen::Password {
                continue;
            }
            self.buf.clear();
            self.send(&format!("{}\n", c.password))?;
            if self.read_until("shell prompt", seen)? == Seen::Prompt {
                self.buf.clear();
                self.logged_in = true;
                return Ok(());
            }
        }
        Err(Error::LoginFailed)
    }

    fn exec(&mut self, command: &str) -> Result<ExecOutput> {
        if !self.logged_in {
            return Err(Error::Refused("not logged in".into()));
        }
        if command.contains(['\n', '\r']) {
            return Err(Error::Refused("commands must be one line".into()));
        }
        self.seq += 1;
        let (begin, end) = (
            format!("__MZD_B{}__", self.seq),
            format!("__MZD_E{}__ ", self.seq),
        );
        self.buf.clear();
        self.send(&format!("echo {begin}; {command}; echo {end}$?\n"))?;
        let found = self.read_until(&format!("end of {command:?}"), |buf| {
            let lines: Vec<&str> = buf.lines().collect();
            let end_at = lines.iter().position(|l| l.starts_with(&end))?;
            let code = lines[end_at][end.len()..].trim().parse::<i32>().ok()?;
            let start = lines[..end_at].iter().rposition(|l| *l == begin)?;
            let body: Vec<&str> = lines[start + 1..end_at]
                .iter()
                .copied()
                .filter(|l| !is_kernel_log(l))
                .collect();
            Some((code, body.join("\n")))
        })?;
        Ok(ExecOutput {
            code: found.0,
            stdout: found.1,
        })
    }

    /// Writes through the shell with `printf` octal escapes (BusyBox has no reliable
    /// base64 on every CMU build), then checks the size and renames into place.
    fn upload(&mut self, path: &str, data: &[u8]) -> Result<()> {
        let ok = path.starts_with('/')
            && path
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._/-".contains(&b))
            && !path.split('/').any(|s| s == "..");
        if !ok {
            return Err(Error::UnsafePath(path.into()));
        }
        let part = format!("{path}.mzd-part");
        let run = |s: &mut Self, cmd: String| -> Result<ExecOutput> {
            let out = s.exec(&cmd)?;
            if out.code != 0 {
                return Err(Error::CommandFailed {
                    command: cmd,
                    code: out.code,
                });
            }
            Ok(out)
        };
        run(self, format!(": > '{part}'"))?;
        for chunk in data.chunks(128) {
            let esc: String = chunk.iter().map(|b| format!("\\{b:03o}")).collect();
            run(self, format!("printf '{esc}' >> '{part}'"))?;
        }
        let size = run(self, format!("wc -c < '{part}'"))?;
        let got = size.stdout.lines().last().unwrap_or("").trim().to_string();
        if got != data.len().to_string() {
            return Err(Error::Refused(format!(
                "upload size mismatch for {path}: {got}"
            )));
        }
        run(self, format!("mv '{part}' '{path}'"))?;
        Ok(())
    }

    fn close(&mut self) -> Result<()> {
        if self.logged_in {
            self.send("exit\n")?;
            self.logged_in = false;
        }
        Ok(())
    }
}

/// Kernel printk lines (`[   12.345678] ...`) interleaved on the console.
fn is_kernel_log(line: &str) -> bool {
    line.strip_prefix('[')
        .and_then(|r| r.split_once(']'))
        .is_some_and(|(ts, _)| {
            !ts.trim().is_empty() && ts.trim().bytes().all(|b| b.is_ascii_digit() || b == b'.')
        })
}

/// Open a real UART at 115200 8N1. `device` must be an explicit `/dev/tty*` path.
#[cfg(feature = "serial")]
pub fn open(device: &str) -> Result<SerialSession<Box<dyn serialport::SerialPort>>> {
    if !device.starts_with("/dev/tty") || device.contains("..") {
        return Err(Error::UnsafePath(device.into()));
    }
    let port = serialport::new(device, BAUD_RATE)
        .data_bits(serialport::DataBits::Eight)
        .parity(serialport::Parity::None)
        .stop_bits(serialport::StopBits::One)
        .flow_control(serialport::FlowControl::None)
        .timeout(Duration::from_millis(100))
        .open()
        .map_err(|e| Error::Io(std::io::Error::other(e)))?;
    Ok(SerialSession::new(port))
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, VecDeque};

    use super::*;

    /// A scripted CMU console: login, tty echo, kernel-log noise, and enough of a shell
    /// to run probes and the upload protocol.
    #[derive(Default)]
    struct MockCmu {
        out: VecDeque<u8>,
        line: String,
        state: u8, // 0 boot, 1 user sent, 2 shell
        user: String,
        files: BTreeMap<String, Vec<u8>>,
    }

    impl MockCmu {
        fn emit(&mut self, s: &str) {
            self.out.extend(s.replace('\n', "\r\n").bytes());
        }

        fn shell(&mut self, cmd: &str) -> (String, i32) {
            if cmd == "cat /jci/version.ini" {
                return (
                    "JCI_SW_VER=\"C0.0.0_70.00.352\"\nJCI_SW_VER_PATCH=\"B\"\n".into(),
                    0,
                );
            }
            if let Some(p) = cmd.strip_prefix(": > '").and_then(|r| r.strip_suffix('\'')) {
                self.files.insert(p.into(), vec![]);
                return (String::new(), 0);
            }
            if let Some(rest) = cmd.strip_prefix("printf '") {
                let (esc, tail) = rest.split_once("' >> '").unwrap();
                let bytes: Vec<u8> = esc
                    .split('\\')
                    .skip(1)
                    .map(|o| u8::from_str_radix(o, 8).unwrap())
                    .collect();
                self.files
                    .get_mut(tail.trim_end_matches('\''))
                    .unwrap()
                    .extend(bytes);
                return (String::new(), 0);
            }
            if let Some(p) = cmd
                .strip_prefix("wc -c < '")
                .and_then(|r| r.strip_suffix('\''))
            {
                return (format!("{}\n", self.files[p].len()), 0);
            }
            if let Some(rest) = cmd.strip_prefix("mv '") {
                let (a, b) = rest.split_once("' '").unwrap();
                let data = self.files.remove(a).unwrap();
                self.files.insert(b.trim_end_matches('\'').into(), data);
                return (String::new(), 0);
            }
            ("sh: not found\n".into(), 127)
        }

        fn handle(&mut self, line: &str) {
            match self.state {
                0 => {
                    if line.is_empty() {
                        self.emit("[  12.3] kernel noise\ncmu login: ");
                    } else {
                        self.user = line.into();
                        self.state = 1;
                        self.emit("Password: ");
                    }
                }
                1 => {
                    if self.user == "user" && line == "jci" {
                        self.state = 2;
                        self.emit("\n# ");
                    } else {
                        self.state = 0;
                        self.emit("\nLogin incorrect\ncmu login: ");
                    }
                }
                _ => {
                    self.emit(&format!("{line}\n")); // tty echo
                    let parts: Vec<&str> = line.splitn(3, "; ").collect();
                    if let [b, cmd, e] = parts[..] {
                        let (out, code) = self.shell(cmd);
                        let begin = b.trim_start_matches("echo ");
                        let end = e.trim_start_matches("echo ").trim_end_matches("$?");
                        self.emit(&format!(
                            "{begin}\n[  99.9] noise mid-command\n{out}{end}{code}\n# "
                        ));
                    }
                }
            }
        }
    }

    impl Read for MockCmu {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.out.is_empty() {
                return Err(ErrorKind::TimedOut.into());
            }
            let n = buf.len().min(self.out.len()).min(7); // small reads, like a UART
            for b in buf.iter_mut().take(n) {
                *b = self.out.pop_front().unwrap();
            }
            Ok(n)
        }
    }

    impl Write for MockCmu {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            for &b in data {
                if b == b'\n' {
                    let line = std::mem::take(&mut self.line);
                    self.handle(&line);
                } else {
                    self.line.push(b as char);
                }
            }
            Ok(data.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn session() -> SerialSession<MockCmu> {
        let mut s = SerialSession::new(MockCmu::default());
        s.timeout = Duration::from_millis(500);
        s.connect().unwrap();
        s.login().unwrap();
        s
    }

    #[test]
    fn logs_in_falling_back_through_credentials() {
        let s = session();
        assert!(s.logged_in);
    }

    #[test]
    fn exec_frames_output_despite_echo_and_noise() {
        let mut s = session();
        let out = s.exec(probe_command("version").unwrap()).unwrap();
        assert_eq!(out.code, 0);
        assert!(out.stdout.contains("70.00.352"), "{out:?}");
        assert_eq!(s.exec("frobnicate").unwrap().code, 127);
        assert!(s.exec("a\nb").is_err());
    }

    #[test]
    fn upload_round_trips_bytes() {
        let mut s = session();
        let data: Vec<u8> = (0..=255u8).cycle().take(700).collect();
        s.upload("/tmp/mzd/run.sh", &data).unwrap();
        assert_eq!(s.port.files["/tmp/mzd/run.sh"], data);
        assert!(matches!(
            s.upload("relative", b""),
            Err(Error::UnsafePath(_))
        ));
    }

    #[test]
    fn login_fails_cleanly() {
        let mut s = SerialSession::new(MockCmu::default());
        s.timeout = Duration::from_millis(200);
        s.credentials = vec![Credentials {
            user: "nobody".into(),
            password: "x".into(),
        }];
        s.connect().unwrap();
        assert!(matches!(s.login(), Err(Error::LoginFailed)));
    }

    #[test]
    fn dry_run_never_shows_passwords() {
        let t = dry_run_transcript("/dev/ttyTHS1", &["version", "rm"]);
        assert!(
            t.iter()
                .all(|l| !l.contains("jci\"") && !l.ends_with("/jci"))
        );
        assert!(t.iter().any(|l| l.contains("refuse unknown probe")));
    }
}
