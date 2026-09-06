fn main() {
    // Embed our own manifest rather than Tauri's default, because the default
    // does not declare Per-Monitor V2 DPI awareness and this app is unusable
    // without it. `src-tauri/src/providers/win/dpi.rs` asserts at runtime that
    // this actually took effect.
    let windows = tauri_build::WindowsAttributes::new().app_manifest(include_str!("app.manifest"));

    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
        .expect("failed to run tauri-build");
}
