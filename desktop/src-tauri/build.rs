fn main() {
    // Build-time config (see src/config.rs). Rebuild when these change.
    println!("cargo:rerun-if-env-changed=CLINILOG_API_URL");
    println!("cargo:rerun-if-env-changed=CLINILOG_SCHOOL_ID");
    tauri_build::build()
}
