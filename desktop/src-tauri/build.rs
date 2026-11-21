fn main() {
    // Build-time config (see src/config.rs). Rebuild when these change.
    println!("cargo:rerun-if-env-changed=CLINILOG_API_URL");
    println!("cargo:rerun-if-env-changed=CLINILOG_SCHOOL_ID");
    // Declaring any rerun-if rule turns off Cargo's default "rerun on any file change",
    // so list the files the build script reads: the icons (embedded in the .exe),
    // the app config and the window permissions.
    println!("cargo:rerun-if-changed=icons");
    println!("cargo:rerun-if-changed=tauri.conf.json");
    println!("cargo:rerun-if-changed=capabilities");
    tauri_build::build()
}
