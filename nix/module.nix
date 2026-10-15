# NixOS module for miia (Jetson Orin Nano on jetpack-nixos). Two capabilities, each a
# single enable option that hides the details:
#
#   hardware.miia.cmuUart.enable  the Jetson UART wired to the CMU console
#   services.miia.cmu.enable      the mzd-tweaks CLI and MCP server
#
# Status: stub. Nothing here has been run on miia yet; values that need hardware
# data are left as TODOs rather than guessed.
{
  config,
  lib,
  pkgs,
  ...
}:

let
  uart = config.hardware.miia.cmuUart;
  cmu = config.services.miia.cmu;
  ttyName = lib.removePrefix "/dev/" uart.device;
in
{
  options.hardware.miia.cmuUart = {
    enable = lib.mkEnableOption "the Jetson UART wired to the Mazda CMU serial console (3.3 V, 115200 8N1)";

    device = lib.mkOption {
      type = lib.types.strMatching "/dev/tty[A-Za-z0-9]+";
      example = "/dev/ttyTHS1";
      description = ''
        UART device node wired to the CMU. No default on purpose: which Orin Nano
        UART is used depends on the harness wiring (TODO: record it from the bench).
      '';
    };
  };

  options.services.miia.cmu = {
    enable = lib.mkEnableOption "mzd-tweaks CMU tooling (CLI and MCP server)";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.callPackage ./package.nix { withSerial = uart.enable; };
      defaultText = lib.literalExpression "pkgs.callPackage ./package.nix { withSerial = config.hardware.miia.cmuUart.enable; }";
      description = "mzd-tweaks package; built with serial support when the CMU UART is enabled.";
    };
  };

  config = lib.mkMerge [
    (lib.mkIf uart.enable {
      # TODO(bench): device-tree overlay enabling the chosen UART's pins on the Orin
      # Nano carrier (pinmux and node name come from the carrier docs and probing):
      #   hardware.deviceTree.overlays = [ { name = "miia-cmu-uart"; dtsFile = ./dts/miia-cmu-uart.dts; } ];
      # TODO(bench): confirm the CMU-side TX/RX pins (community notes say 2S/2T on the
      # rear connector) and that the CMU's levels really are 3.3 V before wiring.

      users.groups.cmu-uart = { };
      services.udev.extraRules = ''
        KERNEL=="${ttyName}", GROUP="cmu-uart", MODE="0660"
      '';
      # The CMU console must not get a login prompt from our side.
      systemd.services."serial-getty@${ttyName}".enable = false;
    })

    (lib.mkIf cmu.enable {
      environment.systemPackages = [ cmu.package ];

      users.groups.miia-cmu = { };
      users.users.miia-cmu = {
        isSystemUser = true;
        group = "miia-cmu";
        extraGroups = lib.optional uart.enable "cmu-uart";
        description = "mzd-tweaks MCP server (spawned over stdio by Hermes)";
      };
    })
  ];
}
