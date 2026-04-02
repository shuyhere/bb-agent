use super::config::PromptSource;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionMessage {
    User(UserMessage),
    Assistant(AssistantMessage),
    Custom(CustomMessage),
    ToolResult(ToolResultMessage),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserMessage {
    pub content: Vec<ContentPart>,
    pub source: PromptSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssistantMessage {
    pub content: String,
    pub stop_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolResultMessage {
    pub tool_name: String,
    pub content: String,
    pub is_error: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomMessage {
    pub custom_type: String,
    pub content: String,
    pub display: Option<String>,
    pub details: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserMessageContent {
    Text(String),
    Parts(Vec<ContentPart>),
}

impl UserMessageContent {
    pub(super) fn into_text_and_images(self) -> (String, Vec<ImageContent>) {
        match self {
            UserMessageContent::Text(text) => (text, Vec::new()),
            UserMessageContent::Parts(parts) => {
                let mut text_parts = Vec::new();
                let mut images = Vec::new();
                for part in parts {
                    match part {
                        ContentPart::Text(text) => text_parts.push(text.text),
                        ContentPart::Image(image) => images.push(image),
                    }
                }
                (text_parts.join("\n"), images)
            }
        }
    }
}

impl From<String> for UserMessageContent {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for UserMessageContent {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

impl From<Vec<ContentPart>> for UserMessageContent {
    fn from(value: Vec<ContentPart>) -> Self {
        Self::Parts(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentPart {
    Text(TextContent),
    Image(ImageContent),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextContent {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageContent {
    pub source: String,
    pub mime_type: Option<String>,
}

pub(super) fn content_from_text_and_images(
    text: String,
    images: Vec<ImageContent>,
) -> Vec<ContentPart> {
    let mut content = vec![ContentPart::Text(TextContent { text })];
    content.extend(images.into_iter().map(ContentPart::Image));
    content
}
