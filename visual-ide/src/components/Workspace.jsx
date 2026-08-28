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
  Activity,
  AlertTriangle,
  ArrowDownToLine,
  Boxes,
  Braces,
  Check,
  CheckCircle2,
  ChevronDown,
  CircleDashed,
  Code2,
  Database,
  Eye,
  FileCode2,
  FileText,
  FolderTree,
  Info,
  ListTree,
  LoaderCircle,
  PanelBottom,
  PanelRight,
  Pencil,
  Play,
  Search,
  ShieldCheck,
  Sparkles,
  Square,
  Table2,
  Target,
  TerminalSquare,
  TriangleAlert,
  Workflow,
  X,
} from 'lucide-react'
import { Brand, StatusBadge } from './Common'
import { MonitorView } from './Monitor'
import { LocaleSwitch, localizeStep, useLanguage } from '../i18n'
import DagEditor from './DagEditor'
import { checkPolicy, executeCode, checkHealth, remediateCode, API_BASE_URL } from '../api'

// executeCode 기본 타임아웃(ms) — api.js 와 동일한 기본값 (ML 훈련 고려 5분)
const EXEC_TIMEOUT_MS = 5 * 60 * 1000
import {
  chartData,
  codeLines,
  pipeline,
  resultRows,
  runnableCode,
  scenario,
} from '../data'

const nodeIcons = {
  load: Database,
  schema: Braces,
  result: Table2,
  compile: Boxes,
  train: Sparkles,
  predict: Target,
}

function PipelineNode({ data }) {
  const Icon = nodeIcons[data.id] ?? Workflow

  return (
    <div
      className={`flow-node flow-node--${data.visualState} flow-node--relation-${data.relation}`}
      aria-label={`${data.label}, ${data.band} band, ${data.evidence}, ${data.visualState}, ${data.relation}`}
    >
      {data.id !== 'load' && <Handle type="target" position={Position.Left} />}
      <div className="flow-node__top">
        <span>
          <Icon size={14} aria-hidden="true" />
          {data.stage}
        </span>
        <i>{data.order}</i>
      </div>
      <span className="flow-node__band">{data.bandLabel ?? data.band}</span>
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
    if (['filter', 'result', 'train', 'predict'].includes(id)) return 'stale'
  }
  if (runState === 'success') return 'success'
  if (runState === 'running') return 'unknown'
  if (id === selectedId) return 'selected'
  if (id === 'schema') return 'warning'
  return 'ready'
}

function useCodeHash(dagCode) {
  const [hash, setHash] = useState('Computing…')

  useEffect(() => {
    let active = true
    const compute = async () => {
      // 실제 실행되는 코드(dagCode)의 무결성 해시를 계산한다.
      const bytes = new TextEncoder().encode(dagCode ?? '')
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
  }, [dagCode])

  return hash
}

function DownloadDemoCsv({ rows }) {
  // CSV 셀 공식 주입(Formula Injection) 방지를 위해 모든 셀을 쿼팅하고
  // '='/'+'/'-'/'@'/탭/CR 로 시작하는 셀은 단일 인용부호로 중화한다.
  const escCell = (value) => {
    const s = String(value ?? '')
    const guarded = /^[=+\-@\t\r]/.test(s) ? `'${s}` : s
    return `"${guarded.replace(/"/g, '""')}"`
  }

  const download = () => {
    const data = rows ?? resultRows
    const columns = data.length > 0 ? Object.keys(data[0]) : []
    const header = columns.map(escCell).join(',')
    const lines = data.map((row) => columns.map((col) => escCell(row[col])).join(','))
    const blob = new Blob([[header, ...lines].join('\n')], {
      type: 'text/csv;charset=utf-8',
    })
    const url = URL.createObjectURL(blob)
    const anchor = document.createElement('a')
    anchor.href = url
    anchor.download = 'xazz-air-quality-result.csv'
    anchor.click()
    URL.revokeObjectURL(url)
  }

  return (
    <button className="button button--tool-secondary" type="button" onClick={download}>
      <ArrowDownToLine size={15} aria-hidden="true" />
      Download result CSV
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
  backendReachable,
  runBlocked,
}) {
  const { t } = useLanguage()
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
  const backendTone =
    backendReachable === null
      ? 'neutral'
      : backendReachable
        ? 'success'
        : 'warning'

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
        <StatusBadge axis="Location" tone={backendTone}>
          {backendReachable === null
            ? 'Checking server'
            : backendReachable
              ? 'xazz-server connected'
              : 'xazz-server offline'}
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
        <LocaleSwitch compact />
        <button
          className="button button--tool-secondary"
          type="button"
          onClick={onLiveCheck}
          disabled={runState === 'running'}
          aria-label={`${t('topbar.liveCheck')}, backend reachability check`}
        >
          <Eye size={16} aria-hidden="true" />
          {t('topbar.liveCheck')}
          <span>{t('topbar.liveCheckHint')}</span>
        </button>
        <button
          ref={fullRunRef}
          className="button button--tool-primary"
          type="button"
          onClick={onFullRun}
          disabled={runState === 'running' || runBlocked}
          title={runBlocked ? 'Full Run is blocked by a policy violation' : undefined}
        >
          <Play size={16} fill="currentColor" aria-hidden="true" />
          {t('topbar.fullRun')}
        </button>
      </div>
    </header>
  )
}

function SourceRail({ selectedId, onSelect }) {
  const { language, t } = useLanguage()
  const selectedIndex = pipeline.findIndex((node) => node.id === selectedId)

  return (
    <aside className="source-rail" aria-label="Project and operation navigation">
      <div className="source-rail__search">
        <Search size={15} aria-hidden="true" />
        <span>{t('rail.search')}</span>
        <kbd>⌘ K</kbd>
      </div>
      <nav className="project-tree" aria-label="Project files">
        <h2>
          <FolderTree size={14} aria-hidden="true" />
          {t('rail.project')}
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
          {t('rail.operations')}
        </h2>
        <p>{t('rail.operationsHelp')}</p>
        <ol>
          {pipeline.map((rawNode, index) => {
            const node = localizeStep(rawNode, language)
            return (
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
                      ? t('rail.selected')
                      : index < selectedIndex
                        ? t('rail.upstream')
                        : t('rail.downstream')}
                  </small>
                </span>
              </button>
            </li>
            )
          })}
        </ol>
      </div>
      <div className="labs-entry">
        <StatusBadge axis="Maturity" tone="future" compact>
          Research
        </StatusBadge>
        <strong>{t('rail.labsTitle')}</strong>
        <p>{t('rail.labsBody')}</p>
      </div>
    </aside>
  )
}

function CanvasToolbar({ view, onView }) {
  const { t } = useLanguage()
  return (
    <div className="canvas-toolbar">
      <div>
        <span className="eyebrow">{t('canvas.title')}</span>
        <strong>air_quality_pipeline</strong>
      </div>
      <div className="segmented-control" aria-label="Canvas view">
        {[
          ['edit', Pencil],
          ['graph', Workflow],
          ['split', PanelRight],
          ['code', Code2],
          ['monitor', Activity],
        ].map(([id, Icon]) => (
          <button
            type="button"
            key={id}
            className={view === id ? 'is-active' : ''}
            aria-pressed={view === id}
            onClick={() => onView(id)}
          >
            <Icon size={14} aria-hidden="true" />
            {t(`canvas.views.${id}`)}
          </button>
        ))}
      </div>
    </div>
  )
}

function PipelineCanvas({ selectedId, onSelect, runState }) {
  const { language, t } = useLanguage()
  const selectedIndex = pipeline.findIndex((node) => node.id === selectedId)
  const nodes = useMemo(
    () =>
      pipeline.map((rawNode, index) => {
        const node = localizeStep(rawNode, language)
        return {
        id: node.id,
        type: 'pipeline',
        position: node.position,
        data: {
          ...node,
          bandLabel: t(`canvas.bands.${rawNode.band}`),
          order: String(index + 1).padStart(2, '0'),
          visualState: getNodeVisualState(node.id, selectedId, runState),
          relation: getNodeRelation(node.id, selectedId),
        },
        selected: node.id === selectedId,
        draggable: false,
        }
      }),
    [selectedId, runState, language, t],
  )

  const edges = useMemo(
    () =>
      pipeline.flatMap((node) => {
        const targetIndex = pipeline.findIndex((item) => item.id === node.id)
        return node.from.map((sourceId) => ({
          id: `${sourceId}-${node.id}`,
          source: sourceId,
          target: node.id,
          animated: runState === 'running',
          className: [
            'flow-edge',
            runState === 'error' && targetIndex >= 3 ? 'flow-edge--stale' : '',
            targetIndex <= selectedIndex ? 'flow-edge--upstream' : 'flow-edge--downstream',
          ].join(' '),
        }))
      }),
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
        minZoom={0.4}
        maxZoom={1.25}
        proOptions={{ hideAttribution: true }}
      >
        <Background color="#2d3b35" gap={24} size={1} />
        <Controls showInteractive={false} position="bottom-right" />
      </ReactFlow>
      <div className={`canvas-scope ${runState === 'running' ? 'is-stale' : ''}`}>
        <Eye size={13} aria-hidden="true" />
        {runState === 'running' ? t('canvas.staleScope') : t('canvas.scope')}
      </div>
    </div>
  )
}

function CodePane({ selectedNode, runState }) {
  const affectedStart = selectedNode.codeLine
  const selectedIndex = pipeline.findIndex((node) => node.id === selectedNode.id)
  const affectedIds =
    selectedNode.id === 'fill'
      ? new Set([13, 14, 15])
      : selectedNode.id === 'filter'
        ? new Set([14, 15])
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
                runState === 'error' && lineNumber === 13 ? 'has-error' : '',
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
      </ol>
    </div>
  )
}

function Inspector({ selectedNode: rawNode, runState, runResult }) {
  const { language, t } = useLanguage()
  const selectedNode = localizeStep(rawNode, language)
  const realRows = Array.isArray(runResult?.rows) ? runResult.rows.length : null
  const detail =
    realRows !== null
      ? {
          ...selectedNode.detail,
          rows:
            realRows !== null
              ? t('inspector.rowsReturned').replace('{n}', realRows)
              : selectedNode.detail.rows,
          schema: Array.isArray(runResult.schema)
            ? t('inspector.fieldCount').replace('{n}', runResult.schema.length)
            : selectedNode.detail.schema,
        }
      : selectedNode.detail
  const state =
    runState === 'error'
      ? ['Pipeline', 'Partial', 'warning']
      : runState === 'success'
        ? ['Pipeline', 'Succeeded', 'success']
        : ['Maturity', 'Connected', 'info']

  return (
    <aside className="inspector" aria-label="Selected operation inspector">
      <div className="inspector__head">
        <div>
          <span className="eyebrow">{t('inspector.selected')}</span>
          <h2>{selectedNode.label}</h2>
        </div>
        <StatusBadge axis={state[0]} tone={state[2]} compact>
          {state[1]}
        </StatusBadge>
      </div>
      <section>
        <h3>{t('inspector.intent')}</h3>
        <p>{selectedNode.detail.intent}</p>
      </section>
      <section>
        <h3>{t('inspector.impact')}</h3>
        <dl className="impact-grid">
          <div>
            <dt>{t('inspector.rows')}</dt>
            <dd>{detail.rows}</dd>
          </div>
          <div>
            <dt>{t('inspector.nulls')}</dt>
            <dd>{detail.nulls}</dd>
          </div>
          <div>
            <dt>{t('inspector.schema')}</dt>
            <dd>{detail.schema}</dd>
          </div>
          <div>
            <dt>{t('inspector.duration')}</dt>
            <dd>{detail.duration}</dd>
          </div>
        </dl>
        <p className={`impact-source ${runState === 'running' ? 'is-stale' : ''}`}>
          {runState === 'running'
            ? t('inspector.stale')
            : realRows !== null
              ? t('inspector.fromRun')
              : t('inspector.structural')}
        </p>
      </section>
      <section>
        <h3>{t('inspector.artifact')}</h3>
        <div className="artifact-row">
          <FileText size={15} aria-hidden="true" />
          <span>{detail.artifact}</span>
        </div>
      </section>
      <section className="lineage-section">
        <h3>{t('inspector.lineage')}</h3>
        <div className="lineage-rail">
          <span>
            {pipeline.findIndex((node) => node.id === selectedNode.id)} {t('inspector.upstream')}
          </span>
          <i aria-hidden="true" />
          <span>
            {pipeline.length -
              pipeline.findIndex((node) => node.id === selectedNode.id) -
              1}{' '}
            {t('inspector.downstream')}
          </span>
        </div>
      </section>
      <div className="inspector__truth">
        <Info size={15} aria-hidden="true" />
        <p>{t('inspector.note')}</p>
      </div>
    </aside>
  )
}

function PreviewTable({ runResult, backendReachable, execError }) {
  const { t } = useLanguage()
  const hasResult = Array.isArray(runResult?.rows) && runResult.rows.length > 0
  const schema = Array.isArray(runResult?.schema) ? runResult.schema : []
  const columns = hasResult && schema.length > 0 ? schema : []

  if (execError) {
    return (
      <div className="result-empty" role="note">
        <TriangleAlert size={16} aria-hidden="true" />
        <p>
          <strong>xazz-server is not reachable</strong>
          <span>
            Full Run could not call {API_BASE_URL}/execute. Start xazz-server, then run
            Live Check or Full Run again.
          </span>
        </p>
      </div>
    )
  }

  if (!hasResult) {
    return (
      <div className="result-empty" role="note">
        <Info size={16} aria-hidden="true" />
        <p>
          <strong>{t('dock.emptyTitle')}</strong>
          <span>{t('dock.emptyBody')}</span>
        </p>
      </div>
    )
  }

  return (
    <div className="result-table-wrap">
      <div className="result-summary">
        <div>
          <strong>{runResult.rows.length}</strong>
          <span>result rows</span>
        </div>
        <div>
          <strong>{columns.length}</strong>
          <span>typed fields</span>
        </div>
        <p>Preview shows the rows returned by xazz-server · real Full Run</p>
      </div>
      <div
        className="data-grid"
        role="table"
        aria-label="Air-quality result rows"
        style={{ '--col-count': columns.length }}
      >
        <div className="data-grid__row data-grid__row--head" role="row">
          {columns.map((col) => (
            <span role="columnheader" key={col.name}>
              {col.name}
              <i className="data-grid__type">{col.type}</i>
            </span>
          ))}
        </div>
        {runResult.rows.slice(0, 50).map((row, rowIndex) => (
          <div className="data-grid__row" role="row" key={rowIndex}>
            {columns.map((col) => (
              <span role="cell" key={col.name}>
                {formatCell(row[col.name])}
              </span>
            ))}
          </div>
        ))}
      </div>
    </div>
  )
}

function formatCell(value) {
  if (value === null || value === undefined) return '∅'
  if (typeof value === 'number') return Number.isInteger(value) ? value : value.toFixed(4)
  return String(value)
}

function fmtEpsilon(value) {
  const v = Number(value)
  if (!Number.isFinite(v)) return '—'
  return String(parseFloat(v.toPrecision(6)))
}

function DeltaPanel({ runResult }) {
  const real = Array.isArray(runResult?.rows)
  if (real) {
    const columns = Array.isArray(runResult.schema) ? runResult.schema.length : 0
    return (
      <div className="delta-panel">
        <div className="delta-rail" aria-label="Real run summary">
          <span style={{ '--value': 100 }}>
            <i />
            <strong>{runResult.rows.length}</strong>
            Result rows
          </span>
          <span style={{ '--value': 100 }}>
            <i />
            <strong>{columns}</strong>
            Columns
          </span>
          <span style={{ '--value': runResult.training ? 100 : 0 }}>
            <i />
            <strong>{runResult.training?.report ? 'ML' : '—'}</strong>
            Training report
          </span>
        </div>
        <p className="delta-note">
          Per-stage row/null deltas are not emitted by the current runtime. Shown values
          come from the returned [xazz:result] rows and schema.
        </p>
      </div>
    )
  }
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
      <p className="delta-note">Synthetic illustrative fixture · not a real run.</p>
    </div>
  )
}

function ChartPanel({ runResult }) {
  const real = Array.isArray(runResult?.rows) && runResult.rows.length > 0
  if (!real) {
    return (
      <div className="chart-panel">
        <div className="chart-panel__heading">
          <div>
            <strong>Mean PM2.5 by district</strong>
            <span>μg/m³ · filtered synthetic sample · top 5 districts</span>
          </div>
          <StatusBadge axis="View" tone="info" compact>
            Synthetic only
          </StatusBadge>
        </div>
        <p className="chart-note">
          No real Full Run result yet. Run Full Run to render a chart from the returned
          rows.
        </p>
      </div>
    )
  }
  const rows = runResult.rows
  const columns = Array.isArray(runResult.schema) ? runResult.schema : []
  const numericColumns = columns.filter((col) => /f64|f32|int|float/i.test(col.type))
  const dimensionColumns = columns.filter((col) => /str|string/i.test(col.type))
  const xKey = dimensionColumns[0]?.name ?? columns[0]?.name ?? ''
  const yKey = numericColumns[0]?.name ?? columns[1]?.name ?? ''
  const grouped = {}
  for (const row of rows) {
    const key = String(row[xKey] ?? '—')
    const value = Number(row[yKey])
    if (Number.isFinite(value)) {
      grouped[key] = grouped[key] ?? []
      grouped[key].push(value)
    }
  }
  const bars = Object.entries(grouped)
    .map(([label, values]) => ({
      label,
      mean: values.reduce((a, b) => a + b, 0) / values.length,
    }))
    .slice(0, 10)
  const max = Math.max(...bars.map((item) => item.mean))
  return (
    <div className="chart-panel">
      <div className="chart-panel__heading">
        <div>
          <strong>
            Mean {yKey} by {xKey}
          </strong>
          <span>computed from real Full Run rows · top {bars.length} groups</span>
        </div>
        <StatusBadge axis="View" tone="info" compact>
          Aggregated
        </StatusBadge>
      </div>
      <div
        className="bar-chart"
        role="img"
        aria-label={`Mean ${yKey} ranges from ${Math.min(...bars.map((item) => item.mean))} to ${max} across ${bars.length} groups.`}
      >
        {bars.map((item) => (
          <div className="bar-chart__row" key={item.label}>
            <span>{item.label}</span>
            <i style={{ '--bar-width': `${(item.mean / max) * 100}%` }} />
            <strong>{item.mean.toFixed(2)}</strong>
          </div>
        ))}
      </div>
    </div>
  )
}

function RunTimeline({ runState, runResult, execError }) {
  const rows = Array.isArray(runResult?.rows) ? runResult.rows.length : undefined
  const items =
    execError
      ? [
          ['Process', 'xazz-server unreachable', 'error'],
          ['Pipeline', `No execute call completed`, 'warning'],
          ['Artifact', 'No outcome returned', 'warning'],
        ]
      : runState === 'running'
        ? [
            ['Process', 'Full Run sent to xazz-server', 'current'],
            ['Pipeline', 'Waiting for structured result', 'pending'],
            ['Artifact', 'No outcome reported yet', 'pending'],
          ]
        : runState === 'error'
          ? [
              ['Process', 'xazz-exec exited with a pipeline error', 'error'],
              ['Pipeline', 'Runtime error found in stderr', 'error'],
              ['Artifact', 'Output cannot be trusted', 'warning'],
            ]
          : runState === 'success'
            ? [
                ['Process', 'xazz-exec exited 0', 'done'],
                ['Pipeline', `Structured result · ${rows} rows`, 'done'],
                ['Artifact', 'Returned in browser · no file written', 'done'],
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
          xazz-server returns only after the process exits. Per-node progress stays Unknown
          in this version.
        </p>
      )}
    </div>
  )
}

function LogsPanel({ runResult, execError }) {
  const logs = Array.isArray(runResult?.logs) ? runResult.logs : []
  const stdout = runResult?.stdout || ''
  if (execError) {
    return (
      <div className="logs-panel" role="note">
        <div className="logs-panel__head">
          <TerminalSquare size={15} aria-hidden="true" />
          <strong>Backend connection failed</strong>
        </div>
        <pre className="logs-panel__body">
          {`xazz-server unreachable · ${API_BASE_URL}\nNo Full Run completed.\n\nStart xazz-server, then run Full Run again.`}
        </pre>
      </div>
    )
  }
  if (logs.length === 0 && !stdout) {
    return (
      <div className="logs-panel" role="note">
        <div className="logs-panel__head">
          <TerminalSquare size={15} aria-hidden="true" />
          <strong>No logs yet</strong>
        </div>
        <pre className="logs-panel__body">Run Full Run to collect xazz-exec stderr and stdout logs.</pre>
      </div>
    )
  }
  return (
    <div className="logs-panel" role="note">
      <div className="logs-panel__head">
        <TerminalSquare size={15} aria-hidden="true" />
        <strong>Full Run logs · xazz-exec</strong>
      </div>
      <div className="logs-panel__stream">
        <pre className="logs-panel__body">
          {logs.map((line) => `[stderr] ${line}`).join('\n')}
          {stdout ? `\n${stdout}` : ''}
        </pre>
      </div>
    </div>
  )
}

function Receipt({ hash, runState, runResult, execError }) {
  const isError = runState === 'error'
  const isSuccess = runState === 'success'
  const realRows = Array.isArray(runResult?.rows) ? runResult.rows.length : null
  const training = runResult?.training
  const dpReport = runResult?.dp

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
            <span className="eyebrow">Run receipt · real Full Run</span>
            <h3>{isRunning ? 'Run receipt is pending' : 'No full-run receipt yet'}</h3>
            <p>
              {isRunning
                ? 'A receipt is available only after xazz-server returns the process and structured-result evidence.'
                : 'Run a confirmed Full Run against xazz-server to produce a receipt.'}
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
            <StatusBadge axis="Integrity" tone={isRunning ? 'neutral' : 'info'}>
              {isRunning ? 'Unknown' : 'Real run · not persisted'}
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
          <span className="eyebrow">Run receipt · real Full Run</span>
          <h3>
            {isError
              ? execError
                ? 'xazz-server unreachable'
                : 'Pipeline exited with an error'
              : 'Pipeline evidence is complete'}
          </h3>
          <p>
            {isError
              ? (execError
                  ? 'Full Run could not reach the backend. No pipeline executed.'
                  : (runResult?.error || 'xazz-exec reported an error in stderr.'))
              : 'Success returned a structured result with no detected runtime error.'}
          </p>
        </div>
        <div className="receipt__axes">
          <StatusBadge axis="Process" tone="neutral">
            {isError ? 'Exited / blocked' : 'Exited'}
          </StatusBadge>
          <StatusBadge axis="Pipeline" tone={isError ? 'warning' : 'success'}>
            {isError ? (execError ? 'Not executed' : 'Partial') : 'Succeeded'}
          </StatusBadge>
          <StatusBadge axis="Control" tone="neutral">
            Not configured
          </StatusBadge>
          <StatusBadge axis="Integrity" tone="info">
            Computed
          </StatusBadge>
          <StatusBadge axis="Artifact" tone="neutral">
            Returned in browser
          </StatusBadge>
        </div>
      </div>
      <dl className="receipt__rows">
        <div>
          <dt>Run ID</dt>
          <dd>Not available from browser /execute</dd>
        </div>
        <div>
          <dt>Endpoint</dt>
          <dd>{API_BASE_URL}/execute</dd>
        </div>
        <div>
          <dt>Result rows</dt>
          <dd>{isError && !execError ? 'Not available in failed run' : (realRows ?? '—')}</dd>
        </div>
        <div>
          <dt>Training</dt>
          <dd>
            {training?.report
              ? `${training.report.model_name} · loss ${Number(training.report.final_train_loss).toFixed(4)}`
              : 'No train statement report'}
          </dd>
        </div>
        <div>
          <dt>Code hash</dt>
          <dd>
            <code title={hash}>{hash.slice(0, 20)}…</code>
            <span>SHA-256 · computed · not persisted</span>
          </dd>
        </div>
        <div>
          <dt>Warnings</dt>
          <dd>{isError ? (execError ? 'Backend connection failed' : 'Pipeline produced stderr') : 'None in returned result'}</dd>
        </div>
        <div>
          <dt>Node durations</dt>
          <dd>Not available in current runtime</dd>
        </div>
        <div>
          <dt>Capability maturity</dt>
          <dd>Connected · real backend</dd>
        </div>
        <div>
          <dt>Policy / DP</dt>
          <dd>
            {dpReport
              ? `${dpReport.mechanism} · ε ${fmtEpsilon(dpReport.epsilon)} · ${fmtEpsilon(dpReport.budget_spent ?? dpReport.epsilon)}/${fmtEpsilon(dpReport.budget_total)} spent`
              : 'No DP query this run'}
          </dd>
        </div>
        <div>
          <dt>Artifact</dt>
          <dd>{isError ? 'Not written · output untrusted' : 'Returned in browser · no file written'}</dd>
        </div>
      </dl>
      {!isError && realRows !== null && <DownloadDemoCsv rows={runResult.rows} />}
    </div>
  )
}

function ResultDock({
  tab,
  onTab,
  runState,
  hash,
  runResult,
  execError,
  backendReachable,
}) {
  const { t } = useLanguage()
  const tabs = [
    ['preview', Table2],
    ['delta', Workflow],
    ['chart', PanelBottom],
    ['logs', TerminalSquare],
    ['receipt', ShieldCheck],
  ]

  const content =
    tab === 'preview' ? (
      <PreviewTable
        runResult={runResult}
        backendReachable={backendReachable}
        execError={execError}
      />
    ) : tab === 'delta' ? (
      <DeltaPanel runResult={runResult} />
    ) : tab === 'chart' ? (
      <ChartPanel runResult={runResult} />
    ) : tab === 'receipt' ? (
      <Receipt hash={hash} runState={runState} runResult={runResult} execError={execError} />
    ) : runState === 'error' ? (
      <LogsPanel runResult={runResult} execError={execError} />
    ) : (
      <RunTimeline runState={runState} runResult={runResult} execError={execError} />
    )

  return (
    <section className="result-dock" aria-label="Pipeline results">
      <div className="result-dock__tabs" role="tablist" aria-label="Result views">
        {tabs.map(([id, Icon]) => (
          <button
            role="tab"
            type="button"
            key={id}
            aria-selected={tab === id}
            className={tab === id ? 'is-active' : ''}
            onClick={() => onTab(id)}
          >
            <Icon size={14} aria-hidden="true" />
            {t(`dock.tabs.${id}`)}
            {id === 'logs' && runState === 'error' && <span>1</span>}
          </button>
        ))}
        <div className="result-dock__scope">
          <span>
            {execError
              ? 'xazz-server offline'
              : runState === 'running'
                ? 'Full Run in progress'
                : runState === 'error'
                  ? 'Last Full Run · errored'
                  : Array.isArray(runResult?.rows)
                    ? 'Real Full Run evidence'
                    : t('dock.notRun')}
          </span>
          <span>
            {t('dock.rows')} {Array.isArray(runResult?.rows) ? runResult.rows.length : '—'}
          </span>
          <span>
            {t('dock.columns')}{' '}
            {Array.isArray(runResult?.schema) ? runResult.schema.length : '—'}
          </span>
        </div>
      </div>
      <div className="result-dock__body" role="tabpanel">
        {runState === 'error' && ['preview', 'delta', 'chart'].includes(tab) && (
          <div className="stale-result-notice" role="note">
            Last Full Run errored · preview is not current evidence
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
            <h2 id="preflight-title">Review what will execute on xazz-server.</h2>
            <p>
              Full Run sends the current <code>example.xzz</code> source to the backend{' '}
              /execute endpoint and displays the returned rows, schema, logs, and
              training report. No projection file is written.
            </p>
          </div>
          <button className="icon-button" type="button" onClick={onClose} aria-label="Close preflight">
            <X aria-hidden="true" />
          </button>
        </div>
        <div className="preflight-grid">
          <section>
            <h3>Runtime readiness · connected check</h3>
            <ul className="runtime-list">
              {['xazz-server', 'xazz-exec', 'burn'].map((name) => (
                <li key={name}>
                  <CircleDashed aria-hidden="true" />
                  <span>
                    <strong>{name}</strong>
                    Verified by Full Run response
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
                <dd>POST {API_BASE_URL}/execute</dd>
              </div>
              <div>
                <dt>Input</dt>
                <dd>visual-ide/data/seoul_air_quality.csv</dd>
              </div>
              <div>
                <dt>Artifact</dt>
                <dd>Not requested · results returned in browser only</dd>
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
                  ? 'Confirmed · real run scope'
                  : 'Check to confirm · real run scope'}
              </strong>
              I understand this runs example.xzz against xazz-server and shows the real
              result. The engine must be reachable at {API_BASE_URL}.
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

function RunOverlay({ onViewLogs, connected }) {
  return (
    <div className="run-overlay" role="status" aria-live="polite">
      <div className="run-overlay__pulse">
        <LoaderCircle aria-hidden="true" />
      </div>
      <div>
        <span className="eyebrow">Process running · xazz-server</span>
        <strong>Waiting for xazz-exec to return evidence</strong>
        <p>
          Full Run is executing against the backend. Node progress is not streamed, so
          status stays Unknown until the structured result returns.
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

export function Workspace({ initialState = 'ready', onStateChange, onHome }) {
  const [runState, setRunState] = useState(initialState)
  const [selectedId, setSelectedId] = useState(initialState === 'error' ? 'fill' : 'filter')
  const [view, setView] = useState(initialState === 'error' ? 'split' : 'split')
  const [dagCode, setDagCode] = useState(runnableCode)
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
    initialState === 'success' || initialState === 'error'
      ? 'Full Run · result from last execution'
      : 'Connect to xazz-server to execute · not yet run',
  )
  const [backendReachable, setBackendReachable] = useState(null)
  const [runResult, setRunResult] = useState(null)
  const [execError, setExecError] = useState(null)
  const [executing, setExecuting] = useState(false)
  const [policyReport, setPolicyReport] = useState(null)
  const [remediation, setRemediation] = useState(null)
  const [guardrailSource, setGuardrailSource] = useState(null)
  const [guardrailChecking, setGuardrailChecking] = useState(false)
  const fullRunRef = useRef(null)
  const hash = useCodeHash(dagCode)
  const selectedNode = pipeline.find((node) => node.id === selectedId) ?? pipeline[0]
  const guardrailBlocked = Boolean(policyReport && !policyReport.safe_to_execute)

  useEffect(() => {
    let active = true
    checkHealth().then((ok) => {
      if (active) setBackendReachable(ok)
    })
    return () => {
      active = false
    }
  }, [])

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

  const executeFullRun = async () => {
    setExecuting(true)
    setRunResult(null)
    setExecError(null)
    setAcknowledged(false)
    setLiveMessage(`Executing on xazz-server · ${API_BASE_URL}`)
    changeState('running')
    try {
      const result = await executeCode(dagCode)
      setRunResult(result)
      setBackendReachable(true)
      if (result.success && !result.error) {
        const rows = Array.isArray(result.rows) ? result.rows.length : 0
        setLiveMessage(`Full Run succeeded · ${rows} result rows`)
        changeState('success')
      } else {
        // A blocked /execute (Policy-as-Code gate) returns 422 with a `policy`
        // report in the body — surface it in the guardrail panel.
        if (result.policy) {
          setPolicyReport(result.policy)
          setGuardrailSource(dagCode)
          setLiveMessage('Full Run blocked by policy guardrail')
        } else {
          setLiveMessage('Full Run exited · pipeline error in xazz-exec')
        }
        changeState('error')
      }
    } catch (err) {
      const aborted =
        (typeof err === 'object' && err !== null && err.name === 'AbortError') ||
        (err instanceof DOMException && err.name === 'AbortError')
      if (aborted) {
        // 요청이 타임아웃됨 — 서버는 응답했을 수도 있으나 오래 걸림
        setBackendReachable(true)
        setExecError('Execution timed out. The server may still be processing (e.g. long training).')
        setLiveMessage(`Execution timed out after ${EXEC_TIMEOUT_MS / 1000}s`)
      } else {
        setBackendReachable(false)
        setExecError(err instanceof Error ? err.message : String(err))
        setLiveMessage(`xazz-server unreachable · ${API_BASE_URL}`)
      }
      setExecuting(false)
      setRunState('error')
      setSelectedId('fill')
      setTab('logs')
      onStateChange('error')
    } finally {
      setExecuting(false)
    }
  }

  const runLiveCheck = () => {
    if (backendReachable === false) {
      setLiveMessage(`xazz-server unreachable · check ${API_BASE_URL}`)
      return
    }
    setLiveMessage(`Checking xazz-server · ${API_BASE_URL}`)
    checkHealth().then((ok) => {
      setBackendReachable(ok)
      setLiveMessage(
        ok
          ? `xazz-server reachable · preview shows last Full Run result`
          : `xazz-server unreachable · check ${API_BASE_URL}`,
      )
    })
  }

  const runPolicyCheck = async () => {
    setGuardrailChecking(true)
    setGuardrailSource(dagCode)
    try {
      const report = await checkPolicy(dagCode)
      if (report && report.policy) {
        setPolicyReport(report.policy)
        setLiveMessage(
          report.policy.safe_to_execute
            ? `Guardrail check passed · ${report.policy.violations?.length ?? 0} violation(s)`
            : `Guardrail blocked · ${report.policy.violations?.length ?? 0} violation(s)`,
        )
      } else {
        setLiveMessage('Guardrail check unavailable · server offline?')
      }
    } finally {
      setGuardrailChecking(false)
    }
  }

  const runRemediate = async () => {
    setGuardrailSource(dagCode)
    try {
      const response = await remediateCode(dagCode)
      if (response && response.remediation) {
        setRemediation(response.remediation)
        if (response.policy) setPolicyReport(response.policy)
        setLiveMessage(
          response.remediation.verified
            ? 'Remediation generated · verified safe'
            : 'Remediation generated · manual review still required',
        )
      }
    } catch {
      setLiveMessage('Remediation unavailable · server offline?')
    }
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
            ? `Last result · stale during Full Run`
            : liveMessage
        }
        fullRunRef={fullRunRef}
        backendReachable={backendReachable}
        runBlocked={guardrailBlocked}
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
            {view === 'monitor' ? (
              <div className="monitor-view-wrap">
                <div className="guardrail-toolbar" aria-label="Guardrail actions">
                  <span>
                    <ShieldCheck size={14} aria-hidden="true" />
                    Policy-as-Code guardrail
                  </span>
                  <button
                    className="button button--tool-secondary"
                    type="button"
                    onClick={runPolicyCheck}
                    disabled={guardrailChecking || runState === 'running'}
                  >
                    {guardrailChecking ? (
                      <LoaderCircle size={15} aria-hidden="true" />
                    ) : (
                      <Search size={15} aria-hidden="true" />
                    )}
                    Check policy
                  </button>
                  <button
                    className="button button--tool-secondary"
                    type="button"
                    onClick={runRemediate}
                    disabled={guardrailChecking || runState === 'running'}
                  >
                    <Sparkles size={15} aria-hidden="true" />
                    Remediate
                  </button>
                </div>
                <MonitorView
                  runState={runState}
                  training={runResult?.training}
                  model={runResult?.model}
                  dp={runResult?.dp}
                  policy={policyReport}
                  remediation={remediation}
                  originalCode={guardrailSource}
                />
              </div>
            ) : view === 'edit' ? (
              <DagEditor
                onCodeChange={setDagCode}
                guardrailStatus={
                  guardrailBlocked
                    ? 'blocked'
                    : policyReport
                      ? 'passed'
                      : undefined
                }
              />
            ) : (
              <>
                {view !== 'code' && (
                  <PipelineCanvas
                    selectedId={selectedId}
                    onSelect={setSelectedId}
                    runState={runState}
                  />
                )}
                {view !== 'graph' && (
                  <CodePane selectedNode={selectedNode} runState={runState} />
                )}
              </>
            )}
          </div>
        </main>
        <Inspector selectedNode={selectedNode} runState={runState} runResult={runResult} />
        <ResultDock
          tab={tab}
          onTab={setTab}
          runState={runState}
          hash={hash}
          runResult={runResult}
          execError={execError}
          backendReachable={backendReachable}
        />
      </div>
      {runState === 'preflight' && (
        <PreflightDialog
          acknowledged={acknowledged}
          onAcknowledge={setAcknowledged}
          onClose={closePreflight}
          onRun={executeFullRun}
        />
      )}
      {runState === 'running' && (
        <RunOverlay onViewLogs={() => setTab('logs')} connected={backendReachable} />
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
