//! Presentation-only sanitizing for text that can originate outside the
//! desktop process: messenger peers, contacts, node/rpc errors and logs.
//!
//! Stored envelopes, canonical addresses, slatepacks and protocol fields are
//! never modified. These helpers only build a bounded, display-safe view.

const BIDI_CONTROLS: &[char] = &[
    '\u{061c}', '\u{200e}', '\u{200f}', '\u{202a}', '\u{202b}', '\u{202c}', '\u{202d}', '\u{202e}',
    '\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}',
];

fn clean(input: &str, keep_newlines: bool) -> String {
    let mut out = String::with_capacity(input.len().min(8192));
    let mut previous_space = false;
    for ch in input.chars() {
        if BIDI_CONTROLS.contains(&ch) || ch == '\u{200b}' || ch == '\u{feff}' {
            continue;
        }
        if ch == '\n' || ch == '\r' {
            if keep_newlines {
                while out.ends_with(' ') {
                    out.pop();
                }
                if !out.ends_with('\n') {
                    out.push('\n');
                }
            } else if !previous_space {
                out.push(' ');
                previous_space = true;
            }
            continue;
        }
        if ch.is_control() || ch == '\t' {
            if !previous_space {
                out.push(' ');
                previous_space = true;
            }
            continue;
        }
        out.push(ch);
        previous_space = ch == ' ';
    }
    out.trim().to_string()
}

fn truncate(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_string();
    }
    let mut out: String = input.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// One-line labels: contacts, node phase, board names, protocol strings.
pub fn single_line(input: &str, max_chars: usize) -> String {
    truncate(&clean(input, false), max_chars)
}

/// Multi-line peer messages. The stored message remains untouched.
pub fn message(input: &str, max_chars: usize) -> String {
    truncate(&clean(input, true), max_chars)
}

/// Logs and diagnostics shown in the UI must never inject another line.
pub fn log_line(input: &str) -> String {
    truncate(&clean(input, false), 512)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_bidi_and_controls_but_keeps_slatepack_text() {
        let raw = "abc\u{202e}def\u{0007}\nline";
        assert_eq!(single_line(raw, 80), "abcdef line");
        assert_eq!(message(raw, 80), "abcdef\nline");
        assert_eq!(log_line(raw), "abcdef line");
    }

    #[test]
    fn truncates_by_display_characters_and_keeps_original_outside() {
        let raw = "BEGINSLATEPACK. payload ENDSLATEPACK.";
        assert_eq!(message(raw, 10).chars().count(), 10);
        assert_eq!(raw, "BEGINSLATEPACK. payload ENDSLATEPACK.");
    }
}
