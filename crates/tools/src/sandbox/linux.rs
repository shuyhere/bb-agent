use super::{PreparedSandboxCommand, SandboxBackend, SandboxSetupError, backend_unavailable_error};
use std::{
    env,
    path::{Path, PathBuf},
};
use tokio::process::Command;

const BWRAP_NAME: &str = "bwrap";

pub(crate) fn prepare_bash_command(
    cwd: &Path,
    command: &str,
) -> Result<PreparedSandboxCommand, SandboxSetupError> {
    let Some(bwrap_path) = find_in_path(BWRAP_NAME) else {
        return Err(SandboxSetupError::BackendUnavailable(
            backend_unavailable_error(
                SandboxBackend::Bwrap,
                format!(
                    "Safety mode requires the Linux bubblewrap backend, but `{BWRAP_NAME}` was not found in PATH."
                ),
            ),
        ));
    };

    let mut sandboxed = Command::new(bwrap_path);
    sandboxed
        .arg("--die-with-parent")
        .arg("--unshare-all")
        .arg("--new-session")
        .arg("--ro-bind")
        .arg("/")
        .arg("/")
        .arg("--dev")
        .arg("/dev")
        .arg("--proc")
        .arg("/proc")
        .arg("--tmpfs")
        .arg("/tmp")
        .arg("--setenv")
        .arg("HOME")
        .arg("/tmp")
        .arg("--setenv")
        .arg("TMPDIR")
        .arg("/tmp")
        .arg("--setenv")
        .arg("XDG_CACHE_HOME")
        .arg("/tmp/.cache")
        .arg("--setenv")
        .arg("XDG_CONFIG_HOME")
        .arg("/tmp/.config")
        .arg("--bind")
        .arg(cwd)
        .arg(cwd)
        .arg("--chdir")
        .arg(cwd)
        .arg("bash")
        .arg("-lc")
        .arg(command);

    Ok(PreparedSandboxCommand {
        command: sandboxed,
        backend: SandboxBackend::Bwrap,
    })
}

fn find_in_path(name: &str) -> Option<PathBuf> {
    let path_var = env::var_os("PATH")?;
    for dir in env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_bwrap_in_absolute_path_list() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join(BWRAP_NAME);
        std::fs::write(&file, "").unwrap();

        let original_path = env::var_os("PATH");
        unsafe {
            env::set_var("PATH", tmp.path());
        }

        let found = find_in_path(BWRAP_NAME);

        match original_path {
            Some(path) => unsafe { env::set_var("PATH", path) },
            None => unsafe { env::remove_var("PATH") },
        }

        assert_eq!(found, Some(file));
    }
}
