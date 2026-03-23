use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Text},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::{
    app::{App, InputMode},
    crypto::available_prekeys,
};

use super::{message_viewport_height, selection_style};

pub(crate) fn render_main(app: &App, frame: &mut Frame<'_>) {
    let area = frame.area();
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(8),
            Constraint::Length(4),
            Constraint::Length(3),
        ])
        .split(area);
    let constraints = if app.show_technical {
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

    let conversation_items = if app.conversations.is_empty() {
        vec![ListItem::new("No conversations yet")]
    } else {
        app.conversations
            .iter()
            .map(|(conversation, contact)| {
                let label = match (&contact.username, &contact.inbox_id) {
                    (Some(username), _) => username.clone(),
                    (None, Some(inbox)) => {
                        format!("inbox: {}", crate::app::view::shorten(inbox, 18))
                    }
                    (None, None) => contact.display_name.clone(),
                };
                let prefix = if app.unread_conversations.contains(&conversation.id) {
                    "* "
                } else {
                    ""
                };
                ListItem::new(format!("{prefix}{label}"))
            })
            .collect()
    };
    let mut conversation_state = ListState::default();
    if !app.conversations.is_empty() {
        conversation_state.select(Some(
            app.selected_conversation
                .min(app.conversations.len().saturating_sub(1)),
        ));
    }
    frame.render_stateful_widget(
        List::new(conversation_items)
            .block(
                Block::default()
                    .title("Conversations")
                    .borders(Borders::ALL),
            )
            .highlight_style(selection_style())
            .highlight_symbol(">> "),
        horizontal[0],
        &mut conversation_state,
    );

    let message_lines = app.message_lines();
    let message_viewport_height = message_viewport_height(horizontal[1]);
    let max_scroll = app.max_message_scroll(horizontal[1], &message_lines);
    let message_scroll = if app.message_auto_follow {
        max_scroll
    } else {
        app.message_scroll.min(max_scroll)
    };
    frame.render_widget(
        Paragraph::new(Text::from(message_lines))
            .block(
                Block::default()
                    .title(format!(
                        "Messages [PgUp/PgDn/Home/End] ({message_viewport_height} lines)"
                    ))
                    .borders(Borders::ALL),
            )
            .wrap(Wrap { trim: false })
            .scroll((message_scroll, 0)),
        horizontal[1],
    );

    if app.show_technical {
        frame.render_widget(technical_panel(app), horizontal[2]);
    }

    frame.render_widget(
        Paragraph::new(app.composer.as_str())
            .block(
                Block::default()
                    .title(match app.input_mode {
                        InputMode::Command => "Composer [Command]",
                        InputMode::Compose => "Composer [Compose]",
                    })
                    .borders(Borders::ALL)
                    .border_style(match app.input_mode {
                        InputMode::Command => Style::default(),
                        InputMode::Compose => Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    }),
            )
            .wrap(Wrap { trim: false }),
        vertical[1],
    );

    let status = Paragraph::new(Text::from(vec![
        Line::from(
            "F1 add contact | F2 tech | F5 sync | F6 logout | F9 compose | PgUp/PgDn scroll | q quit",
        ),
        Line::from(format!(
            "poll={} stored={} unresolved={}",
            app.last_poll_count, app.last_ingest_stored, app.last_ingest_unresolved
        )),
        Line::from(app.status.as_str()),
    ]))
    .block(Block::default().title("Status").borders(Borders::ALL))
    .wrap(Wrap { trim: true });
    frame.render_widget(status, vertical[2]);
}

fn technical_panel(app: &App) -> Paragraph<'_> {
    let mut lines = Vec::new();
    if let Some(server) = &app.active_server {
        lines.push(Line::from(format!("server: {}", server.base_url)));
    }
    if let Some(profile) = &app.active_profile {
        lines.push(Line::from(format!("profile: {}", profile.username)));
        lines.push(Line::from(format!("inbox: {}", profile.inbox_id)));
        lines.push(Line::from(format!(
            "local prekeys: {}",
            available_prekeys(&profile.keys)
        )));
    }
    if let Some((conversation, contact)) = app.current_conversation() {
        lines.push(Line::from(""));
        lines.push(Line::from(format!("contact: {}", contact.display_name)));
        if let Some(username) = &contact.username {
            lines.push(Line::from(format!("username: {username}")));
        }
        if let Some(inbox_id) = &contact.inbox_id {
            lines.push(Line::from(format!("remote inbox: {inbox_id}")));
        }
        if let Some(session) = &conversation.session {
            lines.push(Line::from(format!("session: {}", session.session_id)));
            lines.push(Line::from(format!("send_count: {}", session.send_count)));
            lines.push(Line::from(format!("recv_count: {}", session.receive_count)));
            lines.push(Line::from(format!(
                "await_send_ratchet: {}",
                session.pending_send_ratchet
            )));
        } else {
            lines.push(Line::from("session: none"));
        }
    }
    if let Some(message) = app.messages.last() {
        lines.push(Line::from(""));
        lines.push(Line::from(format!("last kind: {}", message.relay_kind)));
        lines.push(Line::from(format!(
            "last header: {}",
            crate::app::view::shorten(&message.header, 120)
        )));
        if let Some(error_reason) = &message.error_reason {
            lines.push(Line::from(format!("last error: {error_reason}")));
        }
    }
    if let Some(error) = &app.last_receive_error {
        lines.push(Line::from(""));
        lines.push(Line::from(format!("receive error: {error}")));
    }
    lines.push(Line::from(format!(
        "last poll/stored/unresolved: {}/{}/{}",
        app.last_poll_count, app.last_ingest_stored, app.last_ingest_unresolved
    )));

    Paragraph::new(Text::from(lines))
        .block(Block::default().title("Technical").borders(Borders::ALL))
        .wrap(Wrap { trim: true })
}
