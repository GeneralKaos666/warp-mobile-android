// build.rs — compile GLSL shaders to SPIR-V at build time using the NDK's
// `glslc` (shaderc) which ships at:
//   $ANDROID_NDK_ROOT/shader-tools/<host-tag>/glslc
//
// We deliberately do NOT use the `shaderc-rs` crate because shaderc-rs has
// open Android cross-compile issues (google/shaderc-rs#87). The build script
// runs on the **host**, not the target — so as long as glslc is present on
// the host, we can produce SPIR-V binaries that the target Android binary
// just `include_bytes!`s. This means the .spv blobs go into OUT_DIR, never
// into the source tree, and never need the target toolchain.
//
// Web-search refs (2026-04-30):
//   <https://github.com/google/shaderc-rs/issues/87>
//   <https://developer.android.com/ndk/guides/cmake>
//   <https://github.com/PENGUINLIONG/inline-spirv-rs>  (alt approach for inline GLSL)
//
// SPIR-V version: 1.0 (default for NDK r28/r29 glslc; matches our
// VK_API_VERSION_1_1 instance which only requires SPIR-V 1.0).

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    // Only compile shaders for android targets. Host-side cargo test does not
    // need them (vulkan.rs is gated behind cfg(target_os = "android")).
    let target = env::var("TARGET").unwrap_or_default();
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "android" {
        // Still emit re-run hints so a future android target build picks up
        // shader edits.
        println!("cargo:rerun-if-changed=shaders/grid.vert");
        println!("cargo:rerun-if-changed=shaders/grid.frag");
        println!("cargo:rerun-if-changed=shaders/dynamic_grid.vert");
        println!("cargo:rerun-if-changed=shaders/dynamic_grid.frag");
        return;
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let shader_dir = manifest_dir.join("shaders");
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    let glslc = locate_glslc().unwrap_or_else(|e| {
        panic!("M2-S08 build: cannot locate glslc — {}.\n\
            Set NDK_GLSLC=<absolute path> or ensure ANDROID_NDK_ROOT/ANDROID_NDK_HOME/ANDROID_HOME points\n\
            to a valid NDK with shader-tools/<host>/glslc. Tried target={}.",
            e, target);
    });

    println!("cargo:warning=M2-S08 glslc={}", glslc.display());

    for (src_name, out_name) in &[
        ("grid.vert", "grid.vert.spv"),
        ("grid.frag", "grid.frag.spv"),
        // M3-S08: per-cell dynamic grid shaders (runtime mirror).
        ("dynamic_grid.vert", "dynamic_grid.vert.spv"),
        ("dynamic_grid.frag", "dynamic_grid.frag.spv"),
    ] {
        let src = shader_dir.join(src_name);
        let dst = out_dir.join(out_name);
        compile_shader(&glslc, &src, &dst);
        println!("cargo:rerun-if-changed={}", src.display());
    }

    // Re-run build if user changed env vars influencing glslc location.
    println!("cargo:rerun-if-env-changed=NDK_GLSLC");
    println!("cargo:rerun-if-env-changed=ANDROID_NDK_ROOT");
    println!("cargo:rerun-if-env-changed=ANDROID_NDK_HOME");
    println!("cargo:rerun-if-env-changed=ANDROID_HOME");
}

fn compile_shader(glslc: &Path, src: &Path, dst: &Path) {
    let status = Command::new(glslc)
        .arg("-O")               // optimize SPIR-V
        .arg("--target-env=vulkan1.1")
        .arg(src)
        .arg("-o")
        .arg(dst)
        .status()
        .unwrap_or_else(|e| panic!("M2-S08 glslc spawn failed for {}: {}", src.display(), e));
    if !status.success() {
        panic!(
            "M2-S08 glslc compile failed for {} (status={:?}). Run manually with: {} -O --target-env=vulkan1.1 {} -o {}",
            src.display(), status.code(),
            glslc.display(), src.display(), dst.display()
        );
    }
}

fn locate_glslc() -> Result<PathBuf, String> {
    // 1. Explicit override.
    if let Ok(p) = env::var("NDK_GLSLC") {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            return Ok(pb);
        }
    }
    // 2. NDK_HOME / NDK_ROOT.
    for env_var in ["ANDROID_NDK_ROOT", "ANDROID_NDK_HOME"] {
        if let Ok(ndk) = env::var(env_var) {
            if let Some(p) = find_glslc_in_ndk(&ndk) {
                return Ok(p);
            }
        }
    }
    // 3. ANDROID_HOME/ndk/<latest>.
    if let Ok(sdk) = env::var("ANDROID_HOME") {
        let ndk_dir = Path::new(&sdk).join("ndk");
        if let Ok(entries) = std::fs::read_dir(&ndk_dir) {
            // Pick the highest-versioned NDK.
            let mut versions: Vec<PathBuf> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect();
            // Sort descending so newest version comes first.
            versions.sort_by(|a, b| b.cmp(a));
            for ndk in versions {
                if let Some(p) = find_glslc_in_ndk(ndk.to_str().unwrap_or("")) {
                    return Ok(p);
                }
            }
        }
    }
    Err("no NDK_GLSLC / ANDROID_NDK_ROOT / ANDROID_NDK_HOME / ANDROID_HOME ndk/<ver>/shader-tools".into())
}

fn find_glslc_in_ndk(ndk: &str) -> Option<PathBuf> {
    let host_tags = ["darwin-x86_64", "darwin-arm64", "linux-x86_64", "windows-x86_64"];
    for tag in host_tags {
        let candidate = Path::new(ndk)
            .join("shader-tools")
            .join(tag)
            .join("glslc");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}
