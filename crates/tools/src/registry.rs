use crate::types::Tool;

/// Get all built-in tools.
pub fn builtin_tools() -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(crate::read::ReadTool),
        Box::new(crate::bash::BashTool),
        Box::new(crate::edit::EditTool),
        Box::new(crate::write::WriteTool),
        Box::new(crate::find::FindTool),
        Box::new(crate::grep::GrepTool),
        Box::new(crate::ls::LsTool),
        Box::new(crate::web_search::WebSearchTool),
        Box::new(crate::web_fetch::WebFetchTool),
        Box::new(crate::browser_fetch::BrowserFetchTool),
    ]
}
