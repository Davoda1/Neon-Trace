use std::{env, path::PathBuf, process::Command};

#[path = "build_support/windows_resources.rs"]
mod windows_resources;

fn main() {
    println!("cargo:rerun-if-changed=assets/neontrace.ico");
    println!("cargo:rerun-if-changed=assets/neontrace.rc");
    println!("cargo:rerun-if-changed=assets/neontrace.manifest");
    println!("cargo:rerun-if-changed=build_support/windows_resources.rs");
    for variable in [
        "RC",
        "WINDRES",
        "PATH",
        "INCLUDE",
        "WindowsSdkDir",
        "WindowsSDKVersion",
        "VCToolsVersion",
        "VCINSTALLDIR",
        "VSINSTALLDIR",
        "VSCMD_ARG_TGT_ARCH",
    ] {
        println!("cargo:rerun-if-env-changed={variable}");
    }
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
        let target = env::var("TARGET").expect("TARGET");
        let arch = env::var("CARGO_CFG_TARGET_ARCH").expect("CARGO_CFG_TARGET_ARCH");
        // A normal PowerShell session does not have the Windows SDK on PATH.
        // Locate it through the same registry/Visual Studio discovery used by cc,
        // and retain its INCLUDE/PATH setup as well as the executable path.
        let discovered = find_msvc_tools::find(&target, "rc.exe");
        let sdk = find_msvc_tools::find_windows_sdk(&arch);
        let sdk_compiler = sdk.as_ref().and_then(|sdk| {
            sdk.path()
                .map(|path| path.join("rc.exe"))
                .find(|path| path.is_file())
        });
        let includes: Vec<_> = sdk
            .as_ref()
            .into_iter()
            .flat_map(|sdk| sdk.include().map(PathBuf::from))
            .collect();
        let mut command = windows_resources::resource_command(
            env::var_os("RC"),
            discovered,
            sdk_compiler,
            &includes,
            &resource,
        );
        let compiler = command.get_program().to_string_lossy().into_owned();
        let status = command.status().unwrap_or_else(|error| {
            panic!(
                "Could not run Windows resource compiler '{compiler}': {error}. \
                 Install Visual Studio Build Tools with 'Desktop development with C++' \
                 and a Windows 10 or 11 SDK, then run cargo build --release again. \
                 An installed SDK is detected automatically; alternatively run from \
                 Developer PowerShell for Visual Studio or set RC to the full path \
                 of rc.exe. If RC is set, check that it names an executable without \
                 surrounding quotes or command-line arguments."
            )
        });
        assert!(
            status.success(),
            "Windows resource compiler '{compiler}' failed with {status}. \
             See its diagnostic above; ensure the Windows SDK headers are installed. \
             If using a custom SDK, run from its Developer PowerShell or set \
             WindowsSdkDir and WindowsSDKVersion."
        );
        println!("cargo:rustc-link-arg={}", resource.display());
    }
}
