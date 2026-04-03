use std::path::{Path, PathBuf};

use anyhow::Result;
use bb_core::settings::Settings;
use bb_tools::Tool;

#[derive(Debug, Clone, Default)]
pub(crate) struct ExtensionBootstrap {
    pub paths: Vec<PathBuf>,
}

impl ExtensionBootstrap {
    pub(crate) fn from_cli_values(cwd: &Path, values: &[String]) -> Self {
        Self {
            paths: values.iter().map(|value| resolve_input_path(cwd, value)).collect(),
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct ExtensionCommandRegistry;

impl ExtensionCommandRegistry {
    pub(crate) fn is_registered(&self, _text: &str) -> bool {
        false
    }

    pub(crate) async fn execute_text(&self, _text: &str) -> Result<Option<String>> {
        Ok(None)
    }
}

pub(crate) struct RuntimeExtensionSupport {
    pub tools: Vec<Box<dyn Tool>>,
    pub commands: ExtensionCommandRegistry,
}

impl Default for RuntimeExtensionSupport {
    fn default() -> Self {
        Self {
            tools: Vec::new(),
            commands: ExtensionCommandRegistry,
        }
    }
}

pub(crate) async fn load_runtime_extension_support(
    _cwd: &Path,
    _settings: &Settings,
    _bootstrap: &ExtensionBootstrap,
) -> Result<RuntimeExtensionSupport> {
    Ok(RuntimeExtensionSupport::default())
}

fn resolve_input_path(cwd: &Path, value: &str) -> PathBuf {
    let path = Path::new(value);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}
