---
name: mzd-tweaks
description: Plan and build Mazda Connect (CMU) tweak bundles safely with the mzd-tweaks MCP server or CLI. Use when the user wants to customize, check or revert a Mazda Connect head unit (firmware 59.00.5xx or 70.00.0xx-3xx).
---

# mzd-tweaks

Tools (MCP server `mzd-tweaks-mcp`, or the `mzd-tweaks` CLI):
`list_tweaks`, `describe_tweak`, `check_compat`, `plan_bundle`, `build_bundle`,
`serial_probe`.

## Workflow

1. **Get the exact firmware.** Ask for the full version from Settings → System
   Information (e.g. `70.00.352B`, `59.00.540`). Never guess it, and note that the
   sticker on the unit can differ from what the CMU reports (trust the CMU).
2. `check_compat` with that version. If the profile is not a target, say so and stop
   unless the user knowingly accepts an untested profile.
3. `list_tweaks` / `describe_tweak` to choose. Only `ported` tweaks can be built.
   Explain each tweak's risk and notes in plain words.
4. `plan_bundle` with `mode = "check"` first. Show the user the manifest summary:
   files touched, backups and where they go, space needed, warnings.
5. `build_bundle` with `dry_run = true`, then with `dry_run = false` only after the
   user approves, into a directory or stick they name.
6. The user runs the check bundle in the car and shares `mzd-tweaks.log` from the
   stick. Only if preflight passed, repeat with `mode = "apply"`.
7. To revert, build with `action = "uninstall"` for those tweaks.

## Rules

* Never set `acknowledge` yourself. Tweaks that reduce driving safety (e.g.
  `touchscreen-at-speed`, token `driving-distraction`) need the user to ask for it
  explicitly after you have explained the risk.
* Never edit generated `run.sh` or payloads by hand, and never write shell for the CMU.
  If a tweak is missing, say it is unported.
* `sm-watchdog-off` is added automatically for GUI patches and is permanent by design.
* Keep the engine running or a charger connected while a bundle runs; do not remove
  the stick until the log says it finished.
* `serial_probe` needs an explicit device path from the user, is read-only, and is
  dry-run unless the user asks to execute. It is for the bench and is unverified on
  hardware.
* Nothing here has been verified on a real CMU yet; say so when asked.
