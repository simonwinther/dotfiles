# Portable desktop configuration

The `nixos-portable` branch preserves the shared desktop tools, plain configuration, and appearance assets used by the NixOS workstation. It is pinned as the `dotfiles/` submodule in https://github.com/simonwinther/nixos-config. The existing `master` branch remains available for the Arch configuration.

On NixOS, Home Manager owns the configuration links. On other distributions, use GNU Stow for the packages you select. Do not let both manage the same target. Runtime credentials, private keys, build output and caches stay outside this repository.

When cloning the NixOS repository, use `git clone --recurse-submodules`. After updating it, use `git submodule update --init --recursive` to populate the exact committed dotfiles revision.

The package READMEs describe individual tools.
