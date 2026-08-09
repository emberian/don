//! Emit the link arguments a Python extension module needs on macOS.
//!
//! A CPython extension resolves the interpreter symbols at load time, not at link time.
//! `maturin` arranges that for you; this crate is built with plain `cargo` so the
//! `cdylib` needs the flags explicitly. Only the `cdylib` is affected, so `cargo test`
//! (which links the `rlib`) is untouched.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_EXTENSION_MODULE");
    if std::env::var("CARGO_FEATURE_EXTENSION_MODULE").is_err() {
        return;
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-cdylib-link-arg=-undefined");
        println!("cargo:rustc-cdylib-link-arg=dynamic_lookup");
    }
}
