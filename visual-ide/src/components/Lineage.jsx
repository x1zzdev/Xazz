import React, { useState } from 'react'
import { ArrowRight, GitBranch } from 'lucide-react'
import { ServerProblem, Skeleton } from './Common'
import { useLanguage } from '../i18n'
import { ApiError, fetchCatalog } from '../api'

function LineageTable({ entry, selected, onSelect }) {
  const { t } = useLanguage()
  return (
    <table className="lineage-table">
      <thead>
        <tr>
          <th scope="col">{t('lineage.output')}</th>
          <th scope="col">{t('lineage.from')}</th>
          <th scope="col">{t('lineage.step')}</th>
        </tr>
      </thead>
      <tbody>
        {entry.lineage.map((item) => (
          <tr key={item.column} className={selected === item.column ? 'is-selected' : ''}>
            <td>
              <button type="button" aria-pressed={selected === item.column} onClick={() => onSelect(item.column)}>
                {item.column}
              </button>
            </td>
            <td>{item.from?.length ? item.from.join(', ') : t('lineage.derived')}</td>
            <td>{item.created_by_step === 0 ? t('lineage.source') : item.created_by_step}</td>
          </tr>
        ))}
      </tbody>
    </table>
  )
}

// One row per output column: its source chips → the column. A selected column
// highlights its own row and every chip that names one of its sources, so shared
// sources light up across rows.
function LineageDiagram({ entry, selected, onSelect }) {
  const { t } = useLanguage()
  const highlighted = new Set(entry.lineage.find((item) => item.column === selected)?.from ?? [])
  return (
    <ol className="lineage-diagram" aria-label={t('lineage.diagram')}>
      {entry.lineage.map((item) => (
        <li key={item.column} className={selected === item.column ? 'is-selected' : ''}>
          <span className="lineage-diagram__sources">
            {item.from?.length ? (
              item.from.map((source) => (
                <code key={source} className={highlighted.has(source) ? 'is-source' : ''}>
                  {source}
                </code>
              ))
            ) : (
              <em>{t('lineage.derived')}</em>
            )}
          </span>
          <ArrowRight size={12} aria-hidden="true" />
          <button type="button" aria-pressed={selected === item.column} onClick={() => onSelect(item.column)}>
            {item.column}
          </button>
        </li>
      ))}
    </ol>
  )
}

/**
 * Column lineage (#116) from POST /catalog — a static compile of the code the Full
 * Run would send, so it can be traced before or after running. Table first: it is
 * the exact list; the diagram is the same data drawn as rows.
 */
export function LineagePanel({ code }) {
  const { t } = useLanguage()
  const [state, setState] = useState({ status: 'idle' })
  const [view, setView] = useState('table')
  const [pipelineId, setPipelineId] = useState(null)
  const [selected, setSelected] = useState(null)

  const trace = async () => {
    setState({ status: 'loading' })
    try {
      const catalog = await fetchCatalog(code)
      const pipelines = catalog?.pipelines ?? []
      setState({ status: 'ready', pipelines, tracedCode: code })
      setPipelineId(pipelines.at(-1)?.id ?? null)
      setSelected(null)
    } catch (error) {
      setState({ status: error instanceof ApiError ? 'error' : 'offline', error })
    }
  }

  const entry = state.pipelines?.find((pipeline) => pipeline.id === pipelineId)

  return (
    <section className="lineage-panel" aria-label={t('lineage.title')}>
      <h3>
        <GitBranch size={13} aria-hidden="true" /> {t('lineage.title')}
      </h3>
      <p className="lineage-panel__hint">{t('lineage.hint')}</p>
      <button className="button button--tool-secondary button--compact" type="button" onClick={trace}>
        {state.status === 'ready' ? t('lineage.retrace') : t('lineage.trace')}
      </button>
      {state.status === 'loading' && <Skeleton lines={3} label={t('server.loading')} />}
      {(state.status === 'error' || state.status === 'offline') && <ServerProblem state={state} />}
      {state.status === 'ready' && (
        <>
          {state.tracedCode !== code && <p className="lineage-panel__stale">{t('lineage.stale')}</p>}
          {state.pipelines.length === 0 ? (
            <p className="lineage-panel__hint">{t('lineage.empty')}</p>
          ) : (
            <>
              <div className="lineage-panel__controls">
                {state.pipelines.length > 1 && (
                  <label>
                    <span>{t('lineage.pipeline')}</span>
                    <select
                      value={pipelineId ?? ''}
                      onChange={(event) => {
                        setPipelineId(Number(event.target.value))
                        setSelected(null)
                      }}
                    >
                      {state.pipelines.map((pipeline) => (
                        <option key={pipeline.id} value={pipeline.id}>
                          {pipeline.name ?? `#${pipeline.id}`}
                        </option>
                      ))}
                    </select>
                  </label>
                )}
                <div className="segmented-control segmented-control--compact" aria-label={t('lineage.title')}>
                  {['table', 'diagram'].map((id) => (
                    <button
                      key={id}
                      type="button"
                      className={view === id ? 'is-active' : ''}
                      aria-pressed={view === id}
                      onClick={() => setView(id)}
                    >
                      {t(`lineage.${id}`)}
                    </button>
                  ))}
                </div>
              </div>
              <p className="lineage-panel__hint">{t('lineage.select')}</p>
              {entry &&
                (view === 'table' ? (
                  <LineageTable entry={entry} selected={selected} onSelect={setSelected} />
                ) : (
                  <LineageDiagram entry={entry} selected={selected} onSelect={setSelected} />
                ))}
            </>
          )}
        </>
      )}
    </section>
  )
}
