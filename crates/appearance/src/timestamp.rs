//! The small timestamp format language shared by the CLI and the web client.
//!
//! This is deliberately *not* `strftime`: users write `HH:mm:ss`, `DD MMM
//! YYYY`, and so on, and each frontend renders it with its own clock API. The
//! token set is intentionally tiny so both frontends can agree on it:
//!
//! | Token | Renders | strftime |
//! | --- | --- | --- |
//! | `YYYY` | 4-digit year | `%Y` |
//! | `MM` | 2-digit month | `%m` |
//! | `DD` | 2-digit day | `%d` |
//! | `HH` | 2-digit hour (24h) | `%H` |
//! | `mm` | 2-digit minute | `%M` |
//! | `ss` | 2-digit second | `%S` |
//!
//! A token is a whole run of ASCII letters: `MMM` is literal text (a month
//! abbreviation), not `MM` followed by `M`. Everything outside a token is
//! copied through unchanged.

/// The format used when a stored one cannot be repaired.
pub const DEFAULT_TIMESTAMP_FORMAT: &str = "HH:mm:ss";

/// Longest accepted format string, in characters.
pub const MAX_TIMESTAMP_FORMAT_LEN: usize = 64;

const TOKENS: [(&str, &str); 6] = [
    ("YYYY", "%Y"),
    ("MM", "%m"),
    ("DD", "%d"),
    ("HH", "%H"),
    ("mm", "%M"),
    ("ss", "%S"),
];

/// Whether `value` is a usable timestamp format.
///
/// A format may mix literals with tokens, must contain at least one token, must
/// be at most [`MAX_TIMESTAMP_FORMAT_LEN`] characters, and must not contain
/// control characters.
pub fn validate_timestamp_format(value: &str) -> bool {
    if value.chars().count() > MAX_TIMESTAMP_FORMAT_LEN {
        return false;
    }
    if value.chars().any(char::is_control) {
        return false;
    }
    let mut found_token = false;
    let mut rest = value;
    while let Some(ch) = rest.chars().next() {
        if ch.is_ascii_alphabetic() {
            let len = alpha_run_len(rest);
            if token_at(&rest[..len]).is_some() {
                found_token = true;
            }
            rest = &rest[len..];
        } else {
            rest = &rest[ch.len_utf8()..];
        }
    }
    found_token
}

/// Translate the shared token language to a `strftime` string.
///
/// Only the known tokens are replaced; everything else, including `MMM` or
/// `%`, is copied literally.
pub fn timestamp_to_strftime(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(ch) = rest.chars().next() {
        if ch.is_ascii_alphabetic() {
            let len = alpha_run_len(rest);
            let run = &rest[..len];
            match token_at(run) {
                Some((_, strftime)) => out.push_str(strftime),
                None => out.push_str(run),
            }
            rest = &rest[len..];
        } else {
            out.push(ch);
            rest = &rest[ch.len_utf8()..];
        }
    }
    out
}

fn alpha_run_len(rest: &str) -> usize {
    rest.chars()
        .take_while(|ch| ch.is_ascii_alphabetic())
        .map(char::len_utf8)
        .sum()
}

fn token_at(run: &str) -> Option<(&'static str, &'static str)> {
    TOKENS.iter().copied().find(|(token, _)| *token == run)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_literals_and_tokens() {
        let accepted = [
            "HH:mm:ss",
            "HH:mm",
            "DD MMM YYYY HH:mm:ss",
            "[HH:mm]",
            "YYYY-MM-DD",
            "ss",
            "month HH",
            "at HH:mm",
        ];
        for value in accepted {
            assert!(
                validate_timestamp_format(value),
                "{value:?} should be valid"
            );
        }
    }

    #[test]
    fn rejects_empty_control_and_tokenless() {
        let rejected = [
            "",
            "   ",
            "M",
            "Y",
            "MMM",
            "YYYYYYYY",
            "HHmm",
            "%H:%M",
            "no tokens here",
            "HH\u{7}:mm",
            "HH\nmm",
        ];
        for value in rejected {
            assert!(
                !validate_timestamp_format(value),
                "{value:?} should be invalid"
            );
        }
    }

    #[test]
    fn rejects_overlong_formats() {
        let mut ok = String::from("HH");
        ok.push_str(&":".repeat(MAX_TIMESTAMP_FORMAT_LEN - 2));
        assert_eq!(ok.chars().count(), MAX_TIMESTAMP_FORMAT_LEN);
        assert!(validate_timestamp_format(&ok));

        ok.push(':');
        assert!(!validate_timestamp_format(&ok));
    }

    #[test]
    fn replaces_only_known_tokens() {
        let cases: &[(&str, &str)] = &[
            ("DD MMM YYYY HH:mm:ss", "%d MMM %Y %H:%M:%S"),
            ("HH:mm:ss", "%H:%M:%S"),
            ("YYYY-MM-DD", "%Y-%m-%d"),
            ("[HH:mm]", "[%H:%M]"),
            ("%H:%M", "%H:%M"),
            ("month ss", "month %S"),
            ("MMM", "MMM"),
            ("", ""),
        ];
        for (input, expected) in cases {
            assert_eq!(timestamp_to_strftime(input), *expected, "input {input:?}");
        }
    }

    #[test]
    fn tokens_match_whole_runs() {
        assert_eq!(timestamp_to_strftime("YYYY"), "%Y");
        assert_eq!(timestamp_to_strftime("DDHH"), "DDHH");
        assert_eq!(timestamp_to_strftime("YYYYY"), "YYYYY");
        assert_eq!(timestamp_to_strftime("ss.ss"), "%S.%S");
    }
}
