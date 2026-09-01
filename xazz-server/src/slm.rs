// xazz-server/src/slm.rs — 온프레미스 sLM 코드 보정 어댑터 (issue #2)
//
// 파인튜닝된 Qwen2.5-Coder-1.5B 를 Ollama/llama.cpp 로 로컬 서빙하고,
// 정적 가드레일에 차단된 코드를 더 자연스러운 안전 코드로 재작성하게 한다.
//
// ⚠️  가장 중요한 설계 결정: **sLM 의 출력을 그대로 믿지 않는다.**
//
//     생성 모델은 그럴듯하지만 여전히 위반인 코드를 낼 수 있다. 그래서
//     제안된 코드는 반드시 같은 정책 엔진으로 재파싱·재검증되며(guardrail.rs),
//     통과하지 못하면 결정적 보정 결과로 되돌린다. 검증되지 않은 코드가
//     "안전한 대체 코드"라는 이름으로 사용자에게 나가는 일은 없다.
//
// 네트워크
//   로컬호스트로만 나간다. 데이터가 외부로 유출되지 않는 것이 온프레미스
//   sLM 을 쓰는 이유이므로, 엔드포인트 기본값은 127.0.0.1 이다.
//
// 환경변수
//   XAZZ_SLM_ENABLED    "1"/"true" 일 때만 sLM 을 호출한다 (기본 비활성)
//   XAZZ_SLM_ENDPOINT   기본 http://127.0.0.1:11434
//   XAZZ_SLM_MODEL      기본 xazz-guardrail
//   XAZZ_SLM_TIMEOUT_MS 기본 20000

use std::time::Duration;

use http_body_util::{BodyExt, Full};
use hyper::Request;
use hyper::body::Bytes;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use serde::Serialize;
use serde_json::json;
use xazz_compiler::PolicyReport;

/// sLM 서빙 설정.
#[derive(Debug, Clone, Serialize)]
pub struct SlmConfig {
    pub enabled: bool,
    pub endpoint: String,
    pub model: String,
    pub timeout_ms: u64,
}

impl Default for SlmConfig {
    fn default() -> Self {
        SlmConfig {
            enabled: false,
            endpoint: "http://127.0.0.1:11434".to_string(),
            model: "xazz-guardrail".to_string(),
            timeout_ms: 20_000,
        }
    }
}

impl SlmConfig {
    /// 환경변수에서 설정을 읽는다.
    pub fn from_env() -> Self {
        let defaults = SlmConfig::default();
        SlmConfig {
            enabled: std::env::var("XAZZ_SLM_ENABLED")
                .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "on"))
                .unwrap_or(false),
            endpoint: std::env::var("XAZZ_SLM_ENDPOINT").unwrap_or(defaults.endpoint),
            model: std::env::var("XAZZ_SLM_MODEL").unwrap_or(defaults.model),
            timeout_ms: std::env::var("XAZZ_SLM_TIMEOUT_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.timeout_ms),
        }
    }

    /// 엔드포인트가 루프백(로컬) 호스트인지 여부.
    ///
    /// 온프레미스 sLM 의 핵심은 "코드/데이터가 외부로 나가지 않는다"이다.
    /// `XAZZ_SLM_ENDPOINT` 를 원격 호스트로 바꾸면 이 보장이 무너지므로,
    /// 기본값 외의 비-루프백 엔드포인트는 명시적 허용 플래그 없이는 거부한다.
    pub fn is_loopback(&self) -> bool {
        let lower = self.endpoint.to_ascii_lowercase();
        lower.contains("127.0.0.1")
            || lower.contains("localhost")
            || lower.contains("[::1]")
            || lower.contains("::1")
    }
}

/// 비-루프백 sLM 엔드포인트 사용 시 데이터가 외부로 나갈 수 있음을 명시적으로 허용.
const ALLOW_REMOTE_SLM_ENV: &str = "XAZZ_SLM_ALLOW_REMOTE";

/// 루프백이 아닌 sLM 엔드포인트를 거부한다. (기본값은 신뢰, 비-로컬은 차단)
///
/// `XAZZ_SLM_ALLOW_REMOTE=1` 로 명시적 허용 시에만 통과시킨다. 이는 기본 정책이
/// "데이터가 외부로 나가지 않음" 이며, 원격 사용은 사용자가 의도적으로 옵트인해야
/// 한다는 것을 강제한다.
pub fn guard_endpoint(cfg: &SlmConfig) -> Result<(), String> {
    if cfg.is_loopback() {
        return Ok(());
    }
    let allow_remote = std::env::var(ALLOW_REMOTE_SLM_ENV)
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "on"))
        .unwrap_or(false);
    if allow_remote {
        return Ok(());
    }
    Err(format!(
        "sLM endpoint '{}' is not a loopback host. An on-premise sLM must stay on the local machine \
         to guarantee data never leaves the host. To allow a remote endpoint, set {} = 1 explicitly.",
        cfg.endpoint, ALLOW_REMOTE_SLM_ENV
    ))
}

/// 정적 가드레일 리포트를 sLM 프롬프트로 바꾼다.
///
/// 프롬프트는 `experiments/slm_guardrail` 의 학습 데이터 포맷과 동일하다 —
/// 학습 시점과 추론 시점의 형식이 어긋나면 파인튜닝 효과가 사라진다.
pub fn build_prompt(code: &str, report: &PolicyReport) -> String {
    let mut violations = String::new();
    for v in &report.violations {
        violations.push_str(&format!(
            "- [{}] {}: {}\n  fix direction: {}\n",
            v.rule_id, v.rule_name, v.message, v.remediation_hint
        ));
    }

    format!(
        "You are a security remediation assistant for the Xazz DSL (.xzz).\n\
         The code below was blocked by the Policy-as-Code static guardrail.\n\
         Rewrite it as safe code that resolves every violation while preserving the analysis intent as much as possible.\n\n\
         Rules:\n\
         1. Do not output direct identifiers (names, patient numbers, contact info, etc.).\n\
         2. Do not export sensitive attributes row-wise; convert them to groupBy + aggregates.\n\
         3. Attach |> withDp(epsilon: ..., mechanism: laplace) to sensitive aggregates.\n\
         4. Generalize quasi-identifiers via binning (e.g. age -> age_band).\n\
         5. Remove hardcoded personal data and secrets from the code.\n\
         6. Output only .xzz code, no explanation.\n\n\
         === Violations ===\n{}\n\
         === Original code ===\n{}\n\n\
         === Remediated code ===\n",
        violations, code
    )
}

/// 모델 응답에서 `.xzz` 코드만 추출한다.
///
/// 코드펜스(```xzz … ```)가 있으면 그 안을, 없으면 전체를 사용한다.
pub fn extract_code(response: &str) -> String {
    let trimmed = response.trim();
    if let Some(start) = trimmed.find("```") {
        let after = &trimmed[start + 3..];
        // 첫 줄의 언어 태그를 건너뛴다.
        let body_start = after.find('\n').map(|i| i + 1).unwrap_or(0);
        let body = &after[body_start..];
        if let Some(end) = body.find("```") {
            return body[..end].trim().to_string();
        }
        return body.trim().to_string();
    }
    trimmed.to_string()
}

/// Ollama `/api/generate` 를 호출해 보정 코드 후보를 받아온다.
///
/// 실패는 오류가 아니라 "sLM 을 못 썼다"는 사실일 뿐이다 — 호출자는 결정적
/// 보정으로 되돌아가면 된다.
pub async fn propose(code: &str, report: &PolicyReport, cfg: &SlmConfig) -> Result<String, String> {
    if !cfg.enabled {
        return Err("sLM 이 비활성화되어 있습니다 (XAZZ_SLM_ENABLED=1 로 활성화).".to_string());
    }

    // 비-루프백 엔드포인트는 명시적 허용 없이 네트워크 요청을 보내지 않는다.
    guard_endpoint(cfg)?;

    let url = format!("{}/api/generate", cfg.endpoint.trim_end_matches('/'));
    let payload = json!({
        "model": cfg.model,
        "prompt": build_prompt(code, report),
        "stream": false,
        "options": {
            // 보안 보정은 창의성이 아니라 재현성이 중요하다.
            "temperature": 0.1,
            "top_p": 0.9,
            "num_predict": 768
        }
    });

    let request = Request::builder()
        .method("POST")
        .uri(&url)
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(payload.to_string())))
        .map_err(|e| format!("sLM 요청 구성 실패: {}", e))?;

    let client: Client<_, Full<Bytes>> = Client::builder(TokioExecutor::new()).build_http();

    let response = tokio::time::timeout(
        Duration::from_millis(cfg.timeout_ms),
        client.request(request),
    )
    .await
    .map_err(|_| format!("sLM 응답 시간 초과 ({}ms)", cfg.timeout_ms))?
    .map_err(|e| format!("sLM 연결 실패 ({}): {}", url, e))?;

    let status = response.status();
    let body = response
        .into_body()
        .collect()
        .await
        .map_err(|e| format!("sLM 응답 본문 읽기 실패: {}", e))?
        .to_bytes();

    if !status.is_success() {
        return Err(format!(
            "sLM 이 오류를 반환했습니다: HTTP {} — {}",
            status.as_u16(),
            String::from_utf8_lossy(&body)
                .chars()
                .take(200)
                .collect::<String>()
        ));
    }

    let parsed: serde_json::Value =
        serde_json::from_slice(&body).map_err(|e| format!("sLM 응답 JSON 파싱 실패: {}", e))?;

    let text = parsed
        .get("response")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "sLM 응답에 'response' 필드가 없습니다.".to_string())?;

    let extracted = extract_code(text);
    if extracted.trim().is_empty() {
        return Err("sLM 이 빈 코드를 반환했습니다.".to_string());
    }
    Ok(extracted)
}

// ── 유닛 테스트 ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use xazz_compiler::Policy;

    /// 코드펜스가 있으면 그 안의 코드만 뽑아낸다.
    #[test]
    fn extracts_code_from_fence() {
        let raw = "설명입니다.\n```xzz\nv a = b |> select([x]);\n```\n끝.";
        assert_eq!(extract_code(raw), "v a = b |> select([x]);");
    }

    /// 코드펜스가 없으면 전체를 코드로 본다.
    #[test]
    fn extracts_bare_code() {
        assert_eq!(extract_code("  v a = b;  "), "v a = b;");
    }

    /// 닫는 펜스가 없어도 깨지지 않는다.
    #[test]
    fn tolerates_unterminated_fence() {
        assert_eq!(extract_code("```xzz\nv a = b;"), "v a = b;");
    }

    /// 프롬프트에 위반 규칙 ID 와 원본 코드가 모두 들어간다.
    #[test]
    fn prompt_contains_violations_and_code() {
        let src = "type P = { name: string, age_band: string };\n\
                   v x = load(\"d.csv\") :: P |> select([name]);";
        let report = xazz_compiler::check_policy(src, &Policy::builtin());
        let prompt = build_prompt(src, &report);
        assert!(prompt.contains("XZP001"), "위반 ID 누락:\n{}", prompt);
        assert!(prompt.contains("select([name])"), "원본 코드 누락");
        assert!(prompt.contains("withDp"), "보정 규칙 누락");
    }

    /// 기본 설정은 비활성이며, 비활성 상태에서는 네트워크를 건드리지 않는다.
    #[tokio::test]
    async fn disabled_config_never_calls_network() {
        let cfg = SlmConfig::default();
        assert!(!cfg.enabled);
        let report = xazz_compiler::check_policy("v x = ???", &Policy::builtin());
        let result = propose("v x = ???", &report, &cfg).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("비활성화"));
    }

    /// 엔드포인트 기본값은 로컬호스트다 — 데이터가 외부로 나가지 않는다.
    #[test]
    fn default_endpoint_is_loopback() {
        let cfg = SlmConfig::default();
        assert!(
            cfg.endpoint.contains("127.0.0.1"),
            "기본 엔드포인트가 로컬호스트가 아님: {}",
            cfg.endpoint
        );
    }

    /// 루프백 엔드포인트는 항상 통과한다.
    #[test]
    fn loopback_endpoint_passes_guard() {
        for e in [
            "http://127.0.0.1:11434",
            "http://localhost:11434",
            "http://[::1]:11434",
        ] {
            let cfg = SlmConfig {
                enabled: true,
                endpoint: e.to_string(),
                model: "m".to_string(),
                timeout_ms: 1,
            };
            assert!(guard_endpoint(&cfg).is_ok(), "루프백 차단됨: {}", e);
        }
    }

    /// 비-루프백 엔드포인트는 명시적 허용 없이 거부된다.
    #[test]
    fn remote_endpoint_rejected_without_explicit_allow() {
        // 환경변수가 이미 설정돼 있으면 테스트가 오염될 수 있으므로 제거.
        unsafe { std::env::remove_var("XAZZ_SLM_ALLOW_REMOTE") };
        let cfg = SlmConfig {
            enabled: true,
            endpoint: "http://10.0.0.5:11434".to_string(),
            model: "m".to_string(),
            timeout_ms: 1,
        };
        let err = guard_endpoint(&cfg).unwrap_err();
        assert!(
            err.contains("loopback"),
            "오류 메시지에 loopback 없음: {}",
            err
        );
    }
}
