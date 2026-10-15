//! Declarative tweak bundles for the Mazda Connect CMU.
//!
//! Pipeline: [`config::BundleConfig`] + host [`config::Assets`] → [`plan::plan`] (pure,
//! validated against the data in `data/`) → [`render::render`] (pure) → a [`render::Bundle`]
//! of files plus a deterministic `manifest.json`. Writing the bundle anywhere is the job of
//! a transport (see the `mzd-transport` crate).

pub mod catalog;
pub mod config;
pub mod firmware;
pub mod manifest;
pub mod plan;
pub mod render;
pub mod safety;

/// Generator identity, stamped into every bundle.
pub const GENERATOR_NAME: &str = "mzd-tweaks";
pub const GENERATOR_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Upstream commit the catalog was studied from (MZD-AIO-TI 2.8.6).
pub const UPSTREAM_REF: &str = "409c724";
