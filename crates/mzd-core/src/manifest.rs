//! `manifest.json`: what a bundle will do, written next to run.sh. Deterministic: no
//! timestamps, every map is ordered, so the same inputs give the same bytes.

use serde::Serialize;

use crate::catalog::{BackupPolicy, StepOp};
use crate::config::{Action, Mode};
use crate::plan::{Partition, Plan, PlannedTweak, SpaceNeed, sha256_hex};

pub const FORMAT: &str = "mzd-tweaks/manifest@1";

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct Manifest {
    pub format: &'static str,
    pub generator: Generator,
    pub firmware: Firmware,
    pub mode: Mode,
    pub reboot: bool,
    pub entry: &'static str,
    pub tweaks: Vec<PlannedTweak>,
    pub files: Vec<FileEntry>,
    pub space: Vec<SpaceNeed>,
    pub payloads: Vec<PayloadEntry>,
    /// Shell commands to put each touched file back by hand over serial.
    pub manual_restore: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct Generator {
    pub name: &'static str,
    pub version: &'static str,
    pub upstream_ref: &'static str,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct Firmware {
    pub requested: String,
    pub checked_on_cmu: String,
    pub profile: String,
    pub profile_label: String,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct FileEntry {
    pub path: String,
    pub partition: Partition,
    /// `tweak:op` in execution order.
    pub ops: Vec<String>,
    pub backup: Option<String>,
    /// Edited by a permanent prerequisite; never reverted.
    pub permanent: bool,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct PayloadEntry {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}

pub fn backup_location(policy: &BackupPolicy, path: &str) -> String {
    match policy {
        BackupPolicy::Sibling => format!("{path}.mzd-orig"),
        BackupPolicy::Dir { dir } => format!("{dir}{path}"),
    }
}

pub fn build(plan: &Plan) -> Manifest {
    let mut files: Vec<FileEntry> = Vec::new();
    for t in &plan.tweaks {
        for s in &t.steps {
            if s.is_preflight() || matches!(s, StepOp::Exec { .. }) {
                continue;
            }
            let path = s.path();
            let idx = match files.iter().position(|f| f.path == path) {
                Some(i) => i,
                None => {
                    files.push(FileEntry {
                        path: path.to_string(),
                        partition: Partition::of(path),
                        ops: vec![],
                        backup: None,
                        permanent: false,
                    });
                    files.len() - 1
                }
            };
            let f = &mut files[idx];
            f.ops.push(format!("{}:{}", t.id, s.name()));
            f.permanent |= t.prerequisite && s.is_write();
            if matches!(s, StepOp::Backup { .. })
                || (t.action == Action::Uninstall && matches!(s, StepOp::Restore { .. }))
            {
                f.backup = Some(backup_location(&plan.profile.backup, path));
            }
        }
    }
    let manual_restore = files
        .iter()
        .filter_map(|f| {
            f.backup
                .as_ref()
                .map(|b| format!("cat '{b}' > '{}'", f.path))
        })
        .collect();
    let checked_on_cmu = format!("{}{}", plan.firmware.base(), plan.firmware.letter);
    Manifest {
        format: FORMAT,
        generator: Generator {
            name: crate::GENERATOR_NAME,
            version: crate::GENERATOR_VERSION,
            upstream_ref: crate::UPSTREAM_REF,
        },
        firmware: Firmware {
            requested: plan.config.firmware.clone(),
            checked_on_cmu,
            profile: plan.profile.id.clone(),
            profile_label: plan.profile.label.clone(),
        },
        mode: plan.config.mode,
        reboot: plan.config.reboot,
        entry: "run.sh",
        tweaks: plan.tweaks.clone(),
        files,
        space: plan.space.clone(),
        payloads: plan
            .payloads
            .iter()
            .map(|(p, d)| PayloadEntry {
                path: p.clone(),
                bytes: d.len() as u64,
                sha256: sha256_hex(d),
            })
            .collect(),
        manual_restore,
        warnings: plan.warnings.clone(),
    }
}
