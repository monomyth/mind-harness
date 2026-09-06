//! Embed an rpath so the finished binary can load BrainFlow's `@rpath/libBoardController.dylib`.
//! The brainflow crate copies those dylibs into its OUT_DIR at build time but does not set rpath.

use std::path::{Path, PathBuf};

/// BrainFlow ships `libBoardController.dylib` on macOS and `libBoardController.so` on Linux.
/// A lib directory is valid if it contains either flavor.
fn has_board_controller(dir: &Path) -> bool {
    dir.join("libBoardController.dylib").exists() || dir.join("libBoardController.so").exists()
}

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
        .find(|p| has_board_controller(p))
        .expect(
            "BrainFlow libraries not found. Set BRAINFLOW_LIB to the directory that contains \
             libBoardController.dylib (macOS) or libBoardController.so (Linux) \
             (usually brainflow/rust_package/brainflow/lib).",
        );

    let lib = lib.canonicalize().unwrap_or(lib);
    println!("cargo:rerun-if-changed={}", lib.display());
    println!("cargo:rustc-link-search=native={}", lib.display());
    println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib.display());

    // Homebrew labstreaminglayer/tap/lsl installs lsl.framework (not -llsl).
    let fw_candidates = [
        PathBuf::from("/opt/homebrew/Frameworks"),
        PathBuf::from("/usr/local/Frameworks"),
    ];
    if let Some(fw) = fw_candidates
        .into_iter()
        .find(|p| p.join("lsl.framework").exists())
    {
        println!("cargo:rerun-if-changed={}", fw.join("lsl.framework").display());
        println!("cargo:rustc-link-search=framework={}", fw.display());
        println!("cargo:rustc-link-lib=framework=lsl");
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", fw.display());
        println!("cargo:rustc-cfg=has_liblsl");
    }
}
