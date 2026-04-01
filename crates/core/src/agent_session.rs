use std::fmt;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::{agent, config};

/// Main session object for the bb-core public API.
///
/// This is a foundation port of pi's `AgentSession` shape. Runtime integrations
/// that depend on the concrete agent loop, session persistence, settings, model
/// registry, and extension system are intentionally left as TODO-safe hooks.
pub struct AgentSession {
    agent: RuntimeHandle,
    session_manager: RuntimeHandle,
    settings_manager: RuntimeHandle,

    scoped_models: Vec<ScopedModel>,

    // Event subscription state
    unsubscribe_agent: Option<Callback0>,
    event_listeners: Arc<Mutex<Vec<AgentSessionEventListener>>>,
    agent_event_queue_depth: usize,

    /// Tracks pending steering messages for UI display. Removed when delivered.
    steering_messages: Vec<String>,
    /// Tracks pending follow-up messages for UI display. Removed when delivered.
    follow_up_messages: Vec<String>,
    /// Messages queued to be included with the next user prompt as context.
    pending_next_turn_messages: Vec<CustomMessage>,

    // Compaction state
    compaction_in_flight: bool,
    auto_compaction_in_flight: bool,
    overflow_recovery_attempted: bool,

    // Branch summarization state
    branch_summary_in_flight: bool,

    // Retry state
    retry_in_flight: bool,
    retry_attempt: u32,

    // Bash execution state
    bash_in_flight: bool,
    pending_bash_messages: Vec<BashExecutionMessage>,

    // Extension system / runtime state
    extension_runner: Option<RuntimeHandle>,
    turn_index: u64,

    resource_loader: RuntimeHandle,
    custom_tools: Vec<ToolDefinition>,
    base_tool_definitions: Vec<ToolDefinition>,
    cwd: PathBuf,
    initial_active_tool_names: Option<Vec<String>>,
    base_tools_override: Option<Vec<AgentTool>>,
    session_start_event: SessionStartEvent,

    model_registry: RuntimeHandle,

    tool_registry: Vec<AgentTool>,
    tool_definitions: Vec<ToolDefinitionEntry>,
    tool_prompt_snippets: Vec<ToolPromptSnippet>,
    tool_prompt_guidelines: Vec<ToolPromptGuideline>,

    /// Base system prompt without per-turn extension appends.
    base_system_prompt: String,

    /// Session-facing model selection.
    model: Option<ModelRef>,
    thinking_level: ThinkingLevel,
    is_streaming: bool,
}

impl fmt::Debug for AgentSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentSession")
            .field("scoped_models", &self.scoped_models)
            .field("agent_event_queue_depth", &self.agent_event_queue_depth)
            .field("steering_messages", &self.steering_messages)
            .field("follow_up_messages", &self.follow_up_messages)
            .field(
                "pending_next_turn_messages",
                &self.pending_next_turn_messages,
            )
            .field("turn_index", &self.turn_index)
            .field("cwd", &self.cwd)
            .field("session_start_event", &self.session_start_event)
            .field("base_system_prompt", &self.base_system_prompt)
            .field("model", &self.model)
            .field("thinking_level", &self.thinking_level)
            .field("is_streaming", &self.is_streaming)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrintTurnStopReason {
    Completed,
    Error,
    Aborted,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrintTurnResult {
    pub text: String,
    pub stop_reason: PrintTurnStopReason,
    pub error_message: Option<String>,
}

impl PrintTurnResult {
    pub fn is_error(&self) -> bool {
        matches!(
            self.stop_reason,
            PrintTurnStopReason::Error | PrintTurnStopReason::Aborted
        )
    }
}

/// Thin single-shot adapter that mirrors pi's print-mode layering:
/// the CLI owns I/O, while prompt sequencing is owned by core session code.
pub struct ThinPrintSession<F> {
    run_turn: F,
    last_result: Option<PrintTurnResult>,
}

impl<F> ThinPrintSession<F> {
    pub fn new(run_turn: F) -> Self {
        Self {
            run_turn,
            last_result: None,
        }
    }

    pub fn last_result(&self) -> Option<&PrintTurnResult> {
        self.last_result.as_ref()
    }

    pub async fn prompt<Fut, E>(&mut self, text: impl Into<String>) -> Result<&PrintTurnResult, E>
    where
        F: FnMut(String) -> Fut,
        Fut: Future<Output = Result<PrintTurnResult, E>>,
    {
        let result = (self.run_turn)(text.into()).await?;
        self.last_result = Some(result);
        Ok(self
            .last_result
            .as_ref()
            .expect("thin print session stores the last turn result"))
    }

    pub async fn run<Fut, E>(
        &mut self,
        initial_message: Option<String>,
        messages: Vec<String>,
    ) -> Result<Option<&PrintTurnResult>, E>
    where
        F: FnMut(String) -> Fut,
        Fut: Future<Output = Result<PrintTurnResult, E>>,
    {
        if let Some(initial_message) = initial_message {
            self.prompt(initial_message).await?;
        }

        for message in messages {
            self.prompt(message).await?;
        }

        Ok(self.last_result())
    }
}

pub fn parse_model_arg(
    provider: Option<&str>,
    model: Option<&str>,
) -> (String, String, Option<String>) {
    let default_provider = provider.unwrap_or("anthropic").to_string();
    let default_model = match default_provider.as_str() {
        "openai" | "openai-codex" => "gpt-5.4",
        "google" => "gemini-2.5-pro",
        _ => "claude-sonnet-4-20250514",
    };

    let model_str = match model {
        Some(model) => model,
        None => return (default_provider, default_model.to_string(), None),
    };

    let (model_part, thinking) = if let Some(pos) = model_str.rfind(':') {
        let level = &model_str[pos + 1..];
        let valid = ["off", "low", "medium", "high", "minimal", "xhigh"];
        if valid.contains(&level) {
            (&model_str[..pos], Some(level.to_string()))
        } else {
            (model_str, None)
        }
    } else {
        (model_str, None)
    };

    if let Some(pos) = model_part.find('/') {
        let provider_name = &model_part[..pos];
        let model_id = &model_part[pos + 1..];
        (provider_name.to_string(), model_id.to_string(), thinking)
    } else {
        (default_provider, model_part.to_string(), thinking)
    }
}

pub fn messages_to_provider(messages: &[crate::types::AgentMessage]) -> Vec<serde_json::Value> {
    messages
        .iter()
        .filter_map(|msg| match msg {
            crate::types::AgentMessage::User(user) => {
                let text = user
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        crate::types::ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                Some(serde_json::json!({"role": "user", "content": text}))
            }
            crate::types::AgentMessage::Assistant(assistant) => {
                let text = agent::extract_text(&assistant.content);
                let tool_calls: Vec<serde_json::Value> = assistant
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        crate::types::AssistantContent::ToolCall {
                            id,
                            name,
                            arguments,
                        } => Some(serde_json::json!({
                            "id": id,
                            "type": "function",
                            "function": {
                                "name": name,
                                "arguments": serde_json::to_string(arguments).unwrap_or_default()
                            }
                        })),
                        _ => None,
                    })
                    .collect();
                let mut msg = serde_json::json!({"role": "assistant"});
                if !text.is_empty() {
                    msg["content"] = serde_json::json!(text);
                }
                if !tool_calls.is_empty() {
                    msg["tool_calls"] = serde_json::json!(tool_calls);
                }
                Some(msg)
            }
            crate::types::AgentMessage::ToolResult(tool_result) => {
                let text = tool_result
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        crate::types::ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                Some(serde_json::json!({
                    "role": "tool",
                    "tool_call_id": tool_result.tool_call_id,
                    "content": text,
                }))
            }
            crate::types::AgentMessage::CompactionSummary(compaction) => Some(serde_json::json!({
                "role": "user",
                "content": format!("[Previous conversation summary]\n\n{}", compaction.summary),
            })),
            crate::types::AgentMessage::BranchSummary(branch) => Some(serde_json::json!({
                "role": "user",
                "content": format!("[Branch summary]\n\n{}", branch.summary),
            })),
            _ => None,
        })
        .collect()
}

pub fn load_agents_md(cwd: &Path) -> Option<String> {
    let mut contents = Vec::new();

    let global = config::global_dir().join("AGENTS.md");
    if global.exists() {
        if let Ok(content) = std::fs::read_to_string(&global) {
            contents.push(content);
        }
    }

    let mut dir = cwd.to_path_buf();
    let mut scanned = Vec::new();
    loop {
        let agents = dir.join("AGENTS.md");
        if agents.exists() {
            scanned.push(agents);
        } else {
            let claude = dir.join("CLAUDE.md");
            if claude.exists() {
                scanned.push(claude);
            }
        }

        if dir.join(".git").exists() {
            break;
        }
        if !dir.pop() {
            break;
        }
    }

    scanned.reverse();
    for path in scanned {
        if let Ok(content) = std::fs::read_to_string(&path) {
            contents.push(content);
        }
    }

    if contents.is_empty() {
        None
    } else {
        Some(contents.join("\n\n---\n\n"))
    }
}

impl AgentSession {
    pub fn new(config: AgentSessionConfig) -> Self {
        let mut session = Self {
            agent: config.agent,
            session_manager: config.session_manager,
            settings_manager: config.settings_manager,
            scoped_models: config.scoped_models,
            unsubscribe_agent: None,
            event_listeners: Arc::new(Mutex::new(Vec::new())),
            agent_event_queue_depth: 0,
            steering_messages: Vec::new(),
            follow_up_messages: Vec::new(),
            pending_next_turn_messages: Vec::new(),
            compaction_in_flight: false,
            auto_compaction_in_flight: false,
            overflow_recovery_attempted: false,
            branch_summary_in_flight: false,
            retry_in_flight: false,
            retry_attempt: 0,
            bash_in_flight: false,
            pending_bash_messages: Vec::new(),
            extension_runner: None,
            turn_index: 0,
            resource_loader: config.resource_loader,
            custom_tools: config.custom_tools,
            base_tool_definitions: Vec::new(),
            cwd: config.cwd,
            initial_active_tool_names: config.initial_active_tool_names,
            base_tools_override: config.base_tools_override,
            session_start_event: config
                .session_start_event
                .unwrap_or_else(SessionStartEvent::startup),
            model_registry: config.model_registry,
            tool_registry: Vec::new(),
            tool_definitions: Vec::new(),
            tool_prompt_snippets: Vec::new(),
            tool_prompt_guidelines: Vec::new(),
            base_system_prompt: String::new(),
            model: config.model,
            thinking_level: config.thinking_level,
            is_streaming: false,
        };

        // Faithful to pi's constructor shape: subscribe internal handlers first,
        // then build runtime state.
        session.install_agent_subscription();
        session.build_runtime(RuntimeBuildOptions {
            active_tool_names: session.initial_active_tool_names.clone(),
            include_all_extension_tools: true,
        });

        session
    }

    pub fn model(&self) -> Option<&ModelRef> {
        self.model.as_ref()
    }

    pub fn thinking_level(&self) -> ThinkingLevel {
        self.thinking_level
    }

    pub fn is_streaming(&self) -> bool {
        self.is_streaming
    }

    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    pub fn session_start_event(&self) -> &SessionStartEvent {
        &self.session_start_event
    }

    pub fn pending_message_count(&self) -> usize {
        self.steering_messages.len() + self.follow_up_messages.len()
    }

    pub fn get_steering_messages(&self) -> &[String] {
        &self.steering_messages
    }

    pub fn get_follow_up_messages(&self) -> &[String] {
        &self.follow_up_messages
    }

    pub fn subscribe(&mut self, listener: AgentSessionEventListener) -> SubscriptionHandle {
        let mut listeners = self
            .event_listeners
            .lock()
            .expect("agent session event listener mutex poisoned");
        let index = listeners.len();
        listeners.push(listener);
        SubscriptionHandle { index }
    }

    pub fn unsubscribe(&mut self, handle: SubscriptionHandle) -> bool {
        let mut listeners = self
            .event_listeners
            .lock()
            .expect("agent session event listener mutex poisoned");
        if let Some(slot) = listeners.get_mut(handle.index) {
            *slot = Box::new(|_| {});
            true
        } else {
            false
        }
    }

    pub fn emit(&self, event: AgentSessionEvent) {
        self.emit_ref(&event);
    }

    pub fn prompt(
        &mut self,
        text: impl Into<String>,
        options: PromptOptions,
    ) -> Result<(), AgentSessionError> {
        let text = text.into();
        let expand_prompt_templates = options.expand_prompt_templates;

        if expand_prompt_templates && text.starts_with('/') {
            if self.try_execute_extension_command(&text) {
                return Ok(());
            }
        }

        let mut expanded_text = text;
        if expand_prompt_templates {
            expanded_text = self.expand_skill_command(expanded_text);
            expanded_text = self.expand_prompt_template(expanded_text);
        }

        if self.is_streaming {
            match options.streaming_behavior {
                Some(StreamingBehavior::FollowUp) => {
                    self.queue_follow_up(expanded_text, options.images);
                }
                Some(StreamingBehavior::Steer) => {
                    self.queue_steer(expanded_text, options.images);
                }
                None => {
                    return Err(AgentSessionError::AlreadyProcessing);
                }
            }
            return Ok(());
        }

        self.flush_pending_bash_messages();

        if self.model.is_none() {
            return Err(AgentSessionError::NoModelSelected);
        }

        let mut outgoing = Vec::new();
        let mut user_content = Vec::new();
        user_content.push(ContentPart::Text(TextContent {
            text: expanded_text,
        }));
        user_content.extend(options.images.into_iter().map(ContentPart::Image));
        outgoing.push(SessionMessage::User(UserMessage {
            content: user_content,
            source: options.source,
        }));

        outgoing.extend(
            self.pending_next_turn_messages
                .drain(..)
                .map(SessionMessage::Custom),
        );

        self.is_streaming = true;
        self.emit_ref(&AgentSessionEvent::PromptDispatched { messages: outgoing });
        self.wait_for_retry();
        self.is_streaming = false;
        Ok(())
    }

    pub fn steer(
        &mut self,
        text: impl Into<String>,
        images: Vec<ImageContent>,
    ) -> Result<(), AgentSessionError> {
        let text = text.into();
        if text.starts_with('/') {
            self.throw_if_extension_command(&text)?;
        }

        let expanded = self.expand_prompt_template(self.expand_skill_command(text));
        self.queue_steer(expanded, images);
        Ok(())
    }

    pub fn follow_up(
        &mut self,
        text: impl Into<String>,
        images: Vec<ImageContent>,
    ) -> Result<(), AgentSessionError> {
        let text = text.into();
        if text.starts_with('/') {
            self.throw_if_extension_command(&text)?;
        }

        let expanded = self.expand_prompt_template(self.expand_skill_command(text));
        self.queue_follow_up(expanded, images);
        Ok(())
    }

    pub fn send_custom_message(
        &mut self,
        message: CustomMessage,
        options: SendCustomMessageOptions,
    ) -> Result<(), AgentSessionError> {
        if matches!(options.deliver_as, Some(CustomMessageDelivery::NextTurn)) {
            self.pending_next_turn_messages.push(message);
            return Ok(());
        }

        if self.is_streaming {
            match options.deliver_as.unwrap_or(CustomMessageDelivery::Steer) {
                CustomMessageDelivery::Steer => {
                    self.emit_ref(&AgentSessionEvent::CustomMessageQueued {
                        delivery: CustomMessageDelivery::Steer,
                        message: message.clone(),
                    });
                }
                CustomMessageDelivery::FollowUp => {
                    self.emit_ref(&AgentSessionEvent::CustomMessageQueued {
                        delivery: CustomMessageDelivery::FollowUp,
                        message: message.clone(),
                    });
                }
                CustomMessageDelivery::NextTurn => {}
            }
            return Ok(());
        }

        if options.trigger_turn {
            self.emit_ref(&AgentSessionEvent::PromptDispatched {
                messages: vec![SessionMessage::Custom(message)],
            });
            return Ok(());
        }

        self.emit_ref(&AgentSessionEvent::MessageStart {
            message: SessionMessage::Custom(message.clone()),
        });
        self.emit_ref(&AgentSessionEvent::MessageEnd {
            message: SessionMessage::Custom(message),
        });
        Ok(())
    }

    pub fn send_user_message(
        &mut self,
        content: UserMessageContent,
        options: SendUserMessageOptions,
    ) -> Result<(), AgentSessionError> {
        let (text, images) = content.into_text_and_images();
        self.prompt(
            text,
            PromptOptions {
                expand_prompt_templates: false,
                streaming_behavior: options.deliver_as,
                images,
                source: PromptSource::Extension,
            },
        )
    }

    pub fn clear_queue(&mut self) -> QueueState {
        let steering = std::mem::take(&mut self.steering_messages);
        let follow_up = std::mem::take(&mut self.follow_up_messages);
        let state = QueueState {
            steering,
            follow_up,
        };
        self.emit_queue_update();
        state
    }

    pub fn set_model(&mut self, model: ModelRef) {
        self.model = Some(model.clone());
        self.emit_ref(&AgentSessionEvent::ModelChanged {
            model,
            source: ModelChangeSource::Set,
        });
    }

    pub fn set_thinking_level(&mut self, level: ThinkingLevel) {
        self.thinking_level = level;
        self.emit_ref(&AgentSessionEvent::ThinkingLevelChanged { level });
    }

    fn install_agent_subscription(&mut self) {
        // TODO: hook concrete runtime agent events.
        self.unsubscribe_agent = Some(Box::new(|| {}));
    }

    fn build_runtime(&mut self, _options: RuntimeBuildOptions) {
        // TODO: port runtime tool / extension initialization.
    }

    fn emit_ref(&self, event: &AgentSessionEvent) {
        let listeners = self
            .event_listeners
            .lock()
            .expect("agent session event listener mutex poisoned");
        for listener in listeners.iter() {
            listener(event);
        }
    }

    fn emit_queue_update(&self) {
        self.emit_ref(&AgentSessionEvent::QueueUpdate {
            steering: self.steering_messages.clone(),
            follow_up: self.follow_up_messages.clone(),
        });
    }

    fn try_execute_extension_command(&self, _text: &str) -> bool {
        // TODO: integrate extension command execution.
        false
    }

    fn expand_skill_command(&self, text: String) -> String {
        // TODO: integrate resource loader based skill expansion.
        text
    }

    fn expand_prompt_template(&self, text: String) -> String {
        // TODO: integrate prompt template expansion.
        text
    }

    fn queue_steer(&mut self, text: String, images: Vec<ImageContent>) {
        self.steering_messages.push(text.clone());
        self.emit_queue_update();
        self.emit_ref(&AgentSessionEvent::UserMessageQueued {
            delivery: StreamingBehavior::Steer,
            message: UserMessage {
                content: content_from_text_and_images(text, images),
                source: PromptSource::Extension,
            },
        });
    }

    fn queue_follow_up(&mut self, text: String, images: Vec<ImageContent>) {
        self.follow_up_messages.push(text.clone());
        self.emit_queue_update();
        self.emit_ref(&AgentSessionEvent::UserMessageQueued {
            delivery: StreamingBehavior::FollowUp,
            message: UserMessage {
                content: content_from_text_and_images(text, images),
                source: PromptSource::Extension,
            },
        });
    }

    fn throw_if_extension_command(&self, _text: &str) -> Result<(), AgentSessionError> {
        // TODO: detect registered extension commands once the runtime extension
        // registry exists in bb-core. For now, unknown slash-prefixed commands
        // are treated like ordinary user text, matching pi's behavior for
        // non-extension commands.
        Ok(())
    }

    fn flush_pending_bash_messages(&mut self) {
        if self.pending_bash_messages.is_empty() {
            return;
        }
        self.pending_bash_messages.clear();
        self.emit_ref(&AgentSessionEvent::BashMessagesFlushed);
    }

    fn wait_for_retry(&mut self) {
        if self.retry_in_flight {
            self.retry_in_flight = false;
        }
    }
}

#[derive(Debug, Clone)]
pub struct AgentSessionConfig {
    pub agent: RuntimeHandle,
    pub session_manager: RuntimeHandle,
    pub settings_manager: RuntimeHandle,
    pub scoped_models: Vec<ScopedModel>,
    pub resource_loader: RuntimeHandle,
    pub custom_tools: Vec<ToolDefinition>,
    pub cwd: PathBuf,
    pub model_registry: RuntimeHandle,
    pub initial_active_tool_names: Option<Vec<String>>,
    pub base_tools_override: Option<Vec<AgentTool>>,
    pub session_start_event: Option<SessionStartEvent>,
    pub model: Option<ModelRef>,
    pub thinking_level: ThinkingLevel,
}

impl Default for AgentSessionConfig {
    fn default() -> Self {
        Self {
            agent: RuntimeHandle::placeholder("agent"),
            session_manager: RuntimeHandle::placeholder("session_manager"),
            settings_manager: RuntimeHandle::placeholder("settings_manager"),
            scoped_models: Vec::new(),
            resource_loader: RuntimeHandle::placeholder("resource_loader"),
            custom_tools: Vec::new(),
            cwd: PathBuf::new(),
            model_registry: RuntimeHandle::placeholder("model_registry"),
            initial_active_tool_names: None,
            base_tools_override: None,
            session_start_event: None,
            model: None,
            thinking_level: ThinkingLevel::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PromptOptions {
    pub expand_prompt_templates: bool,
    pub streaming_behavior: Option<StreamingBehavior>,
    pub images: Vec<ImageContent>,
    pub source: PromptSource,
}

impl Default for PromptOptions {
    fn default() -> Self {
        Self {
            expand_prompt_templates: true,
            streaming_behavior: None,
            images: Vec::new(),
            source: PromptSource::Interactive,
        }
    }
}

impl PromptOptions {
    pub fn expanded() -> Self {
        Self::default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamingBehavior {
    Steer,
    FollowUp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CustomMessageDelivery {
    Steer,
    FollowUp,
    NextTurn,
}

#[derive(Debug, Clone, Default)]
pub struct SendCustomMessageOptions {
    pub trigger_turn: bool,
    pub deliver_as: Option<CustomMessageDelivery>,
}

#[derive(Debug, Clone, Default)]
pub struct SendUserMessageOptions {
    pub deliver_as: Option<StreamingBehavior>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PromptSource {
    #[default]
    Interactive,
    Extension,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueState {
    pub steering: Vec<String>,
    pub follow_up: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentSessionEvent {
    QueueUpdate {
        steering: Vec<String>,
        follow_up: Vec<String>,
    },
    PromptDispatched {
        messages: Vec<SessionMessage>,
    },
    UserMessageQueued {
        delivery: StreamingBehavior,
        message: UserMessage,
    },
    CustomMessageQueued {
        delivery: CustomMessageDelivery,
        message: CustomMessage,
    },
    MessageStart {
        message: SessionMessage,
    },
    MessageEnd {
        message: SessionMessage,
    },
    ModelChanged {
        model: ModelRef,
        source: ModelChangeSource,
    },
    ThinkingLevelChanged {
        level: ThinkingLevel,
    },
    BashMessagesFlushed,
}

pub type AgentSessionEventListener = Box<dyn Fn(&AgentSessionEvent) + Send + Sync + 'static>;
pub type Callback0 = Box<dyn Fn() + Send + Sync + 'static>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubscriptionHandle {
    index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelChangeSource {
    Set,
    Cycle,
    Restore,
}

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
    fn into_text_and_images(self) -> (String, Vec<ImageContent>) {
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

fn content_from_text_and_images(text: String, images: Vec<ImageContent>) -> Vec<ContentPart> {
    let mut content = vec![ContentPart::Text(TextContent { text })];
    content.extend(images.into_iter().map(ContentPart::Image));
    content
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionStartEvent {
    pub reason: String,
}

impl SessionStartEvent {
    pub fn startup() -> Self {
        Self {
            reason: "startup".to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopedModel {
    pub model: ModelRef,
    pub thinking_level: Option<ThinkingLevel>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRef {
    pub provider: String,
    pub id: String,
    pub reasoning: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThinkingLevel {
    #[default]
    Off,
    Low,
    Medium,
    High,
    XHigh,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BashExecutionMessage {
    pub command: String,
    pub status: BashExecutionStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BashExecutionStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolDefinition {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTool {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolDefinitionEntry {
    pub name: String,
    pub definition: ToolDefinition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolPromptSnippet {
    pub tool_name: String,
    pub snippet: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolPromptGuideline {
    pub tool_name: String,
    pub guidelines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeBuildOptions {
    pub active_tool_names: Option<Vec<String>>,
    pub include_all_extension_tools: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeHandle {
    pub kind: &'static str,
}

impl RuntimeHandle {
    pub fn placeholder(kind: &'static str) -> Self {
        Self { kind }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentSessionError {
    AlreadyProcessing,
    NoModelSelected,
    ExtensionCommandCannotBeQueued,
}

impl fmt::Display for AgentSessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AgentSessionError::AlreadyProcessing => {
                write!(
                    f,
                    "agent is already processing; choose steer or follow-up delivery"
                )
            }
            AgentSessionError::NoModelSelected => write!(f, "no model selected"),
            AgentSessionError::ExtensionCommandCannotBeQueued => {
                write!(f, "extension command cannot be queued")
            }
        }
    }
}

impl std::error::Error for AgentSessionError {}
