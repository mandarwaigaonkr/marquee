use std::{env, fs, path::PathBuf};

fn main() {
    // libmpv is vendored (see vendor/libmpv): link against its import library and
    // put the DLL next to the built executable so `tauri dev` can load it.
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let libmpv_dir = manifest_dir.join("..").join("vendor").join("libmpv");
    println!("cargo:rustc-link-search=native={}", libmpv_dir.display());
    println!("cargo:rerun-if-changed={}", libmpv_dir.join("libmpv-2.dll").display());

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    if let Some(profile_dir) = out_dir.ancestors().nth(3) {
        let dll = libmpv_dir.join("libmpv-2.dll");
        let dest = profile_dir.join("libmpv-2.dll");
        if dll.exists() && !dest.exists() {
            fs::copy(&dll, &dest).expect("copy libmpv-2.dll next to the executable");
        }
    }

    tauri_build::build()
}
