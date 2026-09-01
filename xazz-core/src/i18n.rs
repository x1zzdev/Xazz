//! i18n — minimal multilingual support (English default + Korean retained)
//!
//! Language selection rules:
//!   1. Thread-local override via `set_lang()` in tests/code (parallel-test safe)
//!   2. `XAZZ_LANG=ko` (or `ko_KR`, `kr`) environment variable → Korean
//!   3. Otherwise → English (default)
//!
//! Usage example:
//!   let msg = tr("column", "컬럼");
//!   format!("{} '{}' 을 찾을 수 없습니다", tr("column", "컬럼"), name);

use std::cell::Cell;

/// Supported languages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Ko,
}

thread_local! {
    static OVERRIDE: Cell<Option<Lang>> = const { Cell::new(None) };
}

/// Determine the current language — thread-local override → environment variable → default (English).
pub fn current() -> Lang {
    if let Some(lang) = OVERRIDE.with(Cell::get) {
        return lang;
    }
    match std::env::var("XAZZ_LANG") {
        Ok(v)
            if v.eq_ignore_ascii_case("ko")
                || v.eq_ignore_ascii_case("kr")
                || v.eq_ignore_ascii_case("ko_KR") =>
        {
            Lang::Ko
        }
        _ => Lang::En,
    }
}

/// Thread-local language override (mainly used in tests — parallel-test safe).
pub fn set_lang(lang: Lang) {
    OVERRIDE.with(|c| c.set(Some(lang)));
}

/// Clear the thread-local language override.
pub fn reset_lang() {
    OVERRIDE.with(|c| c.set(None));
}

/// Returns the string for the current language from an (English, Korean) pair.
pub fn tr(en: &'static str, ko: &'static str) -> &'static str {
    match current() {
        Lang::En => en,
        Lang::Ko => ko,
    }
}

/// Whether the current language is Korean.
pub fn is_korean() -> bool {
    current() == Lang::Ko
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_english() {
        reset_lang();
        assert_eq!(current(), Lang::En);
        assert_eq!(tr("row", "행"), "row");
    }

    #[test]
    fn env_var_selects_korean() {
        reset_lang();
        // Rust 2024: manipulating env is marked unsafe (parallel-test safety).
        unsafe { std::env::set_var("XAZZ_LANG", "ko") };
        assert_eq!(current(), Lang::Ko);
        assert_eq!(tr("row", "행"), "행");
        unsafe { std::env::remove_var("XAZZ_LANG") };
    }

    #[test]
    fn thread_local_override_isolation() {
        set_lang(Lang::Ko);
        assert_eq!(current(), Lang::Ko);
        reset_lang();
        assert_eq!(current(), Lang::En);
    }
}
