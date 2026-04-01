use std::path::{Path, PathBuf};

/// Discovered plugin.
#[derive(Clone, Debug)]
pub struct PluginInfo {
    pub name: String,
    pub path: PathBuf,
    pub scope: PluginScope,
}

#[derive(Clone, Debug)]
pub enum PluginScope {
    Global,
    Project,
}

/// Discover plugins from global and project directories.
pub fn discover_plugins(global_dir: &Path, project_dir: Option<&Path>) -> Vec<PluginInfo> {
    let mut plugins = Vec::new();

    // Global plugins
    let global_plugins = global_dir.join("plugins");
    if global_plugins.is_dir() {
        scan_dir(&global_plugins, PluginScope::Global, &mut plugins);
    }

    // Project plugins
    if let Some(proj) = project_dir {
        let project_plugins = proj.join("plugins");
        if project_plugins.is_dir() {
            scan_dir(&project_plugins, PluginScope::Project, &mut plugins);
        }
    }

    plugins
}

fn scan_dir(dir: &Path, scope: PluginScope, plugins: &mut Vec<PluginInfo>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();

        if path.is_file() {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if ext == "ts" || ext == "js" {
                    let name = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("unknown")
                        .to_string();
                    plugins.push(PluginInfo {
                        name,
                        path,
                        scope: scope.clone(),
                    });
                }
            }
        } else if path.is_dir() {
            // Check for index.ts inside directory
            let index = path.join("index.ts");
            if index.exists() {
                let name = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string();
                plugins.push(PluginInfo {
                    name,
                    path: index,
                    scope: scope.clone(),
                });
            }
        }
    }
}
