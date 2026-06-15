//! Minimal styled-text engine for the log viewer: parse ANSI (SGR) colors into
//! ratatui styles, overlay search highlights, and wrap styled runs to a width.
//! Self-contained — no extra dependencies.

use ratatui::style::{Color, Modifier, Style};

/// A run of text sharing one style.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub text: String,
    pub style: Style,
}

impl Run {
    fn new(text: String, style: Style) -> Self {
        Self { text, style }
    }
}

/// Remove ANSI escape sequences, returning plain text (used for export/search).
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            match chars.peek() {
                Some('[') => {
                    chars.next();
                    for c2 in chars.by_ref() {
                        if ('\u{40}'..='\u{7e}').contains(&c2) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    chars.next();
                    while let Some(&c2) = chars.peek() {
                        if c2 == '\u{07}' {
                            chars.next();
                            break;
                        }
                        if c2 == '\u{1b}' {
                            chars.next();
                            if chars.peek() == Some(&'\\') {
                                chars.next();
                            }
                            break;
                        }
                        chars.next();
                    }
                }
                _ => {
                    chars.next();
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Parse a string containing ANSI SGR codes into styled runs. `base` is the
/// starting style; SGR codes mutate it. Non-SGR escape sequences are skipped.
pub fn parse_ansi(s: &str, base: Style) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    let mut cur = String::new();
    let mut style = base;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            match chars.peek() {
                Some('[') => {
                    chars.next();
                    let mut params = String::new();
                    let mut final_byte = None;
                    for c2 in chars.by_ref() {
                        if ('\u{40}'..='\u{7e}').contains(&c2) {
                            final_byte = Some(c2);
                            break;
                        }
                        params.push(c2);
                    }
                    if final_byte == Some('m') {
                        if !cur.is_empty() {
                            runs.push(Run::new(std::mem::take(&mut cur), style));
                        }
                        style = apply_sgr(style, base, &params);
                    }
                }
                Some(']') => {
                    chars.next();
                    while let Some(&c2) = chars.peek() {
                        if c2 == '\u{07}' {
                            chars.next();
                            break;
                        }
                        if c2 == '\u{1b}' {
                            chars.next();
                            if chars.peek() == Some(&'\\') {
                                chars.next();
                            }
                            break;
                        }
                        chars.next();
                    }
                }
                _ => {
                    chars.next();
                }
            }
        } else {
            cur.push(c);
        }
    }
    if !cur.is_empty() {
        runs.push(Run::new(cur, style));
    }
    if runs.is_empty() {
        runs.push(Run::new(String::new(), base));
    }
    runs
}

fn apply_sgr(mut style: Style, base: Style, params: &str) -> Style {
    let codes: Vec<i64> = if params.is_empty() {
        vec![0]
    } else {
        params
            .split(';')
            .map(|p| p.parse::<i64>().unwrap_or(0))
            .collect()
    };
    let mut i = 0;
    while i < codes.len() {
        match codes[i] {
            0 => style = base,
            1 => style = style.add_modifier(Modifier::BOLD),
            2 => style = style.add_modifier(Modifier::DIM),
            3 => style = style.add_modifier(Modifier::ITALIC),
            4 => style = style.add_modifier(Modifier::UNDERLINED),
            22 => {
                style = style
                    .remove_modifier(Modifier::BOLD)
                    .remove_modifier(Modifier::DIM)
            }
            39 => style = style.fg(base.fg.unwrap_or(Color::Reset)),
            30..=37 => style = style.fg(basic_color(codes[i] - 30)),
            90..=97 => style = style.fg(bright_color(codes[i] - 90)),
            38 => {
                if i + 2 < codes.len() && codes[i + 1] == 5 {
                    style = style.fg(Color::Indexed(codes[i + 2] as u8));
                    i += 2;
                } else if i + 4 < codes.len() && codes[i + 1] == 2 {
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
    style
}

fn basic_color(n: i64) -> Color {
    match n {
        0 => Color::Black,
        1 => Color::Red,
        2 => Color::Green,
        3 => Color::Yellow,
        4 => Color::Blue,
        5 => Color::Magenta,
        6 => Color::Cyan,
        _ => Color::Gray,
    }
}

fn bright_color(n: i64) -> Color {
    match n {
        0 => Color::DarkGray,
        1 => Color::LightRed,
        2 => Color::LightGreen,
        3 => Color::LightYellow,
        4 => Color::LightBlue,
        5 => Color::LightMagenta,
        6 => Color::LightCyan,
        _ => Color::White,
    }
}

fn push_run(out: &mut Vec<Run>, chars: &[char], a: usize, b: usize, style: Style) {
    if b > a {
        out.push(Run::new(chars[a..b].iter().collect(), style));
    }
}

/// Overlay a highlight (ASCII case-insensitive) on every occurrence of `needle`
/// within the runs, by splitting runs at match boundaries. Matches are assumed
/// not to straddle a style boundary (true for typical log search terms).
pub fn highlight(runs: Vec<Run>, needle: &str, hl: Modifier) -> Vec<Run> {
    let needle: Vec<char> = needle.chars().map(|c| c.to_ascii_lowercase()).collect();
    if needle.is_empty() {
        return runs;
    }
    let m = needle.len();
    let mut out: Vec<Run> = Vec::new();
    for run in runs {
        let chars: Vec<char> = run.text.chars().collect();
        let lower: Vec<char> = chars.iter().map(|c| c.to_ascii_lowercase()).collect();
        let n = chars.len();
        let mut i = 0;
        let mut run_start = 0;
        while i < n {
            if i + m <= n && lower[i..i + m] == needle[..] {
                push_run(&mut out, &chars, run_start, i, run.style);
                push_run(&mut out, &chars, i, i + m, run.style.add_modifier(hl));
                i += m;
                run_start = i;
            } else {
                i += 1;
            }
        }
        push_run(&mut out, &chars, run_start, n, run.style);
    }
    out
}

/// Wrap styled runs to `width` columns (char-based), splitting runs as needed.
/// Always returns at least one row (possibly empty) so a blank line takes a row.
pub fn wrap_runs(runs: &[Run], width: usize) -> Vec<Vec<Run>> {
    let width = width.max(1);
    let mut rows: Vec<Vec<Run>> = Vec::new();
    let mut row: Vec<Run> = Vec::new();
    let mut col = 0usize;
    for run in runs {
        let mut buf = String::new();
        for ch in run.text.chars() {
            if col >= width {
                if !buf.is_empty() {
                    row.push(Run::new(std::mem::take(&mut buf), run.style));
                }
                rows.push(std::mem::take(&mut row));
                col = 0;
            }
            buf.push(ch);
            col += 1;
        }
        if !buf.is_empty() {
            row.push(Run::new(buf, run.style));
        }
    }
    if !row.is_empty() || rows.is_empty() {
        rows.push(row);
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(runs: &[Run]) -> String {
        runs.iter().map(|r| r.text.as_str()).collect()
    }

    #[test]
    fn parse_plain_is_single_run() {
        let runs = parse_ansi("hello", Style::default());
        assert_eq!(texts(&runs), "hello");
    }

    #[test]
    fn parse_strips_sgr_into_runs() {
        // red "ERR" then reset "ok"
        let s = "\u{1b}[31mERR\u{1b}[0mok";
        let runs = parse_ansi(s, Style::default());
        assert_eq!(texts(&runs), "ERRok");
        assert_eq!(runs[0].style.fg, Some(Color::Red));
    }

    #[test]
    fn strip_matches_parse_text() {
        let s = "\u{1b}[1;32mgreen\u{1b}[0m end";
        assert_eq!(strip_ansi(s), "green end");
    }

    #[test]
    fn highlight_splits_match() {
        let runs = vec![Run::new("find the ITEM here".into(), Style::default())];
        let out = highlight(runs, "item", Modifier::REVERSED);
        // reconstructed text is unchanged
        assert_eq!(texts(&out), "find the ITEM here");
        // one run carries the highlight modifier
        assert!(out
            .iter()
            .any(|r| r.text == "ITEM" && r.style.add_modifier.contains(Modifier::REVERSED)));
    }

    #[test]
    fn wrap_splits_on_width_preserving_runs() {
        let runs = vec![Run::new("abcdefg".into(), Style::default())];
        let rows = wrap_runs(&runs, 3);
        let joined: Vec<String> = rows.iter().map(|r| texts(r)).collect();
        assert_eq!(joined, vec!["abc", "def", "g"]);
    }

    #[test]
    fn wrap_empty_is_one_row() {
        let rows = wrap_runs(&[], 5);
        assert_eq!(rows.len(), 1);
    }
}
