const API_BASE_URL = (
  (import.meta.env && import.meta.env.VITE_API_BASE_URL) || 'http://127.0.0.1:8005'
).replace(/\/+$/, '')

/**
 * GET /health — 서버 연결 상태 확인. 실패는 false 로 반환한다 (throw 하지 않는다).
 */
export async function checkHealth() {
  try {
    const res = await fetch(`${API_BASE_URL}/health`, { signal: AbortSignal.timeout(3000) })
    if (!res.ok) return false
    const body = await res.json()
    return body.status === 'ok'
  } catch {
    return false
  }
}

/**
 * POST /execute — .xzz 소스 코드를 실제 엔진으로 실행하고 ExecuteResponse 를 반환한다.
 * 서버가 응답하지 않으면 예외를 던진다 (호출부에서 연결 실패 상태로 처리).
 *
 * 422 는 예외가 아니다 (issue #2). Policy-as-Code 가드레일이 실행을 차단한
 * 경우이며, 본문에는 통상적인 ExecuteResponse 형태로 차단 사유(`policy`)와
 * 위반 목록(`logs`)이 담겨 온다. 이를 throw 로 바꾸면 사용자에게는
 * "Server responded 422" 만 남고 정작 필요한 차단 사유가 사라진다.
 */
export async function executeCode(code) {
  const res = await fetch(`${API_BASE_URL}/execute`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ code }),
  })
  if (!res.ok && res.status !== 422) {
    throw new Error(`Server responded ${res.status}`)
  }
  return res.json()
}

/**
 * POST /security/policy/check — 실행하지 않고 정적 가드레일 검사만 수행한다 (issue #2).
 *
 * 위반이 있어도 HTTP 200 이다 — 검사 자체는 성공했고, 판정은 `safe_to_execute`
 * 에 담긴다. 편집 중 실시간 표시에 쓰도록 실패는 null 로 돌려준다.
 */
export async function checkPolicy(code) {
  try {
    const res = await fetch(`${API_BASE_URL}/security/policy/check`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ code }),
    })
    if (!res.ok) return null
    return res.json()
  } catch {
    return null
  }
}

/**
 * POST /security/remediate — 차단된 코드의 안전한 대체 코드와 위반 리포트를 받는다 (issue #2).
 *
 * 응답의 `remediation.verified` 가 false 이면 사람이 처리해야 할 위반이 남아
 * 있다는 뜻이므로, 보정 코드를 "안전함"으로 표시해서는 안 된다.
 */
export async function remediateCode(code) {
  const res = await fetch(`${API_BASE_URL}/security/remediate`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ code }),
  })
  if (!res.ok) {
    throw new Error(`Server responded ${res.status}`)
  }
  return res.json()
}

/**
 * GET /security/policy — 활성 Policy-as-Code 정책과 sLM 설정을 조회한다 (issue #2).
 * 실패는 null 로 반환한다.
 */
export async function fetchActivePolicy() {
  try {
    const res = await fetch(`${API_BASE_URL}/security/policy`, {
      signal: AbortSignal.timeout(3000),
    })
    if (!res.ok) return null
    return res.json()
  } catch {
    return null
  }
}

export { API_BASE_URL }