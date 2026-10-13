//! Text and terminal utility functions (ANSI escape sequence measurement and truncation).

/// Calculates the visible character count of a string by skipping ANSI escape sequences.
pub fn visible_len(s: &str) -> usize {
    let mut len = 0;
    let mut in_ansi = false;
    for c in s.chars() {
        if c == '\x1B' {
            in_ansi = true;
        } else if in_ansi {
            if c.is_ascii_alphabetic() {
                in_ansi = false;
            }
        } else {
            len += 1;
        }
    }
    len
}

/// Truncates a string to at most `max_width` visible characters while preserving ANSI escape sequences.
pub fn truncate_visible(s: &str, max_width: usize) -> String {
    let mut res = String::new();
    let mut vlen = 0;
    let mut in_ansi = false;
    let mut ansi_buf = String::new();
    let target = max_width.saturating_sub(3);
    let mut truncated = false;

    for c in s.chars() {
        if c == '\x1B' {
            in_ansi = true;
            ansi_buf.push(c);
        } else if in_ansi {
            ansi_buf.push(c);
            if c.is_ascii_alphabetic() {
                in_ansi = false;
                res.push_str(&ansi_buf);
                ansi_buf.clear();
            }
        } else if vlen < target {
            res.push(c);
            vlen += 1;
        } else {
            truncated = true;
            break;
        }
    }

    if truncated {
        res.push_str("\x1B[0m...");
    }
    res
}

/// Returns true if the character is in common CJK (Chinese, Japanese, Korean) Unicode ranges.
#[inline]
pub fn is_cjk(c: char) -> bool {
    matches!(c,
        '\u{4E00}'..='\u{9FFF}'    // CJK Unified Ideographs
        | '\u{3400}'..='\u{4DBF}'   // CJK Unified Ideographs Extension A
        | '\u{20000}'..='\u{2A6DF}' // CJK Unified Ideographs Extension B
        | '\u{F900}'..='\u{FAFF}'   // CJK Compatibility Ideographs
        | '\u{3040}'..='\u{309F}'   // Hiragana
        | '\u{30A0}'..='\u{30FF}'   // Katakana
        | '\u{AC00}'..='\u{D7AF}'   // Hangul Syllables
    )
}

/// Calibrated fast token estimation based on character category weights (millitokens).
///
/// Unlike naive `len() / 4` heuristics which undercount code/JSON by ~50% and miscalculate
/// multi-byte UTF-8, this estimator calibrates:
/// - ASCII alphanumeric prose (~3.6 chars/token)
/// - Punctuation and syntax symbols (~0.75 tokens/char, critical for JSON/code)
/// - Indentation & whitespace sequences (4 spaces ≈ 1 token)
/// - CJK characters (~1.3 tokens/char)
/// - Emojis and extended Unicode (~1.1 to 2.0 tokens/char)
pub fn estimate_tokens_calibrated(text: &str) -> u32 {
    if text.is_empty() {
        return 0;
    }

    let mut millitokens: u64 = 0;

    for c in text.chars() {
        if c == '\n' {
            millitokens += 700;
        } else if c == ' ' || c == '\t' {
            millitokens += 250;
        } else if c.is_ascii_alphanumeric() {
            millitokens += 280;
        } else if c.is_ascii_punctuation() {
            millitokens += 750;
        } else if is_cjk(c) {
            millitokens += 1300;
        } else if (c as u32) >= 0x1F000 {
            millitokens += 2000;
        } else {
            millitokens += 1100;
        }
    }

    let tokens = ((millitokens + 500) / 1000) as u32;
    tokens.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_visible_len() {
        assert_eq!(visible_len("hello world"), 11);
        assert_eq!(visible_len("\x1b[36;1mhello world\x1b[0m"), 11);
    }

    #[test]
    fn test_truncate_visible() {
        let long = "Lorem ipsum dolor sit amet, consectetur adipiscing elit";
        let truncated = truncate_visible(long, 20);
        assert_eq!(visible_len(&truncated), 20);
        assert!(truncated.ends_with("..."));
    }

    #[test]
    fn test_estimate_tokens_empty() {
        assert_eq!(estimate_tokens_calibrated(""), 0);
    }

    #[test]
    fn test_estimate_tokens_english_prose() {
        let text = "The quick brown fox jumps over the lazy dog.";
        let est = estimate_tokens_calibrated(text);
        // 9 words + period -> ~10-12 tokens
        assert!((9..=14).contains(&est), "Got {}", est);
    }

    #[test]
    fn test_estimate_tokens_json_and_code() {
        let json = r#"{"status": "ok", "code": 200, "items": [1, 2, 3]}"#;
        let est = estimate_tokens_calibrated(json);
        // Naive len() / 4 gives ~12, but actual syntax has ~18-22 tokens
        assert!((16..=26).contains(&est), "Got {}", est);
    }

    #[test]
    fn test_estimate_tokens_cjk() {
        let cjk = "你好，世界！这是一个测试。";
        let est = estimate_tokens_calibrated(cjk);
        // 13 CJK characters + Chinese punctuation -> ~12-18 tokens
        assert!((12..=20).contains(&est), "Got {}", est);
    }

    #[test]
    fn test_estimate_tokens_whitespace() {
        let text = "    let x = 1;\n    let y = 2;\n";
        let est = estimate_tokens_calibrated(text);
        assert!(est >= 8, "Got {}", est);
    }
}
