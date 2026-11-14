fn main() {
    // ggml-metal (whisper.cpp) usa `@available(...)`, que necesita
    // `___isPlatformVersionAtLeast` de compiler-rt. Rust enlaza con
    // `-nodefaultlibs`, así que añadimos libclang_rt.osx a mano en macOS.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        if let Ok(out) = std::process::Command::new("clang")
            .arg("--print-resource-dir")
            .output()
        {
            let dir = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let lib_dir = std::path::Path::new(&dir).join("lib").join("darwin");
            if lib_dir.join("libclang_rt.osx.a").exists() {
                println!("cargo:rustc-link-search=native={}", lib_dir.display());
                println!("cargo:rustc-link-lib=static=clang_rt.osx");
            }
        }
    }
    tauri_build::build()
}
