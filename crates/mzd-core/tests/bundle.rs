//! End-to-end: config → plan → bundle → run the generated run.sh against the fake CMU
//! tree in tests/fixtures/fake-cmu, with every POSIX shell available on the host
//! (BusyBox included when present, since that is what the CMU runs).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use mzd_core::config::{Assets, BundleConfig};
use mzd_core::plan::{PlanError, plan};
use mzd_core::render::{Bundle, render};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/fake-cmu");

fn png(w: u32, h: u32) -> Vec<u8> {
    let mut v = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    v.extend(w.to_be_bytes());
    v.extend(h.to_be_bytes());
    v.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
    v
}

fn assets() -> Assets {
    BTreeMap::from([("bg.png".to_string(), png(800, 480))])
}

fn config(src: &str) -> BundleConfig {
    BundleConfig::from_toml(src).expect("valid toml")
}

fn build(src: &str) -> Bundle {
    let p = plan(&config(src), &assets()).unwrap_or_else(|e| panic!("{e}"));
    render(&p)
}

fn reject(src: &str) -> PlanError {
    plan(&config(src), &assets()).expect_err("config should be rejected")
}

const ALL: &str = r#"
firmware = "70.00.352B"
mode = "apply"
[[tweak]]
id = "no-disclaimer"
[[tweak]]
id = "list-loop"
[[tweak]]
id = "background"
options = { image = "bg.png" }
[[tweak]]
id = "touchscreen-at-speed"
acknowledge = "driving-distraction"
"#;

const UNINSTALL_ALL: &str = r#"
firmware = "70.00.352B"
mode = "apply"
[[tweak]]
id = "no-disclaimer"
action = "uninstall"
[[tweak]]
id = "list-loop"
action = "uninstall"
[[tweak]]
id = "background"
action = "uninstall"
[[tweak]]
id = "touchscreen-at-speed"
action = "uninstall"
"#;

// ---------------------------------------------------------------- planning

#[test]
fn defaults_are_check_mode_and_prerequisites_come_first() {
    let b = build("firmware = \"70.00.352B\"\n[[tweak]]\nid = \"list-loop\"\n");
    let m = &b.manifest;
    assert_eq!(serde_json::to_value(m.mode).unwrap(), "check");
    let ids: Vec<_> = m.tweaks.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, ["sm-watchdog-off", "list-loop"]);
    assert!(
        m.files
            .iter()
            .any(|f| f.path == "/jci/sm/sm.conf" && f.permanent)
    );
}

#[test]
fn rendering_is_deterministic() {
    let a = build(ALL);
    let b = build(ALL);
    assert_eq!(a.files, b.files);
    let names: Vec<_> = a.files.keys().map(String::as_str).collect();
    assert_eq!(
        names,
        [
            "README.txt",
            "manifest.json",
            "payload/background/image.png",
            "payload/list-loop/list-loop.js",
            "payload/no-disclaimer/no-disclaimer.js",
            "run.sh",
        ]
    );
}

#[test]
fn v70_backups_go_to_resources_and_v59_next_to_the_file() {
    let v70 = build(ALL);
    let f = v70
        .manifest
        .files
        .iter()
        .find(|f| f.path.ends_with("Common.js"))
        .unwrap();
    assert_eq!(
        f.backup.as_deref(),
        Some("/tmp/mnt/resources/dev/mzd-tweaks/backup/jci/gui/common/js/Common.js")
    );
    let v59 = build(&ALL.replace("70.00.352B", "59.00.540"));
    let f = v59
        .manifest
        .files
        .iter()
        .find(|f| f.path.ends_with("Common.js"))
        .unwrap();
    assert_eq!(
        f.backup.as_deref(),
        Some("/jci/gui/common/js/Common.js.mzd-orig")
    );
    assert!(
        v59.manifest
            .warnings
            .iter()
            .any(|w| w.contains("patch letter"))
    );
}

#[test]
fn rejects_bad_configs() {
    let one = |fw: &str, body: &str| format!("firmware = \"{fw}\"\n{body}");
    let cases = [
        (
            one("70.00.352B", "[[tweak]]\nid = \"nope\""),
            "unknown tweak",
        ),
        (
            one("70.00.352B", "[[tweak]]\nid = \"speedometer\""),
            "not ported",
        ),
        (
            one("70.00.352B", "[[tweak]]\nid = \"sm-watchdog-off\""),
            "prerequisite",
        ),
        (
            one("70.00.352B", "[[tweak]]\nid = \"touchscreen-at-speed\""),
            "S08",
        ),
        (
            one(
                "70.00.352B",
                "[[tweak]]\nid = \"list-loop\"\n[[tweak]]\nid = \"list-loop\"",
            ),
            "more than once",
        ),
        (
            one(
                "70.00.352B",
                "[[tweak]]\nid = \"list-loop\"\noptions = { x = 1 }",
            ),
            "unknown option",
        ),
        (one("70.00.400", "[[tweak]]\nid = \"list-loop\""), "S01"),
        (one("59.00.443", "[[tweak]]\nid = \"list-loop\""), "S01"),
        (one("31.00.100", "[[tweak]]\nid = \"list-loop\""), "S01"),
        (one("seventy", "[[tweak]]\nid = \"list-loop\""), "S01"),
        (
            one(
                "70.00.352B",
                "[[tweak]]\nid = \"background\"\noptions = { image = \"missing.png\" }",
            ),
            "not loaded",
        ),
    ];
    for (src, needle) in cases {
        let e = reject(&src);
        assert!(
            e.errors.iter().any(|m| m.contains(needle)),
            "{needle}: {:?}",
            e.errors
        );
    }
    let wrong_size = BTreeMap::from([("bg.png".to_string(), png(1024, 600))]);
    let e = plan(
        &config("firmware = \"70.00.352B\"\n[[tweak]]\nid = \"background\"\noptions = { image = \"bg.png\" }"),
        &wrong_size,
    )
    .unwrap_err();
    assert!(
        e.errors.iter().any(|m| m.starts_with("S10")),
        "{:?}",
        e.errors
    );
}

#[test]
fn untested_firmware_needs_explicit_opt_in() {
    let src =
        "firmware = \"70.00.400\"\nallow_untested_firmware = true\n[[tweak]]\nid = \"list-loop\"";
    // Untested profiles carry no compat entries yet, so the tweak is still refused.
    let e = reject(src);
    assert!(
        e.errors
            .iter()
            .any(|m| m.contains("no compatibility entry")),
        "{:?}",
        e.errors
    );
}

// ---------------------------------------------------------------- running run.sh

fn shells() -> Vec<Vec<String>> {
    let mut out = Vec::new();
    for cand in [vec!["sh"], vec!["dash"], vec!["busybox", "sh"]] {
        let ok = Command::new(cand[0])
            .args(&cand[1..])
            .args(["-c", "exit 0"])
            .output()
            .is_ok_and(|o| o.status.success());
        if ok {
            out.push(cand.into_iter().map(String::from).collect());
        }
    }
    assert!(!out.is_empty(), "no POSIX shell found");
    out
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for e in fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let dst = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_tree(&e.path(), &dst);
        } else {
            fs::copy(e.path(), &dst).unwrap();
        }
    }
}

struct Cmu {
    _dir: tempfile::TempDir,
    root: PathBuf,
    usb: PathBuf,
    shell: Vec<String>,
}

impl Cmu {
    fn new(shell: &[String]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        copy_tree(Path::new(FIXTURE), &root);
        let usb = dir.path().join("usb");
        Self {
            root,
            usb,
            _dir: dir,
            shell: shell.to_vec(),
        }
    }

    fn read(&self, p: &str) -> String {
        fs::read_to_string(self.root.join(p.trim_start_matches('/'))).unwrap()
    }

    fn run(&self, bundle: &Bundle) -> Output {
        let _ = fs::remove_dir_all(&self.usb);
        for (path, data) in &bundle.files {
            let dst = self.usb.join(path);
            fs::create_dir_all(dst.parent().unwrap()).unwrap();
            fs::write(dst, data).unwrap();
        }
        Command::new(&self.shell[0])
            .args(&self.shell[1..])
            .arg(self.usb.join("run.sh"))
            .env("MZD_ROOT", &self.root)
            .env("MZD_SIMULATE", "1")
            .env("TMPDIR", self._dir.path())
            .output()
            .unwrap()
    }

    fn snapshot(&self) -> BTreeMap<String, Vec<u8>> {
        fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
            for e in fs::read_dir(dir).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    walk(base, &p, out);
                } else {
                    out.insert(
                        p.strip_prefix(base).unwrap().display().to_string(),
                        fs::read(&p).unwrap(),
                    );
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(&self.root, &self.root, &mut out);
        out
    }
}

fn ok(o: &Output) {
    assert!(
        o.status.success(),
        "run.sh failed:\n{}",
        String::from_utf8_lossy(&o.stderr)
    );
}

#[test]
fn check_mode_changes_nothing() {
    for sh in shells() {
        let cmu = Cmu::new(&sh);
        let before = cmu.snapshot();
        let out = cmu.run(&build(&ALL.replace("mode = \"apply\"", "")));
        ok(&out);
        assert_eq!(cmu.snapshot(), before, "{sh:?}");
        assert!(cmu.usb.join("run.sh.done").exists() && !cmu.usb.join("run.sh").exists());
        let log = fs::read_to_string(cmu.usb.join("mzd-tweaks.log")).unwrap();
        assert!(log.contains("check passed"), "{log}");
    }
}

#[test]
fn firmware_mismatch_aborts_before_any_change() {
    for sh in shells() {
        let cmu = Cmu::new(&sh);
        let before = cmu.snapshot();
        let out = cmu.run(&build(&ALL.replace("70.00.352B", "70.00.335")));
        assert!(!out.status.success());
        assert!(String::from_utf8_lossy(&out.stderr).contains("firmware mismatch"));
        assert_eq!(cmu.snapshot(), before);
    }
}

#[test]
fn failed_anchor_aborts_before_any_change() {
    for sh in shells() {
        let cmu = Cmu::new(&sh);
        fs::write(
            cmu.root.join("jci/gui/apps/system/js/systemApp.js"),
            "legacy replaced file\n",
        )
        .unwrap();
        let before = cmu.snapshot();
        let out = cmu.run(&build(ALL));
        assert!(!out.status.success());
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(
            err.contains("preflight failed") && err.contains("hint:"),
            "{err}"
        );
        assert_eq!(cmu.snapshot(), before);
    }
}

#[test]
fn apply_is_idempotent_and_uninstall_restores() {
    for sh in shells() {
        let cmu = Cmu::new(&sh);
        let original = cmu.snapshot();
        let install = build(ALL);

        ok(&cmu.run(&install));
        let sys = cmu.read("/jci/gui/apps/system/js/systemApp.js");
        assert!(
            sys.ends_with("/* mzd-tweaks:no-disclaimer END */\n"),
            "{sys}"
        );
        assert!(
            cmu.read("/jci/gui/common/js/Common.js")
                .contains("\"Global.AtSpeed_mzdDisabled\" :")
        );
        assert!(
            cmu.read("/jci/sm/sm.conf")
                .contains("--noWatchdogs\" watchdog_enable=\"false\"")
        );
        assert!(
            cmu.read("/jci/scripts/calls.log")
                .contains("set_speed_restriction_config.sh disable")
        );
        let backup = "tmp/mnt/resources/dev/mzd-tweaks/backup/jci/gui/common/js/Common.js";
        assert_eq!(
            fs::read(cmu.root.join(backup)).unwrap(),
            original["jci/gui/common/js/Common.js"]
        );
        assert!(cmu.usb.join("backups/jci/gui/common/js/Common.js").exists());

        let once = cmu.snapshot();
        ok(&cmu.run(&install));
        let mut twice = cmu.snapshot();
        let calls = "jci/scripts/calls.log";
        assert_eq!(
            twice.remove(calls).map(|c| c.len() * 2),
            once.get(calls).map(|c| c.len() * 4)
        );
        let mut once = once;
        once.remove(calls);
        assert_eq!(once, twice, "second apply changed files ({sh:?})");

        ok(&cmu.run(&build(UNINSTALL_ALL)));
        for f in [
            "jci/gui/apps/system/js/systemApp.js",
            "jci/gui/common/controls/List2/js/List2Ctrl.js",
            "jci/gui/common/js/Common.js",
            "jci/gui/common/images/background.png",
        ] {
            assert_eq!(cmu.snapshot()[f], original[f], "{f} not restored ({sh:?})");
        }
        // sm.conf is permanent by design (rule S02).
        assert_ne!(cmu.snapshot()["jci/sm/sm.conf"], original["jci/sm/sm.conf"]);
    }
}
