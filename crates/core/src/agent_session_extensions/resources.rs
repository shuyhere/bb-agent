use super::types::{
    ExtensionsResult, ResourceExtensionPaths, SkillCatalog,
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResourceLoaderState {
    pub extension_paths: ResourceExtensionPaths,
    pub extensions_result: ExtensionsResult,
    pub skills: SkillCatalog,
}

impl ResourceLoaderState {
    pub fn extend_resources(&mut self, paths: ResourceExtensionPaths) {
        self.extension_paths.skill_paths.extend(paths.skill_paths);
        self.extension_paths.prompt_paths.extend(paths.prompt_paths);
        self.extension_paths.theme_paths.extend(paths.theme_paths);
    }

    pub fn reload(&mut self) {
        self.extension_paths = ResourceExtensionPaths::default();
    }

    pub fn get_extensions(&self) -> ExtensionsResult {
        self.extensions_result.clone()
    }

    pub fn get_skills(&self) -> SkillCatalog {
        self.skills.clone()
    }
}
