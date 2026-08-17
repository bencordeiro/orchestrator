fn main() {
    // Tauri's `externalBin` stages the sidecar as `<name>-<target-triple>`, so the
    // runtime lookup needs the triple this binary was built for. `TARGET` only
    // exists in build scripts, so re-export it as a compile-time env var.
    // Using the real triple (rather than a cfg-matched literal) keeps
    // cross-compiled and non-x86_64 targets correct for free.
    println!(
        "cargo:rustc-env=ORCHESTRATOR_TARGET_TRIPLE={}",
        std::env::var("TARGET").unwrap_or_default()
    );

    tauri_build::build()
}
