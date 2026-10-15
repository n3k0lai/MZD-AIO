//! JSON Schemas for the data files, the bundle config and manifest.json live in
//! data/schema/ for editors and other tools. This test keeps them in sync with the
//! Rust types; regenerate with `UPDATE_SCHEMAS=1 cargo test -p mzd-core --test schemas`.

use std::path::Path;

use mzd_core::catalog::{FirmwareFile, Safety, TweaksFile};
use mzd_core::config::BundleConfig;
use mzd_core::manifest::Manifest;
use schemars::{JsonSchema, schema_for};

fn check<T: JsonSchema>(name: &str) -> Option<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/schema").join(name);
    let want = serde_json::to_string_pretty(&schema_for!(T)).unwrap() + "\n";
    if std::env::var_os("UPDATE_SCHEMAS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &want).unwrap();
        return None;
    }
    let have = std::fs::read_to_string(&path).unwrap_or_default();
    (have != want).then(|| name.to_string())
}

#[test]
fn schemas_are_up_to_date() {
    let stale: Vec<String> = [
        check::<FirmwareFile>("firmware.schema.json"),
        check::<TweaksFile>("tweaks.schema.json"),
        check::<Safety>("safety.schema.json"),
        check::<BundleConfig>("bundle-config.schema.json"),
        check::<Manifest>("manifest.schema.json"),
    ]
    .into_iter()
    .flatten()
    .collect();
    assert!(stale.is_empty(), "stale schemas (run with UPDATE_SCHEMAS=1): {stale:?}");
}
