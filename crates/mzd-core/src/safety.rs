//! Static checks on a tweak's resolved steps (rules S02-S06 in data/safety.toml).

use crate::catalog::{Safety, StepOp, Tweak};
use crate::config::Action;

fn safe_path(p: &str) -> bool {
    p.starts_with('/')
        && p.len() > 1
        && p[1..].split('/').all(|seg| {
            !seg.is_empty()
                && seg != "."
                && seg != ".."
                && seg
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        })
}

fn single_line(s: &str) -> bool {
    !s.is_empty() && !s.contains(['\n', '\r', '\0'])
}

fn plain_word(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._/=-".contains(&b))
}

fn safe_block_id(s: &str) -> bool {
    s.bytes().next().is_some_and(|b| b.is_ascii_alphanumeric())
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Problems with `steps` for `tweak` doing `action`; empty when acceptable.
pub fn check_steps(rules: &Safety, tweak: &Tweak, action: Action, steps: &[StepOp]) -> Vec<String> {
    let mut out = Vec::new();
    let at = format!("{} ({action:?})", tweak.id).to_lowercase();
    let protected = |p: &str| rules.protected_paths.iter().any(|x| x == p);

    for step in steps {
        let path = step.path();
        if !safe_path(path) {
            out.push(format!("S05 {at}: unsafe CMU path {path:?}"));
        }
        if protected(path) {
            if matches!(
                step,
                StepOp::Restore { .. } | StepOp::RemoveBlock { .. } | StepOp::InstallFile { .. }
            ) {
                out.push(format!("S02 {at}: {} on protected {path}", step.name()));
            }
            if step.is_write() && (action == Action::Uninstall || !tweak.permanent) {
                out.push(format!(
                    "S02 {at}: only permanent prerequisites may edit protected {path}, never on uninstall"
                ));
            }
        }
        match step {
            StepOp::RequireLiteral {
                literal,
                applied,
                count,
                ..
            } => {
                if !single_line(literal) || applied.as_deref().is_some_and(|a| !single_line(a)) {
                    out.push(format!("S05 {at}: literals must be single non-empty lines"));
                }
                if *count == Some(0) {
                    out.push(format!("S05 {at}: count must be at least 1"));
                }
            }
            StepOp::ReplaceLiteral { find, replace, .. } => {
                if !single_line(find) || !single_line(replace) {
                    out.push(format!(
                        "S05 {at}: find/replace must be single non-empty lines"
                    ));
                } else if find.contains(replace.as_str()) || replace.contains(find.as_str()) {
                    out.push(format!("S06 {at}: find and replace contain each other"));
                }
            }
            StepOp::AppendBlock { block, .. } | StepOp::RemoveBlock { block, .. }
                if !safe_block_id(block) =>
            {
                out.push(format!("S05 {at}: bad block id {block:?}"));
            }
            StepOp::Exec { argv } => {
                if argv.is_empty() || !argv.iter().all(|a| plain_word(a)) {
                    out.push(format!("S05 {at}: exec arguments must be plain words"));
                }
                if !rules
                    .exec_prefixes
                    .iter()
                    .any(|p| path.starts_with(p.as_str()))
                {
                    out.push(format!(
                        "S05 {at}: exec may only run stock scripts ({path})"
                    ));
                }
            }
            _ => {}
        }
    }

    let paths = |pred: fn(&StepOp) -> bool| -> Vec<&str> {
        steps.iter().filter(|s| pred(s)).map(StepOp::path).collect()
    };
    let backed_up = paths(|s| matches!(s, StepOp::Backup { .. }));
    let checked = paths(StepOp::is_preflight);
    for step in steps.iter().filter(|s| s.is_write()) {
        let path = step.path();
        if action == Action::Install && !backed_up.contains(&path) {
            out.push(format!("S03 {at}: {path} is written without a backup step"));
        }
        if !checked.contains(&path) {
            out.push(format!(
                "S04 {at}: {path} is written without a preflight check"
            ));
        }
    }
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::catalog;

    fn tweak(permanent: bool) -> Tweak {
        let mut t = catalog().tweak("no-disclaimer").unwrap().clone();
        t.permanent = permanent;
        t
    }

    fn check(permanent: bool, action: Action, steps: &[StepOp]) -> Vec<String> {
        check_steps(&catalog().safety, &tweak(permanent), action, steps)
    }

    const SM: &str = "/jci/sm/sm.conf";

    #[test]
    fn protected_files_are_never_reverted() {
        let restore = [
            StepOp::RequireBackup { path: SM.into() },
            StepOp::Restore { path: SM.into() },
        ];
        let errs = check(true, Action::Uninstall, &restore);
        assert!(errs.iter().any(|e| e.starts_with("S02")), "{errs:?}");
    }

    #[test]
    fn only_permanent_tweaks_edit_protected_files() {
        let edit = [
            StepOp::RequireFile { path: SM.into() },
            StepOp::Backup { path: SM.into() },
            StepOp::ReplaceLiteral {
                path: SM.into(),
                find: "a=1".into(),
                replace: "a=0".into(),
            },
        ];
        assert!(
            check(false, Action::Install, &edit)
                .iter()
                .any(|e| e.starts_with("S02"))
        );
        assert_eq!(check(true, Action::Install, &edit), Vec::<String>::new());
    }

    #[test]
    fn writes_need_backup_and_preflight() {
        let p = "/jci/gui/x.js";
        let errs = check(
            false,
            Action::Install,
            &[StepOp::RemoveBlock {
                path: p.into(),
                block: "b".into(),
            }],
        );
        assert!(errs.iter().any(|e| e.starts_with("S03")));
        assert!(errs.iter().any(|e| e.starts_with("S04")));
    }

    #[test]
    fn rejects_unsafe_paths_and_exec() {
        let errs = check(
            false,
            Action::Install,
            &[
                StepOp::RequireFile {
                    path: "/jci/../etc/passwd".into(),
                },
                StepOp::Exec {
                    argv: vec!["/bin/rm".into(), "-rf".into()],
                },
                StepOp::Exec {
                    argv: vec!["/jci/scripts/x.sh".into(), "$(reboot)".into()],
                },
            ],
        );
        assert_eq!(
            errs.iter().filter(|e| e.starts_with("S05")).count(),
            3,
            "{errs:?}"
        );
    }

    #[test]
    fn replacements_must_be_idempotent() {
        let p = "/jci/a.js";
        let errs = check(
            false,
            Action::Install,
            &[
                StepOp::RequireFile { path: p.into() },
                StepOp::Backup { path: p.into() },
                StepOp::ReplaceLiteral {
                    path: p.into(),
                    find: "foo".into(),
                    replace: "foobar".into(),
                },
            ],
        );
        assert!(errs.iter().any(|e| e.starts_with("S06")));
    }
}
