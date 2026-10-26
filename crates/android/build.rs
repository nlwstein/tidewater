//! Keeps SDL's JNI entry points in `libmain.so`.
//!
//! SDL's Java classes call C functions found by name (`Java_org_libsdl_app_SDLActivity_nativeRunMain` and about sixty
//! more), and the system calls `JNI_OnLoad` when the library loads. SDL is linked into this library as a static
//! archive, and Rust's own linker settings (a version script that hides every name but its own, and
//! `--gc-sections`) would hide and drop them, so the activity would fail with an `UnsatisfiedLinkError`. This script
//! keeps them by pattern and adds a second version script that exports them. It also links the C++ runtime SDL needs.

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("android") {
        return;
    }
    let script = PathBuf::from(std::env::var("OUT_DIR").expect("cargo sets OUT_DIR")).join("sdl-jni.map");
    std::fs::write(&script, "{ global: Java_org_libsdl_app_*; JNI_OnLoad; };\n").expect("write the version script");
    println!("cargo:rustc-link-arg-cdylib=-Wl,--undefined-glob=Java_org_libsdl_app_*");
    println!("cargo:rustc-link-arg-cdylib=-Wl,--undefined=JNI_OnLoad");
    println!("cargo:rustc-link-arg-cdylib=-Wl,--version-script={}", script.display());
    // SDL's HIDAPI controller driver is C++ on Android (`src/hidapi/android/hid.cpp`), and nothing else brings in a
    // C++ runtime, so the library would fail to load (`cannot locate symbol "__gxx_personality_v0"`). The NDK's static
    // libc++ goes into the library rather than shipping `libc++_shared.so` beside it.
    println!("cargo:rustc-link-arg-cdylib=-lc++_static");
    println!("cargo:rustc-link-arg-cdylib=-lc++abi");
}
