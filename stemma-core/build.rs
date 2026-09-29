//! Names the build this crate is part of.
//!
//! A build made from a known commit says so through `STEMMA_BUILD` (the
//! automated builds set it to the commit), and the version every surface
//! reports carries it as build metadata: `0.0.0+1a2b3c4`. Any other build
//! reports the package version alone.

fn main() {
    println!("cargo:rerun-if-env-changed=STEMMA_BUILD");

    let package = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let version = match std::env::var("STEMMA_BUILD") {
        Ok(build) if !build.trim().is_empty() => format!("{package}+{}", build.trim()),
        _ => package,
    };

    println!("cargo:rustc-env=STEMMA_VERSION={version}");
}
