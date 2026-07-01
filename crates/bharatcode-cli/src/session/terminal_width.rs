use console::measure_text_width;

pub fn display_width(text: &str) -> usize {
    measure_text_width(text)
}

pub fn truncate_to_width(text: &str, max_width: usize, suffix: &str) -> String {
    if display_width(text) <= max_width {
        return text.to_string();
    }
    if max_width == 0 {
        return String::new();
    }

    let suffix_width = display_width(suffix);
    if suffix.is_empty() {
        return take_columns(text, max_width);
    }
    if max_width <= suffix_width {
        return ".".repeat(max_width);
    }

    let mut out = take_columns(text, max_width - suffix_width);
    out.push_str(suffix);
    out
}

fn take_columns(text: &str, max_width: usize) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        out.push(ch);
        if display_width(&out) > max_width {
            out.pop();
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaves_short_text_unchanged() {
        assert_eq!(truncate_to_width("hello", 8, "..."), "hello");
    }

    #[test]
    fn truncates_with_suffix_inside_budget() {
        let out = truncate_to_width("hello world", 8, "...");
        assert_eq!(out, "hello...");
        assert!(display_width(&out) <= 8);
    }

    #[test]
    fn handles_tiny_budgets() {
        assert_eq!(truncate_to_width("hello", 0, "..."), "");
        assert_eq!(truncate_to_width("hello", 2, "..."), "..");
    }

    #[test]
    fn preserves_column_budget_for_wide_chars() {
        let out = truncate_to_width("भारतcode", 7, "...");
        assert!(display_width(&out) <= 7);
        assert!(out.ends_with("..."));
    }
}
