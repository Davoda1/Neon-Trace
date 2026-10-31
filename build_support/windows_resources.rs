use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::Command,
};

/// Assemble the RC invocation without shell parsing, preserving SDK setup even
/// when a caller explicitly chooses a different resource compiler executable.
pub fn resource_command(
    explicit: Option<OsString>,
    discovered: Option<Command>,
    sdk_compiler: Option<PathBuf>,
    sdk_includes: &[PathBuf],
    output: &Path,
) -> Command {
    let mut command = if let Some(compiler) = explicit {
        let mut command = Command::new(compiler);
        if let Some(discovered) = &discovered {
            for (key, value) in discovered.get_envs() {
                if let Some(value) = value {
                    command.env(key, value);
                }
            }
        }
        command
    } else if let Some(discovered) = discovered {
        discovered
    } else {
        Command::new(sdk_compiler.unwrap_or_else(|| PathBuf::from("rc.exe")))
    };
    command.args(["/nologo", "/I", "assets"]);
    // Explicit includes also support an SDK installed without a VS toolset
    // environment. Inherited INCLUDE remains available for custom SDK setups.
    for include in sdk_includes {
        command.arg("/I").arg(include);
    }
    command.arg("/fo").arg(output).arg("assets/neontrace.rc");
    command
}
