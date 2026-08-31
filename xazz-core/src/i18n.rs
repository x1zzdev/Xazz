//! i18n — 최소 다국어 지원 (영어 기본 + 한국어 유지)
//!
//! 언어 선택 규칙:
//!   1. 테스트/코드에서 `set_lang()` 으로 스레드 로컬 오버라이드 (테스트 병렬 안전)
//!   2. `XAZZ_LANG=ko` (또는 `ko_KR`, `kr`) 환경변수 → 한국어
//!   3. 그 외 → 영어 (기본)
//!
//! 사용 예:
//!   let msg = tr("column", "컬럼");
//!   format!("{} '{}' 을 찾을 수 없습니다", tr("column", "컬럼"), name);

use std::cell::Cell;

/// 지원 언어.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Ko,
}

thread_local! {
    static OVERRIDE: Cell<Option<Lang>> = const { Cell::new(None) };
}

/// 현재 언어 결정 — 스레드 로컬 오버라이드 → 환경변수 → 기본(영어).
pub fn current() -> Lang {
    if let Some(lang) = OVERRIDE.with(Cell::get) {
        return lang;
    }
    match std::env::var("XAZZ_LANG") {
        Ok(v) if v.eq_ignore_ascii_case("ko") || v.eq_ignore_ascii_case("kr") || v.eq_ignore_ascii_case("ko_KR") => {
            Lang::Ko
        }
        _ => Lang::En,
    }
}

/// 스레드 로컬 언어 오버라이드 (주로 테스트에서 사용 — 병렬 테스트 안전).
pub fn set_lang(lang: Lang) {
    OVERRIDE.with(|c| c.set(Some(lang)));
}

/// 스레드 로컬 언어 오버라이드 해제.
pub fn reset_lang() {
    OVERRIDE.with(|c| c.set(None));
}

/// (영어, 한국어) 쌍에서 현재 언어의 문자열을 반환한다.
pub fn tr(en: &'static str, ko: &'static str) -> &'static str {
    match current() {
        Lang::En => en,
        Lang::Ko => ko,
    }
}

/// 현재 언어가 한국어인지 여부.
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
        // Rust 2024: env 조작은 unsafe 로 표시된다 (병렬 테스트 안전성).
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