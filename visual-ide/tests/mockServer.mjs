// Mocked xazz-server for the e2e specs. The preview build calls the API on its own
// origin, so page.route answers for the server. The audit fixture is a verbatim
// GET /security/audit/log from a locally built xazz-server.
import { readFileSync } from 'node:fs'

export const auditFixture = JSON.parse(
  readFileSync(new URL('./fixtures/audit-log.real.json', import.meta.url), 'utf8'),
)
export const HASH = auditFixture.records[0].hash

export const defaults = () => ({
  'GET /health': { status: 'ok' },
  'GET /runs': { tenant: '', runs: [] },
  'GET /dp/budget': {
    tenant: '',
    spent_epsilon: 2.5,
    total_epsilon: 10,
    remaining_epsilon: 7.5,
    spent_delta: 0,
    total_delta: 0.0001,
    remaining_delta: 0.0001,
    window_secs: 0,
    window_source: 'global',
    window_started_at: 0,
    resets_at: 0,
  },
  'GET /dp/budget/history': { tenant: '', resets: [] },
  'GET /security/audit/log': auditFixture,
  'GET /security/audit/chain': { intact: true, records: auditFixture.records.length },
  'GET /security/policy': {
    tenant: '',
    origin: 'builtin',
    policy: { id: 'xazz-builtin-pii', version: '1.0.0', domain: 'common', risk_level: 'medium' },
  },
  'GET /security/policy/history': { tenant: '', history: [] },
  'GET /security/policy/history/ttl': { tenant: '', ttl_secs: 0, ttl_source: 'global' },
})

const API = /^\/(execute|health|schema|catalog|runs|dp\/|security\/)/

/**
 * Serves `handlers` keyed "METHOD /path". A value is a JSON body (200), an
 * `{ http, json | text }` answer, a function of the request returning either, or
 * 'abort' to simulate no server.
 * Every API request is recorded for header/body assertions.
 */
export async function mockServer(page, overrides = {}) {
  const handlers = { ...defaults(), ...overrides }
  const requests = []
  await page.route(
    (url) => API.test(url.pathname),
    async (route) => {
      const request = route.request()
      const path = new URL(request.url()).pathname
      requests.push({ method: request.method(), path, headers: request.headers(), body: request.postData() })
      const handler = handlers[`${request.method()} ${path}`]
      let answer = typeof handler === 'function' ? await handler(request) : handler
      if (answer === undefined) answer = { http: 404, text: `no mock for ${request.method()} ${path}` }
      if (answer === 'abort') return route.abort('connectionrefused')
      if (answer.http === undefined) answer = { http: 200, json: answer }
      return answer.json !== undefined
        ? route.fulfill({ status: answer.http, json: answer.json })
        : route.fulfill({ status: answer.http, body: answer.text, contentType: 'text/plain' })
    },
  )
  return requests
}
