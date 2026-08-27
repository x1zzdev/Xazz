// xazz-compiler/src/policy/patterns.rs — 리터럴 스캐너 (Policy-as-Code #2)
//
// `.xzz` 소스 텍스트를 직접 훑어 개인정보(PII) · 비밀키 리터럴을 찾아낸다.
// AST 가 아니라 원본 텍스트를 대상으로 하는 이유는 두 가지다.
//
//   1. 주석에 적힌 비밀키도 유출이다 — AST 에는 주석이 남지 않는다.
//   2. 원본 오프셋에서 정확한 line/col 을 계산할 수 있다.
//
// ⚠️  정규식 크레이트를 쓰지 않는다. xazz-compiler 는 CLI 바이너리에
//     링크되는 경량 크레이트이므로(CONTRIBUTING.md 아키텍처 제약)
//     의존성을 늘리지 않고 손으로 스캐너를 작성한다.

use serde::Serialize;

/// 탐지된 리터럴의 종류
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretKind {
    /// 주민등록번호 / 외국인등록번호 (체크섬 검증 통과)
    ResidentRegistrationNumber,
    /// 휴대전화 · 유선전화 번호
    PhoneNumber,
    /// 이메일 주소
    Email,
    /// 신용카드 번호 (Luhn 검증 통과)
    CreditCard,
    /// 클라우드/서비스 API 키 (AWS, GitHub, OpenAI, Slack …)
    ApiKey,
    /// PEM 개인키 블록
    PrivateKey,
    /// `password=`, `secret=` 형태의 일반 자격증명
    GenericSecret,
}

impl SecretKind {
    /// 한국어 표시 이름
    pub fn label(&self) -> &'static str {
        match self {
            SecretKind::ResidentRegistrationNumber => "주민등록번호",
            SecretKind::PhoneNumber => "전화번호",
            SecretKind::Email => "이메일 주소",
            SecretKind::CreditCard => "신용카드 번호",
            SecretKind::ApiKey => "API 키",
            SecretKind::PrivateKey => "개인키(PEM)",
            SecretKind::GenericSecret => "자격증명",
        }
    }

    /// 이 종류가 개인정보(PII)인지, 비밀정보(Secret)인지 구분한다.
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

/// 소스에서 발견된 민감 리터럴 하나
#[derive(Debug, Clone, Serialize)]
pub struct LiteralFinding {
    pub kind: SecretKind,
    /// 1-base 줄 번호
    pub line: usize,
    /// 1-base 칼럼 번호
    pub col: usize,
    /// 마스킹된 값 — 원본 값은 절대 리포트에 싣지 않는다.
    pub redacted: String,
}

// ── 공개 진입점 ──────────────────────────────────────────────────────────────

/// 소스 전체를 훑어 민감 리터럴을 모두 찾아낸다.
///
/// 같은 (kind, line, col) 은 한 번만 보고한다.
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

// ── 위치 계산 ────────────────────────────────────────────────────────────────

/// 바이트 오프셋 → (line, col). 둘 다 1-base.
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

/// 값을 마스킹한다 — 앞 2글자만 남기고 나머지는 `*`.
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

// ── 저수준 헬퍼 ──────────────────────────────────────────────────────────────

fn is_digit(b: u8) -> bool {
    b.is_ascii_digit()
}

/// `pos` 바로 앞뒤가 숫자가 아닌지 확인해 더 긴 숫자열의 일부가 아님을 보장한다.
fn digit_boundary(bytes: &[u8], start: usize, end: usize) -> bool {
    let before_ok = start == 0 || !is_digit(bytes[start - 1]);
    let after_ok = end >= bytes.len() || !is_digit(bytes[end]);
    before_ok && after_ok
}

// ── 주민등록번호 ─────────────────────────────────────────────────────────────

/// `YYMMDD-SXXXXXX` 형태를 찾고 체크섬까지 검증한다.
///
/// 성별코드 1~8 (1·2 내국인 1900년대, 3·4 2000년대, 5~8 외국인)만 인정한다.
/// 체크섬: 가중치 [2,3,4,5,6,7,8,9,2,3,4,5] 곱의 합에 대해
/// `(11 - sum % 11) % 10` 이 마지막 자리와 같아야 한다.
fn scan_rrn(source: &str, bytes: &[u8], out: &mut Vec<LiteralFinding>) {
    const W: [u32; 12] = [2, 3, 4, 5, 6, 7, 8, 9, 2, 3, 4, 5];
    let n = bytes.len();
    let mut i = 0usize;
    while i + 14 <= n {
        // 6자리 숫자 + '-' + 7자리 숫자
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

// ── 전화번호 ─────────────────────────────────────────────────────────────────

/// 휴대전화(`01X-XXXX-XXXX`) 및 지역번호 유선전화를 찾는다.
///
/// 구분자는 `-` 만 인정한다. 구분자 없는 11자리 숫자는 다른 식별자
/// (예: 우편번호·코드값)와 구분되지 않아 오탐이 크므로 제외한다.
fn scan_phone(source: &str, bytes: &[u8], out: &mut Vec<LiteralFinding>) {
    let n = bytes.len();
    let mut i = 0usize;
    while i < n {
        if bytes[i] != b'0' || (i > 0 && is_digit(bytes[i - 1])) {
            i += 1;
            continue;
        }
        // 국번 그룹: 2~3자리(0으로 시작) → '-' → 3~4자리 → '-' → 4자리
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

// ── 이메일 ───────────────────────────────────────────────────────────────────

/// `local@domain.tld` 를 찾는다. TLD 는 알파벳 2자 이상이어야 한다.
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
        // 마지막 '.' 뒤 라벨이 알파벳 2자 이상이어야 이메일로 인정한다.
        let tld_ok = domain
            .rsplit_once('.')
            .map(|(_, tld)| tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic()))
            .unwrap_or(false);
        if tld_ok {
            push(out, source, start, SecretKind::Email, &source[start..end]);
        }
    }
}

// ── 신용카드 ─────────────────────────────────────────────────────────────────

/// 13~19자리 숫자열(구분자 `-`/공백 허용)을 Luhn 검증으로 확인한다.
///
/// Luhn 만으로는 부족하다. 임의의 긴 숫자열은 약 1/10 확률로 Luhn 을 통과한다.
/// 실제로 나노초 타임스탬프 `1787805001967327111` (19자리)이 Luhn 을 통과해,
/// 임시 경로에 섞인 것만으로 카드번호로 오탐된 사례가 있었다. 그래서 두 조건을 더 건다.
///
/// 1. 구분자 없는 숫자열은 **발급사 식별번호(IIN) 선두 자리**가 그럴듯해야 한다.
///  타임스탬프·일련번호가 흔히 갖는 0·1·7·8 선두는 카드가 아니다.
/// 2. 식별자 문맥(`_` 나 영문자에 붙어 있는 숫자열)은 카드번호가 아니다.
///  `xazz_test_4150_1787805001967327111` 같은 경로·변수명을 배제한다.
///
/// 구분자가 있는 형태(`4111-1111-1111-1111`)는 표기 자체가 강한 신호이므로
/// IIN 검사를 요구하지 않는다.
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
        // 후행 구분자는 잘라낸다.
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

/// 숫자열 앞뒤가 식별자 문맥(`_` 또는 영문자)이 아닌지 확인한다.
///
/// `xazz_test_4150_1787805001967327111` 처럼 변수명·경로 조각에 붙어 있는
/// 숫자열은 카드번호가 아니다.
fn identifier_boundary(bytes: &[u8], start: usize, end: usize) -> bool {
    let ident = |b: u8| b == b'_' || b.is_ascii_alphabetic();
    let before_ok = start == 0 || !ident(bytes[start - 1]);
    let after_ok = end >= bytes.len() || !ident(bytes[end]);
    before_ok && after_ok
}

/// 발급사 식별번호(IIN)의 선두 자리가 실재하는 카드 대역인지 확인한다.
///
/// 국제 브랜드 대역만 인정한다 — 3: Amex(34·37)·JCB·Diners, 4: Visa,
/// 5: Mastercard, 6: Discover·UnionPay, 2: Mastercard 2-시리즈(2221~2720).
///
/// 0·1·7·8·9 로 시작하는 숫자열은 카드번호가 아니다 — 타임스탬프와
/// 일련번호가 대부분 이 대역에 들어간다.
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

/// Luhn 체크섬 — 신용카드 번호의 표준 검증 알고리즘.
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

// ── API 키 ───────────────────────────────────────────────────────────────────

/// 잘 알려진 서비스 토큰 접두사를 찾는다.
fn scan_api_key(source: &str, out: &mut Vec<LiteralFinding>) {
    // (접두사, 접두사 뒤에 이어져야 하는 최소 토큰 길이)
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

// ── PEM 개인키 ───────────────────────────────────────────────────────────────

fn scan_private_key(source: &str, out: &mut Vec<LiteralFinding>) {
    const NEEDLE: &str = "PRIVATE KEY-----";
    let mut from = 0usize;
    while let Some(rel) = source[from..].find(NEEDLE) {
        let at = from + rel;
        // "-----BEGIN ... PRIVATE KEY-----" 형태만 인정한다.
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

// ── 일반 자격증명 ────────────────────────────────────────────────────────────

/// `password = "..."` / `api_key: "..."` 형태의 하드코딩 자격증명을 찾는다.
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

            // 키워드 앞이 식별자 문자면 다른 단어의 일부다.
            if at > 0 {
                let prev = lower.as_bytes()[at - 1];
                if prev.is_ascii_alphanumeric() || prev == b'_' {
                    continue;
                }
            }
            // 키워드 뒤: 공백* [:=] 공백* "값"
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
            // 8자 미만이거나 명백한 플레이스홀더는 무시한다.
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

// ── 유닛 테스트 ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<SecretKind> {
        scan_source(src).into_iter().map(|f| f.kind).collect()
    }

    /// 체크섬이 유효한 주민등록번호는 탐지된다.
    #[test]
    fn detects_valid_rrn() {
        // 900101-1234568 은 체크섬 규칙을 만족하는 합성 번호다 (실존 번호 아님).
        let found = scan_source("v x = a |> filter(rrn == \"900101-1234568\")");
        assert!(
            found
                .iter()
                .any(|f| f.kind == SecretKind::ResidentRegistrationNumber),
            "주민등록번호 미탐지: {:?}",
            found
        );
    }

    /// 체크섬이 틀린 번호는 무시한다 (오탐 방지).
    #[test]
    fn ignores_rrn_with_bad_checksum() {
        let found = kinds("v x = a |> filter(code == \"900101-1234567\")");
        assert!(!found.contains(&SecretKind::ResidentRegistrationNumber));
    }

    /// 성별코드가 범위를 벗어나면 주민번호로 보지 않는다.
    #[test]
    fn ignores_rrn_with_invalid_gender_digit() {
        let found = kinds("v x = a |> filter(code == \"900101-9234567\")");
        assert!(!found.contains(&SecretKind::ResidentRegistrationNumber));
    }

    /// 휴대전화 번호 탐지.
    #[test]
    fn detects_phone_number() {
        let found = kinds("// 담당자 010-1234-5678");
        assert!(found.contains(&SecretKind::PhoneNumber), "{:?}", found);
    }

    /// 날짜(2026-08-27)를 전화번호로 오탐하지 않는다.
    #[test]
    fn ignores_date_as_phone() {
        let found = kinds("// 작성일 2026-08-27");
        assert!(!found.contains(&SecretKind::PhoneNumber), "{:?}", found);
    }

    /// 이메일 탐지 및 TLD 없는 문자열 무시.
    #[test]
    fn detects_email_only_with_tld() {
        assert!(kinds("v x = \"hong@example.com\"").contains(&SecretKind::Email));
        assert!(!kinds("v x = \"user@localhost\"").contains(&SecretKind::Email));
    }

    /// Luhn 을 통과하는 카드번호만 탐지한다.
    #[test]
    fn detects_credit_card_by_luhn() {
        // 4111-1111-1111-1111 은 Luhn 을 통과하는 표준 테스트 번호다.
        assert!(kinds("v x = \"4111-1111-1111-1111\"").contains(&SecretKind::CreditCard));
        assert!(!kinds("v x = \"4111-1111-1111-1112\"").contains(&SecretKind::CreditCard));
    }

    /// 구분자 없는 카드번호도 IIN 이 그럴듯하면 탐지한다.
    #[test]
    fn detects_bare_credit_card_with_valid_iin() {
        assert!(kinds("v x = \"4111111111111111\"").contains(&SecretKind::CreditCard));
        // Mastercard 2-시리즈 (2221~2720)
        assert!(kinds("v x = \"2221000000000009\"").contains(&SecretKind::CreditCard));
    }

    /// 나노초 타임스탬프를 카드번호로 오탐하지 않는다.
    ///
    /// 회귀 방지: `1787805001967327111` (19자리)은 실제로 Luhn 을 통과한다.
    /// 임의의 긴 숫자열은 약 1/10 확률로 Luhn 을 통과하므로, Luhn 만으로
    /// 판정하면 타임스탬프·일련번호가 전부 카드번호로 잡힌다.
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

    /// 카드 대역이 아닌 선두 자리는 구분자가 없으면 인정하지 않는다.
    #[test]
    fn bare_digits_need_a_plausible_card_prefix() {
        // 선두가 1 — Luhn 을 통과해도 카드 대역이 아니다.
        assert!(!kinds("v x = \"1787805001967327111\"").contains(&SecretKind::CreditCard));
        // 선두가 9 — 마찬가지.
        assert!(!kinds("v x = \"9000000000000009\"").contains(&SecretKind::CreditCard));
    }

    /// 식별자·경로에 붙어 있는 숫자열은 카드번호로 보지 않는다.
    #[test]
    fn digits_glued_to_identifiers_are_not_cards() {
        // 4111111111111111 은 유효한 카드번호지만 변수명에 붙어 있으면 아니다.
        assert!(!kinds("v order_4111111111111111 = a;").contains(&SecretKind::CreditCard));
        assert!(!kinds("v x = \"run4111111111111111\";").contains(&SecretKind::CreditCard));
    }

    /// AWS 액세스 키 접두사 탐지.
    #[test]
    fn detects_aws_access_key() {
        assert!(kinds("// AKIAIOSFODNN7EXAMPLE").contains(&SecretKind::ApiKey));
    }

    /// 하드코딩된 비밀번호 탐지.
    #[test]
    fn detects_generic_secret() {
        assert!(kinds("// password = \"hunter2hunter2\"").contains(&SecretKind::GenericSecret));
    }

    /// 플레이스홀더는 자격증명으로 보지 않는다.
    #[test]
    fn ignores_placeholder_secret() {
        assert!(!kinds("// password = \"<YOUR_PASSWORD>\"").contains(&SecretKind::GenericSecret));
        assert!(!kinds("// password = \"********\"").contains(&SecretKind::GenericSecret));
    }

    /// 정상적인 대기질 파이프라인에서는 아무것도 탐지되지 않는다 (오탐 회귀 방지).
    #[test]
    fn clean_pipeline_has_no_findings() {
        let src = "type AQ = { station: string, pm10: float };\n\
                   v a = load(\"examples/data/seoul_air_2024.csv\") :: AQ\n\
                     |> filter(pm10 > 50)\n\
                     |> groupBy(\"station\")\n\
                     |> mean(\"pm10\");";
        assert!(scan_source(src).is_empty(), "{:?}", scan_source(src));
    }

    /// 리포트에 원본 값이 실리지 않는다 (마스킹 검증).
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

    /// line/col 이 1-base 로 정확히 계산된다.
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
