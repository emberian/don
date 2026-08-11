//! Export the four shipped `CrossplayProxy.dll` symbols under their exact
//! names, at their exact ordinals, with the right two marked as data.
//!
//! `#[export_name = "?...@Z"]` does not survive into a cdylib's export set on
//! `i686-pc-windows-msvc` — the symbol lands in the object file but rustc's
//! generated `.def` and the link then disagree. That was measured across all
//! ten of `netsys-shim`'s decorated names (see `../../netsys-shim/build.rs`);
//! explicit `/EXPORT:exported=internal` aliases are deterministic and do not
//! depend on rustc's export-set inference.
//!
//! Two details this DLL needs that `netsys-shim` did not:
//!
//! * **the alias target is written without the leading underscore.** lld-link
//!   prepends the i386 `_` itself when resolving `/EXPORT:a=b`, so spelling
//!   `_proxy_service` here produces a lookup for `__proxy_service` and fails.
//!   Measured, ten symbols at once, in the netsys-shim lane.
//! * **ordinals 3 and 4 are data, not code.** `NvOptimusEnablement` and
//!   `AmdPowerXpressRequestHighPerformance` are the standard hybrid-GPU hints:
//!   a `DWORD` each, which the vendor driver finds by walking the process's
//!   export tables. The shipped DLL puts both in `.data` at RVA `0xb2044` and
//!   `0xb2048` holding `1`. Exporting them as functions would publish the same
//!   address with the wrong kind, so `,DATA` is explicit here.
//!
//! The `@N` ordinals are explicit too. lld-link assigns unspecified ordinals
//! after the highest specified one, so the internal `proxy_*` aliases that Rust
//! also exports cannot displace the shipped four.

/// `(exported name, internal symbol, ordinal, is data)`, copied verbatim from
/// the shipped DLL's export directory. **[measured — `ron-bin/dll/CrossplayProxy.dll`]**
const EXPORTS: &[(&str, &str, u16, bool)] = &[
    (
        r"?Logger@Logging@Crossplay@@YAPAVICrossplayLogger@12@XZ",
        "proxy_logger",
        1,
        false,
    ),
    (
        r"?Service@Crossplay@@YAPAUICrossPlayService@1@XZ",
        "proxy_service",
        2,
        false,
    ),
    (
        "AmdPowerXpressRequestHighPerformance",
        "proxy_amd_power_xpress_request_high_performance",
        3,
        true,
    ),
    (
        "NvOptimusEnablement",
        "proxy_nv_optimus_enablement",
        4,
        true,
    ),
];

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    for (exported, internal, ordinal, is_data) in EXPORTS {
        let kind = if *is_data { ",DATA" } else { "" };
        println!("cargo::rustc-link-arg-cdylib=/EXPORT:{exported}={internal},@{ordinal}{kind}");
    }
}
