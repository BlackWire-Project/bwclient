use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Text},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::app::{App, LoginFocus};

use super::{branding, selection_style};

pub(crate) fn render_login(app: &App, frame: &mut Frame<'_>) {
    let area = frame.area();
    let elapsed_ms = app.started_at.elapsed().as_millis();

    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7),
            Constraint::Length(2),
            Constraint::Min(10),
            Constraint::Length(4),
        ])
        .split(area);

    let hero_block = Block::default()
        .title("BlackWire Client")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(249, 128, 41)))
        .style(Style::default().bg(Color::Rgb(16, 18, 24)));

    let hero_inner_area = hero_block.inner(vertical[0]);
    frame.render_widget(hero_block, vertical[0]);

    let hero_content = branding::animated_hero(hero_inner_area.width, elapsed_ms, app.started_at);
    frame.render_widget(Paragraph::new(hero_content), hero_inner_area);

    let subtitle_text =
        "A relay-backed terminal client with local profiles and encrypted sessions.";
    frame.render_widget(
        Paragraph::new(subtitle_text)
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::Rgb(149, 158, 173)))
            .wrap(Wrap { trim: true }),
        vertical[1],
    );

    let main_content_area = vertical[2];
    let split = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
        .split(main_content_area);

    let server_items = if app.servers.is_empty() {
        vec![ListItem::new("No servers configured")]
    } else {
        app.servers
            .iter()
            .map(|server| ListItem::new(format!("{}  {}", server.name, server.base_url)))
            .collect()
    };
    let profile_items = if app.profiles.is_empty() {
        vec![ListItem::new("No local profiles for this server")]
    } else {
        app.profiles
            .iter()
            .map(|profile| {
                let suffix = if profile.registered {
                    ""
                } else {
                    " (not registered)"
                };
                ListItem::new(format!("{}{}", profile.username, suffix))
            })
            .collect()
    };

    let mut server_state = ListState::default();
    if !app.servers.is_empty() {
        server_state.select(Some(app.selected_server.min(app.servers.len() - 1)));
    }
    let mut profile_state = ListState::default();
    if !app.profiles.is_empty() {
        profile_state.select(Some(app.selected_profile.min(app.profiles.len() - 1)));
    }

    frame.render_stateful_widget(
        List::new(server_items)
            .block(
                Block::default()
                    .title("Servers")
                    .borders(Borders::ALL)
                    .border_style(app.focus_style(LoginFocus::Servers)),
            )
            .highlight_style(selection_style())
            .highlight_symbol(">> "),
        split[0],
        &mut server_state,
    );
    frame.render_stateful_widget(
        List::new(profile_items)
            .block(
                Block::default()
                    .title("Profiles")
                    .borders(Borders::ALL)
                    .border_style(app.focus_style(LoginFocus::Profiles)),
            )
            .highlight_style(selection_style())
            .highlight_symbol(">> "),
        split[1],
        &mut profile_state,
    );

    let status = Paragraph::new(Text::from(vec![
        Line::from("Keys: a add server | n new profile | Tab switch | Enter login | q quit"),
        Line::from(
            "Tip: use different working directories if you want isolated bwclient.db files.",
        ),
        Line::from(app.status.as_str()),
    ]))
    .block(
        Block::default()
            .title("Status")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Rgb(84, 94, 110))),
    )
    .wrap(Wrap { trim: true })
    .style(Style::default().fg(Color::Rgb(84, 94, 110)));
    frame.render_widget(status, vertical[3]);
}
