//! The USB stick layout: files written under a directory (a mounted FAT32 stick, or a
//! staging directory). Dry-run by default: nothing is written until `dry_run` is false.

use std::path::PathBuf;

use crate::{Error, ExecOutput, Result, Transport, safe_relative};

#[derive(Debug)]
pub struct UsbBundle {
    pub root: PathBuf,
    pub dry_run: bool,
    /// Replace files from a previous bundle instead of refusing.
    pub replace: bool,
    /// Paths uploaded (or that would be, in dry-run).
    pub planned: Vec<String>,
}

impl UsbBundle {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            dry_run: true,
            replace: false,
            planned: Vec::new(),
        }
    }
}

impl Transport for UsbBundle {
    fn kind(&self) -> &'static str {
        "usb-bundle"
    }

    fn can_exec(&self) -> bool {
        false
    }

    fn connect(&mut self) -> Result<()> {
        if self.root.exists() && !self.root.is_dir() {
            return Err(Error::Refused(format!(
                "{} is not a directory",
                self.root.display()
            )));
        }
        if !self.replace {
            for marker in ["run.sh", "run.sh.done", "manifest.json"] {
                if self.root.join(marker).exists() {
                    return Err(Error::Refused(format!(
                        "{} already holds a bundle ({marker}); pass replace to overwrite",
                        self.root.display()
                    )));
                }
            }
        }
        if !self.dry_run {
            std::fs::create_dir_all(&self.root)?;
            if self.replace {
                let _ = std::fs::remove_file(self.root.join("run.sh.done"));
            }
        }
        Ok(())
    }

    fn login(&mut self) -> Result<()> {
        Ok(())
    }

    fn exec(&mut self, _command: &str) -> Result<ExecOutput> {
        Err(Error::Unsupported("exec", "usb-bundle"))
    }

    fn upload(&mut self, path: &str, data: &[u8]) -> Result<()> {
        if !safe_relative(path) {
            return Err(Error::UnsafePath(path.to_string()));
        }
        if !self.dry_run {
            let dst = self.root.join(path);
            if let Some(dir) = dst.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(dst, data)?;
        }
        self.planned.push(path.to_string());
        Ok(())
    }

    fn close(&mut self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::deliver;

    fn files() -> BTreeMap<String, Vec<u8>> {
        BTreeMap::from([
            ("run.sh".into(), b"#!/bin/sh\n".to_vec()),
            ("payload/a/b.js".into(), b"x".to_vec()),
        ])
    }

    #[test]
    fn dry_run_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("stick");
        let mut t = UsbBundle::new(&root);
        t.connect().unwrap();
        deliver(&mut t, "", &files()).unwrap();
        assert!(!root.exists());
        assert_eq!(t.planned, ["payload/a/b.js", "run.sh"]);
    }

    #[test]
    fn writes_and_refuses_to_clobber() {
        let dir = tempfile::tempdir().unwrap();
        let mut t = UsbBundle::new(dir.path());
        t.dry_run = false;
        t.connect().unwrap();
        deliver(&mut t, "", &files()).unwrap();
        assert_eq!(
            std::fs::read(dir.path().join("payload/a/b.js")).unwrap(),
            b"x"
        );
        let mut again = UsbBundle::new(dir.path());
        assert!(matches!(again.connect(), Err(Error::Refused(_))));
        assert!(matches!(again.exec("ls"), Err(Error::Unsupported(..))));
    }

    #[test]
    fn rejects_escaping_paths() {
        let mut t = UsbBundle::new("/nonexistent");
        for p in ["../x", "/etc/passwd", "a//b", "a/./b", ""] {
            assert!(matches!(t.upload(p, b""), Err(Error::UnsafePath(_))), "{p}");
        }
    }
}
