# mzd-tweaks

Declarative tweak bundles for the Mazda Connect CMU, with a CLI, an MCP server for AI
agents, and a Nix flake. This is a rewrite of MZD-AIO-TI (see [NOTICE.md](NOTICE.md));
the old Electron GUI has been removed.

> **Status: early, not yet tested on a real car.** Everything below is verified in
> tests against a synthetic CMU tree only. Use check mode first, and expect to need
> the bench before trusting any of it.

## Targets

| Unit | Model | Firmware | Profile |
|---|---|---|---|
| CMU 1 | KB7W | 59.00.540 (sticker 59.00.544) | `v59-5xx` |
| CMU 2 | TA0A | 70.00.352B | `v70-00x` |

Other versions are modelled in `data/firmware.toml` as untested, legacy or blocked.

## Use

```sh
nix build                       # or: cargo build --release
mzd-tweaks list
mzd-tweaks compat 70.00.352B
mzd-tweaks plan examples/cmu2-check.toml
mzd-tweaks build examples/cmu2-check.toml --out /path/to/stick --write
```

Put the stick in the car with the engine running. Check mode runs every preflight
check, writes `mzd-tweaks.log` to the stick and changes nothing. Read the log, then
change `mode = "apply"`.

AI agents: run `mzd-tweaks-mcp` over stdio and load
[skills/mzd-tweaks/SKILL.md](skills/mzd-tweaks/SKILL.md).

NixOS (miia): import `nixosModules.default` and set `services.miia.cmu.enable = true;`.
Bundles can be built as derivations with `lib.mkCmuBundle`.
See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Safety

* Check mode by default; dry-run by default for every write and serial command.
* Every file is backed up before it is written; protected system files are never
  restored or replaced; free space is checked first (v70 rootfs is nearly full, so
  backups go to the resources partition).
* Firmware must match a target profile, and the CMU re-checks it.
* Tweaks that reduce driving safety need an explicit acknowledgement in the config.
* No SSH keys are shipped.

Rules S01–S10 are in `data/safety.toml`.

## Verify on hardware

Not yet confirmed on a real CMU:

* the JS wrap hooks behave the same on 59.00.540 and 70.00.352B
* the CMU's BusyBox has every applet `runtime.sh` uses (tested with BusyBox 1.37)
* `df` on the CMU reports usable space accurately
* `list-loop` with lists that load lazily
* serial: login prompts, credentials, framing (mock-tested only)
* which Orin Nano UART and pinmux miia uses for the CMU console

## Development

```sh
nix develop
cargo test && cargo clippy --workspace --all-targets --all-features -- -D warnings
nix flake check                 # cargo tests, clippy, fmt, example bundle, NixOS VM test
```

License: GPL-3.0-only.
