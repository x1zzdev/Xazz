import React, { useState } from 'react'
import { History, RefreshCw, RotateCcw } from 'lucide-react'
import { ServerProblem, Skeleton, StatusBadge } from './Common'
import { useLanguage } from '../i18n'
import { useServerData } from '../useServerData'
import { ApiError, getAuditRecords, getRun, listRuns } from '../api'

// runs.status is the child process exit status (main.rs `output.status.success()`),
// so it belongs on the Process axis. It is never promoted to a pipeline verdict.
const processLabel = (status) => (status === 'success' ? 'Exited' : 'Exit failed')

function useRunTime() {
  const { language } = useLanguage()
  return (seconds) => new Date(seconds * 1000).toLocaleString(language === 'ko' ? 'ko-KR' : 'en-US')
}

async function loadRunDetail(id) {
  const run = await getRun(id)
  let audit
  try {
    audit = await getAuditRecords(run.code_hash)
  } catch (error) {
    audit = error instanceof ApiError && error.status === 404 ? { matches: 0, records: [] } : null
  }
  return { run, audit }
}

function RunDetail({ id, revision, currentHash, cachedResult, onRestore, onBack }) {
  const { t } = useLanguage()
  const time = useRunTime()
  const [state, reload] = useServerData(() => loadRunDetail(id), `${revision}:${id}`)

  if (state.status === 'loading') return <Skeleton lines={4} label={t('server.loading')} />
  if (state.status === 'error' && state.error.status === 404) {
    return (
      <div className="result-empty" role="note">
        <p>
          <strong>{t('history.notFound').replace('{id}', id)}</strong>
          <span>{t('history.notFoundBody')}</span>
        </p>
        <button className="button button--tool-secondary button--compact" type="button" onClick={onBack}>
          <RefreshCw size={13} aria-hidden="true" />
          {t('gov.refresh')}
        </button>
      </div>
    )
  }
  if (state.status !== 'ready') return <ServerProblem state={state} onRetry={reload} />

  const { run, audit } = state.data
  const sameCode = run.code_hash === currentHash
  const outcomes = [...new Set((audit?.records ?? []).map((record) => record.outcome ?? '—'))]

  return (
    <div className="run-detail" aria-label={t('history.detailLabel').replace('{id}', run.id)}>
      <div className="run-detail__head">
        <strong>Run #{run.id}</strong>
        <StatusBadge axis="Process" tone="neutral" compact>
          {processLabel(run.status)}
        </StatusBadge>
      </div>
      <dl className="receipt__rows">
        <div>
          <dt>{t('history.created')}</dt>
          <dd>{time(run.created_at)}</dd>
        </div>
        <div>
          <dt>{t('history.rows')}</dt>
          <dd>{run.rows}</dd>
        </div>
        <div>
          <dt>Pipeline</dt>
          <dd>{t('history.verdictNotStored')}</dd>
        </div>
        <div>
          <dt>Code hash</dt>
          <dd>
            <code title={run.code_hash}>{run.code_hash.slice(0, 20)}…</code>
            <span>{sameCode ? t('history.sameCode') : t('history.otherCode')}</span>
          </dd>
        </div>
        <div>
          <dt>{t('history.audit')}</dt>
          <dd>
            {audit === null
              ? t('history.auditUnavailable')
              : t('history.auditRecords')
                  .replace('{n}', audit.matches ?? audit.records?.length ?? 0)
                  .replace('{outcomes}', outcomes.join(', ') || '—')}
          </dd>
        </div>
        {run.error && (
          <div>
            <dt>{t('history.error')}</dt>
            <dd>{run.error}</dd>
          </div>
        )}
      </dl>
      {cachedResult ? (
        <button className="button button--tool-secondary button--compact" type="button" onClick={onRestore}>
          <RotateCcw size={13} aria-hidden="true" />
          {t('history.restore')}
        </button>
      ) : (
        <p className="delta-note">{t('history.metadataOnly')}</p>
      )}
    </div>
  )
}

/**
 * Run history (#107). xazz-server keeps metadata for the newest 50 runs per tenant
 * (no rows). A run's rows come back only if this browser session still holds its
 * /execute response — and the screen says which of the two it is showing.
 */
export function RunHistory({ revision, currentHash, sessionResults, onRestore }) {
  const { t } = useLanguage()
  const time = useRunTime()
  const [state, reload] = useServerData(listRuns, revision)
  const [selectedId, setSelectedId] = useState(null)

  if (state.status === 'loading') return <Skeleton lines={4} label={t('server.loading')} />
  if (state.status !== 'ready') return <ServerProblem state={state} onRetry={reload} />
  if (state.data.length === 0) {
    return (
      <div className="result-empty" role="note">
        <History size={16} aria-hidden="true" />
        <p>
          <strong>{t('history.emptyTitle')}</strong>
          <span>{t('history.emptyBody')}</span>
        </p>
      </div>
    )
  }

  return (
    <div className="run-history">
      <div className="run-history__list">
        <div className="run-history__meta">
          <span>{t('history.limit')}</span>
          <button className="button button--tool-secondary button--compact" type="button" onClick={reload}>
            <RefreshCw size={13} aria-hidden="true" />
            {t('gov.refresh')}
          </button>
        </div>
        <ul aria-label={t('history.listLabel')}>
          {state.data.map((run) => (
            <li key={run.id}>
              <button
                type="button"
                className={selectedId === run.id ? 'is-selected' : ''}
                aria-pressed={selectedId === run.id}
                onClick={() => setSelectedId(run.id)}
              >
                <strong>#{run.id}</strong>
                <span className={`run-history__status run-history__status--${run.status}`}>
                  {processLabel(run.status)}
                </span>
                <span>{t('history.rowsShort').replace('{n}', run.rows)}</span>
                <span>{time(run.created_at)}</span>
                <code>{run.code_hash.slice(0, 10)}</code>
              </button>
            </li>
          ))}
        </ul>
      </div>
      <div className="run-history__detail">
        {selectedId === null ? (
          <p className="delta-note">{t('history.pick')}</p>
        ) : (
          <RunDetail
            key={selectedId}
            id={selectedId}
            revision={revision}
            currentHash={currentHash}
            cachedResult={sessionResults.get(selectedId)}
            onRestore={() => onRestore(selectedId)}
            onBack={() => {
              setSelectedId(null)
              reload()
            }}
          />
        )}
      </div>
    </div>
  )
}
