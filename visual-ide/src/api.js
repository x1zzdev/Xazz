// 같은 origin에서 서빙될 때는 빈 문자열('')로 두어 상대 경로(/execute 등)를 사용한다.
// dev(별도 vite 서버)에서는 VITE_API_BASE_URL 로 오버라이드한다.
const API_BASE_URL = (
  (import.meta.env && import.meta.env.VITE_API_BASE_URL) || ''
).replace(/\/+$/, '')

/**
 * Server access for multi-tenant / admin deployments (xazz-server main.rs
 * optional_bearer_auth). Kept in memory only: a bearer token must never be written
 * to localStorage or sessionStorage. With no XAZZ_*_TOKEN set (local mode) the server
 * ignores all three and every request belongs to the default tenant "".
 */
let access = { tenant: '', token: '', actor: '' }

export function getApiAccess() {
  return access
}

export function setApiAccess(next) {
  access = { ...access, ...next }
}

function accessHeaders(extra = {}) {
  const headers = { ...extra }
  if (access.tenant) headers['X-Xazz-Tenant'] = access.tenant
  if (access.token) headers.Authorization = `Bearer ${access.token}`
  if (access.actor) headers['X-Xazz-Actor'] = access.actor
  return headers
}

/** A non-2xx answer. A thrown non-ApiError means the server was not reached. */
export class ApiError extends Error {
  constructor(status, message) {
    super(message)
    this.status = status
  }
}

// Some routes answer errors as plain text, others as {"error": ...}; read either.
async function request(path, { method = 'GET', json, form, timeoutMs = 10_000 } = {}) {
  const res = await fetch(`${API_BASE_URL}${path}`, {
    method,
    headers: accessHeaders(json === undefined ? {} : { 'Content-Type': 'application/json' }),
    body: form ?? (json === undefined ? undefined : JSON.stringify(json)),
    signal: AbortSignal.timeout(timeoutMs),
  })
  const text = await res.text()
  let data = null
  try {
    data = text ? JSON.parse(text) : null
  } catch {
    // plain-text body
  }
  if (!res.ok) {
    throw new ApiError(res.status, data?.error ?? (text || `Server responded ${res.status}`))
  }
  return data
}

/**
 * GET /health — 서버 연결 상태 확인. 실패는 false 로 반환한다 (throw 하지 않는다).
 */
export async function checkHealth() {
  try {
    const res = await fetch(`${API_BASE_URL}/health`, {
      headers: accessHeaders(),
      signal: AbortSignal.timeout(3000),
    })
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
 *
 * 기본 5분 타임아웃(ML 훈련 고려). 무한 대기로 UI 가 'running' 에 갇히는 것을 방지한다.
 * `signal` 로 사용자가 기다림을 멈출 수 있다 — 서버에는 취소 API 가 없으므로 서버 쪽
 * 실행은 계속될 수 있다 (issue #113).
 */
export async function executeCode(code, { timeoutMs = 5 * 60 * 1000, signal } = {}) {
  const timeout = AbortSignal.timeout(timeoutMs)
  const res = await fetch(`${API_BASE_URL}/execute`, {
    method: 'POST',
    headers: accessHeaders({ 'Content-Type': 'application/json' }),
    body: JSON.stringify({ code }),
    signal: signal ? AbortSignal.any([signal, timeout]) : timeout,
  })
  if (!res.ok && res.status !== 422) {
    // 401 (token), 429 (capacity / DP reservation), 500 (runner missing): the server
    // answered, so this is not "offline" — keep its reason.
    const text = await res.text()
    let reason = text
    try {
      reason = JSON.parse(text)?.error ?? text
    } catch {
      // plain-text body
    }
    throw new ApiError(res.status, reason || `Server responded ${res.status}`)
  }
  return res.json()
}

/**
 * POST /security/policy/check — 실행하지 않고 정적 가드레일 검사만 수행한다 (issue #2).
 *
 * 위반이 있어도 HTTP 200 이다 — 검사 자체는 성공했고, 판정은 `safe_to_execute`
 * 에 담긴다. 서버 거부(ApiError: 401, 정책 로드 실패 500)와 연결 실패는 호출부가 구분한다.
 */
export function checkPolicy(code) {
  return request('/security/policy/check', { method: 'POST', json: { code } })
}

/**
 * POST /security/remediate — 차단된 코드의 안전한 대체 코드와 위반 리포트를 받는다 (issue #2).
 *
 * 응답의 `remediation.verified` 가 false 이면 사람이 처리해야 할 위반이 남아
 * 있다는 뜻이므로, 보정 코드를 "안전함"으로 표시해서는 안 된다.
 */
export function remediateCode(code) {
  return request('/security/remediate', { method: 'POST', json: { code } })
}

// ── Run history (#107) ─ server keeps metadata only (no rows), newest 50.
export const listRuns = () => request('/runs').then((data) => data?.runs ?? [])
export const getRun = (id) => request(`/runs/${encodeURIComponent(id)}`)

// ── Audit chain (#108)
export const getAuditLog = () => request('/security/audit/log')
export const getAuditRecords = (hash) =>
  request(`/security/audit/log/${encodeURIComponent(hash)}`)
export const verifyAuditChain = () => request('/security/audit/chain')

// ── Policy packs (#109). A 500 from GET means the pack failed to load and the server
// denies execution until it does (fail-closed) — callers must show that, not hide it.
export const getPolicy = () => request('/security/policy')
export const putPolicy = (policy) => request('/security/policy', { method: 'PUT', json: policy })
export const deletePolicy = () => request('/security/policy', { method: 'DELETE' })
export const getPolicyHistory = ({ limit = 20, offset = 0 } = {}) =>
  request(`/security/policy/history?limit=${limit}&offset=${offset}`)
export const getPolicyTtl = () => request('/security/policy/history/ttl')
export const putPolicyTtl = (ttlSecs) =>
  request('/security/policy/history/ttl', { method: 'PUT', json: { ttl_secs: ttlSecs } })
export const deletePolicyTtl = () => request('/security/policy/history/ttl', { method: 'DELETE' })

// ── Differential-privacy ledger (#110)
export const getDpBudget = () => request('/dp/budget')
export const getDpResetHistory = () => request('/dp/budget/history')
export const resetDpBudget = () => request('/dp/budget/reset', { method: 'POST' })
export const putDpWindow = (windowSecs) =>
  request('/dp/budget/window', { method: 'PUT', json: { window_secs: windowSecs } })
export const deleteDpWindow = () => request('/dp/budget/window', { method: 'DELETE' })

// ── Column lineage (#116) — static compile only, nothing executes.
export const fetchCatalog = (code) =>
  request('/catalog', { method: 'POST', json: { code } }).then((data) => data?.catalog)

// ── CSV schema inference (#114). Server decodes UTF-8, then EUC-KR (CP949), samples
// 100 rows and keeps an upload copy whose path goes back into load(...).
export const MAX_UPLOAD_BYTES = 50 * 1024 * 1024
export function inferSchema(file) {
  const form = new FormData()
  form.append('file', file)
  return request('/schema', { method: 'POST', form, timeoutMs: 60_000 })
}

export { API_BASE_URL }
