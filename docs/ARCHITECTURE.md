# Architecture

mzd-tweaks turns a small declarative config into a USB bundle for the Mazda Connect
CMU: a POSIX `run.sh`, payload files and a `manifest.json`. It is used directly (CLI),
by an AI agent (MCP server and skill), and on miia, a Jetson Orin Nano running NixOS
on jetpack-nixos, which may later also reach
the CMU over its serial console.

Status labels used below: **implemented** (code and tests in this repo, verified as
noted), **design** (described here, not built yet), **bench** (needs real hardware data).

## Principles

### 1. Builds are a pure function of their inputs

* **Implemented.** `flake.lock` pins nixpkgs by revision and content hash; `Cargo.lock`
  pins every crate by checksum. The package builds in the Nix sandbox with no network.
* **Implemented.** A bundle is a pure function of (config, catalog, assets, generator
  version). The generator does no I/O beyond reading the config's own files; there are
  no timestamps or random values in the output, and file order is fixed (`BTreeMap`).
  `manifest.json` records the SHA-256 of every file it ships.
* **Implemented.** `lib.mkCmuBundle` builds a bundle as a Nix derivation, so a stick's
  contents can be reproduced and audited later. `nix build .#example-bundle --rebuild`
  produces bit-identical output.

### 2. Each capability is one module with an `enable` option

The NixOS module (`nix/module.nix`) hides the details behind two options:

| Option | Hides | Status |
|---|---|---|
| `services.miia.cmu.enable` | the package (with serial support when the UART is on), a `miia-cmu` system user | **implemented** (stub; VM-tested) |
| `hardware.miia.cmuUart.enable` + `device` | udev group `cmu-uart`, no getty on that tty, the device-tree overlay for the UART pins | udev/getty **implemented**; DT overlay **bench** (TODO in the module) |

`device` has no default on purpose: which Orin Nano UART is wired to the CMU depends on
the harness. No pinmux or overlay values are invented here.

The MCP server is spawned over stdio by the agent host, so the module does not run it
as a systemd service. If a long-running service is ever needed, it goes behind
`services.miia.cmu.enable` too.

### 3. Patched packages are overlays

* **Implemented.** `overlays.default` adds `mzd-tweaks` to nixpkgs. Any future patched
  dependency (for example a kernel or BusyBox patch needed for the UART) goes in an
  overlay as well, never as an ad-hoc fetch inside a module.

### 4. Test in a VM first, then on hardware

* **Implemented.** `checks.<system>.package` runs the cargo tests in the sandbox. The
  bundle integration tests execute the generated `run.sh` against a fake `/jci` tree
  (`tests/fixtures/fake-cmu`) with `sh`, `dash` when available, and BusyBox 1.37 applets
  only (`MZD_TEST_BUSYBOX_BIN`; set by the Nix build).
* **Implemented.** `checks.x86_64-linux.vm` (`nix/tests/vm.nix`) boots NixOS with both
  options enabled, checks the module wiring (user, group, udev rule, masked getty), then
  builds an apply bundle with the installed CLI and runs it under BusyBox against the
  fake CMU tree.
* **Design / bench.** Hardware-in-the-loop on the bench, see below.

### 5. Start small with good examples

* `examples/cmu2-check.toml` and `examples/cmu1-uninstall.toml`, five ported tweaks, one
  module with two options. Everything else in the catalog is listed as `unported`, with
  its upstream path, until someone ports and tests it.

### 6. No clever abstractions

* Tweaks are data, not code: a closed set of step ops (below). Adding a tweak means
  adding TOML and maybe a JS asset; the generator does not grow.
* One `Transport` trait with two implementations and one `PowerControl` trait with a
  mock. No plugin system, no runtime discovery.

## Layout

```
data/            firmware.toml, tweaks.toml, safety.toml, schema/*.schema.json
assets/cmu/      runtime.sh (sourced by the generated run.sh)
assets/patches/  JS wrap-hook patches (appended as marked blocks)
crates/
  mzd-core       config, compat, plan -> bundle, manifest, safety rules (pure)
  mzd-transport  Transport trait, usb-bundle, serial (feature `serial`), PowerControl mock
  mzd-cli        `mzd-tweaks`
  mzd-mcp        `mzd-tweaks-mcp` (rmcp, stdio)
nix/             package.nix, module.nix, bundle.nix, tests/vm.nix
tests/fixtures/  fake CMU tree (synthetic, not copied from a CMU)
```

## Data

* `firmware.toml`: profiles keyed by version range. **Target**: `v70-00x`
  (70.00.000–360, CMU 2 TA0A on 70.00.352B) and `v59-5xx` (CMU 1 KB7W, 59.00.540).
  Others are `untested`, `legacy` or blocked; building for them is refused unless
  `allow_untested_firmware` is set (untested only).
* `tweaks.toml`: the catalog. Each tweak has a status (`ported`/`unported`), risk,
  per-profile compatibility with evidence (`upstream`, `bench`), options and steps.
* `safety.toml`: protected paths, limits and rules S01–S10, enforced in `mzd-core`
  before anything is generated and re-checked on the CMU where possible.
* JSON Schemas are generated from the Rust types; a test fails if they drift.

### Step ops

The closed set of operations a tweak may use. Each becomes one call into
`assets/cmu/runtime.sh`. Preflight ops run for every tweak before the first write.

| op | phase | effect |
|---|---|---|
| `require_file` | preflight | file exists |
| `require_literal` | preflight | literal present (`count` = exact count); `applied` means already done |
| `require_backup` | preflight | a backup exists (uninstall) |
| `backup` | write | copy once to the profile's backup location; never overwritten |
| `append_block` | write | append an asset between `/* mzd-tweaks:<id> BEGIN/END */` markers |
| `remove_block` | write | remove that marked block |
| `replace_literal` | write | idempotent single-line replacement |
| `install_file` | write | install a payload file from an option (e.g. PNG) |
| `restore` | write | restore from backup |
| `exec` | write | run a stock script under `/jci/scripts/` with plain arguments |

Steps may carry `when`/`unless` on a boolean option.

JS patches wrap prototype functions in a marked block rather than replacing whole
JCI files, because whole-file copies are what made upstream tweaks firmware-specific.

## Bundle and runtime

`run.sh` is generated per bundle: it sources `runtime.sh`, checks the firmware from
`/jci/version.ini`, runs all preflight checks and the space check, then (in `apply` or
`uninstall` mode) the writes, logs to `mzd-tweaks.log` on the stick, and renames itself
to `run.sh.done` so it cannot run twice. `mode = "check"` is the default and changes
nothing. `MZD_ROOT` and `MZD_SIMULATE` let the same script run on a host for tests.

On v70 the rootfs is nearly full, so backups go to the resources partition
(`/tmp/mnt/resources/dev/mzd-tweaks/backup`); on v59 they sit beside the original.

## Transports

* **usb-bundle (implemented):** writes the bundle to a mounted stick. Dry-run by
  default; refuses to overwrite unless asked.
* **serial (implemented, mock-tested, bench):** behind the `serial` cargo feature.
  115200 8N1, explicit device path, dry-run by default. Only read-only probes are
  exposed (version, df, uptime). Login prompts and credentials are unverified.
* **PowerControl (mock only):** power is handled outside this tool (a Jetson service or
  the XIAO supervisor). The trait exists so tests and the HIL rig can be written against it.

## Bench hardware-in-the-loop test (design)

A NixOS test-like script run on miia against a bench CMU:

1. Power-cycle the CMU through the external power service (real `PowerControl`).
2. Wait for the serial console; run the read-only probes; record firmware, `df`, uptime.
3. Build a `check` bundle with `mkCmuBundle`, write it to a USB gadget or stick, boot,
   read `mzd-tweaks.log` and compare against the expected preflight result.
4. Build the `apply` bundle, run it, verify the marked blocks over serial, then the
   `uninstall` bundle, and verify files match the backups (hashes from the manifest).
5. Run for both 59.00.540 and 70.00.352B; promote a tweak's compat evidence to
   `bench` only after it passes.

Open questions: see README.
