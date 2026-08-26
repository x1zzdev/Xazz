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
 */
export async function executeCode(code) {
  const res = await fetch(`${API_BASE_URL}/execute`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ code }),
  })
  if (!res.ok) {
    throw new Error(`Server responded ${res.status}`)
  }
  return res.json()
}

export { API_BASE_URL }