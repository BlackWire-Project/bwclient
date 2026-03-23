use super::*;

impl App {
    pub(crate) fn selected_server_record(&self) -> Option<&ServerRecord> {
        self.servers.get(self.selected_server)
    }

    pub(crate) fn selected_profile_record(&self) -> Option<&LocalProfileRecord> {
        self.profiles.get(self.selected_profile)
    }

    pub(crate) fn current_conversation(&self) -> Option<&(ConversationRecord, ContactRecord)> {
        self.conversations.get(self.selected_conversation)
    }

    pub(crate) fn show_api_error_toast(&mut self, message: String, mode: ToastMode) {
        self.status = message.clone();
        self.toast = Some(ToastState {
            kind: ToastKind::ApiError,
            message: friendly_error_message(&message),
            mode,
            expires_at: toast_expiration(mode),
        });
    }

    pub(crate) fn show_sync_error_toast(&mut self, message: String, mode: ToastMode) {
        self.status = message.clone();
        self.toast = Some(ToastState {
            kind: ToastKind::SyncError,
            message: friendly_error_message(&message),
            mode,
            expires_at: toast_expiration(mode),
        });
    }

    pub(crate) fn dismiss_toast(&mut self) -> bool {
        if self.toast.is_some() {
            self.toast = None;
            return true;
        }
        false
    }

    pub(crate) fn expire_toast_if_needed(&mut self) {
        let should_clear = self
            .toast
            .as_ref()
            .and_then(|toast| toast.expires_at)
            .map(|expires_at| Instant::now() >= expires_at)
            .unwrap_or(false);
        if should_clear {
            self.toast = None;
        }
    }

    pub(crate) fn message_lines(&self) -> Vec<Line<'static>> {
        if self.messages.is_empty() {
            return vec![Line::from("No messages yet")];
        }

        self.messages
            .iter()
            .map(|message| {
                let incoming = match message.direction {
                    MessageDirection::Incoming => true,
                    MessageDirection::Outgoing => false,
                };
                let prefix = if incoming { "<" } else { ">" };
                let body = message.body.as_deref().unwrap_or("<raw envelope>");
                Line::from(vec![
                    Span::styled(
                        format!("[{}] ", message.created_at),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(
                        prefix,
                        if incoming {
                            Style::default().fg(Color::Blue)
                        } else {
                            Style::default().fg(Color::LightGreen)
                        },
                    ),
                    Span::raw(format!(" {}", body)),
                    Span::styled(
                        message
                            .error_reason
                            .as_ref()
                            .map(|error| format!(" ({error})"))
                            .unwrap_or_default(),
                        Style::default().fg(Color::Yellow),
                    ),
                ])
            })
            .collect()
    }

    pub(crate) fn max_message_scroll(&self, area: Rect, message_lines: &[Line<'static>]) -> u16 {
        let visible_height = ui::message_viewport_height(area);
        if visible_height == 0 {
            return 0;
        }

        let total_height = message_lines
            .iter()
            .map(|line| ui::wrapped_line_height(line, area.width.saturating_sub(2)))
            .sum::<u16>();

        total_height.saturating_sub(visible_height)
    }

    pub(crate) fn scroll_messages_page(&mut self, direction: i32) {
        let page = ui::message_viewport_height(self.current_message_area()).max(1);
        if direction < 0 {
            self.message_auto_follow = false;
            self.message_scroll = self.message_scroll.saturating_sub(page);
        } else {
            self.message_scroll = self.message_scroll.saturating_add(page);
        }
        self.normalize_message_scroll();
    }

    pub(crate) fn scroll_messages_to_top(&mut self) {
        self.message_auto_follow = false;
        self.message_scroll = 0;
    }

    pub(crate) fn scroll_messages_to_bottom(&mut self) {
        self.message_auto_follow = true;
        self.message_scroll = u16::MAX;
        self.normalize_message_scroll();
    }

    pub(crate) fn normalize_message_scroll(&mut self) {
        let area = self.current_message_area();
        let lines = self.message_lines();
        let max_scroll = self.max_message_scroll(area, &lines);

        if self.message_auto_follow || self.message_scroll >= max_scroll {
            self.message_auto_follow = true;
            self.message_scroll = max_scroll;
        } else {
            self.message_scroll = self.message_scroll.min(max_scroll);
        }
    }

    pub(crate) fn current_message_area(&self) -> Rect {
        let Ok((width, height)) = crossterm::terminal::size() else {
            return Rect::default();
        };

        let area = Rect::new(0, 0, width, height);
        let vertical = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(8),
                Constraint::Length(4),
                Constraint::Length(3),
            ])
            .split(area);
        let constraints = if self.show_technical {
            vec![
                Constraint::Percentage(24),
                Constraint::Percentage(48),
                Constraint::Percentage(28),
            ]
        } else {
            vec![Constraint::Percentage(28), Constraint::Percentage(72)]
        };
        let horizontal = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(constraints)
            .split(vertical[0]);

        horizontal.get(1).copied().unwrap_or_default()
    }

    pub(crate) fn focus_style(&self, focus: LoginFocus) -> Style {
        if self.login_focus == focus {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        }
    }
}

pub(crate) fn shorten(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    let shortened = value.chars().take(max).collect::<String>();
    format!("{shortened}...")
}

pub(crate) fn toast_expiration(mode: ToastMode) -> Option<Instant> {
    match mode {
        ToastMode::AutoDismiss => Some(Instant::now() + Duration::from_secs(5)),
        ToastMode::Sticky => None,
    }
}

pub(crate) fn friendly_error_message(message: &str) -> String {
    let lower = message.to_ascii_lowercase();
    if lower.contains("already exists") || lower.contains("username already exists") {
        return "Username already exists on this relay.".to_string();
    }
    if lower.contains("failed to call relay health endpoint") {
        return "Could not reach the relay health endpoint.".to_string();
    }
    if lower.contains("failed to call post /users") {
        return "Profile registration request failed.".to_string();
    }
    if lower.contains("failed to fetch bundle") {
        return "Could not fetch the contact bundle from the relay.".to_string();
    }
    if lower.contains("failed to call post /messages") {
        return "Message send request failed.".to_string();
    }
    if lower.contains("poll failed:") {
        return message.replacen("poll failed:", "Relay polling failed:", 1);
    }
    message.to_string()
}
