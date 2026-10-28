# libpulse binding source patch

`libpulse-binding/` is the published libpulse-binding 2.30.1 source from
https://crates.io/crates/libpulse-binding/2.30.1. The original source archive has
SHA-256 `909eb3049e16e373680fe65afe6e2a722ace06b671250cc4849557bc57d6a397`. Registry extraction markers and the dependency's standalone
Cargo lock are omitted; the desktop-ring Cargo.lock owns dependency versions.
Upstream: https://github.com/jnqnfe/pulse-binding-rust. Original MIT and Apache
license texts are retained beside the source.

The sole code change is in `src/operation.rs`: retain a non-null callback pointer
so cancellation frees its allocation, and skip the saved pointer when completion
has already freed it. Upstream 2.30.1 reverses the pointer condition and leaks
cancelled callbacks. `tests/audio.rs` in desktop-ring verifies immediate
cancellation frees a captured value exactly once, including repeated cancel/drop
and cancellation after normal completion.
The test fails against the unmodified release.

Cargo's local patch applies the same fix to Nix and portable builds. Revisit the
patch when upgrading the binding; remove the vendor directory only after the
cancellation regression passes against the replacement dependency.
