# NixOS VM test: boots a machine with both miia CMU options enabled, checks what the
# module set up, then builds an apply bundle with the installed CLI and runs its
# run.sh under BusyBox against the fake CMU tree.
{ pkgs }:

let
  fixture = ../../tests/fixtures/fake-cmu;
  applyConfig = pkgs.writeText "apply.toml" ''
    firmware = "70.00.352B"
    mode = "apply"
    [[tweak]]
    id = "no-disclaimer"
    [[tweak]]
    id = "list-loop"
  '';
  bb = "${pkgs.busybox}/bin";
in
pkgs.testers.runNixOSTest {
  name = "mzd-tweaks-cmu";

  nodes.machine = {
    imports = [ ../module.nix ];
    hardware.miia.cmuUart = {
      enable = true;
      device = "/dev/ttyS1";
    };
    services.miia.cmu.enable = true;
  };

  testScript = ''
    machine.wait_for_unit("multi-user.target")

    with subtest("module wiring"):
        machine.succeed("mzd-tweaks list | grep -q list-loop")
        machine.succeed("id miia-cmu | grep -q cmu-uart")
        machine.succeed("grep -q 'KERNEL==\"ttyS1\", GROUP=\"cmu-uart\"' /etc/udev/rules.d/*.rules")
        machine.succeed("systemctl show -p LoadState serial-getty@ttyS1.service | grep -q masked")
        machine.succeed("ls -l /dev/ttyS1 | grep -q cmu-uart")

    with subtest("bundle runs on a fake CMU under BusyBox"):
        machine.succeed("cp -r ${fixture} /tmp/root && chmod -R u+w /tmp/root")
        machine.succeed("mzd-tweaks build ${applyConfig} --out /tmp/stick --write")
        machine.succeed(
            "cd /tmp && MZD_ROOT=/tmp/root MZD_SIMULATE=1 PATH=${bb} ${bb}/sh /tmp/stick/run.sh"
        )
        machine.succeed("grep -q 'mzd-tweaks:list-loop END' /tmp/root/jci/gui/common/controls/List2/js/List2Ctrl.js")
        machine.succeed("grep -q 'noWatchdogs' /tmp/root/jci/sm/sm.conf")
        machine.succeed("test -f /tmp/stick/run.sh.done && test -f /tmp/stick/mzd-tweaks.log")
  '';
}
