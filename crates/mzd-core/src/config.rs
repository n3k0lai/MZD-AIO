//! Bundle configuration (TOML on disk, JSON over MCP) and host asset loading.

use std::collections::BTreeMap;
use std::path::Path;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::catalog::{OptionKind, catalog};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// The CMU runs every preflight check, logs to the USB stick, and changes nothing.
    #[default]
    Check,
    /// Preflight, backups, then the changes.
    Apply,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    #[default]
    Install,
    Uninstall,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TweakEntry {
    pub id: String,
    #[serde(default)]
    pub action: Action,
    /// Required for tweaks that reduce driving safety; must equal the tweak's token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acknowledge: Option<String>,
    #[serde(default)]
    pub options: BTreeMap<String, serde_json::Value>,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BundleConfig {
    /// Exact firmware of the target CMU, e.g. "70.00.352B".
    pub firmware: String,
    #[serde(default)]
    pub mode: Mode,
    /// Reboot after a successful apply so the GUI reloads.
    #[serde(default = "yes")]
    pub reboot: bool,
    /// Permit firmware newer than anything tested.
    #[serde(default)]
    pub allow_untested_firmware: bool,
    #[serde(rename = "tweak")]
    pub tweaks: Vec<TweakEntry>,
}

impl BundleConfig {
    pub fn from_toml(s: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(s)
    }
}

/// Host files named by file-typed options, keyed by the option's string value.
pub type Assets = BTreeMap<String, Vec<u8>>;

/// Read every file a config's file-typed options name, relative to `base_dir`.
pub fn load_assets(config: &BundleConfig, base_dir: &Path) -> std::io::Result<Assets> {
    let mut assets = Assets::new();
    for entry in &config.tweaks {
        let Some(tweak) = catalog().tweak(&entry.id) else {
            continue;
        };
        for opt in &tweak.options {
            if !matches!(opt.kind, OptionKind::Png { .. }) {
                continue;
            }
            if let Some(serde_json::Value::String(rel)) = entry.options.get(&opt.name) {
                let data = std::fs::read(base_dir.join(rel)).map_err(|e| {
                    std::io::Error::new(
                        e.kind(),
                        format!("{}: option {}: {rel}: {e}", entry.id, opt.name),
                    )
                })?;
                assets.insert(rel.clone(), data);
            }
        }
    }
    Ok(assets)
}
