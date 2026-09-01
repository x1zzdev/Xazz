// xazz-server/src/slm.rs — on-premise sLM code remediation adapter (issue #2)
//
// Serves a fine-tuned Qwen2.5-Coder-1.5B locally via Ollama/llama.cpp and has it
// rewrite code blocked by the static guardrail into more natural safe code.
//
// ⚠️  The most important design decision: **we do not trust the sLM's output as-is.**
//
//     A generative model can produce plausible but still-violating code. So a
//     proposed code is always re-parsed and re-verified by the same policy engine
//     (guardrail.rs); if it does not pass, we fall back to the deterministic
//     remediation result. Unverified code never reaches the user as "safe
//     replacement code".
//
// Network
//   Only goes out to localhost. The reason to use an on-premise sLM is that data
//   never leaks externally, so the endpoint default is 127.0.0.1.
//
// Environment variables
//   XAZZ_SLM_ENABLED    calls the sLM only when "1"/"true" (disabled by default)
//   XAZZ_SLM_ENDPOINT   default http://127.0.0.1:11434
//   XAZZ_SLM_MODEL      default xazz-guardrail
//   XAZZ_SLM_TIMEOUT_MS default 20000

use std::time::Duration;

use http_body_util::{BodyExt, Full};
use hyper::Request;
use hyper::body::Bytes;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use serde::Serialize;
use serde_json::json;
use xazz_compiler::PolicyReport;

/// sLM serving configuration.
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
    /// Reads the configuration from environment variables.
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

    /// Whether the endpoint is a loopback (local) host.
    ///
    /// The core of an on-premise sLM is "code/data never leave the machine".
    /// Changing `XAZZ_SLM_ENDPOINT` to a remote host breaks this guarantee, so a
    /// non-loopback endpoint beyond the default is rejected without an explicit
    /// allow flag.
    pub fn is_loopback(&self) -> bool {
        let lower = self.endpoint.to_ascii_lowercase();
        lower.contains("127.0.0.1")
            || lower.contains("localhost")
            || lower.contains("[::1]")
            || lower.contains("::1")
    }
}

/// Explicitly allows using a non-loopback sLM endpoint, which can send data externally.
const ALLOW_REMOTE_SLM_ENV: &str = "XAZZ_SLM_ALLOW_REMOTE";

/// Rejects a non-loopback sLM endpoint. (The default is trust; non-local is blocked)
///
/// Passes only when `XAZZ_SLM_ALLOW_REMOTE=1` is set explicitly. This enforces the
/// default policy of "data does not leave the machine" and that remote use is an
/// intentional opt-in by the user.
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

/// Converts the static guardrail report into an sLM prompt.
///
/// The prompt matches the training-data format in `experiments/slm_guardrail` —
/// if the training-time and inference-time formats diverge, the fine-tuning
/// effect is lost.
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

/// Extracts only the `.xzz` code from the model response.
///
/// Uses the content inside a code fence (```xzz … ```) if present, otherwise the whole response.
pub fn extract_code(response: &str) -> String {
    let trimmed = response.trim();
    if let Some(start) = trimmed.find("```") {
        let after = &trimmed[start + 3..];
        // skip the language tag on the first line
        let body_start = after.find('\n').map(|i| i + 1).unwrap_or(0);
        let body = &after[body_start..];
        if let Some(end) = body.find("```") {
            return body[..end].trim().to_string();
        }
        return body.trim().to_string();
    }
    trimmed.to_string()
}

/// Calls Ollama `/api/generate` to obtain a remediation code candidate.
///
/// A failure is not an error but simply "the sLM could not be used" — the caller
/// can fall back to deterministic remediation.
pub async fn propose(code: &str, report: &PolicyReport, cfg: &SlmConfig) -> Result<String, String> {
    if !cfg.enabled {
        return Err("sLM 이 비활성화되어 있습니다 (XAZZ_SLM_ENABLED=1 로 활성화).".to_string());
    }

    // Non-loopback endpoints do not send a network request without explicit approval.
    guard_endpoint(cfg)?;

    let url = format!("{}/api/generate", cfg.endpoint.trim_end_matches('/'));
    let payload = json!({
        "model": cfg.model,
        "prompt": build_prompt(code, report),
        "stream": false,
        "options": {
            // Security remediation needs reproducibility, not creativity.
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

// ── unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use xazz_compiler::Policy;

    /// Extracts only the code inside a code fence.
    #[test]
    fn extracts_code_from_fence() {
        let raw = "설명입니다.\n```xzz\nv a = b |> select([x]);\n```\n끝.";
        assert_eq!(extract_code(raw), "v a = b |> select([x]);");
    }

    /// With no code fence, treats the whole response as code.
    #[test]
    fn extracts_bare_code() {
        assert_eq!(extract_code("  v a = b;  "), "v a = b;");
    }

    /// Does not break even without a closing fence.
    #[test]
    fn tolerates_unterminated_fence() {
        assert_eq!(extract_code("```xzz\nv a = b;"), "v a = b;");
    }

    /// The prompt contains both the violation rule IDs and the original code.
    #[test]
    fn prompt_contains_violations_and_code() {
        let src = "type P = { name: string, age_band: string };\n\
                   v x = load(\"d.csv\") :: P |> select([name]);";
        let report = xazz_compiler::check_policy(src, &Policy::builtin());
        let prompt = build_prompt(src, &report);
        assert!(
            prompt.contains("XZP001"),
            "violation ID missing:\n{}",
            prompt
        );
        assert!(prompt.contains("select([name])"), "original code missing");
        assert!(prompt.contains("withDp"), "remediation rule missing");
    }

    /// The default config is disabled, and while disabled the network is never touched.
    #[tokio::test]
    async fn disabled_config_never_calls_network() {
        let cfg = SlmConfig::default();
        assert!(!cfg.enabled);
        let report = xazz_compiler::check_policy("v x = ???", &Policy::builtin());
        let result = propose("v x = ???", &report, &cfg).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("비활성화"));
    }

    /// The endpoint default is localhost — data does not leave the machine.
    #[test]
    fn default_endpoint_is_loopback() {
        let cfg = SlmConfig::default();
        assert!(
            cfg.endpoint.contains("127.0.0.1"),
            "default endpoint is not localhost: {}",
            cfg.endpoint
        );
    }

    /// Loopback endpoints always pass.
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
            assert!(guard_endpoint(&cfg).is_ok(), "loopback blocked: {}", e);
        }
    }

    /// A non-loopback endpoint is rejected without explicit approval.
    #[test]
    fn remote_endpoint_rejected_without_explicit_allow() {
        // Remove the env var if already set, so it does not contaminate the test.
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
            "error message missing 'loopback': {}",
            err
        );
    }
}
