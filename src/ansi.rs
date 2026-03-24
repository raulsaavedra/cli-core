//! Parse ANSI-styled strings into ratatui `Line`/`Span` objects.
//!
//! cli-core's markdown renderer outputs strings with embedded ANSI escape codes.
//! Ratatui does not interpret raw ANSI — it needs styled `Span` objects.
//! This module bridges the gap.

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

/// Parse a string containing ANSI escape codes into a ratatui `Line`.
pub fn parse_line(s: &str) -> Line<'static> {
    let spans = parse_spans(s);
    Line::from(spans)
}

/// Parse multiple lines of ANSI-styled text into ratatui `Line` objects.
pub fn parse_lines(lines: &[String]) -> Vec<Line<'static>> {
    lines.iter().map(|s| parse_line(s)).collect()
}

/// Parse a string containing ANSI escape codes into a vec of styled `Span`s.
fn parse_spans(s: &str) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut current_style = Style::default();
    let mut current_text = String::new();
    let mut chars = s.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            if chars.peek() == Some(&'[') {
                // Flush current text
                if !current_text.is_empty() {
                    spans.push(Span::styled(
                        std::mem::take(&mut current_text),
                        current_style,
                    ));
                }
                chars.next(); // consume '['

                // Collect the parameter string until 'm'
                let mut params = String::new();
                while let Some(&c) = chars.peek() {
                    if c == 'm' {
                        chars.next();
                        break;
                    }
                    params.push(c);
                    chars.next();
                }

                // Apply SGR parameters to current style
                current_style = apply_sgr(&params, current_style);
            } else {
                current_text.push(ch);
            }
        } else {
            current_text.push(ch);
        }
    }

    // Flush remaining text
    if !current_text.is_empty() {
        spans.push(Span::styled(current_text, current_style));
    }

    if spans.is_empty() {
        spans.push(Span::raw(""));
    }

    spans
}

/// Apply SGR (Select Graphic Rendition) parameters to a style.
fn apply_sgr(params: &str, mut style: Style) -> Style {
    if params.is_empty() || params == "0" {
        return Style::default();
    }

    let codes: Vec<u16> = params
        .split(';')
        .filter_map(|s| s.parse::<u16>().ok())
        .collect();

    let mut i = 0;
    while i < codes.len() {
        match codes[i] {
            0 => style = Style::default(),
            1 => style = style.add_modifier(Modifier::BOLD),
            2 => style = style.add_modifier(Modifier::DIM),
            3 => style = style.add_modifier(Modifier::ITALIC),
            4 => style = style.add_modifier(Modifier::UNDERLINED),
            7 => style = style.add_modifier(Modifier::REVERSED),
            9 => style = style.add_modifier(Modifier::CROSSED_OUT),
            22 => style = style.remove_modifier(Modifier::BOLD | Modifier::DIM),
            23 => style = style.remove_modifier(Modifier::ITALIC),
            24 => style = style.remove_modifier(Modifier::UNDERLINED),
            27 => style = style.remove_modifier(Modifier::REVERSED),
            29 => style = style.remove_modifier(Modifier::CROSSED_OUT),
            // Standard foreground colors
            30 => style = style.fg(Color::Black),
            31 => style = style.fg(Color::Red),
            32 => style = style.fg(Color::Green),
            33 => style = style.fg(Color::Yellow),
            34 => style = style.fg(Color::Blue),
            35 => style = style.fg(Color::Magenta),
            36 => style = style.fg(Color::Cyan),
            37 => style = style.fg(Color::White),
            // Extended foreground: 38;5;n (256-color) or 38;2;r;g;b (truecolor)
            38 => {
                if i + 1 < codes.len() {
                    match codes[i + 1] {
                        5 => {
                            // 256-color: 38;5;n
                            if i + 2 < codes.len() {
                                style = style.fg(Color::Indexed(codes[i + 2] as u8));
                                i += 2;
                            }
                        }
                        2 => {
                            // Truecolor: 38;2;r;g;b
                            if i + 4 < codes.len() {
                                style = style.fg(Color::Rgb(
                                    codes[i + 2] as u8,
                                    codes[i + 3] as u8,
                                    codes[i + 4] as u8,
                                ));
                                i += 4;
                            }
                        }
                        _ => {}
                    }
                    i += 1;
                }
            }
            39 => style = style.fg(Color::Reset),
            // Standard background colors
            40 => style = style.bg(Color::Black),
            41 => style = style.bg(Color::Red),
            42 => style = style.bg(Color::Green),
            43 => style = style.bg(Color::Yellow),
            44 => style = style.bg(Color::Blue),
            45 => style = style.bg(Color::Magenta),
            46 => style = style.bg(Color::Cyan),
            47 => style = style.bg(Color::White),
            // Extended background: 48;5;n or 48;2;r;g;b
            48 => {
                if i + 1 < codes.len() {
                    match codes[i + 1] {
                        5 => {
                            if i + 2 < codes.len() {
                                style = style.bg(Color::Indexed(codes[i + 2] as u8));
                                i += 2;
                            }
                        }
                        2 => {
                            if i + 4 < codes.len() {
                                style = style.bg(Color::Rgb(
                                    codes[i + 2] as u8,
                                    codes[i + 3] as u8,
                                    codes[i + 4] as u8,
                                ));
                                i += 4;
                            }
                        }
                        _ => {}
                    }
                    i += 1;
                }
            }
            49 => style = style.bg(Color::Reset),
            // Bright foreground colors
            90 => style = style.fg(Color::DarkGray),
            91 => style = style.fg(Color::LightRed),
            92 => style = style.fg(Color::LightGreen),
            93 => style = style.fg(Color::LightYellow),
            94 => style = style.fg(Color::LightBlue),
            95 => style = style.fg(Color::LightMagenta),
            96 => style = style.fg(Color::LightCyan),
            97 => style = style.fg(Color::White),
            _ => {} // Unknown code — ignore
        }
        i += 1;
    }

    style
}
