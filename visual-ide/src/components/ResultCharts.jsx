import React, { useState } from 'react'
import { StatusBadge } from './Common'
import { useLanguage } from '../i18n'
import { fiveNumber, histogram, numericValues } from '../chartStats'

const isNumeric = (col) => /f64|f32|float|int|^[ui]\d+$/i.test(col.type)
const isText = (col) => /str|string/i.test(col.type)
const fmt = (value) => (Number.isInteger(value) ? String(value) : value.toFixed(2))
const MAX_GROUPS = 8

// Builders return { title, note?, plot, table } for one view; plain functions, not components.
function meanByGroup(rows, xKey, yKey, t) {
  const grouped = {}
  for (const row of rows) {
    const value = Number(row[yKey])
    if (Number.isFinite(value)) (grouped[String(row[xKey] ?? '—')] ??= []).push(value)
  }
  const bars = Object.entries(grouped)
    .map(([label, values]) => ({ label, mean: values.reduce((a, b) => a + b, 0) / values.length }))
    .slice(0, 10)
  const max = Math.max(...bars.map((item) => item.mean))
  return {
    title: t('charts.meanTitle').replace('{y}', yKey).replace('{x}', xKey),
    plot: (
      <div
        className="bar-chart"
        role="img"
        aria-label={`Mean ${yKey} ranges from ${Math.min(...bars.map((item) => item.mean))} to ${max} across ${bars.length} groups.`}
      >
        {bars.map((item) => (
          <div className="bar-chart__row" key={item.label} title={`${item.label}: ${item.mean.toFixed(4)}`}>
            <span>{item.label}</span>
            <i style={{ '--bar-width': `${(item.mean / max) * 100}%` }} />
            <strong>{item.mean.toFixed(2)}</strong>
          </div>
        ))}
      </div>
    ),
    table: [[xKey, `mean ${yKey}`], ...bars.map((item) => [item.label, item.mean.toFixed(4)])],
  }
}

function distribution(rows, column, t) {
  const { values, missing } = numericValues(rows, column)
  const bins = histogram(values)
  const peak = Math.max(...bins.map((bin) => bin.count))
  const range = (bin) => `${fmt(bin.start)}–${fmt(bin.end)}`
  return {
    title: t('charts.distributionTitle').replace('{column}', column),
    note: t('charts.sample').replace('{n}', values.length).replace('{missing}', missing),
    plot: (
      <div className="histogram">
        <div
          className="histogram__bins"
          role="img"
          aria-label={t('charts.distributionLabel')
            .replace('{column}', column)
            .replace('{bins}', bins.length)
            .replace('{n}', values.length)}
        >
          {bins.map((bin) => (
            <div className="histogram__bin" key={bin.start} title={`${range(bin)}: ${bin.count}`}>
              {bin.count === peak && <strong>{bin.count}</strong>}
              <i style={{ '--bar-height': `${(bin.count / peak) * 100}%` }} />
            </div>
          ))}
        </div>
        <div className="histogram__axis" aria-hidden="true">
          <span>{fmt(bins[0].start)}</span>
          <span>{fmt(bins.at(-1).end)}</span>
        </div>
      </div>
    ),
    table: [[column, t('charts.rows')], ...bins.map((bin) => [range(bin), bin.count])],
  }
}

function boxPlot(rows, column, group, t) {
  const byGroup = new Map()
  for (const row of rows) {
    const key = group ? String(row[group] ?? '—') : t('charts.allRows')
    if (!byGroup.has(key)) byGroup.set(key, [])
    byGroup.get(key).push(row)
  }
  const boxes = [...byGroup]
    .map(([label, groupRows]) => ({ label, values: numericValues(groupRows, column).values }))
    .filter((item) => item.values.length > 0)
    .sort((a, b) => b.values.length - a.values.length)
    .slice(0, MAX_GROUPS)
    .map((item) => ({ label: item.label, ...fiveNumber(item.values) }))
  const low = Math.min(...boxes.map((box) => box.min))
  const high = Math.max(...boxes.map((box) => box.max))
  const at = (value) => `${high === low ? 50 : ((value - low) / (high - low)) * 100}%`
  const summary = (box) =>
    `${box.label} · n ${box.n} · min ${fmt(box.min)} · Q1 ${fmt(box.q1)} · median ${fmt(box.median)} · Q3 ${fmt(box.q3)} · max ${fmt(box.max)}` +
    (box.outliers.length ? ` · ${box.outliers.length} outlier(s)` : '')
  return {
    title: group ? t('charts.boxTitle').replace('{column}', column).replace('{group}', group) : t('charts.boxTitleAll').replace('{column}', column),
    note:
      byGroup.size > MAX_GROUPS
        ? t('charts.groupsShown').replace('{shown}', MAX_GROUPS).replace('{total}', byGroup.size)
        : t('charts.boxNote'),
    plot: (
      <div className="boxplot" role="img" aria-label={boxes.map(summary).join('; ')}>
        {boxes.map((box) => (
          <div className="boxplot__row" key={box.label} title={summary(box)}>
            <span>{box.label}</span>
            <div className="boxplot__track">
              <i className="boxplot__whisker" style={{ left: at(box.whiskerLow), right: `calc(100% - ${at(box.whiskerHigh)})` }} />
              <i className="boxplot__box" style={{ left: at(box.q1), right: `calc(100% - ${at(box.q3)})` }} />
              <i className="boxplot__median" style={{ left: at(box.median) }} />
              {box.outliers.map((value, index) => (
                <i className="boxplot__outlier" key={index} style={{ left: at(value) }} />
              ))}
            </div>
            <strong>{fmt(box.median)}</strong>
          </div>
        ))}
        <div className="histogram__axis" aria-hidden="true">
          <span>{fmt(low)}</span>
          <span>{fmt(high)}</span>
        </div>
      </div>
    ),
    table: [
      [group || '—', 'n', 'min', 'Q1', 'median', 'Q3', 'max'],
      ...boxes.map((box) => [box.label, box.n, ...[box.min, box.q1, box.median, box.q3, box.max].map(fmt)]),
    ],
  }
}

/**
 * Result charts (#174), drawn only from rows a real Full Run returned. Loss curves,
 * ROC and confusion matrices need per-epoch history and classification metrics the
 * runtime does not emit yet (#117, #129, #163), so that view says so instead.
 */
export function ResultCharts({ runResult }) {
  const { t } = useLanguage()
  const rows = runResult.rows
  const columns = Array.isArray(runResult.schema) ? runResult.schema : []
  const numeric = columns.filter(isNumeric).map((col) => col.name)
  const text = columns.filter(isText).map((col) => col.name)
  const [mode, setMode] = useState('mean')
  const [picked, setPicked] = useState('')
  // All rows by default: a result grouped one row per group has nothing to box.
  const [pickedGroup, setPickedGroup] = useState('')
  const column = numeric.includes(picked) ? picked : numeric[0]
  const group = text.includes(pickedGroup) ? pickedGroup : ''

  const view = !column
    ? null
    : mode === 'distribution'
      ? distribution(rows, column, t)
      : mode === 'box'
        ? boxPlot(rows, column, group, t)
        : mode === 'mean'
          ? meanByGroup(rows, text[0] ?? columns[0]?.name ?? '', column, t)
          : null

  return (
    <div className="chart-panel chart-panel--real">
      <div className="chart-panel__toolbar">
        <div className="segmented-control segmented-control--compact" aria-label={t('charts.modes')}>
          {['mean', 'distribution', 'box', 'training'].map((id) => (
            <button
              key={id}
              type="button"
              className={mode === id ? 'is-active' : ''}
              aria-pressed={mode === id}
              onClick={() => setMode(id)}
            >
              {t(`charts.mode.${id}`)}
            </button>
          ))}
        </div>
        {mode !== 'training' && (
          <div className="chart-panel__pickers">
            {numeric.length > 0 && (
              <label>
                <span>{t('charts.column')}</span>
                <select value={column} onChange={(event) => setPicked(event.target.value)}>
                  {numeric.map((name) => (
                    <option key={name}>{name}</option>
                  ))}
                </select>
              </label>
            )}
            {mode === 'box' && (
              <label>
                <span>{t('charts.groupBy')}</span>
                <select value={group} onChange={(event) => setPickedGroup(event.target.value)}>
                  <option value="">{t('charts.allRows')}</option>
                  {text.map((name) => (
                    <option key={name}>{name}</option>
                  ))}
                </select>
              </label>
            )}
          </div>
        )}
      </div>
      <div className="chart-panel__heading">
        <div>
          <strong>{view?.title ?? t(`charts.mode.${mode}`)}</strong>
          {mode !== 'training' && <span>{view?.note ?? t('charts.fromRun')}</span>}
        </div>
        <StatusBadge axis="View" tone="info" compact>
          {mode === 'training' ? 'Not available' : 'Real Full Run rows'}
        </StatusBadge>
      </div>
      {mode === 'training' ? (
        <p className="chart-note chart-note--inline">{t('charts.trainingNa')}</p>
      ) : !view ? (
        <p className="chart-note chart-note--inline">{t('charts.noNumeric')}</p>
      ) : (
        <>
          {view.plot}
          <details>
            <summary>{t('charts.table')}</summary>
            <table>
              <thead>
                <tr>
                  {view.table[0].map((head) => (
                    <th scope="col" key={head}>
                      {head}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {view.table.slice(1).map((cells, index) => (
                  <tr key={index}>
                    {cells.map((cell, cellIndex) => (
                      <td key={cellIndex}>{cell}</td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
          </details>
        </>
      )}
    </div>
  )
}
