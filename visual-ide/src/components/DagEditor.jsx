import React, { useCallback, useMemo, useRef, useState } from 'react'
import {
  addEdge,
  Background,
  BackgroundVariant,
  Controls,
  Handle,
  MiniMap,
  Position,
  ReactFlow,
  useEdgesState,
  useNodesState,
  useReactFlow,
  ReactFlowProvider,
} from '@xyflow/react'
import '@xyflow/react/dist/style.css'
import {
  ArrowUpDown,
  BarChart2,
  BrainCircuit,
  Check,
  Columns,
  Copy,
  Database,
  Filter,
  GraduationCap,
  Group,
  Hash,
  Lock,
  PenLine,
  Plus,
  Play,
  RotateCcw,
  Scissors,
  ShieldCheck,
  Sparkles,
  Trash2,
} from 'lucide-react'
import { transpileToX1zz } from '../transpiler/x1zzTranspiler'
import { useLanguage } from '../i18n'
import { DAG_DEFAULT_PARAMS, DAG_TOOLS, NODE_PARAM_FIELDS, SEED_SCHEMA, detectCsvSchema, seedFromStaticPipeline } from '../dag/dagTools'

// ── 아이콘 레지스트리 (문자열 → 컴포넌트) ───────────────────────────────────
const ICONS = {
  Database,
  Columns,
  Filter,
  PenLine,
  Trash2,
  ArrowUpDown,
  Scissors,
  Group,
  Hash,
  BarChart2,
  BrainCircuit,
  GraduationCap,
  Sparkles,
  ShieldCheck,
  Lock,
}
const getIcon = (name) => ICONS[name] || Plus

// ── 커스텀 노드 ─────────────────────────────────────────────────────────────
function DagNode({ id, data, selected }) {
  const Icon = getIcon(data?.icon)
  const isSource = data?.source === true
  const guardrail = data?.guardrailStatus
  return (
    <div className={`dag-node ${data?.category || 'prep'} ${selected ? 'is-selected' : ''}`}>
      {!isSource && <Handle type="target" position={Position.Left} id="in" className="dag-handle dag-handle--in" />}
      <div className="dag-node__body">
        <span className="dag-node__icon"><Icon size={14} aria-hidden="true" /></span>
        <span className="dag-node__label">{data?.label || id}</span>
        {data?.category === 'security' && guardrail && (
          <span
            className={`dag-node__guardrail dag-node__guardrail--${guardrail}`}
            title={`Guardrail: ${guardrail}`}
          >
            {guardrail === 'blocked' ? '!' : guardrail === 'passed' ? '✓' : '·'}
          </span>
        )}
      </div>
      <Handle type="source" position={Position.Right} id="out" className="dag-handle dag-handle--out" />
    </div>
  )
}

// `node.type` is the tool id: React Flow picks the renderer with it and the
// transpiler looks up NODE_MAPPINGS with it. Registering every tool id here keeps
// both readings of the same field in agreement. 'dag' stays registered so a DAG
// already saved under the old scheme still renders while it is migrated on load.
const nodeTypes = {
  ...Object.fromEntries(DAG_TOOLS.map((tool) => [tool.id, DagNode])),
  dag: DagNode,
}

// Legacy saved DAGs stored every node as type 'dag', which NODE_MAPPINGS cannot
// resolve, so those nodes were silently dropped from the generated code. The tool
// id is recoverable from the label the palette wrote alongside it.
const TOOL_ID_BY_NAME = new Map(DAG_TOOLS.map((tool) => [tool.name, tool.id]))

function migrateLegacyNodes(nodes) {
  return nodes.map((node) => {
    if (node.type !== 'dag') return node
    const recovered = TOOL_ID_BY_NAME.get(node.data?.label)
    return recovered ? { ...node, type: recovered } : node
  })
}


function DagCanvasInner({ onCodeChange, guardrailStatus }) {
  const { t } = useLanguage()
  const seed = useMemo(() => {
    try {
      const saved = localStorage.getItem('xazz_dag')
      if (saved) {
        const parsed = JSON.parse(saved)
        if (parsed?.nodes?.length) {
          return { nodes: migrateLegacyNodes(parsed.nodes), edges: parsed.edges || [] }
        }
      }
    } catch (_) {}
    return seedFromStaticPipeline()
  }, [])

  const [nodes, setNodes, onNodesChange] = useNodesState(seed.nodes)
  const [edges, setEdges, onEdgesChange] = useEdgesState(seed.edges)
  const [selectedId, setSelectedId] = useState(null)
  const [copied, setCopied] = useState(false)
  const { screenToFlowPosition } = useReactFlow()

  // 마지막 가드레일 검사 결과(통과/차단/미검사)를 security 카테고리 노드에
  // 배지로 새긴다. 순수 display 데이터라 transpile 결과에는 영향이 없다.
  React.useEffect(() => {
    setNodes((nds) =>
      nds.map((n) =>
        n.data?.category === 'security'
          ? { ...n, data: { ...n.data, guardrailStatus } }
          : n,
      ),
    )
  }, [guardrailStatus, setNodes])

  const generatedCode = useMemo(() => {
    try {
      return transpileToX1zz(nodes, edges)
    } catch (err) {
      return `// Transpilation error: ${err.message}`
    }
  }, [nodes, edges])

  React.useEffect(() => {
    if (onCodeChange) onCodeChange(generatedCode)
  }, [generatedCode, onCodeChange])

  const onConnect = useCallback(
    (params) => setEdges((eds) => addEdge({ ...params, id: `e-${Date.now()}` }, eds)),
    [setEdges],
  )

  // ── 드래그&드롭으로 노드 추가 ────────────────────────────────────────────
  const onDragOver = useCallback((event) => {
    event.preventDefault()
    event.dataTransfer.dropEffect = 'move'
  }, [])

  const onDrop = useCallback(
    (event) => {
      event.preventDefault()
      const type = event.dataTransfer.getData('application/xazz-tool')
      if (!type) return
      const position = screenToFlowPosition({ x: event.clientX, y: event.clientY })
      addNode(type, position)
    },
    [screenToFlowPosition],
  )

  // 노드 추가 + 자동 배치 (선택 노드 오른쪽에 이어붙이기, 엣지 자동 연결)
  const addNode = (type, position) => {
    const tool = DAG_TOOLS.find((t) => t.id === type)
    const params = { ...(DAG_DEFAULT_PARAMS[type] || {}) }
    if (type === 'fileInput') params.detectedSchema = JSON.parse(JSON.stringify(SEED_SCHEMA))

    setNodes((nds) => {
      let pos = position
      if (!pos) {
        const anchor = nds.find((n) => n.id === selectedId) || nds[nds.length - 1]
        pos = anchor ? { x: anchor.position.x + 270, y: anchor.position.y } : { x: 60, y: 80 }
      }
      const id = `${type}-${Date.now().toString(36)}`
      const newNode = {
        id,
        type,
        position: pos,
        data: { label: tool?.name || type, category: tool?.category, icon: tool?.icon, source: type === 'fileInput', parameters: params },
      }
      // 자동 연결: fileInput이 아니면 마지막 비-source 노드에 연결
      if (type !== 'fileInput') {
        const last = nds.filter((n) => !n.data?.source).slice(-1)[0]
        if (last) {
          setEdges((eds) => [...eds, { id: `e-${Date.now()}`, source: last.id, target: id }])
        }
      }
      setSelectedId(id)
      return [...nds, newNode]
    })
  }

  const removeSelected = () => {
    if (!selectedId) return
    setNodes((nds) => nds.filter((n) => n.id !== selectedId))
    setEdges((eds) => eds.filter((e) => e.source !== selectedId && e.target !== selectedId))
    setSelectedId(null)
  }

  const resetDag = () => {
    localStorage.removeItem('xazz_dag')
    const s = seedFromStaticPipeline()
    setNodes(s.nodes)
    setEdges(s.edges)
    setSelectedId(null)
  }

  const saveDag = () => {
    localStorage.setItem('xazz_dag', JSON.stringify({ nodes, edges }))
    window.dispatchEvent(new CustomEvent('xazz-dag-saved'))
  }

  const copyCode = () => {
    navigator.clipboard.writeText(generatedCode)
    setCopied(true)
    setTimeout(() => setCopied(false), 1500)
  }

  return (
    <div className="dag-editor">
      {/* 좌측 도구 팔레트 (드래그 가능) */}
      <aside className="dag-palette">
        <div className="dag-palette__title">
          <span className="eyebrow">{t('dag.palette')}</span>
          <p className="dag-palette__help">{t('dag.paletteHelp')}</p>
        </div>
        {Object.entries(
          DAG_TOOLS.reduce((acc, t) => {
            ;(acc[t.category] = acc[t.category] || []).push(t)
            return acc
          }, {}),
        ).map(([cat, tools]) => (
          <div key={cat} className="dag-palette__group">
            <span className={`dag-palette__cat dag-palette__cat--${cat}`}>{t(`dag.categories.${cat}`)}</span>
            {tools.map((tool) => {
              const Icon = getIcon(tool.icon)
              return (
                <button
                  key={tool.id}
                  type="button"
                  className={`dag-palette__tool dag-palette__tool--${cat}`}
                  onClick={() => addNode(tool.id)}
                  onDragStart={(e) => {
                    e.dataTransfer.setData('application/xazz-tool', tool.id)
                    e.dataTransfer.effectAllowed = 'move'
                  }}
                  draggable
                  title={tool.description}
                >
                  <Icon size={13} aria-hidden="true" />
                  <span>{tool.name}</span>
                </button>
              )
            })}
          </div>
        ))}
      </aside>

      {/* 중앙 캔버스 */}
      <div className="dag-canvas" onDragOver={onDragOver} onDrop={onDrop}>
        <ReactFlow
          nodes={nodes}
          edges={edges}
          nodeTypes={nodeTypes}
          onNodesChange={onNodesChange}
          onEdgesChange={onEdgesChange}
          onConnect={onConnect}
          onNodeClick={(_, n) => setSelectedId(n.id)}
          onPaneClick={() => setSelectedId(null)}
          fitView
          fitViewOptions={{ padding: 0.15 }}
          minZoom={0.3}
          maxZoom={1.5}
          proOptions={{ hideAttribution: true }}
          defaultEdgeOptions={{ animated: true, style: { stroke: '#34d399', strokeWidth: 2 } }}
        >
          <Background variant={BackgroundVariant.Dots} gap={18} size={1} color="rgba(255,255,255,0.07)" />
          <Controls showInteractive={false} />
          <MiniMap pannable zoomable nodeColor={(n) => (n.data?.category === 'ml' ? '#34d399' : n.data?.category === 'security' ? '#f59e0b' : '#3b82f6')} maskColor="rgba(15,20,26,0.7)" />
        </ReactFlow>
        <div className="dag-canvas__hint">
          {t('dag.hint')}
        </div>
      </div>

      {/* 우측 코드 + 파라미터 */}
      <aside className="dag-side">
        <div className="dag-side__section">
          <div className="dag-side__head">
            <span className="eyebrow">{t('dag.generated')}</span>
            <button type="button" className="dag-mini-btn" onClick={copyCode} title={t('dag.copy')}>
              {copied ? <Check size={12} /> : <Copy size={12} />}{' '}
              {copied ? t('dag.copied') : t('dag.copy')}
            </button>
          </div>
          <pre className="dag-code">{generatedCode}</pre>
        </div>

        <div className="dag-side__section dag-side__section--params">
          <div className="dag-side__head">
            <span className="eyebrow">{t('dag.params')}</span>
            {selectedId && (
              <button type="button" className="dag-mini-btn dag-mini-btn--danger" onClick={removeSelected} title={t('dag.delete')}>
                <Trash2 size={12} /> Delete
              </button>
            )}
          </div>
          {selectedId ? (
            <NodeParamsEditor nodeId={selectedId} nodes={nodes} setNodes={setNodes} />
          ) : (
            <p className="dag-side__empty">{t('dag.paramsEmpty')}</p>
          )}
        </div>

        <div className="dag-side__actions">
          <button type="button" className="dag-btn dag-btn--run" onClick={saveDag}>
            <Play size={14} /> {t('dag.save')}
          </button>
          <button type="button" className="dag-btn dag-btn--ghost" onClick={resetDag}>
            <RotateCcw size={13} /> {t('dag.reset')}
          </button>
        </div>
      </aside>
    </div>
  )
}

// ── 선택 노드 파라미터 편집 (NODE_PARAM_FIELDS 기반 직관적 폼) ──────────────
function NodeParamsEditor({ nodeId, nodes, setNodes }) {
  const { t } = useLanguage()
  const node = nodes.find((n) => n.id === nodeId)
  if (!node) return null
  const type = node.type
  const p = node.data?.parameters || {}
  const fields = NODE_PARAM_FIELDS[type] || []
  const update = (patch) =>
    setNodes((nds) => nds.map((n) => (n.id === nodeId ? { ...n, data: { ...n.data, parameters: { ...p, ...patch } } } : n)))

  const renderField = (f) => {
    const value = p[f.key]
    if (f.type === 'select') {
      return (
        <select className="dag-field__input" value={value ?? ''} onChange={(e) => update({ [f.key]: e.target.value })}>
          {f.options.map((o) => (
            <option key={o} value={o}>{o}</option>
          ))}
        </select>
      )
    }
    if (f.type === 'checkbox') {
      return <input type="checkbox" checked={!!value} onChange={(e) => update({ [f.key]: e.target.checked })} />
    }
    return (
      <input
        className="dag-field__input"
        type={f.type === 'number' ? 'number' : 'text'}
        step={f.step}
        value={value ?? ''}
        placeholder={f.placeholder}
        onChange={(e) => update({ [f.key]: f.type === 'number' ? Number(e.target.value) : e.target.value })}
      />
    )
  }

  if (!fields.length) return <span className="dag-p__empty">{t('dag.noParams')}</span>

  // fileInput: 파일 선택 → CSV 컬럼/타입 자동감지로 스키마 설정 (실패 위험 제거)
  if (type === 'fileInput') {
    return (
      <div className="dag-params">
        <label className="dag-field">
          <span className="dag-field__label">{t('dag.filePick')}</span>
          <input
            className="dag-field__input"
            type="file"
            accept=".csv,.txt"
            onChange={async (e) => {
              const file = e.target.files?.[0]
              if (!file) return
              try {
                const text = await file.text()
                const schema = detectCsvSchema(text)
                update({ filePath: file.name, detectedSchema: schema })
                window.alert(
                  `${t('dag.detected').replace('{n}', schema.length)}: ${schema.map((c) => `${c.name}:${c.type}`).join(', ')}`,
                )
              } catch (err) {
                window.alert(`${t('dag.readFailed')}: ${err.message}`)
              }
              e.target.value = ''
            }}
          />
        </label>
        {fields.map((f) => (
          <label key={f.key} className="dag-field">
            <span className="dag-field__label">{f.label}{f.hint ? <i> · {f.hint}</i> : null}</span>
            {renderField(f)}
          </label>
        ))}
        {Array.isArray(p.detectedSchema) && p.detectedSchema.length > 0 && (
          <div className="dag-field">
            <span className="dag-field__label">{t('dag.detectedSchema')}</span>
            <div className="dag-schema-tags">
              {p.detectedSchema.map((c, i) => (
                <span key={i} className="dag-schema-tag">{c.name}<i>{c.type}</i></span>
              ))}
            </div>
          </div>
        )}
      </div>
    )
  }

  return (
    <div className="dag-params">
      {fields.map((f) => (
        <label key={f.key} className="dag-field">
          <span className="dag-field__label">{f.label}{f.hint ? <i> · {f.hint}</i> : null}</span>
          {renderField(f)}
        </label>
      ))}
    </div>
  )
}

export default function DagEditor(props) {
  return (
    <ReactFlowProvider>
      <DagCanvasInner {...props} />
    </ReactFlowProvider>
  )
}