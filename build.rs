//! Discover BrainFlow native libraries and embed an rpath/link-search so the
//! finished binary loads them without LD_LIBRARY_PATH / DYLD_LIBRARY_PATH.
//!
//! Target-aware (`CARGO_CFG_TARGET_OS`): Linux `.so` and macOS `.dylib`.
//! Layout: honor `BRAINFLOW_LIB` first; else sibling
//! `../brainflow/rust_package/brainflow/lib`.

use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=BRAINFLOW_LIB");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let required = match target_os.as_str() {
        "linux" => [
            "libBoardController.so",
            "libDataHandler.so",
            "libMLModule.so",
        ],
        "macos" => [
            "libBoardController.dylib",
            "libDataHandler.dylib",
            "libMLModule.dylib",
        ],
        other => {
            panic!(
                "unsupported target OS `{other}` for BrainFlow native linking; \
                 supported: linux, macos"
            );
        }
    };

    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let mac_absolute = PathBuf::from("/Users/monomyth/github/brainflow/rust_package/brainflow/lib");
    let sibling = manifest.join("../brainflow/rust_package/brainflow/lib");

    let lib = match std::env::var_os("BRAINFLOW_LIB") {
        Some(raw) => {
            let dir = PathBuf::from(raw);
            require_libs(&dir, &required, "BRAINFLOW_LIB");
            dir
        }
        None if mac_absolute.exists() => {
            require_libs(&mac_absolute, &required, "Mac absolute path");
            mac_absolute
        }
        None => {
            require_libs(&sibling, &required, "sibling ../brainflow/rust_package/brainflow/lib");
            sibling
        }
    };

    let lib = lib.canonicalize().unwrap_or(lib);
    for name in &required {
        println!("cargo:rerun-if-changed={}", lib.join(name).display());
    }
    println!("cargo:rerun-if-changed={}", lib.display());
    println!("cargo:rustc-link-search=native={}", lib.display());
    println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib.display());

    // Homebrew labstreaminglayer/tap/lsl installs lsl.framework (not -llsl).
    // Gate to macOS target only — Linux LSL is not part of this portability work.
    if target_os == "macos" {
        let fw_candidates = [
            PathBuf::from("/opt/homebrew/Frameworks"),
            PathBuf::from("/usr/local/Frameworks"),
        ];
        if let Some(fw) = fw_candidates
            .into_iter()
            .find(|p| p.join("lsl.framework").exists())
        {
            println!(
                "cargo:rerun-if-changed={}",
                fw.join("lsl.framework").display()
            );
            println!("cargo:rustc-link-search=framework={}", fw.display());
            println!("cargo:rustc-link-lib=framework=lsl");
            println!("cargo:rustc-link-arg=-Wl,-rpath,{}", fw.display());
            println!("cargo:rustc-cfg=has_liblsl");
        }
    }
}

fn require_libs(dir: &Path, required: &[&str; 3], source: &str) {
    let missing: Vec<&str> = required
        .iter()
        .copied()
        .filter(|name| !dir.join(name).exists())
        .collect();
    if missing.is_empty() {
        return;
    }
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_else(|_| "?".into());
    panic!(
        "BrainFlow native libraries not found for target OS `{target_os}`.\n\
         Looked in ({source}): {}\n\
         Missing: {}\n\
         Set BRAINFLOW_LIB to the directory that contains these files, or place \
         a sibling brainflow checkout at ../brainflow (see README).",
        dir.display(),
        missing.join(", ")
    );
}
