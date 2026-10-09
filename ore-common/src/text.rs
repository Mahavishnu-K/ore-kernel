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
}
