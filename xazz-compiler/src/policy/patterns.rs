// xazz-compiler/src/policy/patterns.rs — literal scanner (Policy-as-Code #2)
//
// Scans the `.xzz` source text directly to find PII and secret-key literals.
// The reasons for targeting the raw text rather than the AST are two-fold.
//
//   1. A secret key written in a comment is still a leak — comments are not kept in the AST.
//   2. Exact line/col can be computed from the raw offsets.
//
// ⚠️  No regex crate is used. Since xazz-compiler is a lightweight crate linked into the
//     CLI binary (CONTRIBUTING.md architecture constraint), the scanner is written by
//     hand rather than adding a dependency.

use serde::Serialize;

/// Kind of detected literal
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretKind {
    /// Resident registration number / foreigner registration number (checksum verified)
    ResidentRegistrationNumber,
    /// Mobile phone or landline number
    PhoneNumber,
    /// Email address
    Email,
    /// Credit card number (Luhn verified)
    CreditCard,
    /// Cloud/service API key (AWS, GitHub, OpenAI, Slack …)
    ApiKey,
    /// PEM private key block
    PrivateKey,
    /// Generic credential in the form of `password=`, `secret=`
    GenericSecret,
}

impl SecretKind {
    /// Display name
    pub fn label(&self) -> &'static str {
        use xazz_core::i18n::is_korean;
        if is_korean() {
            match self {
                SecretKind::ResidentRegistrationNumber => "주민등록번호",
                SecretKind::PhoneNumber => "전화번호",
                SecretKind::Email => "이메일 주소",
                SecretKind::CreditCard => "신용카드 번호",
                SecretKind::ApiKey => "API 키",
                SecretKind::PrivateKey => "개인키(PEM)",
                SecretKind::GenericSecret => "자격증명",
            }
        } else {
            match self {
                SecretKind::ResidentRegistrationNumber => "resident registration number",
                SecretKind::PhoneNumber => "phone number",
                SecretKind::Email => "email address",
                SecretKind::CreditCard => "credit card number",
                SecretKind::ApiKey => "API key",
                SecretKind::PrivateKey => "private key (PEM)",
                SecretKind::GenericSecret => "credential",
            }
        }
    }

    /// Whether this kind is PII or a secret.
    pub fn is_pii(&self) -> bool {
        matches!(
            self,
            SecretKind::ResidentRegistrationNumber
                | SecretKind::PhoneNumber
                | SecretKind::Email
                | SecretKind::CreditCard
        )
    }
}

/// One sensitive literal found in the source
#[derive(Debug, Clone, Serialize)]
pub struct LiteralFinding {
    pub kind: SecretKind,
    /// 1-based line number
    pub line: usize,
    /// 1-based column number
    pub col: usize,
    /// Masked value — the raw value is never put in a report.
    pub redacted: String,
}

// ── Public entry point ───────────────────────────────────────────────────────

/// Scans the whole source and finds every sensitive literal.
///
/// The same (kind, line, col) is reported only once.
pub fn scan_source(source: &str) -> Vec<LiteralFinding> {
    let bytes = source.as_bytes();
    let mut out: Vec<LiteralFinding> = Vec::new();

    scan_rrn(source, bytes, &mut out);
    scan_phone(source, bytes, &mut out);
    scan_email(source, bytes, &mut out);
    scan_credit_card(source, bytes, &mut out);
    scan_api_key(source, &mut out);
    scan_private_key(source, &mut out);
    scan_generic_secret(source, &mut out);

    out.sort_by_key(|f| (f.line, f.col));
    out.dedup_by(|a, b| a.kind == b.kind && a.line == b.line && a.col == b.col);
    out
}

// ── Position calculation ─────────────────────────────────────────────────────

/// Byte offset → (line, col). Both are 1-based.
fn line_col(source: &str, offset: usize) -> (usize, usize) {
    let mut line = 1usize;
    let mut col = 1usize;
    for (i, ch) in source.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

/// Masks a value — keeps the first 2 characters and replaces the rest with `*`.
fn redact(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= 2 {
        return "*".repeat(chars.len().max(1));
    }
    let head: String = chars[..2].iter().collect();
    format!("{}{}", head, "*".repeat(chars.len() - 2))
}

fn push(out: &mut Vec<LiteralFinding>, source: &str, offset: usize, kind: SecretKind, raw: &str) {
    let (line, col) = line_col(source, offset);
    out.push(LiteralFinding {
        kind,
        line,
        col,
        redacted: redact(raw),
    });
}

// ── Low-level helpers ────────────────────────────────────────────────────────

fn is_digit(b: u8) -> bool {
    b.is_ascii_digit()
}

/// Ensures the bytes right before and after are not digits, so the run is not part
/// of a longer digit sequence.
fn digit_boundary(bytes: &[u8], start: usize, end: usize) -> bool {
    let before_ok = start == 0 || !is_digit(bytes[start - 1]);
    let after_ok = end >= bytes.len() || !is_digit(bytes[end]);
    before_ok && after_ok
}

// ── Resident registration number ─────────────────────────────────────────────

/// Finds the `YYMMDD-SXXXXXX` form and verifies the checksum too.
///
/// Only gender codes 1–8 are accepted (1·2 natives born 1900s, 3·4 born 2000s, 5–8 foreigners).
/// Checksum: with the sum of the products of weights [2,3,4,5,6,7,8,9,2,3,4,5],
/// `(11 - sum % 11) % 10` must equal the last digit.
fn scan_rrn(source: &str, bytes: &[u8], out: &mut Vec<LiteralFinding>) {
    const W: [u32; 12] = [2, 3, 4, 5, 6, 7, 8, 9, 2, 3, 4, 5];
    let n = bytes.len();
    let mut i = 0usize;
    while i + 14 <= n {
        // 6 digits + '-' + 7 digits
        let head_ok = (0..6).all(|k| is_digit(bytes[i + k]));
        if head_ok && bytes[i + 6] == b'-' && (7..14).all(|k| is_digit(bytes[i + k])) {
            let digits: Vec<u32> = (0..14)
                .filter(|k| *k != 6)
                .map(|k| (bytes[i + k] - b'0') as u32)
                .collect();
            let gender = digits[6];
            let month = digits[2] * 10 + digits[3];
            let day = digits[4] * 10 + digits[5];
            let valid_shape = (1..=8).contains(&gender)
                && (1..=12).contains(&month)
                && (1..=31).contains(&day)
                && digit_boundary(bytes, i, i + 14);
            if valid_shape {
                let sum: u32 = (0..12).map(|k| digits[k] * W[k]).sum();
                let check = (11 - (sum % 11)) % 10;
                if check == digits[12] {
                    push(
                        out,
                        source,
                        i,
                        SecretKind::ResidentRegistrationNumber,
                        &source[i..i + 14],
                    );
                    i += 14;
                    continue;
                }
            }
        }
        i += 1;
    }
}

// ── Phone number ─────────────────────────────────────────────────────────────

/// Finds mobile phone numbers (`01X-XXXX-XXXX`) and landlines with area codes.
///
/// Only `-` is accepted as a separator. An 11-digit run without separators is excluded,
/// since it cannot be distinguished from other identifiers (e.g. zip codes, code values)
/// and would cause many false positives.
fn scan_phone(source: &str, bytes: &[u8], out: &mut Vec<LiteralFinding>) {
    let n = bytes.len();
    let mut i = 0usize;
    while i < n {
        if bytes[i] != b'0' || (i > 0 && is_digit(bytes[i - 1])) {
            i += 1;
            continue;
        }
        // Exchange group: 2–3 digits (starting with 0) → '-' → 3–4 digits → '-' → 4 digits
        let mut matched = false;
        for head in [3usize, 2] {
            for mid in [4usize, 3] {
                let total = head + 1 + mid + 1 + 4;
                if i + total > n {
                    continue;
                }
                let ok = (0..head).all(|k| is_digit(bytes[i + k]))
                    && bytes[i + head] == b'-'
                    && (0..mid).all(|k| is_digit(bytes[i + head + 1 + k]))
                    && bytes[i + head + 1 + mid] == b'-'
                    && (0..4).all(|k| is_digit(bytes[i + head + mid + 2 + k]))
                    && digit_boundary(bytes, i, i + total);
                if ok {
                    push(
                        out,
                        source,
                        i,
                        SecretKind::PhoneNumber,
                        &source[i..i + total],
                    );
                    i += total;
                    matched = true;
                    break;
                }
            }
            if matched {
                break;
            }
        }
        if !matched {
            i += 1;
        }
    }
}

// ── Email ────────────────────────────────────────────────────────────────────

/// Finds `local@domain.tld`. The TLD must be at least 2 alphabetic characters.
fn scan_email(source: &str, bytes: &[u8], out: &mut Vec<LiteralFinding>) {
    let n = bytes.len();
    let local_ok =
        |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'%' | b'+' | b'-');
    let domain_ok = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-');

    for at in 0..n {
        if bytes[at] != b'@' {
            continue;
        }
        // local part
        let mut start = at;
        while start > 0 && local_ok(bytes[start - 1]) {
            start -= 1;
        }
        if start == at {
            continue;
        }
        // domain part
        let mut end = at + 1;
        while end < n && domain_ok(bytes[end]) {
            end += 1;
        }
        let domain = &source[at + 1..end];
        // The label after the last '.' must be at least 2 alphabetic characters to count as an email.
        let tld_ok = domain
            .rsplit_once('.')
            .map(|(_, tld)| tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic()))
            .unwrap_or(false);
        if tld_ok {
            push(out, source, start, SecretKind::Email, &source[start..end]);
        }
    }
}

// ── Credit card ──────────────────────────────────────────────────────────────

/// Verifies 13–19 digit runs (separators `-`/space allowed) with the Luhn check.
///
/// Luhn alone is not enough. A random long digit run passes Luhn with probability ~1/10.
/// In fact a nanosecond timestamp `1787805001967327111` (19 digits) passed Luhn and was
/// misreported as a card number merely because it appeared in a temp path. So two more
/// conditions are added.
///
/// 1. A digit run without separators must have a plausible **issuer identification number (IIN)** first digit.
///  Timestamps and serial numbers commonly start with 0·1·7·8, which are not cards.
/// 2. A digit run in an identifier context (attached to `_` or letters) is not a card number.
///  Paths and variable names like `xazz_test_4150_1787805001967327111` are excluded.
///
/// The separated form (`4111-1111-1111-1111`) is a strong signal by itself, so
/// the IIN check is not required for it.
fn scan_credit_card(source: &str, bytes: &[u8], out: &mut Vec<LiteralFinding>) {
    let n = bytes.len();
    let mut i = 0usize;
    while i < n {
        if !is_digit(bytes[i]) || (i > 0 && (is_digit(bytes[i - 1]) || bytes[i - 1] == b'-')) {
            i += 1;
            continue;
        }
        let mut j = i;
        let mut digits: Vec<u32> = Vec::new();
        let mut has_separator = false;
        while j < n && (is_digit(bytes[j]) || bytes[j] == b'-' || bytes[j] == b' ') {
            if is_digit(bytes[j]) {
                digits.push((bytes[j] - b'0') as u32);
            } else {
                has_separator = true;
            }
            j += 1;
            if digits.len() >= 19 {
                break;
            }
        }
        // Trim any trailing separator.
        let mut end = j;
        while end > i && !is_digit(bytes[end - 1]) {
            end -= 1;
        }
        let structurally_plausible = has_separator || plausible_iin(&digits);
        if (13..=19).contains(&digits.len())
            && structurally_plausible
            && identifier_boundary(bytes, i, end)
            && luhn(&digits)
            && digit_boundary(bytes, i, end)
        {
            push(out, source, i, SecretKind::CreditCard, &source[i..end]);
            i = end;
            continue;
        }
        i += 1;
    }
}

/// Ensures the digit run is not in an identifier context (`_` or letters).
///
/// Digit runs attached to variable names or path fragments like
/// `xazz_test_4150_1787805001967327111` are not card numbers.
fn identifier_boundary(bytes: &[u8], start: usize, end: usize) -> bool {
    let ident = |b: u8| b == b'_' || b.is_ascii_alphabetic();
    let before_ok = start == 0 || !ident(bytes[start - 1]);
    let after_ok = end >= bytes.len() || !ident(bytes[end]);
    before_ok && after_ok
}

/// Checks whether the first digit of the issuer identification number (IIN) is a real card range.
///
/// Only international brand ranges are accepted — 3: Amex(34·37)·JCB·Diners, 4: Visa,
/// 5: Mastercard, 6: Discover·UnionPay, 2: Mastercard 2-series(2221~2720).
///
/// Runs starting with 0·1·7·8·9 are not card numbers — timestamps and serial numbers
/// mostly fall in those ranges.
fn plausible_iin(digits: &[u32]) -> bool {
    match digits.first() {
        Some(3) | Some(4) | Some(5) | Some(6) => true,
        Some(2) => {
            if digits.len() < 4 {
                return false;
            }
            let prefix = digits[0] * 1000 + digits[1] * 100 + digits[2] * 10 + digits[3];
            (2221..=2720).contains(&prefix)
        }
        _ => false,
    }
}

/// Luhn checksum — the standard validation algorithm for credit card numbers.
fn luhn(digits: &[u32]) -> bool {
    let mut sum = 0u32;
    let mut double = false;
    for d in digits.iter().rev() {
        let mut v = *d;
        if double {
            v *= 2;
            if v > 9 {
                v -= 9;
            }
        }
        sum += v;
        double = !double;
    }
    sum.is_multiple_of(10)
}

// ── API key ──────────────────────────────────────────────────────────────────

/// Finds well-known service token prefixes.
fn scan_api_key(source: &str, out: &mut Vec<LiteralFinding>) {
    // (prefix, minimum token length that must follow the prefix)
    const PREFIXES: &[(&str, usize)] = &[
        ("AKIA", 16),
        ("ASIA", 16),
        ("ghp_", 36),
        ("gho_", 36),
        ("github_pat_", 22),
        ("xoxb-", 10),
        ("xoxp-", 10),
        ("sk-", 20),
        ("AIza", 30),
    ];

    for (prefix, min_len) in PREFIXES {
        let mut from = 0usize;
        while let Some(rel) = source[from..].find(prefix) {
            let at = from + rel;
            let rest = &source[at + prefix.len()..];
            let token_len = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
                .count();
            if token_len >= *min_len {
                let end = at + prefix.len() + token_len;
                push(out, source, at, SecretKind::ApiKey, &source[at..end]);
            }
            from = at + prefix.len();
        }
    }
}

// ── PEM private key ──────────────────────────────────────────────────────────

fn scan_private_key(source: &str, out: &mut Vec<LiteralFinding>) {
    const NEEDLE: &str = "PRIVATE KEY-----";
    let mut from = 0usize;
    while let Some(rel) = source[from..].find(NEEDLE) {
        let at = from + rel;
        // Only accept the "-----BEGIN ... PRIVATE KEY-----" form.
        let head_start = source[..at].rfind("-----BEGIN").unwrap_or(usize::MAX);
        if head_start != usize::MAX && at - head_start <= 40 {
            push(
                out,
                source,
                head_start,
                SecretKind::PrivateKey,
                "-----BEGIN PRIVATE KEY-----",
            );
        }
        from = at + NEEDLE.len();
    }
}

// ── Generic credential ───────────────────────────────────────────────────────

/// Finds hardcoded credentials in the form of `password = "..."` / `api_key: "..."`.
fn scan_generic_secret(source: &str, out: &mut Vec<LiteralFinding>) {
    const KEYS: &[&str] = &[
        "password",
        "passwd",
        "secret",
        "api_key",
        "apikey",
        "access_token",
        "private_key",
        "client_secret",
    ];

    let lower = source.to_ascii_lowercase();
    for key in KEYS {
        let mut from = 0usize;
        while let Some(rel) = lower[from..].find(key) {
            let at = from + rel;
            from = at + key.len();

            // If the character before the keyword is an identifier character, it is part of another word.
            if at > 0 {
                let prev = lower.as_bytes()[at - 1];
                if prev.is_ascii_alphanumeric() || prev == b'_' {
                    continue;
                }
            }
            // After the keyword: spaces* [:=] spaces* "value"
            let mut p = at + key.len();
            let b = lower.as_bytes();
            while p < b.len() && (b[p] == b' ' || b[p] == b'\t') {
                p += 1;
            }
            if p >= b.len() || (b[p] != b'=' && b[p] != b':') {
                continue;
            }
            p += 1;
            while p < b.len() && (b[p] == b' ' || b[p] == b'\t') {
                p += 1;
            }
            if p >= b.len() || b[p] != b'"' {
                continue;
            }
            let value_start = p + 1;
            let Some(close_rel) = source[value_start..].find('"') else {
                continue;
            };
            let value = &source[value_start..value_start + close_rel];
            // Ignore values shorter than 8 characters or obvious placeholders.
            let placeholder = value.starts_with('<')
                || value.starts_with('$')
                || value.starts_with("${")
                || value.eq_ignore_ascii_case("changeme")
                || value.chars().all(|c| c == '*' || c == 'x' || c == 'X');
            if value.chars().count() >= 8 && !placeholder {
                push(out, source, at, SecretKind::GenericSecret, value);
            }
        }
    }
}

// ── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<SecretKind> {
        scan_source(src).into_iter().map(|f| f.kind).collect()
    }

    /// A resident registration number with a valid checksum is detected.
    #[test]
    fn detects_valid_rrn() {
        // 900101-1234568 is a synthetic number satisfying the checksum rule (not a real one).
        let found = scan_source("v x = a |> filter(rrn == \"900101-1234568\")");
        assert!(
            found
                .iter()
                .any(|f| f.kind == SecretKind::ResidentRegistrationNumber),
            "주민등록번호 미탐지: {:?}",
            found
        );
    }

    /// Numbers with an incorrect checksum are ignored (false-positive guard).
    #[test]
    fn ignores_rrn_with_bad_checksum() {
        let found = kinds("v x = a |> filter(code == \"900101-1234567\")");
        assert!(!found.contains(&SecretKind::ResidentRegistrationNumber));
    }

    /// A gender code out of range is not treated as a resident number.
    #[test]
    fn ignores_rrn_with_invalid_gender_digit() {
        let found = kinds("v x = a |> filter(code == \"900101-9234567\")");
        assert!(!found.contains(&SecretKind::ResidentRegistrationNumber));
    }

    /// Mobile phone number detection.
    #[test]
    fn detects_phone_number() {
        let found = kinds("// 담당자 010-1234-5678");
        assert!(found.contains(&SecretKind::PhoneNumber), "{:?}", found);
    }

    /// A date (2026-08-27) is not misdetected as a phone number.
    #[test]
    fn ignores_date_as_phone() {
        let found = kinds("// 작성일 2026-08-27");
        assert!(!found.contains(&SecretKind::PhoneNumber), "{:?}", found);
    }

    /// Email detection and ignoring strings without a TLD.
    #[test]
    fn detects_email_only_with_tld() {
        assert!(kinds("v x = \"hong@example.com\"").contains(&SecretKind::Email));
        assert!(!kinds("v x = \"user@localhost\"").contains(&SecretKind::Email));
    }

    /// Only card numbers passing Luhn are detected.
    #[test]
    fn detects_credit_card_by_luhn() {
        // 4111-1111-1111-1111 is the standard test number that passes Luhn.
        assert!(kinds("v x = \"4111-1111-1111-1111\"").contains(&SecretKind::CreditCard));
        assert!(!kinds("v x = \"4111-1111-1111-1112\"").contains(&SecretKind::CreditCard));
    }

    /// A bare card number with a plausible IIN is also detected.
    #[test]
    fn detects_bare_credit_card_with_valid_iin() {
        assert!(kinds("v x = \"4111111111111111\"").contains(&SecretKind::CreditCard));
        // Mastercard 2-series (2221~2720)
        assert!(kinds("v x = \"2221000000000009\"").contains(&SecretKind::CreditCard));
    }

    /// A nanosecond timestamp is not misdetected as a card number.
    ///
    /// Regression guard: `1787805001967327111` (19 digits) actually passes Luhn.
    /// Since a random long digit run passes Luhn with probability ~1/10, judging
    /// by Luhn alone would flag every timestamp and serial number as a card number.
    #[test]
    fn does_not_flag_luhn_passing_timestamp() {
        for path in [
            "/tmp/xazz_test_4150_1787805001967327111/pipeline.xzz",
            "/tmp/xazz_test_4150_1787805001967335759/data.csv",
        ] {
            let src = format!("type S = {{ a: int }};\nv p = load(\"{}\") :: S;", path);
            let found = kinds(&src);
            assert!(
                !found.contains(&SecretKind::CreditCard),
                "타임스탬프를 카드번호로 오탐: {}\n탐지 결과: {:?}",
                path,
                scan_source(&src)
            );
        }
    }

    /// First digits outside the card ranges are rejected when there is no separator.
    #[test]
    fn bare_digits_need_a_plausible_card_prefix() {
        // Leading digit 1 — passes Luhn but is not a card range.
        assert!(!kinds("v x = \"1787805001967327111\"").contains(&SecretKind::CreditCard));
        // Leading digit 9 — same.
        assert!(!kinds("v x = \"9000000000000009\"").contains(&SecretKind::CreditCard));
    }

    /// Digit runs attached to identifiers or paths are not treated as card numbers.
    #[test]
    fn digits_glued_to_identifiers_are_not_cards() {
        // 4111111111111111 is a valid card number, but not when glued to a variable name.
        assert!(!kinds("v order_4111111111111111 = a;").contains(&SecretKind::CreditCard));
        assert!(!kinds("v x = \"run4111111111111111\";").contains(&SecretKind::CreditCard));
    }

    /// AWS access key prefix detection.
    #[test]
    fn detects_aws_access_key() {
        assert!(kinds("// AKIAIOSFODNN7EXAMPLE").contains(&SecretKind::ApiKey));
    }

    /// Hardcoded password detection.
    #[test]
    fn detects_generic_secret() {
        assert!(kinds("// password = \"hunter2hunter2\"").contains(&SecretKind::GenericSecret));
    }

    /// Placeholders are not treated as credentials.
    #[test]
    fn ignores_placeholder_secret() {
        assert!(!kinds("// password = \"<YOUR_PASSWORD>\"").contains(&SecretKind::GenericSecret));
        assert!(!kinds("// password = \"********\"").contains(&SecretKind::GenericSecret));
    }

    /// Nothing is detected in a normal air-quality pipeline (false-positive regression guard).
    #[test]
    fn clean_pipeline_has_no_findings() {
        let src = "type AQ = { station: string, pm10: float };\n\
                   v a = load(\"examples/data/seoul_air_2024.csv\") :: AQ\n\
                     |> filter(pm10 > 50)\n\
                     |> groupBy(\"station\")\n\
                     |> mean(\"pm10\");";
        assert!(scan_source(src).is_empty(), "{:?}", scan_source(src));
    }

    /// Raw values never end up in the report (masking verification).
    #[test]
    fn findings_are_redacted() {
        let found = scan_source("v x = a |> filter(rrn == \"900101-1234568\")");
        for f in &found {
            assert!(
                !f.redacted.contains("1234568"),
                "원본 값 노출: {}",
                f.redacted
            );
            assert!(f.redacted.contains('*'), "마스킹 없음: {}", f.redacted);
        }
    }

    /// line/col are computed accurately as 1-based.
    #[test]
    fn reports_accurate_line_and_col() {
        let src = "line one\n// 010-1234-5678";
        let found = scan_source(src);
        let phone = found
            .iter()
            .find(|f| f.kind == SecretKind::PhoneNumber)
            .expect("전화번호 미탐지");
        assert_eq!(phone.line, 2);
        assert_eq!(phone.col, 4);
    }
}
