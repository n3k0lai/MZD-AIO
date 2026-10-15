# The mzd-tweaks CLI and MCP server. A pure function of this repository and the
# pinned nixpkgs: every crate is locked by hash in Cargo.lock, nothing is fetched
# during the build, and the test suite (including the generated CMU script run
# under BusyBox) runs in the build sandbox.
{
  lib,
  rustPlatform,
  busybox,
  # Real UART support for the bench (serialport crate, no libudev).
  withSerial ? false,
}:

rustPlatform.buildRustPackage {
  pname = "mzd-tweaks";
  version = "0.1.0";

  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../crates
      ../data
      ../assets
      ../tests
    ];
  };

  cargoLock.lockFile = ../Cargo.lock;

  buildFeatures = lib.optionals withSerial [
    "mzd-cli/serial"
    "mzd-mcp/serial"
  ];

  # Run the generated run.sh with BusyBox applets only, as on the CMU.
  env.MZD_TEST_BUSYBOX_BIN = "${busybox}/bin";

  meta = {
    description = "Declarative tweak bundles for the Mazda Connect CMU (CLI and MCP server)";
    homepage = "https://github.com/n3k0lai/MZD-AIO";
    license = lib.licenses.gpl3Only;
    mainProgram = "mzd-tweaks";
    platforms = lib.platforms.linux;
  };
}
