# YMM4 native adapter

This crate is the native half of the YMM4 bridge. It exposes a small C ABI and
keeps the broker `RenderSession` on a dedicated Rust thread. The AEX itself is
still loaded by AEXCompat's isolated worker process.

Build from the repository root:

```powershell
cargo fmt --manifest-path bridges\ymm4-native\Cargo.toml -- --check
cargo test --manifest-path bridges\ymm4-native\Cargo.toml
cargo build --release --manifest-path bridges\ymm4-native\Cargo.toml
```

The resulting `aexcompat_ymm4_native.dll` is copied beside the managed YMM4
plugin. `AEXCOMPAT_YMM4_REPOSITORY` must point at this repository so the bridge
can find the built worker under `target\minihost-build`.
