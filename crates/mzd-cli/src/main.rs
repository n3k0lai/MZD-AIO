//! `mzd-tweaks`: plan and build CMU tweak bundles from a TOML config.
//! Everything is a dry run unless `--write` / `--execute` is given.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use mzd_core::catalog::{TweakStatus, catalog};
use mzd_core::config::{BundleConfig, load_assets};
use mzd_core::firmware::check_compat;
use mzd_core::plan::plan;
use mzd_core::render::render;
use mzd_transport::serial::{PROBES, dry_run_transcript};
use mzd_transport::usb::UsbBundle;
use mzd_transport::{Transport, deliver};

#[derive(Parser)]
#[command(
    name = "mzd-tweaks",
    version,
    about = "Declarative tweak bundles for the Mazda Connect CMU"
)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List tweaks (ported only, unless --all).
    List {
        #[arg(long)]
        all: bool,
    },
    /// Show one tweak's catalog entry as JSON.
    Describe { id: String },
    /// Which tweaks can be built for a firmware version.
    Compat { firmware: String },
    /// Validate a config and print the manifest it would produce.
    Plan { config: PathBuf },
    /// Build a bundle. Dry run unless --write.
    Build {
        config: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        write: bool,
        /// Overwrite a previous bundle in --out.
        #[arg(long)]
        replace: bool,
    },
    /// Run read-only probes over the CMU serial console. Dry run unless --execute.
    Probe {
        /// Explicit serial device, e.g. /dev/ttyTHS1.
        #[arg(long)]
        device: String,
        #[arg(long = "probe", default_values = ["version"])]
        probes: Vec<String>,
        #[arg(long)]
        execute: bool,
    },
}

fn load(path: &Path) -> Result<(BundleConfig, mzd_core::config::Assets)> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let config =
        BundleConfig::from_toml(&text).with_context(|| format!("parsing {}", path.display()))?;
    let base = path.parent().unwrap_or(Path::new("."));
    let assets = load_assets(&config, base)?;
    Ok((config, assets))
}

fn print_json<T: serde::Serialize>(v: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(v)?);
    Ok(())
}

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Cmd::List { all } => {
            for t in catalog()
                .tweaks
                .iter()
                .filter(|t| all || t.status == TweakStatus::Ported)
            {
                let risk = t
                    .risk
                    .map_or("-".to_string(), |r| format!("{r:?}").to_lowercase());
                println!(
                    "{:<28} {:<9} {:<7} {}",
                    t.id,
                    format!("{:?}", t.status).to_lowercase(),
                    risk,
                    t.title
                );
            }
        }
        Cmd::Describe { id } => match catalog().tweak(&id) {
            Some(t) => print_json(t)?,
            None => bail!("unknown tweak {id:?}"),
        },
        Cmd::Compat { firmware } => print_json(&check_compat(&firmware)?)?,
        Cmd::Plan { config } => {
            let (config, assets) = load(&config)?;
            let p = plan(&config, &assets)?;
            for w in &p.warnings {
                eprintln!("warning: {w}");
            }
            print_json(&render(&p).manifest)?;
        }
        Cmd::Build {
            config,
            out,
            write,
            replace,
        } => {
            let (config, assets) = load(&config)?;
            let p = plan(&config, &assets)?;
            for w in &p.warnings {
                eprintln!("warning: {w}");
            }
            let bundle = render(&p);
            let mut usb = UsbBundle::new(&out);
            usb.dry_run = !write;
            usb.replace = replace;
            usb.connect()?;
            let files = deliver(&mut usb, "", &bundle.files)?;
            usb.close()?;
            let verb = if write {
                "wrote"
            } else {
                "would write (dry run, pass --write)"
            };
            println!("{verb} {} files to {}:", files.len(), out.display());
            for f in files {
                println!("  {f}");
            }
        }
        Cmd::Probe {
            device,
            probes,
            execute,
        } => {
            let refs: Vec<&str> = probes.iter().map(String::as_str).collect();
            if !execute {
                println!("dry run (pass --execute to open {device}):");
                for line in dry_run_transcript(&device, &refs) {
                    println!("  {line}");
                }
                return Ok(());
            }
            probe(&device, &refs)?;
        }
    }
    Ok(())
}

#[cfg(feature = "serial")]
fn probe(device: &str, probes: &[&str]) -> Result<()> {
    use mzd_transport::serial::probe_command;
    let commands: Vec<&str> = probes
        .iter()
        .map(|p| {
            probe_command(p).with_context(|| format!("unknown probe {p:?}; known: {PROBES:?}"))
        })
        .collect::<Result<_>>()?;
    let mut s = mzd_transport::serial::open(device)?;
    s.connect()?;
    s.login()?;
    for (name, cmd) in probes.iter().zip(commands) {
        let out = s.exec(cmd)?;
        println!("== {name} (exit {})\n{}", out.code, out.stdout);
    }
    s.close()?;
    Ok(())
}

#[cfg(not(feature = "serial"))]
fn probe(_device: &str, _probes: &[&str]) -> Result<()> {
    let _ = PROBES;
    bail!("this build has no serial support; rebuild with --features serial")
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}
