use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use std::time::Instant;

pub(crate) fn banner() -> Vec<Line<'static>> {
    let style = Style::default()
        .fg(Color::Rgb(216, 220, 226))
        .add_modifier(Modifier::BOLD);

    vec![
        Line::from(Span::styled(
            "  ____    _            _    __        ___           ",
            style,
        )),
        Line::from(Span::styled(
            " | __ )  | | __ _  ___| | __\\ \\      / (_)_ __ ___  ",
            style,
        )),
        Line::from(Span::styled(
            " |  _ \\  | |/ _` |/ __| |/ / \\ \\ /\\ / /| | '__/ _ \\ ",
            style,
        )),
        Line::from(Span::styled(
            " | |_) | | | (_| | (__|   <   \\ V  V / | | | |  __/ ",
            style,
        )),
        Line::from(Span::styled(
            " |____/  |_|\\__,_|\\___|_|\\_\\   \\_/\\_/  |_|_|  \\___| ",
            style,
        )),
        Line::from(Span::styled(
            "                                                      ",
            style,
        )),
    ]
}

#[allow(dead_code)]
pub(crate) fn left_cable(phase: usize) -> Vec<Line<'static>> {
    let idle = Style::default().fg(Color::Rgb(76, 84, 98));
    let glow = Style::default()
        .fg(Color::Rgb(110, 214, 255))
        .add_modifier(Modifier::BOLD);
    let hot = Style::default()
        .fg(Color::Rgb(194, 241, 255))
        .add_modifier(Modifier::BOLD);

    (0..6)
        .map(|row| {
            let pulse = (phase + row) % 18;
            let trail = pulse.saturating_sub(1);
            let mut spans = Vec::new();
            for idx in 0..18 {
                let style = if idx == pulse {
                    hot
                } else if idx == trail {
                    glow
                } else {
                    idle
                };
                spans.push(Span::styled("=", style));
            }
            Line::from(spans)
        })
        .collect()
}

fn lcg(seed: &mut u64) -> u32 {
    *seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
    (*seed >> 16) as u32
}

fn hash_str_to_u64(s: &str) -> u64 {
    let mut hash: u64 = 5381;
    for c in s.chars() {
        hash = hash.wrapping_mul(33).wrapping_add(c as u64);
    }
    hash
}

pub(crate) fn animated_hero(
    width: u16,
    elapsed_ms: u128,
    started_at: Instant,
) -> Vec<Line<'static>> {
    let cable_idle = Style::default().fg(Color::Rgb(76, 84, 98));
    let cable_glow = Style::default()
        .fg(Color::Rgb(110, 214, 255))
        .add_modifier(Modifier::BOLD);
    let cable_hot = Style::default()
        .fg(Color::Rgb(194, 241, 255))
        .add_modifier(Modifier::BOLD);

    let all_banner_lines = banner();
    let banner_lines = &all_banner_lines[0..5];
    let banner_height = banner_lines.len() as u16;
    let banner_width = banner_lines[0].width() as u16;

    let total_height = banner_height;
    let banner_x_offset = (width.saturating_sub(banner_width)) / 2;

    let pulse_speed_divisor = 100;
    let anim_loop_distance = (width + 10) as u128;
    let phase = (elapsed_ms / pulse_speed_divisor) % anim_loop_distance;

    const NUM_PULSES: u128 = 20;
    let mut phase_shifts = [0; NUM_PULSES as usize];

    let mut seed = hash_str_to_u64(&format!("{:?}", started_at));

    for i in 0..NUM_PULSES {
        let random_offset = lcg(&mut seed) as u128;
        phase_shifts[i as usize] = random_offset % anim_loop_distance;
    }

    let mut final_lines = Vec::with_capacity(total_height as usize);

    for y in 0..total_height {
        let mut spans = Vec::with_capacity(width as usize);
        let banner_line = &banner_lines[y as usize];

        let mut row_pulse_positions = Vec::new();
        for i in 0..NUM_PULSES {
            if i % (banner_height as u128) == (y as u128) {
                let pulse_phase = (phase + phase_shifts[i as usize]) % anim_loop_distance;
                let pos = anim_loop_distance.saturating_sub(pulse_phase);
                row_pulse_positions.push(pos);
            }
        }

        for x in 0..width {
            let banner_char_idx = x.saturating_sub(banner_x_offset);
            let is_in_banner_hitbox = x >= banner_x_offset && banner_char_idx < banner_width;

            if is_in_banner_hitbox {
                let span = &banner_line.spans[0];
                let ch = span
                    .content
                    .chars()
                    .nth(banner_char_idx as usize)
                    .unwrap_or(' ');
                spans.push(Span::styled(ch.to_string(), span.style));
            } else {
                let x_pos = x as u128;
                let is_hot = row_pulse_positions.iter().any(|&pos| pos == x_pos);
                let is_glow = row_pulse_positions
                    .iter()
                    .any(|&pos| x_pos == pos.saturating_sub(1) || x_pos == pos.saturating_add(1));

                let style = if is_hot {
                    cable_hot
                } else if is_glow {
                    cable_glow
                } else {
                    cable_idle
                };
                spans.push(Span::styled("=", style));
            }
        }
        final_lines.push(Line::from(spans));
    }

    final_lines
}
