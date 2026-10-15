# Build a CMU bundle as a derivation: the USB stick contents are a pure function of
# the config, its assets and the generator version, so a bundle can be rebuilt
# bit-for-bit later and its manifest.json audited before it goes near a car.
#
#   mkCmuBundle { src = ./my-bundles; config = "cmu2-check.toml"; }
{ runCommand, mzd-tweaks }:

{
  src,
  config,
  name ? "cmu-bundle",
}:

runCommand name { nativeBuildInputs = [ mzd-tweaks ]; } ''
  mzd-tweaks build ${src}/${config} --out $out --write
''
