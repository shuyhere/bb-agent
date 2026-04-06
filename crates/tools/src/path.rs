use std::path::{Path, PathBuf};

pub(crate) fn resolve_path(cwd: &Path, path_str: &str) -> PathBuf {
    let path_str = path_str.strip_prefix('@').unwrap_or(path_str);
    let path = Path::new(path_str);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}
