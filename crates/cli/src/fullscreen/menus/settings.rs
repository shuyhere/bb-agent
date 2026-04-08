use super::*;

fn execution_mode_menu_label(mode: ExecutionMode) -> &'static str {
    match mode {
        ExecutionMode::Safety => "Safety — current project only",
        ExecutionMode::Yolo => "Yolo — full access",
    }
}

fn persist_fullscreen_retry_settings(
    enabled: bool,
    max_retries: u32,
    base_delay_ms: u64,
    max_delay_ms: u64,
) -> Result<()> {
    let mut settings = Settings::load_global();
    settings.retry.enabled = enabled;
    settings.retry.max_retries = max_retries.max(1);
    settings.retry.base_delay_ms = base_delay_ms.max(1_000);
    settings.retry.max_delay_ms = max_delay_ms.max(settings.retry.base_delay_ms);
    settings.save_global()?;
    Ok(())
}

fn persist_fullscreen_execution_mode(mode: ExecutionMode) -> Result<()> {
    let mut settings = Settings::load_global();
    settings.execution_mode = Some(mode);
    settings.save_global()?;
    Ok(())
}

impl FullscreenController {
    pub(super) fn current_color_theme_name(&self) -> &'static str {
        self.color_theme.name()
    }

    pub(super) fn open_settings_menu(&mut self) {
        let items = vec![
            SelectItem {
                label: format!("Color theme [{}]", self.current_color_theme_name()),
                detail: Some("User input block & spinner colors".to_string()),
                value: "color-theme".to_string(),
            },
            SelectItem {
                label: format!("Thinking level [{}]", self.session_setup.thinking_level),
                detail: Some("Reasoning depth".to_string()),
                value: "thinking".to_string(),
            },
            SelectItem {
                label: format!(
                    "Execution mode [{}]",
                    self.session_setup.tool_ctx.execution_policy.as_str()
                ),
                detail: Some("Safety — current project only • Yolo — full access".to_string()),
                value: "execution-mode".to_string(),
            },
            SelectItem {
                label: format!(
                    "Auto-retry [{}]",
                    if self.session_setup.retry_enabled {
                        "true"
                    } else {
                        "false"
                    }
                ),
                detail: Some("Retry retryable provider errors".to_string()),
                value: "retry-enabled".to_string(),
            },
            SelectItem {
                label: format!("Retry attempts [{}]", self.session_setup.retry_max_retries),
                detail: Some("Maximum retry attempts".to_string()),
                value: "retry-max".to_string(),
            },
            SelectItem {
                label: format!(
                    "Retry base delay [{}s]",
                    self.session_setup.retry_base_delay_ms / 1000
                ),
                detail: Some("Initial retry backoff".to_string()),
                value: "retry-delay".to_string(),
            },
            SelectItem {
                label: format!(
                    "Retry max delay [{}s]",
                    self.session_setup.retry_max_delay_ms / 1000
                ),
                detail: Some("Maximum allowed retry delay".to_string()),
                value: "retry-max-delay".to_string(),
            },
        ];

        self.send_command(FullscreenCommand::OpenSelectMenu {
            menu_id: "settings".to_string(),
            title: "Settings".to_string(),
            items,
            selected_value: None,
        });
    }

    pub(super) fn open_setting_values_menu(&mut self, setting_id: &str) {
        let (title, values): (&str, Vec<&str>) = match setting_id {
            "color-theme" => (
                "Color theme",
                vec!["pink", "lavender", "ocean", "mint", "sunset", "slate"],
            ),
            "thinking" => (
                "Thinking level",
                vec!["off", "low", "medium", "high", "xhigh"],
            ),
            "execution-mode" => ("Execution mode", vec!["safety", "yolo"]),
            "retry-enabled" => ("Auto-retry", vec!["true", "false"]),
            "retry-max" => ("Retry attempts", vec!["1", "2", "3", "4", "5"]),
            "retry-delay" => ("Retry base delay", vec!["1s", "2s", "5s", "10s"]),
            "retry-max-delay" => ("Retry max delay", vec!["10s", "30s", "60s", "120s"]),
            _ => {
                self.send_command(FullscreenCommand::SetStatusLine(format!(
                    "Unknown setting: {setting_id}"
                )));
                return;
            }
        };

        let items = if setting_id == "execution-mode" {
            vec![
                SelectItem {
                    label: execution_mode_menu_label(ExecutionMode::Safety).to_string(),
                    detail: Some(
                        "Restrict write/edit tools to the current project directory".to_string(),
                    ),
                    value: "safety".to_string(),
                },
                SelectItem {
                    label: execution_mode_menu_label(ExecutionMode::Yolo).to_string(),
                    detail: Some("Allow write/edit tools with full filesystem access".to_string()),
                    value: "yolo".to_string(),
                },
            ]
        } else {
            values
                .into_iter()
                .map(|value| SelectItem {
                    label: value.to_string(),
                    detail: None,
                    value: value.to_string(),
                })
                .collect()
        };

        self.send_command(FullscreenCommand::OpenSelectMenu {
            menu_id: format!("settings:{setting_id}"),
            title: title.to_string(),
            items,
            selected_value: None,
        });
    }

    pub(super) fn apply_setting_value(&mut self, setting_id: &str, value: &str) -> Result<()> {
        match setting_id {
            "color-theme" => {
                if let Some(theme) = bb_tui::fullscreen::spinner::ColorTheme::from_name(value) {
                    self.color_theme = theme;
                    self.send_command(FullscreenCommand::SetColorTheme(theme));
                    // Persist to settings
                    let mut settings = bb_core::settings::Settings::load_global();
                    settings.color_theme = Some(value.to_string());
                    let _ = settings.save_global();
                    self.mark_local_settings_saved();
                    self.send_command(FullscreenCommand::SetStatusLine(format!(
                        "Color theme: {value}"
                    )));
                } else {
                    self.send_command(FullscreenCommand::SetStatusLine(format!(
                        "Unknown color theme: {value}"
                    )));
                }
            }
            "thinking" => {
                let level = ThinkingLevel::parse(value).unwrap_or(ThinkingLevel::Medium);
                self.session_setup.thinking_level = level.as_str().to_string();
                self.runtime_host.session_mut().set_thinking_level(level);
                self.publish_footer();
                self.send_command(FullscreenCommand::SetStatusLine(format!(
                    "Thinking: {value}"
                )));
            }
            "execution-mode" => {
                let mode = match value {
                    "yolo" => ExecutionMode::Yolo,
                    _ => ExecutionMode::Safety,
                };
                self.session_setup.tool_ctx.execution_policy =
                    bb_tools::ExecutionPolicy::from(mode);
                persist_fullscreen_execution_mode(mode)?;
                self.mark_local_settings_saved();
                self.send_command(FullscreenCommand::SetStatusLine(format!(
                    "Execution mode: {}",
                    execution_mode_menu_label(mode)
                )));
            }
            "retry-enabled" => {
                self.session_setup.retry_enabled = value == "true";
                persist_fullscreen_retry_settings(
                    self.session_setup.retry_enabled,
                    self.session_setup.retry_max_retries,
                    self.session_setup.retry_base_delay_ms,
                    self.session_setup.retry_max_delay_ms,
                )?;
                self.mark_local_settings_saved();
                self.send_command(FullscreenCommand::SetStatusLine(format!(
                    "Auto-retry: {value}"
                )));
            }
            "retry-max" => {
                let parsed = value
                    .parse::<u32>()
                    .unwrap_or(self.session_setup.retry_max_retries);
                self.session_setup.retry_max_retries = parsed.max(1);
                persist_fullscreen_retry_settings(
                    self.session_setup.retry_enabled,
                    self.session_setup.retry_max_retries,
                    self.session_setup.retry_base_delay_ms,
                    self.session_setup.retry_max_delay_ms,
                )?;
                self.mark_local_settings_saved();
                self.send_command(FullscreenCommand::SetStatusLine(format!(
                    "Retry attempts: {}",
                    self.session_setup.retry_max_retries
                )));
            }
            "retry-delay" => {
                let secs = value.trim_end_matches('s').parse::<u64>().unwrap_or(1);
                self.session_setup.retry_base_delay_ms = secs.max(1) * 1000;
                if self.session_setup.retry_max_delay_ms < self.session_setup.retry_base_delay_ms {
                    self.session_setup.retry_max_delay_ms = self.session_setup.retry_base_delay_ms;
                }
                persist_fullscreen_retry_settings(
                    self.session_setup.retry_enabled,
                    self.session_setup.retry_max_retries,
                    self.session_setup.retry_base_delay_ms,
                    self.session_setup.retry_max_delay_ms,
                )?;
                self.mark_local_settings_saved();
                self.send_command(FullscreenCommand::SetStatusLine(format!(
                    "Retry base delay: {value}"
                )));
            }
            "retry-max-delay" => {
                let secs = value.trim_end_matches('s').parse::<u64>().unwrap_or(10);
                self.session_setup.retry_max_delay_ms = secs.max(1) * 1000;
                if self.session_setup.retry_max_delay_ms < self.session_setup.retry_base_delay_ms {
                    self.session_setup.retry_max_delay_ms = self.session_setup.retry_base_delay_ms;
                }
                persist_fullscreen_retry_settings(
                    self.session_setup.retry_enabled,
                    self.session_setup.retry_max_retries,
                    self.session_setup.retry_base_delay_ms,
                    self.session_setup.retry_max_delay_ms,
                )?;
                self.mark_local_settings_saved();
                self.send_command(FullscreenCommand::SetStatusLine(format!(
                    "Retry max delay: {value}"
                )));
            }
            _ => {
                self.send_command(FullscreenCommand::SetStatusLine(format!(
                    "Unknown setting: {setting_id}"
                )));
            }
        }
        Ok(())
    }
}
