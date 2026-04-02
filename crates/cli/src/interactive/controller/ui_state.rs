use super::*;

impl InteractiveMode {
    pub(super) fn take_last_submitted_text(&mut self) -> String {
        self.streaming.pending_working_message
            .take()
            .unwrap_or_else(|| String::new())
    }

    pub(super) fn sync_static_sections(&mut self) {
        self.rebuild_chat_container();
        self.rebuild_pending_container();
        self.rebuild_status_container();
        self.rebuild_footer();
    }

    pub(super) fn refresh_ui(&mut self) {
        self.rebuild_chat_container();
        self.rebuild_pending_container();
        self.rebuild_status_container();
        self.rebuild_footer();
        // When an overlay is open, force full redraw to avoid stale content
        // from differential rendering artifacts.
        if self.ui.tui.has_overlay() {
            self.ui.tui.force_render();
        } else {
            self.ui.tui.render();
        }
    }

    /// Lightweight render that skips rebuilding the chat container.
    /// Use for typing in the editor where chat content hasn't changed.
    pub(super) fn render_editor_frame(&mut self) {
        // Only rebuild status (cheap), skip chat + pending + footer (expensive).
        self.rebuild_status_container();
        // Mark root dirty since status/editor changed.
        self.ui.tui.invalidate_root();
        self.ui.tui.render();
    }

    pub(super) fn rebuild_header(&mut self) {
        self.render_cache.header_lines.clear();
        if !self.options.quiet_startup {
            let dim = "\x1b[90m";
            let reset = "\x1b[0m";
            let bold = "\x1b[1m";
            let cyan = "\x1b[36m";
            self.render_cache.header_lines.push(format!(
                "{bold}{cyan}BB-Agent{reset} v{}",
                self.version
            ));
            self.render_cache.header_lines.push(format!(
                "{dim}Ctrl-C exit . / commands . ! bash . F2 thinking . /help for more{reset}"
            ));
        }

        if let Ok(mut header) = self.ui.header_container.lock() {
            header.clear();
            if !self.render_cache.header_lines.is_empty() {
                header.add(Box::new(Text::new(&self.render_cache.header_lines.join("\n"))));
                header.add(Box::new(Spacer::new(1)));
            }
        }
    }

    pub(super) fn rebuild_chat_container(&mut self) {
        let lines = self.chat_render_lines();
        Self::replace_container_lines(&self.ui.chat_container, &lines);
        self.ui.tui.invalidate_root();
    }

    /// Cache rendered lines for all completed chat items.
    /// Call after adding a finalized message (not during streaming).
    pub(super) fn snapshot_chat_cache(&mut self) {
        let width = self.ui.tui.columns();
        let item_count = self.controller.session.render_state.chat_items.len();
        let prefix = Self::render_items_to_lines(
            &mut self.controller.session.render_state.chat_items, width,
        );
        self.render_cache.cached_chat_lines_prefix = prefix;
        self.render_cache.cached_chat_line_count = item_count;
        self.render_cache.cached_chat_width = width;
    }

    /// Invalidate the chat line cache (call when items are removed/replaced).
    pub(super) fn invalidate_chat_cache(&mut self) {
        self.render_cache.cached_chat_line_count = 0;
        self.render_cache.cached_chat_lines_prefix.clear();
    }

    pub(super) fn rebuild_pending_container(&mut self) {
        self.sync_pending_render_state();
        let lines = self.pending_render_lines();
        Self::replace_container_lines(&self.ui.pending_messages_container, &lines);
    }

    pub(super) fn rebuild_status_container(&mut self) {
        if let Ok(mut container) = self.ui.status_container.lock() {
            if let Some((style, message)) = &self.streaming.status_loader {
                let mut reused = false;
                if container.children.len() == 1 {
                    if let Some(loader) = container.children[0]
                        .as_any_mut()
                        .downcast_mut::<StatusLoaderComponent>()
                    {
                        if &loader.style == style {
                            loader.set_message(message.clone());
                            reused = true;
                        } else {
                            loader.stop();
                        }
                    }
                }
                if !reused {
                    container.clear();
                    container.add(Box::new(StatusLoaderComponent::new(*style, message.clone())));
                }
                return;
            }

            let recent = self
                .render_cache.status_lines
                .iter()
                .rev()
                .take(3)
                .cloned()
                .collect::<Vec<_>>();
            let mut recent = recent;
            recent.reverse();

            if recent.is_empty() {
                if let Some(loader) = container
                    .children
                    .get_mut(0)
                    .and_then(|child| child.as_any_mut().downcast_mut::<StatusLoaderComponent>())
                {
                    loader.stop();
                }
                container.clear();
                return;
            }

            let text = recent.join("\n");
            if container.children.len() == 1 {
                if let Some(existing) = container.children[0].as_any_mut().downcast_mut::<Text>() {
                    existing.set(&text);
                    return;
                }
                if let Some(loader) = container.children[0]
                    .as_any_mut()
                    .downcast_mut::<StatusLoaderComponent>()
                {
                    loader.stop();
                }
            }
            container.clear();
            container.add(Box::new(Text::new(&text)));
        }
    }

    pub(super) fn footer_usage_totals(&self) -> (u64, u64, u64, u64, f64) {
        let mut total_input = 0_u64;
        let mut total_output = 0_u64;
        let mut total_cache_read = 0_u64;
        let mut total_cache_write = 0_u64;
        let mut total_cost = 0.0_f64;

        if let Ok(rows) = store::get_entries(&self.session_setup.conn, &self.session_setup.session_id) {
            for row in rows {
                if let Ok(entry) = store::parse_entry(&row) {
                    if let bb_core::types::SessionEntry::Message {
                        message: bb_core::types::AgentMessage::Assistant(message),
                        ..
                    } = entry
                    {
                        total_input += message.usage.input;
                        total_output += message.usage.output;
                        total_cache_read += message.usage.cache_read;
                        total_cache_write += message.usage.cache_write;
                        total_cost += message.usage.cost.total;
                    }
                }
            }
        }

        (
            total_input,
            total_output,
            total_cache_read,
            total_cache_write,
            total_cost,
        )
    }

    pub(super) fn available_provider_count(&self) -> usize {
        crate::login::authenticated_providers().len()
    }

    /// Check if the current model's provider is using OAuth subscription (vs API key).
    pub(super) fn is_using_oauth_subscription(&self) -> bool {
        let provider = &self.session_setup.model.provider;
        // Check both the model provider and openai-codex alias.
        let check_providers: &[&str] = match provider.as_str() {
            "openai" => &["openai", "openai-codex"],
            _ => &[provider.as_str()],
        };
        for &p in check_providers {
            if let Some(source) = crate::login::auth_source(p) {
                if source == crate::login::AuthSource::BbAuth {
                    // Check if it's an OAuth entry (not API key).
                    if crate::login::is_oauth_entry(p) {
                        return true;
                    }
                }
            }
        }
        false
    }

    pub(super) fn rebuild_footer(&mut self) {
        self.ui.footer_data_provider
            .set_cwd(self.controller.runtime_host.cwd().to_path_buf());
        self.ui.footer_data_provider
            .set_available_provider_count(self.available_provider_count());

        let (input_tokens, output_tokens, cache_read, cache_write, cost) = self.footer_usage_totals();
        let context_usage = self.controller.runtime_host.runtime().get_context_usage();
        let context_percent = context_usage
            .as_ref()
            .and_then(|usage| usage.percent.map(|p| p as f64));
        let context_window = context_usage
            .as_ref()
            .map(|usage| usage.context_window as u64)
            .unwrap_or(self.session_setup.model.context_window);
        let session_row = store::get_session(&self.session_setup.conn, &self.session_setup.session_id)
            .ok()
            .flatten();

        let footer = Footer::new(FooterData {
            model_name: self.session_setup.model.id.clone(),
            provider: self.session_setup.model.provider.clone(),
            cwd: self.controller.runtime_host.cwd().display().to_string(),
            git_branch: self.ui.footer_data_provider.get_git_branch(),
            session_name: session_row.and_then(|row| row.name),
            input_tokens,
            output_tokens,
            cache_read,
            cache_write,
            cost,
            context_percent,
            context_window,
            auto_compact: true,
            thinking_level: if self.session_setup.model.reasoning {
                Some(self.session_setup.thinking_level.clone())
            } else {
                None
            },
            available_provider_count: self.ui.footer_data_provider.get_available_provider_count(),
            is_subscription: self.is_using_oauth_subscription(),
        });

        self.render_cache.footer_lines = footer.render(self.ui.tui.columns());
        Self::replace_container_lines(&self.ui.footer_container, &self.render_cache.footer_lines);
    }

    pub(super) fn render_widgets(&mut self) {
        // No extra spacing around editor — pi doesn't have it
        self.render_cache.widgets_above_lines = vec![];
        self.render_cache.widgets_below_lines = vec![];
        Self::replace_container_lines(&self.ui.widget_container_above, &self.render_cache.widgets_above_lines);
        Self::replace_container_lines(&self.ui.widget_container_below, &self.render_cache.widgets_below_lines);
    }

    pub(super) fn replace_container_lines(container: &Arc<Mutex<Container>>, lines: &[String]) {
        if let Ok(mut container) = container.lock() {
            container.clear();
            if lines.is_empty() {
                return;
            }
            container.add(Box::new(Text::new(&lines.join("\n"))));
        }
    }
}
