//! Runtime translation for strings that are built in Rust (core/runtime
//! errors, node/scan status, formatted amounts). Slint's `@tr` covers static
//! UI literals; this module covers everything that cannot be a literal in
//! `.slint` and is chosen at run time.
//!
//! The catalog is intentionally a plain table: exact matches first, then
//! templates where `{}`/`{name}` placeholders match any text. Russian is the
//! fallback (and the default language); an English miss returns the Russian
//! text rather than an empty or invented string.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

static ENGLISH: AtomicBool = AtomicBool::new(false);
static MISSING: AtomicUsize = AtomicUsize::new(0);

pub const FALLBACK: &str = "Operation failed; details are in the log";

pub fn set_english(english: bool) {
    ENGLISH.store(english, Ordering::SeqCst);
}

pub fn is_english() -> bool {
    ENGLISH.load(Ordering::SeqCst)
}

/// Translate a final string. Exact catalog entries win; template entries are
/// tried in order. An unknown Russian string becomes an English fallback (and
/// is logged), so the English UI never shows untranslated Russian.
pub fn tr(text: &str) -> String {
    if !is_english() || text.is_empty() {
        return text.to_string();
    }
    for (ru, en) in CATALOG {
        if !ru.contains('{') {
            if *ru == text {
                return (*en).to_string();
            }
        }
    }
    for (ru, en) in CATALOG {
        if ru.contains('{') {
            if let Some(captures) = capture(ru, text) {
                return fill(en, &captures);
            }
        }
    }
    if has_cyrillic(text) {
        // No Russian leaks into the English UI. The original message is kept in
        // the log for diagnosis; the generic fallback is what the user sees.
        MISSING.fetch_add(1, Ordering::Relaxed);
        eprintln!("i18n: missing English translation: {text}");
        return FALLBACK.to_string();
    }
    text.to_string()
}

pub fn missing_translations() -> usize {
    MISSING.load(Ordering::Relaxed)
}

fn has_cyrillic(text: &str) -> bool {
    text.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c))
}

/// Translate a message that may itself contain a nested error: the outer
/// template and the captured error are both run through the catalog.
pub fn tr_with(text: &str, nested: &str) -> String {
    if !is_english() {
        return replace_first_placeholder(text, nested);
    }
    let translated = tr(text);
    replace_first_placeholder(&translated, &tr(nested))
}

fn replace_first_placeholder(template: &str, value: &str) -> String {
    let Some(start) = template.find('{') else {
        return template.to_string();
    };
    let Some(end) = template[start..].find('}') else {
        return template.to_string();
    };
    let mut output = String::with_capacity(template.len() + value.len());
    output.push_str(&template[..start]);
    output.push_str(value);
    output.push_str(&template[start + end + 1..]);
    output
}

fn capture(pattern: &str, text: &str) -> Option<Vec<String>> {
    let tokens = tokens(pattern);
    let mut rest = text;
    let mut captures = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        match token {
            Token::Literal(literal) => {
                if !rest.starts_with(literal) {
                    return None;
                }
                rest = &rest[literal.len()..];
            }
            Token::Placeholder => {
                let next = tokens[index + 1..].iter().find_map(|token| match token {
                    Token::Literal(literal) => Some(*literal),
                    Token::Placeholder => None,
                });
                match next {
                    Some(literal) => {
                        let position = rest.find(literal)?;
                        captures.push(rest[..position].to_string());
                        rest = &rest[position..];
                    }
                    None => {
                        captures.push(rest.to_string());
                        rest = "";
                    }
                }
            }
        }
    }
    if rest.is_empty() {
        Some(captures)
    } else {
        None
    }
}

fn fill(template: &str, captures: &[String]) -> String {
    let mut output = String::new();
    let mut captures = captures.iter();
    let mut chars = template.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '{' {
            let mut placeholder = String::new();
            for inner in chars.by_ref() {
                if inner == '}' {
                    break;
                }
                placeholder.push(inner);
            }
            if let Some(value) = captures.next() {
                output.push_str(value);
            }
        } else {
            output.push(ch);
        }
    }
    output
}

enum Token<'a> {
    Literal(&'a str),
    Placeholder,
}

fn tokens(pattern: &str) -> Vec<Token<'_>> {
    let mut out = Vec::new();
    let mut start = 0;
    let bytes = pattern.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'{' {
            if start < index {
                out.push(Token::Literal(&pattern[start..index]));
            }
            while index < bytes.len() && bytes[index] != b'}' {
                index += 1;
            }
            out.push(Token::Placeholder);
            index += 1;
            start = index;
        } else {
            index += 1;
        }
    }
    if start < pattern.len() {
        out.push(Token::Literal(&pattern[start..]));
    }
    if out.is_empty() {
        out.push(Token::Literal(""));
    }
    out
}

/// Catalog of Rust-built strings. Russian is the key; English is the value.
/// Keep placeholders in the same order in both strings.
include!("i18n_catalog.rs");

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn exact_and_template_translation() {
        let _guard = LOCK.lock().unwrap();
        set_english(true);
        assert_eq!(tr("кошелёк открыт"), "wallet unlocked");
        assert_eq!(tr("скан: 3 / 10"), "scan: 3 / 10");
        let before = missing_translations();
        assert_eq!(tr("неизвестное сообщение"), FALLBACK);
        assert!(missing_translations() > before);
        set_english(false);
        assert_eq!(tr("кошелёк открыт"), "кошелёк открыт");
    }

    #[test]
    fn english_never_leaks_cyrillic() {
        let _guard = LOCK.lock().unwrap();
        set_english(true);
        for text in [
            "совсем неизвестная строка",
            "ошибка без перевода: 42",
            "нода не запущена",
        ] {
            let translated = tr(text);
            assert!(
                !has_cyrillic(&translated),
                "translation leaked: {translated}"
            );
        }
        set_english(false);
    }

    #[test]
    fn core_error_is_translated() {
        let _guard = LOCK.lock().unwrap();
        set_english(true);
        assert_eq!(
            tr("узел недоступен: 127.0.0.1:3413"),
            "node unreachable: 127.0.0.1:3413"
        );
        set_english(false);
    }

    #[test]
    fn catalog_has_no_duplicate_keys_and_matching_placeholders() {
        let mut seen = std::collections::HashSet::new();
        for (ru, en) in CATALOG {
            assert!(seen.insert(*ru), "duplicate Russian catalog key: {ru}");
            let named = |s: &str| {
                let mut names = Vec::new();
                let mut rest = s;
                while let Some(start) = rest.find('{') {
                    let Some(end) = rest[start..].find('}') else {
                        break;
                    };
                    let name = &rest[start + 1..start + end];
                    if !name.is_empty() {
                        names.push(name.to_string());
                    }
                    rest = &rest[start + end + 1..];
                }
                names.sort_unstable();
                names
            };
            assert_eq!(
                named(ru),
                named(en),
                "placeholder mismatch for catalog key: {ru}"
            );
            assert_eq!(
                ru.matches("{}").count(),
                en.matches("{}").count(),
                "positional placeholder mismatch for catalog key: {ru}"
            );
        }
    }

    #[test]
    fn nested_error_is_translated() {
        let _guard = LOCK.lock().unwrap();
        set_english(true);
        assert_eq!(tr("скан: {e}"), "scan: {e}");
        assert_eq!(
            tr_with("скан: {e}", "нода не запущена"),
            "scan: node is not running"
        );
        set_english(false);
    }
}
