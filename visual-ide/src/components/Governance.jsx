import React, { useEffect, useId, useRef, useState } from 'react'
import {
  FileClock,
  KeyRound,
  Link2,
  RefreshCw,
  Scale,
  ShieldHalf,
  TriangleAlert,
} from 'lucide-react'
import { ServerProblem, Skeleton, StatusBadge } from './Common'
import { MonitorPanel } from './Monitor'
import { useLanguage } from '../i18n'
import { useServerData } from '../useServerData'
import { findFirstBreak } from '../auditChain'
import {
  ApiError,
  checkInference,
  deleteDpWindow,
  deletePolicy,
  deletePolicyTtl,
  getApiAccess,
  getAuditLog,
  getAuditRecords,
  getDpBudget,
  getDpResetHistory,
  getPolicy,
  getPolicyHistory,
  getPolicyTtl,
  getPolicyTtlHistory,
  putPolicy,
  putPolicyTtl,
  putDpWindow,
  resetDpBudget,
  setApiAccess,
  verifyAuditChain,
} from '../api'

const short = (hash) => (typeof hash === 'string' ? hash.slice(0, 12) : '—')
const num = (value) => {
  const v = Number(value)
  return Number.isFinite(v) ? String(parseFloat(v.toPrecision(6))) : '—'
}

function useLocaleTime() {
  const { language } = useLanguage()
  const locale = language === 'ko' ? 'ko-KR' : 'en-US'
  return (value) => {
    const date = typeof value === 'number' ? new Date(value * 1000) : new Date(value)
    return Number.isNaN(date.getTime()) ? String(value) : date.toLocaleString(locale)
  }
}

const tenantLabel = (tenant, t) =>
  tenant ? t('gov.namedTenant').replace('{tenant}', tenant) : t('gov.defaultTenant')

/**
 * Destructive actions go through the native <dialog>: showModal() already gives a
 * focus trap, Escape and an inert background. Focus starts on Cancel, the safe choice.
 */
export function ConfirmDialog({ open, title, body, confirmLabel, onConfirm, onCancel, busy }) {
  const { t } = useLanguage()
  const ref = useRef(null)
  const titleId = useId() // two dialogs share this panel group; a fixed id would cross-label them
  useEffect(() => {
    const dialog = ref.current
    if (!dialog) return
    if (open && !dialog.open) dialog.showModal()
    if (!open && dialog.open) dialog.close()
  }, [open])

  return (
    <dialog
      ref={ref}
      className="confirm-dialog"
      aria-labelledby={titleId}
      onCancel={(event) => {
        event.preventDefault()
        onCancel()
      }}
    >
      <h2 id={titleId}>{title}</h2>
      <p>{body}</p>
      <div className="confirm-dialog__actions">
        <button className="button button--tool-secondary" type="button" onClick={onCancel} autoFocus>
          {t('gov.cancel')}
        </button>
        <button className="button button--danger" type="button" onClick={onConfirm} disabled={busy}>
          {confirmLabel}
        </button>
      </div>
    </dialog>
  )
}

function PanelStatus({ state, onRetry, lines = 3 }) {
  const { t } = useLanguage()
  if (state.status === 'loading') return <Skeleton lines={lines} label={t('server.loading')} />
  return <ServerProblem state={state} onRetry={onRetry} />
}

function RefreshButton({ onClick, label }) {
  return (
    <button className="button button--tool-secondary button--compact" type="button" onClick={onClick}>
      <RefreshCw size={13} aria-hidden="true" />
      {label}
    </button>
  )
}

// ── Server access ───────────────────────────────────────────────────────────

function AccessPanel({ onApply }) {
  const { t } = useLanguage()
  const [draft, setDraft] = useState(getApiAccess)
  const field = (key, type = 'text') => (
    <label className="gov-field">
      <span>{t(`gov.access.${key}`)}</span>
      <input
        type={type}
        value={draft[key]}
        autoComplete="off"
        spellCheck={false}
        onChange={(event) => setDraft({ ...draft, [key]: event.target.value.trim() })}
      />
    </label>
  )
  return (
    <details className="gov-access">
      <summary>
        <KeyRound size={13} aria-hidden="true" />
        {t('gov.access.title')} · {tenantLabel(getApiAccess().tenant, t)}
      </summary>
      <form
        onSubmit={(event) => {
          event.preventDefault()
          setApiAccess(draft)
          onApply()
        }}
      >
        <div className="gov-access__fields">
          {field('tenant')}
          {field('token', 'password')}
          {field('actor')}
        </div>
        <p className="monitor-caveat">{t('gov.access.note')}</p>
        <button className="button button--tool-secondary button--compact" type="submit">
          {t('gov.access.apply')}
        </button>
      </form>
    </details>
  )
}

// ── Differential-privacy ledger (#110) ──────────────────────────────────────

function useNow(active) {
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    if (!active) return undefined
    const id = window.setInterval(() => setNow(Date.now()), 1000)
    return () => window.clearInterval(id)
  }, [active])
  return now
}

function countdown(seconds) {
  const s = Math.max(0, Math.round(seconds))
  const h = Math.floor(s / 3600)
  const m = Math.floor((s % 3600) / 60)
  return `${h ? `${h}h ` : ''}${m}m ${s % 60}s`
}

function DpLedgerPanel({ revision }) {
  const { t } = useLanguage()
  const time = useLocaleTime()
  const [state, reload, replace] = useServerData(getDpBudget, revision)
  const [historyState, reloadResets] = useServerData(getDpResetHistory, revision)
  const [confirming, setConfirming] = useState(false)
  const [busy, setBusy] = useState(false)
  const [notice, setNotice] = useState(null)
  const [windowDraft, setWindowDraft] = useState('')
  useEffect(() => setNotice(null), [revision])
  const data = state.status === 'ready' ? state.data : null
  const windowed = Number(data?.window_secs) > 0 && Number(data?.resets_at) > 0
  const now = useNow(windowed)
  // Past resets_at the shown spend belongs to the previous window; read the new one.
  // Once per boundary: a browser clock ahead of the server gets the same resets_at
  // back, and re-reading it on every answer would loop.
  const rolled = windowed && now >= data.resets_at * 1000
  const reloadedFor = useRef(null)
  useEffect(() => {
    if (!rolled || reloadedFor.current === data.resets_at) return
    reloadedFor.current = data.resets_at
    reload()
  }, [rolled, reload, data?.resets_at])

  const reset = async () => {
    const accessAtStart = getApiAccess()
    setBusy(true)
    try {
      const after = await resetDpBudget()
      if (getApiAccess() === accessAtStart) {
        setNotice({ tone: 'ok', text: (tr) => tr('gov.dp.resetDone').replace('{spent}', num(after?.spent_epsilon)) })
        reloadResets()
      }
    } catch (error) {
      if (getApiAccess() === accessAtStart) {
        setNotice({ tone: 'error', text: (tr) => `${tr('gov.dp.resetFailed')}: ${error.message}` })
      }
    } finally {
      setBusy(false)
      setConfirming(false)
      if (getApiAccess() === accessAtStart) reload()
    }
  }

  const changeWindow = async (action) => {
    const accessAtStart = getApiAccess()
    setBusy(true)
    try {
      const after = await action()
      if (getApiAccess() === accessAtStart) {
        replace(after)
        setNotice({ tone: 'ok', text: (tr) => tr('gov.dp.windowSaved') })
      }
    } catch (error) {
      if (getApiAccess() === accessAtStart) {
        setNotice({ tone: 'error', text: () => error.message })
      }
    } finally {
      setBusy(false)
    }
  }

  const saveWindow = (event) => {
    event.preventDefault()
    const secs = Number(windowDraft)
    if (!/^\d+$/.test(windowDraft.trim()) || !Number.isSafeInteger(secs)) {
      setNotice({ tone: 'error', text: (tr) => tr('gov.dp.windowInvalid') })
      return
    }
    changeWindow(() => putDpWindow(secs))
  }

  const total = Number(data?.total_epsilon)
  const spent = Number(data?.spent_epsilon)
  const pct = total > 0 ? Math.min(100, Math.max(0, (spent / total) * 100)) : 0

  return (
    <MonitorPanel
      contract={data ? 'measured' : 'implemented'}
      icon={Scale}
      title={t('gov.dp.title')}
      unit={t('gov.dp.unit')}
      maturity="Beta"
      scope={
        data
          ? t('gov.dp.scope').replace('{tenant}', tenantLabel(data.tenant, t))
          : t('gov.dp.scopeEmpty')
      }
    >
      {!data ? (
        <PanelStatus state={state} onRetry={reload} />
      ) : (
        <>
          <div
            className="monitor-budget monitor-budget--measured"
            role="img"
            aria-label={t('gov.dp.trackLabel')
              .replace('{spent}', num(spent))
              .replace('{total}', num(total))}
          >
            <div className="monitor-budget__track">
              <i style={{ '--bar-width': `${pct}%` }} />
            </div>
            <div className="monitor-budget__legend">
              <span>
                {num(spent)} / {num(total)} ε {t('gov.dp.spent')}
              </span>
              <span>
                {num(data.remaining_epsilon)} ε {t('gov.dp.left')}
              </span>
            </div>
          </div>
          <dl className="monitor-facts">
            <div>
              <dt>δ {t('gov.dp.spent')}</dt>
              <dd>
                {num(data.spent_delta)} / {num(data.total_delta)}
              </dd>
            </div>
            <div>
              <dt>{t('gov.dp.window')}</dt>
              <dd>
                {Number(data.window_secs) > 0
                  ? `${data.window_secs}s · ${data.window_source}`
                  : `${t('gov.dp.noWindow')} · ${data.window_source}`}
              </dd>
            </div>
            <div>
              <dt>{t('gov.dp.resetsAt')}</dt>
              <dd>
                {windowed
                  ? `${time(data.resets_at)} · ${countdown(data.resets_at - now / 1000)}`
                  : t('gov.dp.manualOnly')}
              </dd>
            </div>
            <div>
              <dt>{t('gov.dp.reserved')}</dt>
              <dd>
                {data.reserved_epsilon !== undefined
                  ? `${num(data.reserved_epsilon)} ε`
                  : t('gov.dp.reservedNa')}
              </dd>
            </div>
          </dl>
          <form className="gov-inline-form" noValidate onSubmit={saveWindow}>
            <label className="gov-field">
              <span>{t('gov.dp.windowInput')}</span>
              <input
                type="number"
                min="0"
                step="1"
                value={windowDraft}
                onChange={(event) => setWindowDraft(event.target.value)}
              />
            </label>
            <button className="button button--tool-secondary button--compact" type="submit" disabled={busy}>
              {t('gov.dp.windowSave')}
            </button>
            <button
              className="button button--tool-secondary button--compact"
              type="button"
              disabled={busy || data.window_source !== 'tenant'}
              onClick={() => changeWindow(deleteDpWindow)}
            >
              {t('gov.dp.windowClear')}
            </button>
          </form>
          <p className="monitor-caveat">{t('gov.dp.windowNote')}</p>
          <div className="gov-actions">
            <RefreshButton onClick={reload} label={t('gov.refresh')} />
            <button
              className="button button--tool-secondary button--compact"
              type="button"
              onClick={() => setConfirming(true)}
            >
              {t('gov.dp.reset')}
            </button>
          </div>
          <div className="gov-subsection">
            <strong><FileClock size={13} aria-hidden="true" /> {t('gov.dp.history')}</strong>
            {historyState.status !== 'ready' ? (
              <PanelStatus state={historyState} onRetry={reloadResets} lines={2} />
            ) : historyState.data?.resets?.length ? (
              <div className="gov-table-wrap">
                <table className="gov-table">
                  <caption className="sr-only">{t('gov.dp.history')}</caption>
                  <thead>
                    <tr>
                      <th scope="col">{t('gov.dp.historyWhen')}</th>
                      <th scope="col">{t('gov.dp.historyActor')}</th>
                      <th scope="col">{t('gov.dp.historyBefore')} ε</th>
                      <th scope="col">{t('gov.dp.historyBefore')} δ</th>
                    </tr>
                  </thead>
                  <tbody>
                    {historyState.data.resets.map((entry) => (
                      <tr key={entry.id}>
                        <td>{time(entry.reset_at)}</td>
                        <td>{entry.actor || t('gov.defaultTenant')}</td>
                        <td>{num(entry.spent_epsilon_before)}</td>
                        <td>{num(entry.spent_delta_before)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            ) : (
              <p className="monitor-empty">{t('gov.dp.historyEmpty')}</p>
            )}
          </div>
        </>
      )}
      {notice && (
        <p className={`gov-notice gov-notice--${notice.tone}`} role="status">
          {notice.text(t)}
        </p>
      )}
      <ConfirmDialog
        open={confirming}
        title={t('gov.dp.confirmTitle')}
        body={t('gov.dp.confirmBody').replace('{tenant}', tenantLabel(data?.tenant, t))}
        confirmLabel={t('gov.dp.reset')}
        onConfirm={reset}
        onCancel={() => setConfirming(false)}
        busy={busy}
      />
    </MonitorPanel>
  )
}

// ── Audit hash chain (#108) ─────────────────────────────────────────────────

async function loadAudit() {
  const [log, chain] = await Promise.all([getAuditLog(), verifyAuditChain()])
  const records = Array.isArray(log?.records) ? log.records : []
  // The replay needs Web Crypto, which browsers only expose on HTTPS or localhost
  // (not e.g. http://<LAN-IP> with XAZZ_BIND=0.0.0.0). Its absence must not hide
  // the server's verdict.
  let firstBreak = null
  let replayUnavailable = false
  try {
    firstBreak = await findFirstBreak(records)
  } catch {
    replayUnavailable = true
  }
  return { records, intact: chain?.intact === true, firstBreak, replayUnavailable }
}

function AuditChainPanel({ revision }) {
  const { t } = useLanguage()
  const time = useLocaleTime()
  const [state, reload] = useServerData(loadAudit, revision)
  const [query, setQuery] = useState('')
  const [lookup, setLookup] = useState(null)
  const data = state.status === 'ready' ? state.data : null
  const brokenAt = data?.firstBreak?.position

  const find = async (event) => {
    event.preventDefault()
    if (!query) return
    try {
      const found = await getAuditRecords(query)
      setLookup({ tone: 'ok', text: (tr) => tr('gov.audit.lookupFound').replace('{n}', found?.matches ?? 0) })
    } catch (error) {
      setLookup({
        tone: 'error',
        text: (tr) =>
          error instanceof ApiError && error.status === 404 ? tr('gov.audit.lookupNone') : error.message,
      })
    }
  }

  const verdict = !data ? null : data.intact ? (
    <p className="gov-verdict gov-verdict--ok">
      <StatusBadge axis="Integrity" tone="success">
        Verified
      </StatusBadge>
      {t('gov.audit.intact').replace('{n}', data.records.length)}
    </p>
  ) : (
    <div className="gov-verdict gov-verdict--broken" role="alert">
      <StatusBadge axis="Integrity" tone="danger">
        Mismatch
      </StatusBadge>
      <p>
        {data.firstBreak
          ? t(`gov.audit.break.${data.firstBreak.reason}`).replace(/\{index\}/g, data.firstBreak.index)
          : data.replayUnavailable
            ? t('gov.audit.replayUnavailable')
            : t('gov.audit.breakUnlocated')}
      </p>
    </div>
  )

  return (
    <MonitorPanel
      contract={data ? 'measured' : 'implemented'}
      icon={Link2}
      title={t('gov.audit.title')}
      unit="SHA-256 · append-only · prev_hash → record_hash"
      maturity="Beta"
      scope={t('gov.audit.scope')}
    >
      {!data ? (
        <PanelStatus state={state} onRetry={reload} />
      ) : (
        <>
          {verdict}
          {data.intact && data.firstBreak && (
            <p className="monitor-caveat">
              {t('gov.audit.replayDisagrees').replace('{index}', data.firstBreak.index)}
            </p>
          )}
          {data.records.length === 0 ? (
            <p className="monitor-empty">{t('gov.audit.empty')}</p>
          ) : (
            <div className="gov-table-wrap">
              <table className="gov-table">
                <caption className="sr-only">{t('gov.audit.title')}</caption>
                <thead>
                  <tr>
                    <th scope="col">#</th>
                    <th scope="col">{t('gov.audit.when')}</th>
                    <th scope="col">{t('gov.audit.outcome')}</th>
                    <th scope="col">code hash</th>
                    <th scope="col">prev_hash</th>
                    <th scope="col">record_hash</th>
                  </tr>
                </thead>
                <tbody>
                  {data.records
                    .map((record, position) => ({ record, position }))
                    .reverse()
                    .slice(0, 50)
                    .map(({ record, position }) => (
                      // A tampered log can repeat an index, so position is the identity.
                      <tr key={position} className={position === brokenAt ? 'is-broken' : ''}>
                        <td>{record.index}</td>
                        <td>{time(record.timestamp)}</td>
                        <td>
                          {record.outcome ?? '—'}
                          {record.prompt_hash && (
                            <span
                              className="gov-tag"
                              title={`prompt ${record.prompt_hash}\nresponse ${record.response_hash}\nmodel ${record.model_fingerprint ?? '—'}`}
                            >
                              {t('gov.audit.inference')}
                            </span>
                          )}
                        </td>
                        <td>
                          <code title={record.hash}>{short(record.hash)}</code>
                        </td>
                        <td>
                          <code title={record.prev_hash}>{short(record.prev_hash)}</code>
                        </td>
                        <td>
                          <code title={record.record_hash}>{short(record.record_hash)}</code>
                        </td>
                      </tr>
                    ))}
                </tbody>
              </table>
              {data.records.length > 50 && (
                <p className="monitor-caveat">
                  {t('gov.audit.truncated').replace('{n}', data.records.length)}
                </p>
              )}
            </div>
          )}
          <p className="monitor-caveat">{t('gov.audit.inferenceNote')}</p>
          <form className="gov-inline-form" onSubmit={find}>
            <label className="gov-field">
              <span>{t('gov.audit.lookup')}</span>
              <input
                value={query}
                spellCheck={false}
                placeholder="sha256…"
                onChange={(event) => setQuery(event.target.value.trim())}
              />
            </label>
            <button className="button button--tool-secondary button--compact" type="submit">
              {t('gov.audit.lookupAction')}
            </button>
            <RefreshButton onClick={reload} label={t('gov.audit.verify')} />
          </form>
          {lookup && (
            <p className={`gov-notice gov-notice--${lookup.tone}`} role="status">
              {lookup.text(t)}
            </p>
          )}
        </>
      )}
    </MonitorPanel>
  )
}

// ── Runtime inference output gate (#244) ────────────────────────────────────

function InferenceCheckPanel({ onChecked }) {
  const { t } = useLanguage()
  const [draft, setDraft] = useState({ code: '', prompt: '', response: '', model_fingerprint: '' })
  const [busy, setBusy] = useState(false)
  const [result, setResult] = useState(null)
  const [error, setError] = useState(null)
  const draftVersion = useRef(0)
  const changeDraft = (name, value) => {
    draftVersion.current += 1
    setDraft((current) => ({ ...current, [name]: value }))
    setResult(null)
    setError(null)
  }
  const field = (name, rows = 2) => (
    <label className="gov-field gov-field--wide">
      <span>{t(`gov.inference.${name}`)}</span>
      {rows ? (
        <textarea rows={rows} value={draft[name]} autoComplete="off" spellCheck={false}
          onChange={(event) => changeDraft(name, event.target.value)} />
      ) : (
        <input value={draft[name]} autoComplete="off" spellCheck={false}
          onChange={(event) => changeDraft(name, event.target.value)} />
      )}
    </label>
  )
  const submit = async (event) => {
    event.preventDefault()
    const submittedVersion = draftVersion.current
    setBusy(true)
    setError(null)
    setResult(null)
    try {
      const checked = await checkInference(draft)
      if (draftVersion.current === submittedVersion) setResult(checked)
      onChecked()
    } catch (problem) {
      if (draftVersion.current === submittedVersion) setError(problem)
    } finally {
      setBusy(false)
    }
  }

  return (
    <MonitorPanel contract="implemented" icon={ShieldHalf} title={t('gov.inference.title')}
      unit="POST /security/inference/check" maturity="Beta" scope={t('gov.inference.scope')}>
      <p className="monitor-caveat">{t('gov.inference.privacyNote')}</p>
      <form className="gov-pack-form" onSubmit={submit}>
        {field('code')}
        {field('prompt')}
        {field('response', 4)}
        {field('model_fingerprint', 0)}
        <div className="gov-actions">
          <button className="button button--tool-primary button--compact" type="submit"
            disabled={busy || !draft.code.trim() || !draft.prompt.trim() || !draft.response.trim()}>
            {busy ? t('server.loading') : t('gov.inference.check')}
          </button>
          <button className="button button--tool-secondary button--compact" type="button"
            disabled={busy}
            onClick={() => { draftVersion.current += 1; setDraft({ code: '', prompt: '', response: '', model_fingerprint: '' }); setResult(null); setError(null) }}>
            {t('gov.inference.clear')}
          </button>
        </div>
      </form>
      {error && <p className="gov-notice gov-notice--error" role="alert">{error.message}</p>}
      {result && (
        <div className="gov-subsection" role="status">
          <strong>
            <StatusBadge axis="Control" tone={result.safe_to_emit ? 'success' : 'danger'}>
              {result.safe_to_emit ? t('gov.inference.allowed') : t('gov.inference.blocked')}
            </StatusBadge>
          </strong>
          <dl className="monitor-facts">
            <div><dt>{t('gov.inference.auditIndex')}</dt><dd>{result.audit_index}</dd></div>
            <div><dt>{t('gov.inference.chain')}</dt><dd>{result.chain_valid ? t('gov.inference.valid') : t('gov.inference.invalid')}</dd></div>
            <div><dt>{t('gov.inference.promptHash')}</dt><dd className="gov-hash">{result.prompt_hash}</dd></div>
            <div><dt>{t('gov.inference.responseHash')}</dt><dd className="gov-hash">{result.response_hash}</dd></div>
          </dl>
          <div className="gov-subsection">
            <strong>{t('gov.inference.findings')}</strong>
            {result.findings?.length ? (
              <ul className="gov-timeline">
                {result.findings.map((finding, index) => (
                  <li key={`${finding.kind}-${finding.line}-${finding.col}-${index}`}>
                    <code>{finding.kind}</code>
                    <span>{finding.line}:{finding.col} · {finding.redacted}</span>
                  </li>
                ))}
              </ul>
            ) : <p className="monitor-empty">{t('gov.inference.noFindings')}</p>}
          </div>
        </div>
      )}
    </MonitorPanel>
  )
}

// ── Policy packs (#109) ─────────────────────────────────────────────────────

const loadPolicyHistory = () =>
  Promise.all([getPolicyHistory({ limit: 20 }), getPolicyTtl()]).then(([page, ttl]) => ({
    entries: page?.history ?? [],
    ttl,
  }))

function packName(json) {
  if (!json || typeof json !== 'object') return '—'
  return `${json.id ?? '?'}@${json.version ?? '?'}`
}

function PolicyPackPanel({ revision, onPolicyChange }) {
  const { t } = useLanguage()
  const time = useLocaleTime()
  const [policyState, reloadPolicy] = useServerData(getPolicy, revision)
  const [historyState, reloadHistory] = useServerData(loadPolicyHistory, revision)
  const [ttlOffset, setTtlOffset] = useState(0)
  useEffect(() => setTtlOffset(0), [revision])
  const [ttlHistoryState, reloadTtlHistory] = useServerData(
    () => getPolicyTtlHistory({ offset: ttlOffset }), `${revision}:${ttlOffset}`,
  )
  const [draft, setDraft] = useState('')
  const [notice, setNotice] = useState(null)
  const [confirming, setConfirming] = useState(false)
  const [busy, setBusy] = useState(false)
  const [ttlDraft, setTtlDraft] = useState('')
  const active = policyState.status === 'ready' ? policyState.data : null
  const tenantPack = typeof active?.origin === 'string' && active.origin.startsWith('tenant:')
  const loadFailed = policyState.status === 'error' && policyState.error.status === 500

  const reloadAll = () => {
    reloadPolicy()
    reloadHistory()
  }

  const refreshTtlHistory = () => {
    if (ttlOffset === 0) reloadTtlHistory()
    else setTtlOffset(0)
  }

  const act = async (action, success) => {
    setBusy(true)
    try {
      const result = await action()
      setNotice({ tone: 'ok', text: (tr) => success(result, tr) })
      return true
    } catch (error) {
      setNotice({ tone: 'error', text: () => error.message })
      return false
    } finally {
      setBusy(false)
      reloadAll()
    }
  }

  const install = (event) => {
    event.preventDefault()
    let pack
    try {
      pack = JSON.parse(draft)
    } catch (error) {
      setNotice({ tone: 'error', text: (tr) => `${tr('gov.policy.invalidJson')}: ${error.message}` })
      return
    }
    act(() => putPolicy(pack), (result, tr) =>
      tr('gov.policy.installed').replace('{pack}', packName(result?.policy)),
    ).then((ok) => ok && onPolicyChange())
  }

  const readFile = async (event) => {
    const file = event.target.files?.[0]
    event.target.value = ''
    if (file) setDraft(await file.text())
  }

  const history = historyState.status === 'ready' ? historyState.data.entries : null
  const ttl = historyState.status === 'ready' ? historyState.data.ttl : null

  return (
    <MonitorPanel
      contract={active ? 'measured' : 'implemented'}
      icon={ShieldHalf}
      title={t('gov.policy.title')}
      unit={active ? packName(active.policy) : 'PUT · DELETE /security/policy'}
      maturity="Beta"
      scope={t('gov.policy.scope').replace(
        '{tenant}',
        tenantLabel(active?.tenant ?? getApiAccess().tenant, t),
      )}
    >
      {loadFailed ? (
        <div className="gov-verdict gov-verdict--broken" role="alert">
          <TriangleAlert size={15} aria-hidden="true" />
          <p>
            <strong>{t('gov.policy.failClosed')}</strong> {policyState.error.message}
          </p>
        </div>
      ) : !active ? (
        <PanelStatus state={policyState} onRetry={reloadPolicy} lines={2} />
      ) : (
        <dl className="monitor-facts">
          <div>
            <dt>{t('gov.policy.active')}</dt>
            <dd>{packName(active.policy)}</dd>
          </div>
          <div>
            <dt>{t('gov.policy.source')}</dt>
            <dd>
              {tenantPack ? t('gov.policy.tenantPack') : t('gov.policy.globalPolicy')} ·{' '}
              <code>{active.origin}</code>
            </dd>
          </div>
          <div>
            <dt>domain · risk</dt>
            <dd>
              {active.policy?.domain ?? '—'} · {active.policy?.risk_level ?? '—'}
            </dd>
          </div>
        </dl>
      )}

      <form className="gov-pack-form" onSubmit={install}>
        <label className="gov-field gov-field--wide">
          <span>{t('gov.policy.packJson')}</span>
          <textarea
            rows={4}
            value={draft}
            spellCheck={false}
            placeholder='{"id": "…", "version": "…", …}'
            onChange={(event) => setDraft(event.target.value)}
          />
        </label>
        <div className="gov-actions">
          <label className="button button--tool-secondary button--compact gov-file">
            {t('gov.policy.chooseFile')}
            <input type="file" accept=".json,application/json" onChange={readFile} />
          </label>
          <button
            className="button button--tool-secondary button--compact"
            type="button"
            disabled={!active}
            onClick={() => setDraft(JSON.stringify(active.policy, null, 2))}
          >
            {t('gov.policy.fromActive')}
          </button>
          <button
            className="button button--tool-primary button--compact"
            type="submit"
            disabled={busy || !draft.trim()}
          >
            {t('gov.policy.install')}
          </button>
          <button
            className="button button--tool-secondary button--compact"
            type="button"
            disabled={busy || !tenantPack}
            onClick={() => setConfirming(true)}
          >
            {t('gov.policy.remove')}
          </button>
        </div>
      </form>
      <p className="monitor-caveat">{t('gov.policy.adminNote')}</p>
      {notice && (
        <p className={`gov-notice gov-notice--${notice.tone}`} role="status">
          {notice.text(t)}
        </p>
      )}

      <div className="gov-subsection">
        <strong>
          <FileClock size={13} aria-hidden="true" /> {t('gov.policy.history')}
        </strong>
        {!history ? (
          <PanelStatus state={historyState} onRetry={reloadHistory} lines={2} />
        ) : history.length === 0 ? (
          <p className="monitor-empty">{t('gov.policy.historyEmpty')}</p>
        ) : (
          <ol className="gov-timeline">
            {history.map((entry) => (
              <li key={entry.id}>
                <code>{entry.action}</code>
                <span>
                  {entry.action === 'delete'
                    ? packName(entry.old_policy_json)
                    : packName(entry.new_policy_json)}
                </span>
                <span>
                  {entry.changed_by || t('gov.defaultTenant')} · {time(entry.changed_at)}
                </span>
              </li>
            ))}
          </ol>
        )}
      </div>

      {ttl && (
        <form
          className="gov-inline-form"
          onSubmit={(event) => {
            event.preventDefault()
            const secs = Number(ttlDraft)
            // Number('') is 0, which the server reads as "keep forever".
            if (ttlDraft.trim() === '' || !Number.isInteger(secs) || secs < 0) {
              setNotice({ tone: 'error', text: (tr) => tr('gov.policy.ttlInvalid') })
              return
            }
            act(() => putPolicyTtl(secs), (_, tr) => tr('gov.policy.ttlSaved'))
              .then((ok) => ok && refreshTtlHistory())
          }}
        >
          <label className="gov-field">
            <span>
              {t('gov.policy.ttl')} · {ttl.ttl_secs}s · {ttl.ttl_source}
            </span>
            <input
              type="number"
              min="0"
              step="1"
              value={ttlDraft}
              placeholder={t('gov.policy.ttlPlaceholder')}
              onChange={(event) => setTtlDraft(event.target.value)}
            />
          </label>
          <button
            className="button button--tool-secondary button--compact"
            type="submit"
            disabled={busy || ttlDraft.trim() === ''}
          >
            {t('gov.policy.ttlSave')}
          </button>
          <button
            className="button button--tool-secondary button--compact"
            type="button"
            disabled={busy || ttl.ttl_source !== 'tenant'}
            onClick={() => act(deletePolicyTtl, (_, tr) => tr('gov.policy.ttlCleared'))
              .then((ok) => ok && refreshTtlHistory())}
          >
            {t('gov.policy.ttlClear')}
          </button>
        </form>
      )}

      <div className="gov-subsection">
        <strong><FileClock size={13} aria-hidden="true" /> {t('gov.policy.ttlHistory')}</strong>
        {ttlHistoryState.status !== 'ready' ? (
          <PanelStatus state={ttlHistoryState} onRetry={reloadTtlHistory} lines={2} />
        ) : ttlHistoryState.data?.history?.length ? (
          <div className="gov-table-wrap">
            <table className="gov-table">
              <caption className="sr-only">{t('gov.policy.ttlHistory')}</caption>
              <thead><tr>
                <th scope="col">{t('gov.policy.ttlWhen')}</th>
                <th scope="col">{t('gov.policy.ttlActor')}</th>
                <th scope="col">{t('gov.policy.ttlAction')}</th>
                <th scope="col">{t('gov.policy.ttlOld')}</th>
                <th scope="col">{t('gov.policy.ttlNew')}</th>
              </tr></thead>
              <tbody>
                {ttlHistoryState.data.history.map((entry) => (
                  <tr key={entry.id}>
                    <td>{time(entry.changed_at)}</td>
                    <td>{entry.changed_by || t('gov.defaultTenant')}</td>
                    <td>{entry.action}</td>
                    <td>{entry.old_ttl_secs ?? '—'}</td>
                    <td>{entry.new_ttl_secs ?? '—'}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <p className="monitor-empty">{t('gov.policy.ttlHistoryEmpty')}</p>
        )}
        <div className="gov-actions">
          <button className="button button--tool-secondary button--compact" type="button"
            disabled={ttlOffset === 0} onClick={() => setTtlOffset(Math.max(0, ttlOffset - 20))}>
            {t('gov.policy.previous')}
          </button>
          <span>{t('gov.policy.ttlPage').replace('{n}', String(Math.floor(ttlOffset / 20) + 1))}</span>
          <button className="button button--tool-secondary button--compact" type="button"
            disabled={ttlHistoryState.status !== 'ready' || (ttlHistoryState.data?.history?.length ?? 0) < 20}
            onClick={() => setTtlOffset(ttlOffset + 20)}>
            {t('gov.policy.next')}
          </button>
        </div>
      </div>

      <ConfirmDialog
        open={confirming}
        title={t('gov.policy.confirmTitle')}
        body={t('gov.policy.confirmBody').replace('{tenant}', tenantLabel(active?.tenant, t))}
        confirmLabel={t('gov.policy.remove')}
        onConfirm={async () => {
          const ok = await act(deletePolicy, (result, tr) =>
            result?.deleted ? tr('gov.policy.removed') : tr('gov.policy.nothingRemoved'),
          )
          if (ok) onPolicyChange()
          setConfirming(false)
        }}
        onCancel={() => setConfirming(false)}
        busy={busy}
      />
    </MonitorPanel>
  )
}

/**
 * Live governance state read straight from xazz-server. Unlike the run panels above
 * it, nothing here depends on a Full Run: it is what the tenant allows and what the
 * server has recorded. `revision` bumps after each run and each access change.
 */
export function GovernanceSection({ revision, onAccessChange, onPolicyChange }) {
  const { t } = useLanguage()
  const [auditRevision, setAuditRevision] = useState(0)
  return (
    <section className="gov-section" aria-labelledby="gov-heading">
      <header className="gov-section__head">
        <h2 id="gov-heading">{t('gov.heading')}</h2>
        <p>{t('gov.subheading')}</p>
      </header>
      <AccessPanel onApply={onAccessChange} />
      <DpLedgerPanel revision={revision} />
      <AuditChainPanel revision={`${revision}:${auditRevision}`} />
      <InferenceCheckPanel onChecked={() => setAuditRevision((value) => value + 1)} />
      <PolicyPackPanel revision={revision} onPolicyChange={onPolicyChange} />
    </section>
  )
}
