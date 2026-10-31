#[path = "../build_support/windows_resources.rs"]
mod windows_resources;

use std::{ffi::OsStr, path::Path, process::Command};

#[test]
fn detected_sdk_works_without_rc_on_path_and_keeps_space_paths_intact() {
    let compiler = Path::new("C:/Program Files (x86)/Windows Kits/10/bin/10.0.26100.0/x64/rc.exe");
    let include = Path::new("C:/Program Files (x86)/Windows Kits/10/Include/10.0.26100.0/um");
    let output = Path::new("C:/Build output/neontrace.res");
    let command = windows_resources::resource_command(
        None,
        None,
        Some(compiler.to_owned()),
        &[include.to_owned()],
        output,
    );
    assert_eq!(command.get_program(), compiler.as_os_str());
    assert_eq!(
        command.get_args().collect::<Vec<_>>(),
        [
            OsStr::new("/nologo"),
            OsStr::new("/I"),
            OsStr::new("assets"),
            OsStr::new("/I"),
            include.as_os_str(),
            OsStr::new("/fo"),
            output.as_os_str(),
            OsStr::new("assets/neontrace.rc"),
        ]
    );
}

#[test]
fn explicit_rc_wins_and_retains_sdk_environment() {
    let mut discovered = Command::new("discovered-rc.exe");
    discovered.env("INCLUDE", "sdk-um;sdk-shared;ucrt");
    discovered.env("PATH", "sdk-bin;vs-bin");
    let command = windows_resources::resource_command(
        Some("C:/My SDK/rc.exe".into()),
        Some(discovered),
        Some("fallback-rc.exe".into()),
        &[],
        Path::new("neontrace.res"),
    );
    assert_eq!(command.get_program(), "C:/My SDK/rc.exe");
    let variables = command.get_envs().collect::<Vec<_>>();
    assert!(variables.contains(&(
        OsStr::new("INCLUDE"),
        Some(OsStr::new("sdk-um;sdk-shared;ucrt"))
    )));
    assert!(variables.contains(&(OsStr::new("PATH"), Some(OsStr::new("sdk-bin;vs-bin")))));
}

#[test]
fn developer_environment_is_preferred_over_sdk_fallback() {
    let mut discovered = Command::new("configured-rc.exe");
    discovered.env("INCLUDE", "configured-sdk");
    let command = windows_resources::resource_command(
        None,
        Some(discovered),
        Some("fallback-rc.exe".into()),
        &[],
        Path::new("neontrace.res"),
    );
    assert_eq!(command.get_program(), "configured-rc.exe");
    assert!(
        command.get_envs().any(|(key, value)| {
            key == "INCLUDE" && value == Some(OsStr::new("configured-sdk"))
        })
    );
}

#[test]
fn custom_compiler_on_path_is_still_supported() {
    let command =
        windows_resources::resource_command(None, None, None, &[], Path::new("neontrace.res"));
    assert_eq!(command.get_program(), "rc.exe");
}
