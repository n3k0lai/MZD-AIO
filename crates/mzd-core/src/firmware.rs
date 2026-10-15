//! Firmware versions and profile matching. The profiles themselves are data
//! (`data/firmware.toml`).

use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::catalog::{Profile, ProfileStatus, catalog};

/// A CMU firmware version such as `70.00.352B`, `59.00.540` or `59.00.544A-NA`.
/// The raw `JCI_SW_VER` form (`C0.0.0_70.00.352`) is accepted too.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub major: u16,
    pub mid: u16,
    pub patch: u16,
    /// Patch letter (`JCI_SW_VER_PATCH`), empty if not given.
    pub letter: String,
    /// Region flavor (`JCI_SW_FLAVOR`), empty if not given.
    pub region: String,
}

#[derive(Debug, thiserror::Error)]
#[error("unrecognised firmware version {0:?}; expected e.g. \"70.00.352B\" or \"59.00.540\"")]
pub struct ParseVersionError(pub String);

impl Version {
    pub fn parse(input: &str) -> Result<Self, ParseVersionError> {
        let err = || ParseVersionError(input.to_string());
        let s = input.trim();
        let s = s.rsplit_once('_').map_or(s, |(_, v)| v);
        let (s, region) = match s.split_once('-') {
            Some((v, r))
                if (2..=4).contains(&r.len()) && r.bytes().all(|b| b.is_ascii_uppercase()) =>
            {
                (v, r)
            }
            Some(_) => return Err(err()),
            None => (s, ""),
        };
        let mut parts = s.split('.');
        let (Some(major), Some(mid), Some(rest), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(err());
        };
        let digits = |p: &str, n: usize| {
            (p.len() == n && p.bytes().all(|b| b.is_ascii_digit()))
                .then(|| p.parse::<u16>().ok())
                .flatten()
        };
        let (patch, letter) = match rest.len() {
            3 => (rest, ""),
            4 if rest.as_bytes()[3].is_ascii_uppercase() => rest.split_at(3),
            _ => return Err(err()),
        };
        Ok(Self {
            major: digits(major, 2).ok_or_else(err)?,
            mid: digits(mid, 2).ok_or_else(err)?,
            patch: digits(patch, 3).ok_or_else(err)?,
            letter: letter.to_string(),
            region: region.to_string(),
        })
    }

    /// `70.00.352`: the part the CMU's `JCI_SW_VER` carries.
    pub fn base(&self) -> String {
        format!("{:02}.{:02}.{:03}", self.major, self.mid, self.patch)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.base(), self.letter)?;
        if !self.region.is_empty() {
            write!(f, "-{}", self.region)?;
        }
        Ok(())
    }
}

/// Profile used for versions that match nothing, and for development builds.
pub fn blocked_profile() -> Profile {
    Profile {
        id: "blocked".into(),
        label: "below 55, unknown, or a development build".into(),
        major_min: None,
        major: 0,
        patch_min: 0,
        patch_max: 0,
        status: ProfileStatus::Blocked,
        first_access: crate::catalog::FirstAccess::None,
        backup: crate::catalog::BackupPolicy::Sibling,
        rootfs_reserve_kib: 0,
        rootfs_tight: false,
        units: vec![],
        notes: vec![
            "Upstream documents a permanent logo boot loop on v30/31/33; development builds (middle field not 00) are refused.".into(),
        ],
    }
}

pub fn profile_for(v: &Version) -> Profile {
    if v.mid != 0 {
        return blocked_profile();
    }
    catalog()
        .profiles
        .iter()
        .find(|p| {
            (p.major_min.unwrap_or(p.major)..=p.major).contains(&v.major)
                && (p.patch_min..=p.patch_max).contains(&v.patch)
        })
        .cloned()
        .unwrap_or_else(blocked_profile)
}

/// Verdict for one tweak on one firmware.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TweakCompat {
    pub id: String,
    pub status: String,
    /// `upstream`, `bench`, or `none`.
    pub evidence: String,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CompatReport {
    pub firmware: String,
    pub profile: String,
    pub profile_label: String,
    pub profile_status: String,
    /// Whether bundles can be built for this firmware at all.
    pub buildable: bool,
    pub notes: Vec<String>,
    pub tweaks: Vec<TweakCompat>,
}

pub fn check_compat(firmware: &str) -> Result<CompatReport, ParseVersionError> {
    let v = Version::parse(firmware)?;
    let profile = profile_for(&v);
    let tweaks = catalog()
        .tweaks
        .iter()
        .filter(|t| !t.prerequisite)
        .map(|t| {
            let entry = t.compat.iter().find(|c| c.profile == profile.id);
            TweakCompat {
                id: t.id.clone(),
                status: format!("{:?}", t.status).to_lowercase(),
                evidence: entry.map_or("none".into(), |c| {
                    format!("{:?}", c.evidence).to_lowercase()
                }),
                note: entry.and_then(|c| c.note.clone()),
            }
        })
        .collect();
    Ok(CompatReport {
        firmware: v.to_string(),
        profile: profile.id.clone(),
        profile_label: profile.label.clone(),
        profile_status: format!("{:?}", profile.status).to_lowercase(),
        buildable: matches!(
            profile.status,
            ProfileStatus::Target | ProfileStatus::Untested
        ),
        notes: profile.notes.clone(),
        tweaks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_forms() {
        let v = Version::parse("70.00.352B").unwrap();
        assert_eq!(
            (v.major, v.mid, v.patch, v.letter.as_str()),
            (70, 0, 352, "B")
        );
        assert_eq!(
            Version::parse("C0.0.0_59.00.540").unwrap().base(),
            "59.00.540"
        );
        let r = Version::parse("59.00.544A-NA").unwrap();
        assert_eq!((r.letter.as_str(), r.region.as_str()), ("A", "NA"));
        assert_eq!(r.to_string(), "59.00.544A-NA");
    }

    #[test]
    fn rejects_garbage() {
        for bad in [
            "",
            "70",
            "70.0.352",
            "70.00.35",
            "70.00.352BB",
            "70.00.352-n",
            "v70.00.352",
        ] {
            assert!(Version::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn matches_profiles() {
        let id = |s: &str| profile_for(&Version::parse(s).unwrap()).id;
        assert_eq!(id("70.00.352B"), "v70-00x");
        assert_eq!(id("70.00.361"), "v70-361plus");
        assert_eq!(id("59.00.540"), "v59-5xx");
        assert_eq!(id("59.00.443"), "v59-4xx");
        assert_eq!(id("56.00.100"), "v55-58");
        assert_eq!(id("33.00.000"), "blocked");
        assert_eq!(id("70.01.352"), "blocked");
    }
}
