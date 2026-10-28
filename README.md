# Portable desktop configuration

The `master` branch contains the shared desktop tools, plain configuration, and appearance assets used across distributions. This repository is pinned as the `dotfiles/` submodule in https://github.com/simonwinther/nixos-config.

On NixOS, Home Manager owns the configuration links. On other distributions, use GNU Stow for the packages you select. Do not let both manage the same target. Runtime credentials, private keys, build output and caches stay outside this repository.

When cloning the NixOS repository, use `git clone --recurse-submodules`. After updating it, use `git submodule update --init --recursive` to populate the exact committed dotfiles revision.

The package READMEs describe individual tools.

Rust tools keep small `*-build [--check]` shell wrappers in their packages.
They share `bin/.local/share/dotfiles/rust-build.sh`, resolved through the
checkout, so they also work before Stow. For example, from this directory:

```sh
./desktop-tools/.local/bin/project-picker-build --check
```

Each tool holds an exclusive `flock` on its cache's `build.lock` from source
copying through publication. A fresh, complete source snapshot replaces
`build/` after copying succeeds, removing deleted modules, tests, and assets.
The committed `Cargo.lock` is required; Cargo builds, Clippy, and tests use
`--locked`. `--check` runs formatting, Clippy, and tests before the release
build. Builds run at reduced priority with two jobs by default; set
`CARGO_BUILD_JOBS` to override that limit.

Snapshots, Cargo output, and published executables live under
`${XDG_CACHE_HOME:-$HOME/.cache}/<tool>/{build,target,bin}`. Executables are
replaced atomically after a successful build. Build failures preserve the
published executables, and copying failures also preserve the previous source
snapshot. The workspace slider uses this workflow too; its launcher runs the
published copy in `workspaces-slider/bin/`.
