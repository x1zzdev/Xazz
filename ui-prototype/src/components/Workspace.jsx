import React, { useEffect, useMemo, useRef, useState } from 'react'
import {
  Background,
  Controls,
  Handle,
  Position,
  ReactFlow,
} from '@xyflow/react'
import '@xyflow/react/dist/style.css'
import {
  AlertTriangle,
  ArrowDownToLine,
  Braces,
  Check,
  CheckCircle2,
  ChevronDown,
  CircleAlert,
  CircleDashed,
  Clock3,
  Code2,
  Database,
  Eye,
  FileCode2,
  FileText,
  FolderTree,
  Hash,
  Info,
  ListTree,
  LoaderCircle,
  LockKeyhole,
  MessageSquareText,
  PanelBottom,
  PanelRight,
  Play,
  RefreshCw,
  RotateCcw,
  Search,
  Server,
  ShieldCheck,
  Sparkles,
  Square,
  Table2,
  TerminalSquare,
  TriangleAlert,
  Workflow,
  X,
  XCircle,
} from 'lucide-react'
import { Brand, StatusBadge } from './Common'
import {
  chartData,
  codeLines,
  pipeline,
  resultRows,
  scenario,
} from '../data'

function PipelineNode({ data }) {
  const Icon =
    data.id === 'load'
      ? Database
      : data.id === 'schema'
        ? Braces
        : data.id === 'result'
          ? Table2
          : Workflow

  return (
    <div
      className={`flow-node flow-node--${data.visualState} flow-node--relation-${data.relation}`}
      aria-label={`${data.label}, ${data.evidence}, ${data.visualState}, ${data.relation}`}
    >
      {data.id !== 'load' && <Handle type="target" position={Position.Left} />}
      <div className="flow-node__top">
        <span>
          <Icon size={14} aria-hidden="true" />
          {data.stage}
        </span>
        <i>{data.order}</i>
      </div>
      <strong>{data.label}</strong>
      <span className="flow-node__evidence">{data.evidence}</span>
      <span className="flow-node__state">
        {data.relation === 'selected'
          ? data.visualState
          : `${data.relation} · ${data.visualState}`}
      </span>
      {data.id !== 'result' && <Handle type="source" position={Position.Right} />}
    </div>
  )
}

const nodeTypes = { pipeline: PipelineNode }

function getNodeRelation(id, selectedId) {
  const index = pipeline.findIndex((node) => node.id === id)
  const selectedIndex = pipeline.findIndex((node) => node.id === selectedId)
  if (index === selectedIndex) return 'selected'
  return index < selectedIndex ? 'upstream' : 'downstream'
}

function getNodeVisualState(id, selectedId, runState) {
  if (runState === 'error') {
    if (id === 'fill') return 'failed'
    if (['filter', 'result'].includes(id)) return 'stale'
  }
  if (runState === 'success') return 'success'
  if (runState === 'running') return 'unknown'
  if (id === selectedId) return 'selected'
  if (id === 'schema') return 'warning'
  return 'ready'
}

function useCodeHash() {
  const [hash, setHash] = useState('Computing…')

  useEffect(() => {
    let active = true
    const compute = async () => {
      const bytes = new TextEncoder().encode(codeLines.join('\n'))
      const digest = await crypto.subtle.digest('SHA-256', bytes)
      const value = Array.from(new Uint8Array(digest))
        .map((byte) => byte.toString(16).padStart(2, '0'))
        .join('')
      if (active) setHash(value)
    }
    compute()
    return () => {
      active = false
    }
  }, [])

  return hash
}

function DownloadDemoCsv() {
  const download = () => {
    const header = 'observed_at,district,pm25,temperature_c'
    const rows = resultRows.map((row) =>
      [row.observed_at, row.district, row.pm25, row.temperature_c].join(','),
    )
    const blob = new Blob([[header, ...rows].join('\n')], {
      type: 'text/csv;charset=utf-8',
    })
    const url = URL.createObjectURL(blob)
    const anchor = document.createElement('a')
    anchor.href = url
    anchor.download = 'xazz-air-quality-demo.csv'
    anchor.click()
    URL.revokeObjectURL(url)
  }

  return (
    <button className="button button--tool-secondary" type="button" onClick={download}>
      <ArrowDownToLine size={15} aria-hidden="true" />
      Download demo CSV
    </button>
  )
}

function WorkspaceTopbar({
  runState,
  onHome,
  onLiveCheck,
  onFullRun,
  liveMessage,
  fullRunRef,
  isInert,
}) {
  const processTone =
    runState === 'running'
      ? 'info'
      : 'neutral'
  const processLabel =
    runState === 'running'
      ? 'Running'
      : ['success', 'error'].includes(runState)
        ? 'Exited'
        : 'Not started'

  return (
    <header
      className="workspace-topbar"
      inert={isInert ? '' : undefined}
      aria-hidden={isInert ? 'true' : undefined}
    >
      <div className="workspace-topbar__brand">
        <Brand onHome={onHome} inverse />
        <span className="topbar-divider" aria-hidden="true" />
        <button className="project-crumb" type="button">
          air-quality-sample
          <ChevronDown size={14} aria-hidden="true" />
        </button>
        <span className="file-crumb">example.xzz</span>
      </div>
      <div className="workspace-topbar__status">
        <StatusBadge axis="Location" tone="info">
          Local demo
        </StatusBadge>
        <StatusBadge axis="Process" tone={processTone}>
          {processLabel}
        </StatusBadge>
        <StatusBadge axis="Maturity" tone="success">
          Available
        </StatusBadge>
      </div>
      <div className="workspace-topbar__actions">
        <span className="live-message" role="status" aria-live="polite">
          {liveMessage}
        </span>
        <button
          className="button button--tool-secondary"
          type="button"
          onClick={onLiveCheck}
          disabled={runState === 'running'}
          aria-label="Live Check demo, Future contract, 100 synthetic rows"
        >
          <Eye size={16} aria-hidden="true" />
          Live Check
          <span>Demo · 100 rows</span>
        </button>
        <button
          ref={fullRunRef}
          className="button button--tool-primary"
          type="button"
          onClick={onFullRun}
          disabled={runState === 'running'}
        >
          <Play size={16} fill="currentColor" aria-hidden="true" />
          Full Run
        </button>
      </div>
    </header>
  )
}

function SourceRail({ selectedId, onSelect }) {
  const selectedIndex = pipeline.findIndex((node) => node.id === selectedId)

  return (
    <aside className="source-rail" aria-label="Project and operation navigation">
      <div className="source-rail__search">
        <Search size={15} aria-hidden="true" />
        <span>Search project</span>
        <kbd>⌘ K</kbd>
      </div>
      <nav className="project-tree" aria-label="Project files">
        <h2>
          <FolderTree size={14} aria-hidden="true" />
          Project
        </h2>
        <button type="button" className="tree-row tree-row--active">
          <FileCode2 size={15} aria-hidden="true" />
          example.xzz
        </button>
        <button type="button" className="tree-row">
          <Database size={15} aria-hidden="true" />
          seoul_air_quality.csv
        </button>
        <button type="button" className="tree-row">
          <FileText size={15} aria-hidden="true" />
          xazz.toml
        </button>
      </nav>
      <div className="operation-list">
        <h2>
          <ListTree size={14} aria-hidden="true" />
          Pipeline operations
        </h2>
        <p>Keyboard-selectable mirror of the canvas.</p>
        <ol>
          {pipeline.map((node, index) => (
            <li key={node.id}>
              <button
                type="button"
                className={[
                  selectedId === node.id ? 'is-selected' : '',
                  index < selectedIndex ? 'is-upstream' : '',
                  index > selectedIndex ? 'is-downstream' : '',
                ].join(' ')}
                aria-current={selectedId === node.id ? 'step' : undefined}
                aria-label={`${node.label}, ${
                  index === selectedIndex
                    ? 'selected'
                    : index < selectedIndex
                      ? 'upstream'
                      : 'downstream'
                }, ${node.evidence}`}
                onClick={() => onSelect(node.id)}
              >
                <span>{String(index + 1).padStart(2, '0')}</span>
                <span>
                  <strong>{node.label}</strong>
                  <small>{node.evidence}</small>
                  <small className="operation-relation">
                    {index === selectedIndex
                      ? 'Selected'
                      : index < selectedIndex
                        ? 'Upstream'
                        : 'Downstream'}
                  </small>
                </span>
              </button>
            </li>
          ))}
        </ol>
      </div>
      <div className="labs-entry">
        <StatusBadge axis="Maturity" tone="future" compact>
          Research
        </StatusBadge>
        <strong>Models &amp; policy</strong>
        <p>Separated from current Core.</p>
      </div>
    </aside>
  )
}

function CanvasToolbar({ view, onView }) {
  return (
    <div className="canvas-toolbar">
      <div>
        <span className="eyebrow">Compiler Canvas</span>
        <strong>air_quality_pipeline</strong>
      </div>
      <div className="segmented-control" aria-label="Canvas view">
        {[
          ['graph', Workflow, 'Graph'],
          ['split', PanelRight, 'Split'],
          ['code', Code2, 'Code'],
        ].map(([id, Icon, label]) => (
          <button
            type="button"
            key={id}
            className={view === id ? 'is-active' : ''}
            aria-pressed={view === id}
            onClick={() => onView(id)}
          >
            <Icon size={14} aria-hidden="true" />
            {label}
          </button>
        ))}
      </div>
    </div>
  )
}

function PipelineCanvas({ selectedId, onSelect, runState }) {
  const selectedIndex = pipeline.findIndex((node) => node.id === selectedId)
  const nodes = useMemo(
    () =>
      pipeline.map((node, index) => ({
        id: node.id,
        type: 'pipeline',
        position: { x: 22 + index * 174, y: index % 2 === 0 ? 70 : 132 },
        data: {
          ...node,
          order: String(index + 1).padStart(2, '0'),
          visualState: getNodeVisualState(node.id, selectedId, runState),
          relation: getNodeRelation(node.id, selectedId),
        },
        selected: node.id === selectedId,
        draggable: false,
      })),
    [selectedId, runState],
  )

  const edges = useMemo(
    () =>
      pipeline.slice(0, -1).map((node, index) => ({
        id: `${node.id}-${pipeline[index + 1].id}`,
        source: node.id,
        target: pipeline[index + 1].id,
        animated: runState === 'running',
        className: [
          'flow-edge',
          runState === 'error' && index >= 2 ? 'flow-edge--stale' : '',
          index < selectedIndex ? 'flow-edge--upstream' : 'flow-edge--downstream',
        ].join(' '),
      })),
    [runState, selectedIndex],
  )

  return (
    <div className="react-flow-wrap" aria-label="Pipeline graph">
      <ReactFlow
        nodes={nodes}
        edges={edges}
        nodeTypes={nodeTypes}
        onNodeClick={(_, node) => onSelect(node.id)}
        nodesDraggable={false}
        nodesConnectable={false}
        elementsSelectable
        fitView
        fitViewOptions={{ padding: 0.14 }}
        minZoom={0.65}
        maxZoom={1.25}
        proOptions={{ hideAttribution: true }}
      >
        <Background color="#2d3b35" gap={24} size={1} />
        <Controls showInteractive={false} position="bottom-right" />
      </ReactFlow>
      <div className={`canvas-scope ${runState === 'running' ? 'is-stale' : ''}`}>
        <Eye size={13} aria-hidden="true" />
        {runState === 'running'
          ? 'Last Live Check · stale while Full Run is pending'
          : 'Live Check demo · Future contract · 100 synthetic rows · no backend call'}
      </div>
    </div>
  )
}

function CodePane({ selectedNode, runState, draftApplied }) {
  const affectedStart = selectedNode.codeLine
  const selectedIndex = pipeline.findIndex((node) => node.id === selectedNode.id)
  const affectedIds =
    selectedNode.id === 'fill'
      ? new Set([9, 10, 11])
      : selectedNode.id === 'filter'
        ? new Set([10, 11])
        : new Set([selectedNode.codeLine])

  return (
    <div className="code-pane" aria-label=".xzz source code">
      <div className="code-pane__head">
        <span>
          <Code2 size={14} aria-hidden="true" />
          example.xzz
        </span>
        <span>
          Selected line {affectedStart}
          {runState === 'error' && ' · runtime evidence attached'}
        </span>
      </div>
      <ol>
        {codeLines.map((line, index) => {
          const lineNumber = index + 1
          const ownerIndex = pipeline.findIndex(
            (node) => node.codeLine === lineNumber,
          )
          const relation =
            ownerIndex < 0
              ? null
              : ownerIndex === selectedIndex
                ? 'selected'
                : ownerIndex < selectedIndex
                  ? 'upstream'
                  : 'downstream'
          return (
            <li
              key={`${lineNumber}-${line}`}
              aria-label={
                relation ? `${relation} operation, code line ${lineNumber}` : undefined
              }
              className={[
                lineNumber === selectedNode.codeLine ? 'is-selected' : '',
                affectedIds.has(lineNumber) ? 'is-affected' : '',
                runState === 'error' && lineNumber === 9 ? 'has-error' : '',
                relation ? `is-${relation}` : '',
              ].join(' ')}
            >
              <span>{lineNumber}</span>
              <code>{line || ' '}</code>
              {lineNumber === selectedNode.codeLine && (
                <i aria-label="Selected graph operation" />
              )}
            </li>
          )
        })}
        {draftApplied && (
          <li className="draft-line">
            <span>9+</span>
            <code>typed = air |&gt; cast(pm25, Float)</code>
            <em>Draft only</em>
          </li>
        )}
      </ol>
    </div>
  )
}

function Inspector({ selectedNode, runState }) {
  const failedCurrentRun = runState === 'error' && selectedNode.id === 'fill'
  const detail = failedCurrentRun
    ? {
        ...selectedNode.detail,
        rows: 'Not available in failed run',
        nulls: 'Not available in failed run',
        schema: 'Not available in failed run',
        artifact: 'Not written · output untrusted',
      }
    : selectedNode.detail
  const state =
    failedCurrentRun
      ? ['Pipeline', 'Partial', 'warning']
      : runState === 'success'
        ? ['Pipeline', 'Succeeded', 'success']
        : ['Maturity', 'Demo', 'info']

  return (
    <aside className="inspector" aria-label="Selected operation inspector">
      <div className="inspector__head">
        <div>
          <span className="eyebrow">Selected operation</span>
          <h2>{selectedNode.label}</h2>
        </div>
        <StatusBadge axis={state[0]} tone={state[2]} compact>
          {state[1]}
        </StatusBadge>
      </div>
      <section>
        <h3>Intent</h3>
        <p>{selectedNode.detail.intent}</p>
      </section>
      <section>
        <h3>Impact</h3>
        <dl className="impact-grid">
          <div>
            <dt>Rows</dt>
            <dd>{detail.rows}</dd>
          </div>
          <div>
            <dt>Nulls</dt>
            <dd>{detail.nulls}</dd>
          </div>
          <div>
            <dt>Schema</dt>
            <dd>{detail.schema}</dd>
          </div>
          <div>
            <dt>Duration</dt>
            <dd>{detail.duration}</dd>
          </div>
        </dl>
        <p className={`impact-source ${runState === 'running' ? 'is-stale' : ''}`}>
          {failedCurrentRun
            ? 'Current Full Run · failed-run impact unavailable. Last Live Check values are not substituted.'
            : runState === 'running'
              ? 'Last Live Check · stale while Full Run is pending.'
              : 'Synthetic fixture · Live Check demo · Future contract · no backend call.'}
        </p>
      </section>
      <section>
        <h3>Artifact</h3>
        <div className="artifact-row">
          <FileText size={15} aria-hidden="true" />
          <span>{detail.artifact}</span>
        </div>
      </section>
      <section className="lineage-section">
        <h3>Lineage</h3>
        <div className="lineage-rail">
          <span>
            {pipeline.findIndex((node) => node.id === selectedNode.id)} upstream
          </span>
          <i aria-hidden="true" />
          <span>
            {pipeline.length -
              pipeline.findIndex((node) => node.id === selectedNode.id) -
              1}{' '}
            downstream
          </span>
        </div>
      </section>
      <div className="inspector__truth">
        <Info size={15} aria-hidden="true" />
        <p>
          Metrics the current runtime does not emit remain labelled “Not emitted,”
          rather than inferred.
        </p>
      </div>
    </aside>
  )
}

function PreviewTable() {
  return (
    <div className="result-table-wrap">
      <div className="result-summary">
        <div>
          <strong>{scenario.resultCount}</strong>
          <span>result rows</span>
        </div>
        <div>
          <strong>0</strong>
          <span>null values</span>
        </div>
        <div>
          <strong>4</strong>
          <span>typed fields</span>
        </div>
        <p>Preview shows 6 of {scenario.resultCount} rows · synthetic data</p>
      </div>
      <div className="data-grid" role="table" aria-label="Air-quality result rows">
        <div className="data-grid__row data-grid__row--head" role="row">
          {['observed_at', 'district', 'pm25 · μg/m³', 'temperature · °C'].map(
            (heading) => (
              <span role="columnheader" key={heading}>
                {heading}
              </span>
            ),
          )}
        </div>
        {resultRows.slice(0, 6).map((row) => (
          <div className="data-grid__row" role="row" key={`${row.observed_at}-${row.district}`}>
            <span role="cell">{row.observed_at}</span>
            <span role="cell">{row.district}</span>
            <span role="cell">{row.pm25.toFixed(1)}</span>
            <span role="cell">{row.temperature_c.toFixed(1)}</span>
          </div>
        ))}
      </div>
    </div>
  )
}

function DeltaPanel() {
  return (
    <div className="delta-panel">
      <div className="delta-rail" aria-label="Row count change">
        <span style={{ '--value': 100 }}>
          <i />
          <strong>100</strong>
          Source
        </span>
        <span style={{ '--value': 100 }}>
          <i />
          <strong>100</strong>
          Fill null
        </span>
        <span style={{ '--value': scenario.resultCount }}>
          <i />
          <strong>{scenario.resultCount}</strong>
          Threshold
        </span>
      </div>
      <dl>
        <div>
          <dt>Null delta</dt>
          <dd>−{scenario.sourceNulls}</dd>
        </div>
        <div>
          <dt>Row delta</dt>
          <dd>−{scenario.removedCount}</dd>
        </div>
        <div>
          <dt>Type delta</dt>
          <dd>Float? → Float</dd>
        </div>
      </dl>
    </div>
  )
}

function ChartPanel() {
  const max = Math.max(...chartData.map((item) => item.mean))
  return (
    <div className="chart-panel">
      <div className="chart-panel__heading">
        <div>
          <strong>Mean PM2.5 by district</strong>
          <span>μg/m³ · filtered synthetic sample · top 5 districts</span>
        </div>
        <StatusBadge axis="View" tone="info" compact>
          Aggregated
        </StatusBadge>
      </div>
      <div
        className="bar-chart"
        role="img"
        aria-label={`Mean PM2.5 ranges from ${Math.min(
          ...chartData.map((item) => item.mean),
        )} to ${max} micrograms per cubic metre across five districts.`}
      >
        {chartData.map((item) => (
          <div className="bar-chart__row" key={item.district}>
            <span>{item.district}</span>
            <i style={{ '--bar-width': `${(item.mean / max) * 100}%` }} />
            <strong>{item.mean}</strong>
          </div>
        ))}
      </div>
      <details>
        <summary>Table alternative</summary>
        <table>
          <thead>
            <tr>
              <th>District</th>
              <th>Mean PM2.5</th>
              <th>Rows</th>
            </tr>
          </thead>
          <tbody>
            {chartData.map((item) => (
              <tr key={item.district}>
                <td>{item.district}</td>
                <td>{item.mean} μg/m³</td>
                <td>{item.count}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </details>
    </div>
  )
}

function RunTimeline({ runState }) {
  const items =
    runState === 'running'
      ? [
          ['Process', 'xazz-runner started xazz-exec', 'current'],
          ['Pipeline', 'Waiting for structured result', 'pending'],
          ['Artifact', 'No outcome reported yet', 'pending'],
        ]
      : runState === 'error'
        ? [
            ['Process', 'Exited with code 0', 'done'],
            ['Pipeline', 'Runtime error found in stderr', 'error'],
            ['Artifact', 'Output cannot be trusted', 'warning'],
          ]
        : runState === 'success'
          ? [
              ['Process', 'Exited with code 0', 'done'],
              ['Pipeline', `Structured result · ${scenario.resultCount} rows`, 'done'],
              ['Artifact', 'Not requested · optional export available', 'done'],
            ]
          : [
              ['Process', 'Not started', 'pending'],
              ['Pipeline', 'Not evaluated', 'pending'],
              ['Artifact', 'No run outcome', 'pending'],
            ]

  return (
    <div className="run-timeline">
      {items.map(([axis, description, status]) => (
        <div className={`run-timeline__item run-timeline__item--${status}`} key={axis}>
          <span className="run-timeline__marker" aria-hidden="true">
            {status === 'current' ? (
              <LoaderCircle />
            ) : status === 'done' ? (
              <Check />
            ) : status === 'error' ? (
              <X />
            ) : status === 'warning' ? (
              <AlertTriangle />
            ) : (
              <CircleDashed />
            )}
          </span>
          <div>
            <strong>{axis}</strong>
            <span>{description}</span>
          </div>
        </div>
      ))}
      {runState === 'running' && (
        <p className="timeline-truth">
          The current server returns only after the process exits. Per-node progress stays
          Unknown in this honest prototype.
        </p>
      )}
    </div>
  )
}

function Receipt({ hash, runState }) {
  const isError = runState === 'error'
  const isSuccess = runState === 'success'

  if (!isError && !isSuccess) {
    const isRunning = runState === 'running'
    return (
      <div className="receipt receipt--pending">
        <div className="receipt__summary">
          <div className="receipt__icon is-pending">
            {isRunning ? (
              <LoaderCircle aria-hidden="true" />
            ) : (
              <CircleDashed aria-hidden="true" />
            )}
          </div>
          <div>
            <span className="eyebrow">Run receipt · synthetic prototype</span>
            <h3>{isRunning ? 'Run receipt is pending' : 'No full-run receipt yet'}</h3>
            <p>
              {isRunning
                ? 'A receipt is available only after process and structured-result evidence return.'
                : 'Start a confirmed Full Run before interpreting process or pipeline outcome.'}
            </p>
          </div>
          <div className="receipt__axes">
            <StatusBadge axis="Process" tone="neutral">
              {isRunning ? 'Running' : 'Not started'}
            </StatusBadge>
            <StatusBadge axis="Pipeline" tone="neutral">
              {isRunning ? 'Unknown' : 'Not evaluated'}
            </StatusBadge>
            <StatusBadge axis="Control" tone="neutral">
              Not configured
            </StatusBadge>
            <StatusBadge axis="Integrity" tone="neutral">
              Not computed
            </StatusBadge>
            <StatusBadge axis="Artifact" tone="neutral">
              No outcome
            </StatusBadge>
          </div>
        </div>
      </div>
    )
  }

  return (
    <div className="receipt">
      <div className="receipt__summary">
        <div className={`receipt__icon ${isError ? 'is-warning' : ''}`}>
          {isError ? <TriangleAlert aria-hidden="true" /> : <CheckCircle2 aria-hidden="true" />}
        </div>
        <div>
          <span className="eyebrow">Run receipt · synthetic prototype</span>
          <h3>{isError ? 'Process exited; pipeline is partial' : 'Pipeline evidence is complete'}</h3>
          <p>
            {isError
              ? 'Exit code 0 is recorded separately from the runtime error found in stderr.'
              : 'Success requires a structured result and no detected runtime or requested-artifact warning.'}
          </p>
        </div>
        <div className="receipt__axes">
          <StatusBadge axis="Process" tone="neutral">
            Exited
          </StatusBadge>
          <StatusBadge axis="Pipeline" tone={isError ? 'warning' : 'success'}>
            {isError ? 'Partial' : 'Succeeded'}
          </StatusBadge>
          <StatusBadge axis="Control" tone="neutral">
            Not configured
          </StatusBadge>
          <StatusBadge axis="Integrity" tone="info">
            Computed
          </StatusBadge>
          <StatusBadge axis="Artifact" tone="neutral">
            {isError ? 'Not written' : 'Not requested'}
          </StatusBadge>
        </div>
      </div>
      <dl className="receipt__rows">
        <div>
          <dt>Run ID</dt>
          <dd>Not available in browser prototype</dd>
        </div>
        <div>
          <dt>Fixture ID</dt>
          <dd>air-quality-v1 · synthetic</dd>
        </div>
        <div>
          <dt>Run time</dt>
          <dd>Not available in browser prototype</dd>
        </div>
        <div>
          <dt>Engine version</dt>
          <dd>Not available in browser prototype</dd>
        </div>
        <div>
          <dt>Execution location</dt>
          <dd>Local browser demo · no backend called</dd>
        </div>
        <div>
          <dt>Code hash</dt>
          <dd>
            <code title={hash}>{hash.slice(0, 20)}…</code>
            <span>SHA-256 · computed · not persisted</span>
          </dd>
        </div>
        <div>
          <dt>Rows</dt>
          <dd>
            {isError
              ? 'Failed-node delta unavailable · structured-result marker present'
              : `100 input → ${scenario.resultCount} output`}
          </dd>
        </div>
        <div>
          <dt>Warnings</dt>
          <dd>
            {isError
              ? 'Runtime type mismatch in synthetic error fixture'
              : 'None in selected synthetic success fixture'}
          </dd>
        </div>
        <div>
          <dt>Node durations</dt>
          <dd>Not available in current runtime</dd>
        </div>
        <div>
          <dt>Capability maturity</dt>
          <dd>Demo · synthetic browser prototype</dd>
        </div>
        <div>
          <dt>Policy / DP</dt>
          <dd>Not available in this version · Research</dd>
        </div>
        <div>
          <dt>Artifact</dt>
          <dd>
            {isError
              ? 'Not written · output untrusted'
              : 'Not requested by run · optional export after result'}
          </dd>
        </div>
      </dl>
      {!isError && <DownloadDemoCsv />}
    </div>
  )
}

function ErrorRecovery({ onOpenCode, onApplyDraft, draftApplied, onRetry }) {
  const [explained, setExplained] = useState(false)
  return (
    <div className="error-recovery" role="alert">
      <div className="error-recovery__head">
        <span className="error-recovery__icon">
          <XCircle aria-hidden="true" />
        </span>
        <div>
          <span className="eyebrow">Pipeline evidence · Partial</span>
          <h3>Fill null failed, even though the process exited 0.</h3>
          <p>
            A String value reached <code>fillNull(pm25, 31.0)</code> at line 9.
            Filter and Result are stale; the output artifact is not trusted.
          </p>
        </div>
        <div className="error-recovery__axes">
          <StatusBadge axis="Process" tone="neutral">
            Exited
          </StatusBadge>
          <StatusBadge axis="Pipeline" tone="warning">
            Partial
          </StatusBadge>
        </div>
      </div>
      <div className="error-recovery__evidence">
        <div>
          <span>What happened</span>
          <strong>Runtime type mismatch</strong>
        </div>
        <div>
          <span>Where</span>
          <strong>Fill null · line 9</strong>
        </div>
        <div>
          <span>Affected</span>
          <strong>2 downstream nodes</strong>
        </div>
        <div>
          <span>Safe next step</span>
          <strong>Review cast, then full rerun</strong>
        </div>
      </div>
      {explained && (
        <div className="explanation-note">
          <MessageSquareText aria-hidden="true" />
          <p>
            The schema was inferred from 100 rows. A later value may still drift. Add an
            explicit cast as a draft, review the diff, then run the complete pipeline
            again. This explanation is deterministic prototype copy—not an sLM response.
          </p>
        </div>
      )}
      {draftApplied && (
        <div className="draft-note" role="status">
          <Sparkles aria-hidden="true" />
          <p>
            Draft prepared: add <code>cast(pm25, Float)</code>. Nothing has been applied
            to a project file.
          </p>
        </div>
      )}
      <div className="error-recovery__actions">
        <button className="button button--tool-secondary" type="button" onClick={() => setExplained(!explained)}>
          <MessageSquareText size={15} aria-hidden="true" />
          {explained ? 'Hide explanation' : 'Explain'}
        </button>
        <button className="button button--tool-primary" type="button" onClick={onOpenCode}>
          <Code2 size={15} aria-hidden="true" />
          Open code
        </button>
        <button className="button button--tool-secondary" type="button" onClick={onApplyDraft}>
          <Sparkles size={15} aria-hidden="true" />
          Apply as draft
        </button>
        <button className="button button--tool-secondary" type="button" onClick={onRetry}>
          <RefreshCw size={15} aria-hidden="true" />
          Review preflight to rerun
        </button>
      </div>
      <div className="future-actions">
        <button type="button" disabled>
          Retry from here
          <span>Future</span>
        </button>
        <button type="button" disabled>
          <RotateCcw size={14} aria-hidden="true" />
          Restore last success
          <span>Future</span>
        </button>
      </div>
    </div>
  )
}

function ResultDock({
  tab,
  onTab,
  runState,
  hash,
  onOpenCode,
  onApplyDraft,
  draftApplied,
  onRetry,
}) {
  const tabs = [
    ['preview', Table2, 'Preview'],
    ['delta', Workflow, 'Delta'],
    ['chart', PanelBottom, 'Chart'],
    ['logs', TerminalSquare, 'Logs'],
    ['receipt', ShieldCheck, 'Receipt'],
  ]

  const content =
    tab === 'preview' ? (
      <PreviewTable />
    ) : tab === 'delta' ? (
      <DeltaPanel />
    ) : tab === 'chart' ? (
      <ChartPanel />
    ) : tab === 'receipt' ? (
      <Receipt hash={hash} runState={runState} />
    ) : runState === 'error' ? (
      <ErrorRecovery
        onOpenCode={onOpenCode}
        onApplyDraft={onApplyDraft}
        draftApplied={draftApplied}
        onRetry={onRetry}
      />
    ) : (
      <RunTimeline runState={runState} />
    )

  return (
    <section className="result-dock" aria-label="Pipeline results">
      <div className="result-dock__tabs" role="tablist" aria-label="Result views">
        {tabs.map(([id, Icon, label]) => (
          <button
            role="tab"
            type="button"
            key={id}
            aria-selected={tab === id}
            className={tab === id ? 'is-active' : ''}
            onClick={() => onTab(id)}
          >
            <Icon size={14} aria-hidden="true" />
            {label}
            {id === 'logs' && runState === 'error' && <span>1</span>}
          </button>
        ))}
        <div className="result-dock__scope">
          <span>
            {runState === 'running'
              ? 'Last Live Check · stale'
              : runState === 'error'
                ? 'Last Live Check · stale · not current run'
                : 'Synthetic fixture'}
          </span>
          <span>Rows {scenario.resultCount}</span>
          <span>Columns 4</span>
        </div>
      </div>
      <div className="result-dock__body" role="tabpanel">
        {runState === 'error' && ['preview', 'delta', 'chart'].includes(tab) && (
          <div className="stale-result-notice" role="note">
            Last Live Check · stale · not current Full Run evidence
          </div>
        )}
        {content}
      </div>
    </section>
  )
}

function PreflightDialog({
  acknowledged,
  onAcknowledge,
  onClose,
  onRun,
}) {
  const dialogRef = useRef(null)
  const confirmationRef = useRef(null)
  const onCloseRef = useRef(onClose)
  onCloseRef.current = onClose

  useEffect(() => {
    confirmationRef.current?.focus()

    const handleKeyDown = (event) => {
      if (event.key === 'Escape') {
        event.preventDefault()
        onCloseRef.current()
        return
      }

      if (event.key !== 'Tab' || !dialogRef.current) return

      const focusable = Array.from(
        dialogRef.current.querySelectorAll(
          'button:not([disabled]), input:not([disabled]), [href], [tabindex]:not([tabindex="-1"])',
        ),
      )
      const first = focusable[0]
      const last = focusable.at(-1)

      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault()
        last?.focus()
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault()
        first?.focus()
      }
    }

    document.addEventListener('keydown', handleKeyDown)
    return () => {
      document.removeEventListener('keydown', handleKeyDown)
    }
  }, [])

  return (
    <div className="dialog-backdrop">
      <section
        ref={dialogRef}
        className="preflight-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="preflight-title"
      >
        <div className="preflight-dialog__head">
          <div>
            <span className="eyebrow">Full Run · explicit confirmation</span>
            <h2 id="preflight-title">Review what will execute locally.</h2>
            <p>
              This browser prototype models the future runtime-readiness contract. It
              does not call a backend or write a run artifact.
            </p>
          </div>
          <button className="icon-button" type="button" onClick={onClose} aria-label="Close preflight">
            <X aria-hidden="true" />
          </button>
        </div>
        <div className="preflight-grid">
          <section>
            <h3>Runtime readiness · synthetic state</h3>
            <ul className="runtime-list">
              {['xazz', 'xazz-runner', 'xazz-exec'].map((name) => (
                <li key={name}>
                  <CircleDashed aria-hidden="true" />
                  <span>
                    <strong>{name}</strong>
                    Future contract · not verified
                  </span>
                </li>
              ))}
            </ul>
          </section>
          <section>
            <h3>Execution contract</h3>
            <dl className="preflight-facts">
              <div>
                <dt>Location</dt>
                <dd>Local browser demo · backend not called</dd>
              </div>
              <div>
                <dt>Input</dt>
                <dd>100 deterministic synthetic rows</dd>
              </div>
              <div>
                <dt>Artifact</dt>
                <dd>Not requested · optional export after result</dd>
              </div>
              <div>
                <dt>Control</dt>
                <dd>Not configured · policy is Research</dd>
              </div>
            </dl>
          </section>
        </div>
        <label className="preflight-warning">
          <input
            ref={confirmationRef}
            type="checkbox"
            checked={acknowledged}
            onChange={(event) => onAcknowledge(event.target.checked)}
          />
          <span>
            <span className="preflight-warning__check" aria-hidden="true">
              {acknowledged ? <Check /> : <Square />}
            </span>
            <span>
              <strong>
                {acknowledged
                  ? 'Confirmed · synthetic run scope'
                  : 'Check to confirm · synthetic run scope'}
              </strong>
              I understand this browser-only prototype simulates the future runtime chain
              with 100 deterministic rows. It makes no backend call or repository write.
            </span>
          </span>
        </label>
        <div className="preflight-dialog__axes">
          <StatusBadge axis="Process" tone="neutral">
            Not started
          </StatusBadge>
          <StatusBadge axis="Pipeline" tone="neutral">
            Not evaluated
          </StatusBadge>
          <StatusBadge axis="Control" tone="neutral">
            Not configured
          </StatusBadge>
          <StatusBadge axis="Run confirmation" tone={acknowledged ? 'success' : 'warning'}>
            {acknowledged ? 'Confirmed' : 'Required'}
          </StatusBadge>
          <StatusBadge axis="Integrity" tone="neutral">
            Not computed
          </StatusBadge>
        </div>
        <div className="preflight-dialog__actions">
          <button className="button button--tool-secondary" type="button" onClick={onClose}>
            Back to canvas
          </button>
          <button
            className="button button--tool-primary"
            type="button"
            disabled={!acknowledged}
            onClick={onRun}
          >
            <Play size={16} fill="currentColor" aria-hidden="true" />
            Start full run
          </button>
        </div>
      </section>
    </div>
  )
}

function RunOverlay({ onViewLogs }) {
  return (
    <div className="run-overlay" role="status" aria-live="polite">
      <div className="run-overlay__pulse">
        <LoaderCircle aria-hidden="true" />
      </div>
      <div>
        <span className="eyebrow">Process running · demo state</span>
        <strong>Waiting for xazz-exec to return evidence</strong>
        <p>
          Current API progress is not streamed. Node status remains Unknown until stdout,
          stderr, and artifact outcome can be evaluated together.
        </p>
      </div>
      <div className="run-overlay__actions">
        <button className="button button--tool-secondary" type="button" onClick={onViewLogs}>
          <TerminalSquare size={15} aria-hidden="true" />
          View logs
        </button>
        <button className="button button--tool-secondary" type="button" disabled>
          <Square size={14} aria-hidden="true" />
          Cancel unavailable
        </button>
      </div>
    </div>
  )
}

function PrototypeNavigator({ onSuccess, onError }) {
  return (
    <aside className="prototype-navigator" aria-label="Prototype navigator">
      <span>Prototype navigator · not product UI</span>
      <button type="button" onClick={onSuccess}>
        Show success
      </button>
      <button type="button" onClick={onError}>
        Show runtime error
      </button>
    </aside>
  )
}

export function Workspace({ initialState = 'ready', onStateChange, onHome }) {
  const [runState, setRunState] = useState(initialState)
  const [selectedId, setSelectedId] = useState(initialState === 'error' ? 'fill' : 'filter')
  const [view, setView] = useState(initialState === 'error' ? 'split' : 'split')
  const [tab, setTab] = useState(
    initialState === 'error'
      ? 'logs'
      : initialState === 'success'
        ? 'receipt'
        : initialState === 'running'
          ? 'logs'
          : 'preview',
  )
  const [acknowledged, setAcknowledged] = useState(false)
  const [liveMessage, setLiveMessage] = useState(
    'Live Check demo · Future contract',
  )
  const [draftApplied, setDraftApplied] = useState(false)
  const fullRunRef = useRef(null)
  const hash = useCodeHash()
  const selectedNode = pipeline.find((node) => node.id === selectedId) ?? pipeline[0]

  useEffect(() => {
    setRunState(initialState)
    if (initialState === 'error') {
      setSelectedId('fill')
      setTab('logs')
    } else if (initialState === 'success') {
      setTab('receipt')
    } else if (initialState === 'running') {
      setTab('logs')
    }
  }, [initialState])

  const changeState = (nextState) => {
    setRunState(nextState)
    if (nextState === 'error') {
      setSelectedId('fill')
      setTab('logs')
    } else if (nextState === 'success') {
      setTab('receipt')
    } else if (nextState === 'running') {
      setTab('logs')
    }
    onStateChange(nextState)
  }

  const runLiveCheck = () => {
    setLiveMessage('Checking 100 synthetic rows · demo…')
    window.setTimeout(
      () =>
        setLiveMessage(
          `Live Check demo · Future contract · ${scenario.sourceNulls} nulls found`,
        ),
      500,
    )
  }

  const openPreflight = () => {
    setAcknowledged(false)
    changeState('preflight')
  }

  const closePreflight = () => {
    changeState('ready')
    window.requestAnimationFrame(() => fullRunRef.current?.focus())
  }

  return (
    <div className="workspace-page">
      <a
        className="skip-link skip-link--dark"
        href="#compiler-canvas"
        inert={runState === 'preflight' ? '' : undefined}
        aria-hidden={runState === 'preflight' ? 'true' : undefined}
      >
        Skip to Compiler Canvas
      </a>
      <WorkspaceTopbar
        runState={runState}
        onHome={onHome}
        onLiveCheck={runLiveCheck}
        onFullRun={openPreflight}
        liveMessage={
          runState === 'running'
            ? 'Last Live Check · stale during Full Run'
            : liveMessage
        }
        fullRunRef={fullRunRef}
        isInert={runState === 'preflight'}
      />
      <div
        className="workspace-shell"
        inert={runState === 'preflight' ? '' : undefined}
        aria-hidden={runState === 'preflight' ? 'true' : undefined}
      >
        <SourceRail selectedId={selectedId} onSelect={setSelectedId} />
        <main className="compiler-area" id="compiler-canvas">
          <CanvasToolbar view={view} onView={setView} />
          <div
            className={`compiler-split compiler-split--${view}`}
            data-testid="compiler-split"
          >
            {view !== 'code' && (
              <PipelineCanvas
                selectedId={selectedId}
                onSelect={setSelectedId}
                runState={runState}
              />
            )}
            {view !== 'graph' && (
              <CodePane
                selectedNode={selectedNode}
                runState={runState}
                draftApplied={draftApplied}
              />
            )}
          </div>
        </main>
        <Inspector selectedNode={selectedNode} runState={runState} />
        <ResultDock
          tab={tab}
          onTab={setTab}
          runState={runState}
          hash={hash}
          onOpenCode={() => {
            setView('code')
            setSelectedId('fill')
          }}
          onApplyDraft={() => {
            setDraftApplied(true)
            setView('code')
          }}
          draftApplied={draftApplied}
          onRetry={openPreflight}
        />
      </div>
      {runState === 'preflight' && (
        <PreflightDialog
          acknowledged={acknowledged}
          onAcknowledge={setAcknowledged}
          onClose={closePreflight}
          onRun={() => changeState('running')}
        />
      )}
      {runState === 'running' && (
        <>
          <RunOverlay onViewLogs={() => setTab('logs')} />
          <PrototypeNavigator
            onSuccess={() => changeState('success')}
            onError={() => changeState('error')}
          />
        </>
      )}
      <div className="workspace-mobile-note">
        <Brand inverse />
        <Workflow aria-hidden="true" />
        <h1>Compiler Canvas is a desktop tool.</h1>
        <p>
          The mobile landing experience is supported. Open this workspace at 1024px or
          wider to inspect graph, code, and evidence together.
        </p>
        <button className="button button--tool-secondary" type="button" onClick={onHome}>
          Return to landing
        </button>
      </div>
    </div>
  )
}
