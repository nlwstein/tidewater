//! Embeds the application icon in `tidewater.exe` (Windows targets only), so Explorer, the taskbar and the Start menu
//! show it for the file itself. On every other target (macOS, Linux, iOS, Android, PS Vita) this does nothing at all.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../packaging/windows/tidewater.ico");
    println!("cargo:rerun-if-env-changed=TIDEWATER_VERSION");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("../../packaging/windows/tidewater.ico");
    res.set("ProductName", "Tidewater");
    res.set("FileDescription", "Tidewater");
    // The tag is the only version (`Cargo.toml` stays at its placeholder), so the file's properties say the tag's.
    if let Some(v) = std::env::var("TIDEWATER_VERSION").ok().filter(|v| !v.is_empty()) {
        res.set("FileVersion", &v);
        res.set("ProductVersion", &v);
    }
    if let Err(e) = res.compile() {
        panic!("could not embed the Windows icon: {e}");
    }
}
