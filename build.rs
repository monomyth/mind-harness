//! Embed an rpath so the finished binary can load BrainFlow's `@rpath/libBoardController.dylib`.
//! The brainflow crate copies those dylibs into its OUT_DIR at build time but does not set rpath.

use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let from_env = std::env::var("BRAINFLOW_LIB").ok().map(PathBuf::from);
    let candidates = [
        from_env,
        Some(PathBuf::from(
            "/Users/monomyth/github/brainflow/rust_package/brainflow/lib",
        )),
        Some(manifest.join("../brainflow/rust_package/brainflow/lib")),
    ];

    let lib = candidates
        .into_iter()
        .flatten()
        .find(|p| p.join("libBoardController.dylib").exists())
        .expect(
            "BrainFlow dylibs not found. Set BRAINFLOW_LIB to the directory that contains \
             libBoardController.dylib (usually brainflow/rust_package/brainflow/lib).",
        );

    let lib = lib.canonicalize().unwrap_or(lib);
    println!("cargo:rerun-if-changed={}", lib.display());
    println!("cargo:rustc-link-search=native={}", lib.display());
    println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib.display());
}
