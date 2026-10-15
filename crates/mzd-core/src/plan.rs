//! Config → validated, ordered plan. Pure: host files arrive through [`Assets`].

use std::collections::BTreeMap;
use std::fmt;

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::catalog::{
    BackupPolicy, Evidence, OptionKind, Profile, ProfileStatus, StepOp, Tweak, TweakStatus, asset,
    catalog,
};
use crate::config::{Action, Assets, BundleConfig};
use crate::firmware::{Version, profile_for};
use crate::safety::check_steps;

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct PlannedTweak {
    pub id: String,
    pub title: String,
    pub action: Action,
    pub prerequisite: bool,
    pub evidence: Evidence,
    /// Options as given, with host files replaced by their size and hash.
    pub options: BTreeMap<String, Value>,
    pub steps: Vec<StepOp>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Partition {
    Rootfs,
    Resources,
    DataPersist,
}

impl Partition {
    pub fn of(path: &str) -> Self {
        if path.starts_with("/tmp/mnt/resources/") {
            Self::Resources
        } else if path.starts_with("/tmp/mnt/data_persist/") {
            Self::DataPersist
        } else {
            Self::Rootfs
        }
    }

    /// Mount point to `df` on the CMU.
    pub fn mount(self) -> &'static str {
        match self {
            Self::Rootfs => "/",
            Self::Resources => "/tmp/mnt/resources",
            Self::DataPersist => "/tmp/mnt/data_persist",
        }
    }
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct SpaceNeed {
    pub partition: Partition,
    /// Upper bound of bytes added by appended blocks and installed files.
    pub payload_bytes: u64,
    /// Files whose backups land here; the CMU sizes them exactly before writing.
    pub backups: Vec<String>,
    pub reserve_kib: u32,
}

#[derive(Debug, Clone)]
pub struct Plan {
    pub config: BundleConfig,
    pub firmware: Version,
    pub profile: Profile,
    pub tweaks: Vec<PlannedTweak>,
    pub preflight: Vec<StepOp>,
    pub backups: Vec<String>,
    pub payloads: BTreeMap<String, Vec<u8>>,
    pub space: Vec<SpaceNeed>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct PlanError {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "bundle config rejected:")?;
        for e in &self.errors {
            writeln!(f, "  - {e}")?;
        }
        Ok(())
    }
}

impl std::error::Error for PlanError {}

/// Bundle-relative path for the payload a step installs, if any.
pub fn payload_path(tweak: &str, step: &StepOp) -> Option<String> {
    match step {
        StepOp::AppendBlock { asset, .. } => {
            let name = asset.rsplit('/').next().unwrap_or(asset);
            Some(format!("payload/{tweak}/{name}"))
        }
        StepOp::InstallFile { option, .. } => Some(format!("payload/{tweak}/{option}.png")),
        _ => None,
    }
}

/// Marker lines delimiting an appended block. Must match assets/cmu/runtime.sh.
pub fn block_markers(block: &str) -> (String, String) {
    (
        format!("/* mzd-tweaks:{block} BEGIN */"),
        format!("/* mzd-tweaks:{block} END */"),
    )
}

pub fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Width and height from a PNG's IHDR chunk.
pub fn png_size(data: &[u8]) -> Option<(u32, u32)> {
    const SIG: &[u8] = b"\x89PNG\r\n\x1a\n";
    if data.len() < 24 || &data[..8] != SIG || &data[12..16] != b"IHDR" {
        return None;
    }
    let be = |o: usize| u32::from_be_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]);
    Some((be(16), be(20)))
}

/// A tweak to plan: prerequisites carry no options.
type Selected<'a> = (&'a Tweak, Action, Option<&'a BTreeMap<String, Value>>);

/// Validate `config` and produce an ordered plan, or every problem found.
pub fn plan(config: &BundleConfig, assets: &Assets) -> Result<Plan, PlanError> {
    let cat = catalog();
    let rules = &cat.safety;
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let fail = |errors: Vec<String>, warnings: Vec<String>| Err(PlanError { errors, warnings });

    // S01: exact firmware, known profile.
    let firmware = match Version::parse(&config.firmware) {
        Ok(v) => v,
        Err(e) => return fail(vec![format!("S01 {e}")], warnings),
    };
    let profile = profile_for(&firmware);
    match profile.status {
        ProfileStatus::Target => {}
        ProfileStatus::Untested if config.allow_untested_firmware => warnings.push(format!(
            "firmware {firmware} is untested; allowed by allow_untested_firmware"
        )),
        ProfileStatus::Untested => {
            return fail(
                vec![format!(
                    "S01 firmware {firmware} is newer than anything tested; set allow_untested_firmware to proceed"
                )],
                warnings,
            );
        }
        ProfileStatus::Legacy => {
            return fail(
                vec![format!(
                    "S01 firmware {firmware} ({}) is not supported by this rewrite yet",
                    profile.label
                )],
                warnings,
            );
        }
        ProfileStatus::Blocked => {
            return fail(
                vec![format!(
                    "S01 firmware {firmware} is refused: {}",
                    profile.notes.join(" ")
                )],
                warnings,
            );
        }
    }
    if firmware.letter.is_empty() {
        warnings.push(format!(
            "no patch letter given (e.g. \"{}B\"); the CMU will only check {}",
            firmware.base(),
            firmware.base()
        ));
    }

    // Resolve entries.
    let mut selected: Vec<(&Tweak, Action, &BTreeMap<String, Value>)> = Vec::new();
    for entry in &config.tweaks {
        let Some(tweak) = cat.tweak(&entry.id) else {
            errors.push(format!("unknown tweak {:?}", entry.id));
            continue;
        };
        if tweak.status == TweakStatus::Unported {
            errors.push(format!("{} is catalogued but not ported yet", tweak.id));
            continue;
        }
        if tweak.prerequisite {
            errors.push(format!(
                "{} is a prerequisite; it is added automatically",
                tweak.id
            ));
            continue;
        }
        if selected.iter().any(|(t, ..)| t.id == tweak.id) {
            errors.push(format!("{} is listed more than once", tweak.id));
            continue;
        }
        if entry.action == Action::Install
            && tweak.acknowledge.is_some()
            && entry.acknowledge != tweak.acknowledge
        {
            errors.push(format!(
                "S08 {} reduces driving safety ({}). Add acknowledge = {:?} to install it",
                tweak.id,
                tweak.acknowledge_why.as_deref().unwrap_or(""),
                tweak.acknowledge.as_deref().unwrap_or("")
            ));
            continue;
        }
        selected.push((tweak, entry.action, &entry.options));
    }

    // Prerequisites first, in order of first use.
    let mut ordered: Vec<Selected<'_>> = Vec::new();
    for (t, action, _) in &selected {
        if *action == Action::Install {
            for r in &t.requires {
                if !ordered.iter().any(|(o, ..)| &o.id == r)
                    && let Some(p) = cat.tweak(r)
                {
                    ordered.push((p, Action::Install, None));
                }
            }
        }
    }
    ordered.extend(selected.iter().map(|(t, a, o)| (*t, *a, Some(*o))));

    let empty = BTreeMap::new();
    let mut tweaks = Vec::new();
    let mut payloads = BTreeMap::new();
    for (tweak, action, opts) in ordered {
        let opts = opts.unwrap_or(&empty);
        let Some(compat) = tweak.compat.iter().find(|c| c.profile == profile.id) else {
            errors.push(format!(
                "{} has no compatibility entry for {}",
                tweak.id, profile.label
            ));
            continue;
        };
        if compat.evidence != Evidence::Bench && action == Action::Install {
            warnings.push(format!(
                "{}: not bench-verified on {} yet; run mode \"check\" first",
                tweak.id, profile.label
            ));
        }
        if action == Action::Uninstall && tweak.permanent {
            errors.push(format!("{} is permanent and has no uninstall", tweak.id));
            continue;
        }

        let (resolved, redacted, problems) =
            resolve_options(tweak, action, opts, assets, rules.payload_max_bytes);
        if !problems.is_empty() {
            errors.extend(problems);
            continue;
        }
        let source = if action == Action::Install {
            &tweak.install
        } else {
            &tweak.uninstall
        };
        let steps: Vec<StepOp> = source
            .iter()
            .filter(|s| s.when.as_ref().is_none_or(|n| resolved.flag(n)))
            .filter(|s| s.unless.as_ref().is_none_or(|n| !resolved.flag(n)))
            .map(|s| s.op.clone())
            .collect();

        let problems = check_steps(rules, tweak, action, &steps);
        if !problems.is_empty() {
            errors.extend(problems);
            continue;
        }
        for step in &steps {
            let Some(path) = payload_path(&tweak.id, step) else {
                continue;
            };
            let data = match step {
                StepOp::AppendBlock {
                    block, asset: a, ..
                } => {
                    let (begin, end) = block_markers(block);
                    let body = asset(a).unwrap_or_default().trim();
                    format!("{begin}\n{body}\n{end}\n").into_bytes()
                }
                StepOp::InstallFile { option, .. } => {
                    resolved.files.get(option).cloned().unwrap_or_default()
                }
                _ => continue,
            };
            payloads.insert(path, data);
        }
        tweaks.push(PlannedTweak {
            id: tweak.id.clone(),
            title: tweak.title.clone(),
            action,
            prerequisite: tweak.prerequisite,
            evidence: compat.evidence,
            options: redacted,
            steps,
        });
    }
    if !errors.is_empty() {
        return fail(errors, warnings);
    }

    let all: Vec<&StepOp> = tweaks.iter().flat_map(|t| &t.steps).collect();
    let mut backups: Vec<String> = Vec::new();
    for s in &all {
        if let StepOp::Backup { path } = s
            && !backups.contains(path)
        {
            backups.push(path.clone());
        }
    }
    let mut preflight: Vec<StepOp> = backups
        .iter()
        .map(|p| StepOp::RequireFile { path: p.clone() })
        .collect();
    for s in all.iter().filter(|s| s.is_preflight()) {
        if !preflight.contains(s) {
            preflight.push((*s).clone());
        }
    }

    let space = space_needs(
        &tweaks,
        &backups,
        &payloads,
        &profile,
        rules.default_reserve_kib,
    );
    if let Some(root) = space.iter().find(|s| s.partition == Partition::Rootfs)
        && profile.rootfs_tight
        && root.payload_bytes > rules.rootfs_warn_bytes
    {
        warnings.push(format!(
            "this bundle writes up to {} bytes to a nearly full rootfs ({})",
            root.payload_bytes, profile.label
        ));
    }

    Ok(Plan {
        config: config.clone(),
        firmware,
        profile,
        tweaks,
        preflight,
        backups,
        payloads,
        space,
        warnings,
    })
}

#[derive(Default)]
struct Resolved {
    flags: BTreeMap<String, bool>,
    files: BTreeMap<String, Vec<u8>>,
}

impl Resolved {
    fn flag(&self, name: &str) -> bool {
        self.flags.get(name).copied().unwrap_or(false)
    }
}

fn resolve_options(
    tweak: &Tweak,
    action: Action,
    given: &BTreeMap<String, Value>,
    assets: &Assets,
    max_bytes: u64,
) -> (Resolved, BTreeMap<String, Value>, Vec<String>) {
    let mut out = Resolved::default();
    let mut redacted = BTreeMap::new();
    let mut problems = Vec::new();
    for key in given.keys() {
        if !tweak.options.iter().any(|o| &o.name == key) {
            problems.push(format!("{}: unknown option {key:?}", tweak.id));
        }
    }
    for opt in &tweak.options {
        let value = given.get(&opt.name);
        match (&opt.kind, value) {
            (OptionKind::Bool { default }, None) => {
                out.flags.insert(opt.name.clone(), *default);
            }
            (OptionKind::Bool { .. }, Some(Value::Bool(b))) => {
                out.flags.insert(opt.name.clone(), *b);
                redacted.insert(opt.name.clone(), Value::Bool(*b));
            }
            (OptionKind::Png { width, height }, Some(Value::String(name))) => {
                let Some(data) = assets.get(name) else {
                    problems.push(format!(
                        "{}: file {name:?} for option {} was not loaded",
                        tweak.id, opt.name
                    ));
                    continue;
                };
                if data.len() as u64 > max_bytes {
                    problems.push(format!(
                        "S10 {}: {name} is {} bytes; limit is {max_bytes}",
                        tweak.id,
                        data.len()
                    ));
                    continue;
                }
                match png_size(data) {
                    Some((w, h)) if (w, h) == (*width, *height) => {}
                    Some((w, h)) => {
                        problems.push(format!(
                            "S10 {}: {name} is {w}x{h}; the CMU screen is {width}x{height}",
                            tweak.id
                        ));
                        continue;
                    }
                    None => {
                        problems.push(format!("S10 {}: {name} is not a PNG", tweak.id));
                        continue;
                    }
                }
                redacted.insert(
                    opt.name.clone(),
                    serde_json::json!({ "bytes": data.len(), "sha256": sha256_hex(data) }),
                );
                out.files.insert(opt.name.clone(), data.clone());
            }
            (OptionKind::Png { .. }, None) if action == Action::Uninstall => {}
            (OptionKind::Png { .. }, None) => {
                problems.push(format!("{}: option {} is required", tweak.id, opt.name));
            }
            (_, Some(v)) => problems.push(format!(
                "{}: option {} has the wrong type ({v})",
                tweak.id, opt.name
            )),
        }
    }
    (out, redacted, problems)
}

fn space_needs(
    tweaks: &[PlannedTweak],
    backups: &[String],
    payloads: &BTreeMap<String, Vec<u8>>,
    profile: &Profile,
    default_reserve: u32,
) -> Vec<SpaceNeed> {
    let mut needs: BTreeMap<Partition, SpaceNeed> = BTreeMap::new();
    fn need(
        needs: &mut BTreeMap<Partition, SpaceNeed>,
        p: Partition,
        reserve: u32,
    ) -> &mut SpaceNeed {
        needs.entry(p).or_insert_with(|| SpaceNeed {
            partition: p,
            payload_bytes: 0,
            backups: vec![],
            reserve_kib: reserve,
        })
    }
    let reserve = |p: Partition| {
        if p == Partition::Rootfs {
            profile.rootfs_reserve_kib
        } else {
            default_reserve
        }
    };
    for t in tweaks {
        for s in &t.steps {
            if let Some(path) = payload_path(&t.id, s) {
                let p = Partition::of(s.path());
                need(&mut needs, p, reserve(p)).payload_bytes +=
                    payloads.get(&path).map_or(0, |d| d.len() as u64);
            }
        }
    }
    for b in backups {
        let p = match &profile.backup {
            BackupPolicy::Dir { dir } => Partition::of(&format!("{dir}/")),
            BackupPolicy::Sibling => Partition::of(b),
        };
        need(&mut needs, p, reserve(p)).backups.push(b.clone());
    }
    needs.into_values().collect()
}
