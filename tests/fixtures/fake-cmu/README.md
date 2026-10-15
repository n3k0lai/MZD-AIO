# Fake CMU tree

Synthetic stand-ins for the CMU files the ported tweaks touch. They contain only the
anchor lines the tweaks look for, not Mazda/JCI code. Used by the Rust integration tests
and the Nix checks to run generated `run.sh` files against a fake root (`MZD_ROOT`).
