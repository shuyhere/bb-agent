use std::path::{Path, PathBuf};

/// Discovered plugin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginInfo {
    name: String,
    path: PathBuf,
    scope: PluginScope,
}

impl PluginInfo {
    pub fn new(name: impl Into<String>, path: PathBuf, scope: PluginScope) -> Self {
        Self {
            name: name.into(),
            path,
            scope,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn scope(&self) -> PluginScope {
        self.scope
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PluginScope {
    Global,
    Project,
}

/// Discover plugins from global and project directories.
pub fn discover_plugins(global_dir: &Path, project_dir: Option<&Path>) -> Vec<PluginInfo> {
    let mut plugins = Vec::new();

    let global_plugins = global_dir.join("plugins");
    if global_plugins.is_dir() {
        scan_dir(&global_plugins, PluginScope::Global, &mut plugins);
    }

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
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();

        if path.is_file() {
            if let Some(ext) = path.extension().and_then(|ext| ext.to_str())
                && matches!(ext, "ts" | "js")
            {
                let name = path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or("unknown")
                    .to_string();
                plugins.push(PluginInfo::new(name, path, scope));
            }
        } else if path.is_dir() {
            let index = path.join("index.ts");
            if index.exists() {
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("unknown")
                    .to_string();
                plugins.push(PluginInfo::new(name, index, scope));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("bb-plugin-discovery-{name}-{unique}"))
    }

    #[test]
    fn discovers_global_and_project_plugins_with_scopes() {
        let global_dir = temp_dir("global");
        let project_dir = temp_dir("project");
        let global_plugins = global_dir.join("plugins");
        let project_plugins = project_dir.join("plugins");
        std::fs::create_dir_all(&global_plugins).expect("create global plugins dir");
        std::fs::create_dir_all(project_plugins.join("nested"))
            .expect("create project plugins dir");
        std::fs::write(
            global_plugins.join("alpha.js"),
            "module.exports = () => {};",
        )
        .expect("write alpha plugin");
        std::fs::write(project_plugins.join("beta.ts"), "export = () => {};")
            .expect("write beta plugin");
        std::fs::write(
            project_plugins.join("nested/index.ts"),
            "export = () => {};",
        )
        .expect("write nested plugin");

        let plugins = discover_plugins(&global_dir, Some(&project_dir));
        let discovered = plugins
            .iter()
            .map(|plugin| {
                (
                    plugin.name().to_string(),
                    plugin.scope(),
                    plugin
                        .path()
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or_default()
                        .to_string(),
                )
            })
            .collect::<Vec<_>>();

        assert!(discovered.contains(&(
            "alpha".to_string(),
            PluginScope::Global,
            "alpha.js".to_string()
        )));
        assert!(discovered.contains(&(
            "beta".to_string(),
            PluginScope::Project,
            "beta.ts".to_string()
        )));
        assert!(discovered.contains(&(
            "nested".to_string(),
            PluginScope::Project,
            "index.ts".to_string()
        )));

        let _ = std::fs::remove_dir_all(global_dir);
        let _ = std::fs::remove_dir_all(project_dir);
    }

    #[test]
    fn ignores_non_plugin_files() {
        let global_dir = temp_dir("ignore");
        let plugins_dir = global_dir.join("plugins");
        std::fs::create_dir_all(&plugins_dir).expect("create plugins dir");
        std::fs::write(plugins_dir.join("README.md"), "not a plugin").expect("write readme");
        std::fs::write(plugins_dir.join("config.json"), "{}").expect("write config");

        let plugins = discover_plugins(&global_dir, None);
        assert!(plugins.is_empty());

        let _ = std::fs::remove_dir_all(global_dir);
    }
}
