//! Pure presentation formatting shared by wallet, messenger and history
//! views. No wallet state, no Slint types and no I/O.

pub fn amount(raw: &str) -> String {
    let raw = raw.trim();
    if raw.is_empty() {
        return "0.000000000".to_string();
    }
    if raw.contains('.') {
        return raw.to_string();
    }
    let (sign, digits) = match raw.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", raw),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return raw.to_string();
    }
    let value: u128 = match digits.parse() {
        Ok(value) => value,
        Err(_) => return raw.to_string(),
    };
    let whole = value / 1_000_000_000;
    let frac = value % 1_000_000_000;
    format!("{sign}{whole}.{frac:09}")
}

pub fn short_address(address: &str) -> String {
    let chars: Vec<char> = address.chars().collect();
    if chars.len() <= 24 {
        return address.to_string();
    }
    let head: String = chars.iter().take(12).collect();
    let tail: String = chars[chars.len() - 8..].iter().collect();
    format!("{head}…{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amounts_are_not_reinterpreted_twice() {
        assert_eq!(amount("1500000000"), "1.500000000");
        assert_eq!(amount("-1500000000"), "-1.500000000");
        assert_eq!(amount("1.5"), "1.5");
        assert_eq!(amount(""), "0.000000000");
    }

    #[test]
    fn short_address_keeps_head_and_tail() {
        assert_eq!(short_address("short"), "short");
        assert!(short_address("tgrin1abcdefghijklmnopqrstuvwxyz").contains('…'));
    }
}
