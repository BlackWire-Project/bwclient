use ratatui::{
    Frame,
    text::{Line, Text},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::app::{App, ContactFormState, FormState, ToastKind, ToastMode};

use super::{centered_rect, toast_body_style, toast_border_style};

pub(crate) fn render_form_overlay(frame: &mut Frame<'_>, form: &FormState) {
    let popup = centered_rect(60, 40, frame.area());
    frame.render_widget(Clear, popup);
    let mut lines = Vec::new();
    for (idx, field) in form.fields.iter().enumerate() {
        let marker = if idx == form.index { ">" } else { " " };
        lines.push(Line::from(format!(
            "{marker} {}: {}",
            field.label, field.value
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from("Enter submit | Tab next field | Esc cancel"));

    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .block(Block::default().title(form.title).borders(Borders::ALL))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

pub(crate) fn render_contact_overlay(frame: &mut Frame<'_>, form: &ContactFormState) {
    let popup = centered_rect(60, 40, frame.area());
    frame.render_widget(Clear, popup);
    let mode = if form.use_username {
        "username"
    } else {
        "inbox_id"
    };
    frame.render_widget(
        Paragraph::new(Text::from(vec![
            Line::from(format!(
                "{} display_name: {}",
                if form.field_index == 0 { ">" } else { " " },
                form.display_name
            )),
            Line::from(format!(
                "{} {}: {}",
                if form.field_index == 1 { ">" } else { " " },
                mode,
                form.value
            )),
            Line::from(""),
            Line::from("F2 toggle username/inbox mode"),
            Line::from("Enter submit | Tab next field | Esc cancel"),
        ]))
        .block(Block::default().title("Add Contact").borders(Borders::ALL))
        .wrap(Wrap { trim: false }),
        popup,
    );
}

pub(crate) fn render_toast(app: &App, frame: &mut Frame<'_>) {
    let Some(toast) = &app.toast else {
        return;
    };

    let area = centered_rect(56, 22, frame.area());
    let title = match toast.kind {
        ToastKind::ApiError => "API Error",
        ToastKind::SyncError => "Sync Error",
    };
    let footer = match toast.mode {
        ToastMode::AutoDismiss => "This message will close automatically.",
        ToastMode::Sticky => "Press Esc to dismiss.",
    };
    let lines = vec![
        Line::from(toast.message.as_str()),
        Line::from(""),
        Line::from(footer),
    ];

    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(toast_border_style())
                    .style(toast_body_style()),
            )
            .wrap(Wrap { trim: false }),
        area,
    );
}
