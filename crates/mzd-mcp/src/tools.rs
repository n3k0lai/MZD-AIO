//! Tool logic, independent of the MCP plumbing so it can be tested directly.

use std::path::{Path, PathBuf};

use mzd_core::catalog::{TweakStatus, catalog};
use mzd_core::config::{BundleConfig, load_assets};
use mzd_core::firmware::check_compat;
use mzd_core::plan::plan;
use mzd_core::render::render;
use mzd_transport::serial::{dry_run_transcript, probe_command};
use mzd_transport::usb::UsbBundle;
use mzd_transport::{Transport, deliver};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

pub type ToolResult = Result<Value, String>;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListArgs {
    /// Also list catalogued tweaks that are not ported yet.
    #[serde(default)]
    pub include_unported: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct IdArgs {
    /// Tweak id, e.g. "list-loop".
    pub id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CompatArgs {
    /// Exact CMU firmware, e.g. "70.00.352B" or "59.00.540".
    pub firmware: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PlanArgs {
    pub config: BundleConfig,
    /// Directory that relative file options (e.g. a background image) resolve against.
    #[serde(default)]
    pub base_dir: Option<String>,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BuildArgs {
    pub config: BundleConfig,
    #[serde(default)]
    pub base_dir: Option<String>,
    /// Directory to write the USB layout into (a mounted stick or a staging dir).
    pub out_dir: String,
    /// Default true: report what would be written without writing.
    #[serde(default = "yes")]
    pub dry_run: bool,
    /// Overwrite a previous bundle in out_dir.
    #[serde(default)]
    pub replace: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProbeArgs {
    /// Explicit serial device on the bench Jetson, e.g. "/dev/ttyTHS1". Required.
    pub device: String,
    /// Read-only probes to run: "version", "df", "uptime".
    #[serde(default = "default_probes")]
    pub probes: Vec<String>,
    /// Default true: describe the session without opening the device.
    #[serde(default = "yes")]
    pub dry_run: bool,
}

fn default_probes() -> Vec<String> {
    vec!["version".into()]
}

fn to_value<T: serde::Serialize>(v: &T) -> ToolResult {
    serde_json::to_value(v).map_err(|e| e.to_string())
}

pub fn list_tweaks(a: ListArgs) -> ToolResult {
    let rows: Vec<Value> = catalog()
        .tweaks
        .iter()
        .filter(|t| !t.prerequisite && (a.include_unported || t.status == TweakStatus::Ported))
        .map(|t| {
            json!({
                "id": t.id, "status": t.status, "title": t.title, "summary": t.summary,
                "risk": t.risk, "needs_acknowledge": t.acknowledge,
            })
        })
        .collect();
    Ok(json!({ "tweaks": rows }))
}

pub fn describe_tweak(a: IdArgs) -> ToolResult {
    catalog()
        .tweak(&a.id)
        .map(to_value)
        .unwrap_or_else(|| Err(format!("unknown tweak {:?}", a.id)))
}

pub fn compat(a: CompatArgs) -> ToolResult {
    check_compat(&a.firmware)
        .map_err(|e| e.to_string())
        .and_then(|r| to_value(&r))
}

fn base(dir: &Option<String>) -> PathBuf {
    dir.as_deref()
        .map_or_else(|| PathBuf::from("."), PathBuf::from)
}

pub fn plan_bundle(a: PlanArgs) -> ToolResult {
    let assets = load_assets(&a.config, &base(&a.base_dir)).map_err(|e| e.to_string())?;
    match plan(&a.config, &assets) {
        Ok(p) => to_value(&render(&p).manifest),
        Err(e) => Err(serde_json::to_string_pretty(&e).unwrap_or_else(|_| e.to_string())),
    }
}

pub fn build_bundle(a: BuildArgs) -> ToolResult {
    let assets = load_assets(&a.config, &base(&a.base_dir)).map_err(|e| e.to_string())?;
    let p = plan(&a.config, &assets)
        .map_err(|e| serde_json::to_string_pretty(&e).unwrap_or_default())?;
    let bundle = render(&p);
    let mut usb = UsbBundle::new(Path::new(&a.out_dir));
    usb.dry_run = a.dry_run;
    usb.replace = a.replace;
    usb.connect().map_err(|e| e.to_string())?;
    let files = deliver(&mut usb, "", &bundle.files).map_err(|e| e.to_string())?;
    Ok(json!({
        "dry_run": a.dry_run,
        "out_dir": a.out_dir,
        "files": files,
        "mode": bundle.manifest.mode,
        "warnings": bundle.manifest.warnings,
    }))
}

pub fn serial_probe(a: ProbeArgs) -> ToolResult {
    if !a.device.starts_with("/dev/tty") {
        return Err(format!(
            "device must be an explicit /dev/tty* path, got {:?}",
            a.device
        ));
    }
    let probes: Vec<&str> = a.probes.iter().map(String::as_str).collect();
    if let Some(bad) = probes.iter().find(|p| probe_command(p).is_none()) {
        return Err(format!(
            "unknown probe {bad:?}; only read-only probes are allowed"
        ));
    }
    if a.dry_run {
        return Ok(
            json!({ "dry_run": true, "transcript": dry_run_transcript(&a.device, &probes) }),
        );
    }
    run_probes(&a.device, &probes)
}

#[cfg(feature = "serial")]
fn run_probes(device: &str, probes: &[&str]) -> ToolResult {
    let err = |e: mzd_transport::Error| e.to_string();
    let mut s = mzd_transport::serial::open(device).map_err(err)?;
    s.connect().map_err(err)?;
    s.login().map_err(err)?;
    let mut results = serde_json::Map::new();
    for p in probes {
        let out = s.exec(probe_command(p).unwrap_or("true")).map_err(err)?;
        results.insert(
            (*p).into(),
            json!({ "exit": out.code, "stdout": out.stdout }),
        );
    }
    s.close().map_err(err)?;
    Ok(json!({ "dry_run": false, "results": results }))
}

#[cfg(not(feature = "serial"))]
fn run_probes(_device: &str, _probes: &[&str]) -> ToolResult {
    Err("this server was built without serial support (cargo feature \"serial\")".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(v: Value) -> BundleConfig {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn plan_and_build_default_to_dry_run() {
        let config = json!({ "firmware": "70.00.352B", "tweak": [{ "id": "list-loop" }] });
        let m = plan_bundle(PlanArgs {
            config: cfg(config.clone()),
            base_dir: None,
        })
        .unwrap();
        assert_eq!(m["mode"], "check");
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("stick").display().to_string();
        let args: BuildArgs =
            serde_json::from_value(json!({ "config": config, "out_dir": out })).unwrap();
        let r = build_bundle(args).unwrap();
        assert_eq!(r["dry_run"], true);
        assert!(!dir.path().join("stick").exists());
    }

    #[test]
    fn errors_are_reported_not_panicked() {
        let bad = cfg(json!({ "firmware": "31.00.100", "tweak": [{ "id": "list-loop" }] }));
        assert!(
            plan_bundle(PlanArgs {
                config: bad,
                base_dir: None
            })
            .unwrap_err()
            .contains("S01")
        );
        assert!(describe_tweak(IdArgs { id: "nope".into() }).is_err());
    }

    #[test]
    fn serial_probe_requires_device_and_is_dry_by_default() {
        let a: ProbeArgs = serde_json::from_value(json!({ "device": "/dev/ttyTHS1" })).unwrap();
        assert_eq!(serial_probe(a).unwrap()["dry_run"], true);
        assert!(serde_json::from_value::<ProbeArgs>(json!({})).is_err());
        let a: ProbeArgs = serde_json::from_value(json!({ "device": "COM3" })).unwrap();
        assert!(serial_probe(a).is_err());
        let a: ProbeArgs =
            serde_json::from_value(json!({ "device": "/dev/ttyTHS1", "probes": ["rm"] })).unwrap();
        assert!(serial_probe(a).is_err());
    }
}
