import React, { useState } from 'react'
import { StatusBadge } from './Common'
import { useLanguage } from '../i18n'
import { fiveNumber, histogram, numericValues } from '../chartStats'

// Anchored: list[f64] / array[f64] contain "f64" but arrive as text, not numbers.
const isNumeric = (col) => /^(?:[fiu]\d+|float|int|integer|double)$/i.test(col.type)
const isText = (col) => /^(?:str|string|utf8)$/i.test(col.type)
// Four significant digits keep small-magnitude columns (lr, p-values) distinguishable.
const fmt = (value) => (Number.isInteger(value) ? String(value) : String(parseFloat(value.toPrecision(4))))
const MAX_GROUPS = 8
const MAX_BARS = 10

/** Rows split by the text value of `group` (Map: any label, including "constructor", is safe). */
function groupRows(rows, group) {
  const groups = new Map()
  for (const row of rows) {
    const key = String(row[group] ?? '—')
    if (!groups.has(key)) groups.set(key, [])
    groups.get(key).push(row)
  }
  return groups
}

// Builders return { title, note?, plot, table } for one view; plain functions, not components.
function meanByGroup(rows, xKey, yKey, t) {
  if (!xKey) return { title: t('charts.meanTitle').replace('{y}', yKey).replace('{x}', '—'), empty: t('charts.noGroupColumn') }
  const groups = groupRows(rows, xKey)
  // Nulls are not zeros: each mean is over the group's numeric values only.
  const bars = [...groups]
    .map(([label, groupRowsOfLabel]) => ({ label, values: numericValues(groupRowsOfLabel, yKey).values }))
    .filter((item) => item.values.length > 0)
    .map((item) => ({ label: item.label, n: item.values.length, mean: item.values.reduce((a, b) => a + b, 0) / item.values.length }))
  const shown = bars.slice(0, MAX_BARS)
  const title = t('charts.meanTitle').replace('{y}', yKey).replace('{x}', xKey)
  const table = [[xKey, 'n', `mean ${yKey}`], ...shown.map((item) => [item.label, item.n, fmt(item.mean)])]
  if (shown.length === 0) return { title, empty: t('charts.noValues').replace('{column}', yKey) }
  const note = bars.length > MAX_BARS ? t('charts.barsShown').replace('{shown}', MAX_BARS).replace('{total}', bars.length) : undefined
  // Bars start at zero; a negative mean has no length on this axis, so it is table-only.
  if (shown.some((item) => item.mean < 0)) return { title, note, empty: t('charts.negativeMeans'), table }
  const max = Math.max(...shown.map((item) => item.mean))
  return {
    title,
    note,
    plot: (
      <div
        className="bar-chart"
        role="img"
        aria-label={`Mean ${yKey} ranges from ${fmt(Math.min(...shown.map((item) => item.mean)))} to ${fmt(max)} across ${shown.length} groups.`}
      >
        {shown.map((item) => (
          <div className="bar-chart__row" key={item.label} title={`${item.label}: ${fmt(item.mean)} (n ${item.n})`}>
            <span>{item.label}</span>
            <i style={{ '--bar-width': `${max > 0 ? (item.mean / max) * 100 : 0}%` }} />
            <strong>{fmt(item.mean)}</strong>
          </div>
        ))}
      </div>
    ),
    table,
  }
}

function distribution(rows, column, t) {
  const { values, missing } = numericValues(rows, column)
  const title = t('charts.distributionTitle').replace('{column}', column)
  if (values.length === 0) return { title, empty: t('charts.noValues').replace('{column}', column) }
  const bins = histogram(values)
  const peak = Math.max(...bins.map((bin) => bin.count))
  const range = (bin) => `${fmt(bin.start)}–${fmt(bin.end)}`
  return {
    title,
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
  const byGroup = group ? groupRows(rows, group) : new Map([[t('charts.allRows'), rows]])
  const title = group
    ? t('charts.boxTitle').replace('{column}', column).replace('{group}', group)
    : t('charts.boxTitleAll').replace('{column}', column)
  const boxes = [...byGroup]
    .map(([label, groupRows]) => ({ label, values: numericValues(groupRows, column).values }))
    .filter((item) => item.values.length > 0)
    .sort((a, b) => b.values.length - a.values.length)
    .slice(0, MAX_GROUPS)
    .map((item) => ({ label: item.label, ...fiveNumber(item.values) }))
  if (boxes.length === 0) return { title, empty: t('charts.noValues').replace('{column}', column) }
  const low = Math.min(...boxes.map((box) => box.min))
  const high = Math.max(...boxes.map((box) => box.max))
  const at = (value) => `${high === low ? 50 : ((value - low) / (high - low)) * 100}%`
  const summary = (box) =>
    `${box.label} · n ${box.n} · min ${fmt(box.min)} · Q1 ${fmt(box.q1)} · median ${fmt(box.median)} · Q3 ${fmt(box.q3)} · max ${fmt(box.max)}` +
    (box.outliers.length ? ` · ${box.outliers.length} outlier(s)` : '')
  return {
    title,
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
          ? meanByGroup(rows, text[0], column, t)
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
      ) : view.empty && !view.table ? (
        <p className="chart-note chart-note--inline">{view.empty}</p>
      ) : (
        <>
          {view.plot ?? <p className="chart-note">{view.empty}</p>}
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
