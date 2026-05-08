pub fn scroll_status_label(scroll: u16, max_scroll: u16) -> String {
    let scroll = scroll.min(max_scroll);
    let state = if scroll >= max_scroll { "END" } else { "more" };
    format!("scroll {scroll}/{max_scroll} · {state}")
}

pub fn scrollable_hint(base: &str, scroll: u16, max_scroll: u16) -> String {
    format!("{base} · {}", scroll_status_label(scroll, max_scroll))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scroll_status_reports_more_before_end() {
        assert_eq!(scroll_status_label(1, 3), "scroll 1/3 · more");
    }

    #[test]
    fn scroll_status_reports_end_at_or_beyond_max() {
        assert_eq!(scroll_status_label(3, 3), "scroll 3/3 · END");
        assert_eq!(scroll_status_label(9, 3), "scroll 3/3 · END");
    }
}
