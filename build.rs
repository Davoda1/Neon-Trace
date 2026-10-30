use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=assets/neontrace.ico");
    println!("cargo:rerun-if-changed=assets/neontrace.rc");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    if env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("gnu") {
        let resource = out.join("neontrace-res.o");
        let compiler = env::var("WINDRES").unwrap_or_else(|_| {
            if env::var("HOST").unwrap_or_default().contains("windows") {
                "windres".to_owned()
            } else {
                "x86_64-w64-mingw32-windres".to_owned()
            }
        });
        let status = Command::new(compiler)
            .args(["-I", "assets", "assets/neontrace.rc", "-O", "coff", "-o"])
            .arg(&resource)
            .status()
            .expect("Windows build requires GNU windres; set WINDRES when cross-compiling");
        assert!(status.success(), "Windows resource compilation failed");
        println!("cargo:rustc-link-arg={}", resource.display());
    } else {
        let resource = out.join("neontrace.res");
        let compiler = env::var("RC").unwrap_or_else(|_| "rc.exe".into());
        let status = Command::new(compiler)
            .args(["/nologo", "/Iassets", "/fo"])
            .arg(&resource)
            .arg("assets/neontrace.rc")
            .status()
            .expect("Windows SDK resource compiler rc.exe is required");
        assert!(status.success(), "Windows resource compilation failed");
        println!("cargo:rustc-link-arg={}", resource.display());
    }
}
