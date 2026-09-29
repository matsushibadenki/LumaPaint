# LumaPaint Rust compatibility patch

Source: crates.io `block` 0.1.6, https://github.com/SSheldon/rust-block.
Upstream author: Steven Sheldon. License: MIT (preserved in Cargo.toml).
The published archive and upstream repository do not include a separate license file.

The Metal backend (`wgpu-hal` / `metal`) still requires this API. The original
`extern static _NSConcreteStackBlock: Class` uses an empty enum, triggering
Rust's `uninhabited_static` future-incompatibility warning.

Declare the runtime symbol as its 32-pointer array and take its raw address,
cast to the existing opaque class pointer. Do not construct a reference to an
uninhabited value or load the first pointer of the array. Public APIs, block
layout, copying and release behavior remain unchanged.
Spell the formerly implicit C ABI as `extern "C"` to avoid the current
`missing_abi` deprecation warnings now that the crate is a local dependency.

ABI reference: https://github.com/llvm/llvm-project/blob/main/compiler-rt/lib/BlocksRuntime/Block_private.h
The same array representation is used by `block2` 0.6.2 (`src/ffi.rs`).

The original Objective-C fixture dependency is absent from the crates.io
archive, so upstream unit tests are disabled in this vendored manifest.
The replacement macOS integration test covers invocation, heap copy, retain,
captured state and release: `cargo test -p block --test runtime --locked`.

Remove this patch when the Metal dependency no longer uses the incompatible
upstream release. Do not suppress the Rust warning globally.
