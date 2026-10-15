//! The data in `data/` and `assets/`, embedded at build time and parsed once.

use std::sync::OnceLock;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const FIRMWARE_TOML: &str = include_str!("../../../data/firmware.toml");
const TWEAKS_TOML: &str = include_str!("../../../data/tweaks.toml");
const SAFETY_TOML: &str = include_str!("../../../data/safety.toml");

/// The CMU-side sh runtime embedded into every run.sh.
pub const RUNTIME_SH: &str = include_str!("../../../assets/cmu/runtime.sh");

/// Asset files steps may reference, keyed by their path under `assets/`.
const ASSETS: &[(&str, &str)] = &[
    (
        "patches/no-disclaimer.js",
        include_str!("../../../assets/patches/no-disclaimer.js"),
    ),
    (
        "patches/list-loop.js",
        include_str!("../../../assets/patches/list-loop.js"),
    ),
];

pub fn asset(path: &str) -> Option<&'static str> {
    ASSETS.iter().find(|(p, _)| *p == path).map(|(_, s)| *s)
}

// ------------------------------------------------------------------ firmware.toml

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ProfileStatus {
    Target,
    Untested,
    Legacy,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum FirstAccess {
    SerialAutorun,
    UsbDataretrieval,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "strategy", rename_all = "kebab-case", deny_unknown_fields)]
pub enum BackupPolicy {
    /// `<file>.mzd-orig` next to the file.
    Sibling,
    /// Mirrored tree under `dir`.
    Dir { dir: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub major_min: Option<u16>,
    pub major: u16,
    pub patch_min: u16,
    pub patch_max: u16,
    pub status: ProfileStatus,
    pub first_access: FirstAccess,
    pub backup: BackupPolicy,
    pub rootfs_reserve_kib: u32,
    pub rootfs_tight: bool,
    pub units: Vec<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FirmwareFile {
    pub profile: Vec<Profile>,
}

// ------------------------------------------------------------------ tweaks.toml

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum TweakStatus {
    Ported,
    Unported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Risk {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Evidence {
    /// MZD-AIO-TI shipped it for this firmware; our implementation is unverified.
    Upstream,
    /// Verified by us on real hardware.
    Bench,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Compat {
    pub profile: String,
    pub evidence: Evidence,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum OptionKind {
    Bool {
        #[serde(default)]
        default: bool,
    },
    /// A host PNG file, checked against an exact size.
    Png { width: u32, height: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TweakOption {
    pub name: String,
    #[serde(flatten)]
    pub kind: OptionKind,
    pub description: String,
}

/// One operation on the CMU, as written in the catalog. See data/tweaks.toml.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum StepOp {
    RequireFile {
        path: String,
    },
    RequireLiteral {
        path: String,
        literal: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        count: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        applied: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hint: Option<String>,
    },
    RequireBackup {
        path: String,
    },
    Backup {
        path: String,
    },
    AppendBlock {
        path: String,
        block: String,
        asset: String,
    },
    RemoveBlock {
        path: String,
        block: String,
    },
    ReplaceLiteral {
        path: String,
        find: String,
        replace: String,
    },
    InstallFile {
        path: String,
        option: String,
    },
    Restore {
        path: String,
    },
    Exec {
        argv: Vec<String>,
    },
}

impl StepOp {
    pub fn path(&self) -> &str {
        match self {
            Self::Exec { argv } => argv.first().map_or("", String::as_str),
            Self::RequireFile { path }
            | Self::RequireLiteral { path, .. }
            | Self::RequireBackup { path }
            | Self::Backup { path }
            | Self::AppendBlock { path, .. }
            | Self::RemoveBlock { path, .. }
            | Self::ReplaceLiteral { path, .. }
            | Self::InstallFile { path, .. }
            | Self::Restore { path } => path,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::RequireFile { .. } => "require_file",
            Self::RequireLiteral { .. } => "require_literal",
            Self::RequireBackup { .. } => "require_backup",
            Self::Backup { .. } => "backup",
            Self::AppendBlock { .. } => "append_block",
            Self::RemoveBlock { .. } => "remove_block",
            Self::ReplaceLiteral { .. } => "replace_literal",
            Self::InstallFile { .. } => "install_file",
            Self::Restore { .. } => "restore",
            Self::Exec { .. } => "exec",
        }
    }

    /// Runs in preflight, before anything is written.
    pub fn is_preflight(&self) -> bool {
        matches!(
            self,
            Self::RequireFile { .. } | Self::RequireLiteral { .. } | Self::RequireBackup { .. }
        )
    }

    /// Changes a CMU file.
    pub fn is_write(&self) -> bool {
        matches!(
            self,
            Self::AppendBlock { .. }
                | Self::RemoveBlock { .. }
                | Self::ReplaceLiteral { .. }
                | Self::InstallFile { .. }
                | Self::Restore { .. }
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Step {
    #[serde(flatten)]
    pub op: StepOp,
    /// Only include this step when the named bool option is true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<String>,
    /// Only include this step when the named bool option is false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unless: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Tweak {
    pub id: String,
    pub status: TweakStatus,
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk: Option<Risk>,
    /// Added automatically by tweaks that list it in `requires`; never user-selected.
    #[serde(default)]
    pub prerequisite: bool,
    /// Has no uninstall by design.
    #[serde(default)]
    pub permanent: bool,
    #[serde(default)]
    pub requires: Vec<String>,
    /// Token the config must repeat to install a tweak that reduces driving safety.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acknowledge: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acknowledge_why: Option<String>,
    #[serde(default)]
    pub compat: Vec<Compat>,
    #[serde(default)]
    pub upstream: Vec<String>,
    #[serde(default)]
    pub notes: Vec<String>,
    #[serde(default, rename = "option")]
    pub options: Vec<TweakOption>,
    #[serde(default)]
    pub install: Vec<Step>,
    #[serde(default)]
    pub uninstall: Vec<Step>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TweaksFile {
    pub tweak: Vec<Tweak>,
}

// ------------------------------------------------------------------ safety.toml

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub id: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Safety {
    pub protected_paths: Vec<String>,
    pub exec_prefixes: Vec<String>,
    pub payload_max_bytes: u64,
    pub rootfs_warn_bytes: u64,
    pub default_reserve_kib: u32,
    #[serde(rename = "rule")]
    pub rules: Vec<Rule>,
}

// ------------------------------------------------------------------ loading

#[derive(Debug)]
pub struct Catalog {
    pub profiles: Vec<Profile>,
    pub tweaks: Vec<Tweak>,
    pub safety: Safety,
}

impl Catalog {
    pub fn tweak(&self, id: &str) -> Option<&Tweak> {
        self.tweaks.iter().find(|t| t.id == id)
    }

    pub fn profile(&self, id: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.id == id)
    }
}

/// The embedded catalog. Panics only if the files in `data/` are malformed, which the
/// test suite (and therefore `nix flake check`) catches before anything ships.
pub fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let firmware: FirmwareFile = toml::from_str(FIRMWARE_TOML).expect("data/firmware.toml");
        let tweaks: TweaksFile = toml::from_str(TWEAKS_TOML).expect("data/tweaks.toml");
        let safety: Safety = toml::from_str(SAFETY_TOML).expect("data/safety.toml");
        Catalog {
            profiles: firmware.profile,
            tweaks: tweaks.tweak,
            safety,
        }
    })
}

/// Consistency problems in the embedded data (empty when sound). Run by the tests.
pub fn self_check() -> Vec<String> {
    let c = catalog();
    let mut problems = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for t in &c.tweaks {
        if !seen.insert(t.id.as_str()) {
            problems.push(format!("duplicate tweak id {}", t.id));
        }
        let ported = t.status == TweakStatus::Ported;
        if ported && t.install.is_empty() {
            problems.push(format!("{}: ported without install steps", t.id));
        }
        if ported && !t.permanent && t.uninstall.is_empty() {
            problems.push(format!("{}: not permanent but has no uninstall", t.id));
        }
        if !ported && (!t.install.is_empty() || !t.uninstall.is_empty()) {
            problems.push(format!("{}: unported tweaks must not carry steps", t.id));
        }
        if ported && t.risk.is_none() {
            problems.push(format!("{}: ported tweaks need a risk level", t.id));
        }
        if t.acknowledge.is_some() != t.acknowledge_why.is_some() {
            problems.push(format!(
                "{}: acknowledge and acknowledge_why go together",
                t.id
            ));
        }
        for r in &t.requires {
            if !c.tweak(r).is_some_and(|p| p.prerequisite) {
                problems.push(format!("{}: requires unknown prerequisite {r}", t.id));
            }
        }
        for cp in &t.compat {
            if c.profile(&cp.profile).is_none() {
                problems.push(format!(
                    "{}: compat names unknown profile {}",
                    t.id, cp.profile
                ));
            }
        }
        for s in t.install.iter().chain(&t.uninstall) {
            for name in [&s.when, &s.unless].into_iter().flatten() {
                let ok = t
                    .options
                    .iter()
                    .any(|o| &o.name == name && matches!(o.kind, OptionKind::Bool { .. }));
                if !ok {
                    problems.push(format!(
                        "{}: when/unless names unknown bool option {name}",
                        t.id
                    ));
                }
            }
            match &s.op {
                StepOp::AppendBlock { asset: a, .. } if asset(a).is_none() => {
                    problems.push(format!("{}: unknown asset {a}", t.id));
                }
                StepOp::InstallFile { option, .. }
                    if !t
                        .options
                        .iter()
                        .any(|o| &o.name == option && matches!(o.kind, OptionKind::Png { .. })) =>
                {
                    problems.push(format!(
                        "{}: install_file names unknown file option {option}",
                        t.id
                    ));
                }
                _ => {}
            }
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_data_is_consistent() {
        assert_eq!(self_check(), Vec::<String>::new());
    }

    #[test]
    fn catalog_has_expected_shape() {
        let c = catalog();
        let ported: Vec<_> = c
            .tweaks
            .iter()
            .filter(|t| t.status == TweakStatus::Ported && !t.prerequisite)
            .map(|t| t.id.as_str())
            .collect();
        assert_eq!(
            ported,
            [
                "no-disclaimer",
                "list-loop",
                "background",
                "touchscreen-at-speed"
            ]
        );
        assert!(
            c.tweaks
                .iter()
                .filter(|t| t.status == TweakStatus::Unported)
                .count()
                >= 30
        );
        assert!(
            c.safety
                .protected_paths
                .contains(&"/jci/sm/sm.conf".to_string())
        );
    }
}
