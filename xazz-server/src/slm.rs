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
use hyper::body::Bytes;
use hyper::Request;
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
}

/// 정적 가드레일 리포트를 sLM 프롬프트로 바꾼다.
///
/// 프롬프트는 `experiments/slm_guardrail` 의 학습 데이터 포맷과 동일하다 —
/// 학습 시점과 추론 시점의 형식이 어긋나면 파인튜닝 효과가 사라진다.
pub fn build_prompt(code: &str, report: &PolicyReport) -> String {
    let mut violations = String::new();
    for v in &report.violations {
        violations.push_str(&format!(
            "- [{}] {}: {}\n  보정 방향: {}\n",
            v.rule_id, v.rule_name, v.message, v.remediation_hint
        ));
    }

    format!(
        "당신은 Xazz DSL(.xzz) 보안 코드 보정기입니다.\n\
         아래 코드는 Policy-as-Code 정적 가드레일에 차단되었습니다.\n\
         위반을 모두 해소하되 분석 의도는 최대한 보존하는 안전한 코드로 다시 작성하세요.\n\n\
         규칙:\n\
         1. 직접 식별자(이름·환자번호·연락처 등)는 출력하지 않습니다.\n\
         2. 민감 속성은 행 단위로 내보내지 말고 groupBy + 집계로 바꿉니다.\n\
         3. 민감 속성 집계에는 |> withDp(epsilon: ..., mechanism: laplace) 를 붙입니다.\n\
         4. 준식별자는 구간화(예: age → age_band)해 일반화합니다.\n\
         5. 하드코딩된 개인정보·비밀키는 코드에서 제거합니다.\n\
         6. 설명 없이 .xzz 코드만 출력합니다.\n\n\
         === 위반 내역 ===\n{}\n\
         === 원본 코드 ===\n{}\n\n\
         === 보정된 코드 ===\n",
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
}
