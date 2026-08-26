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
} from '@xyflow/react'
import '@xyflow/react/dist/style.css'
import {
  ArrowUpDown,
  BarChart2,
  BrainCircuit,
  Columns,
  Database,
  Filter,
  GraduationCap,
  Group,
  Hash,
  Lock,
  PenLine,
  Plus,
  Scissors,
  ShieldCheck,
  Sparkles,
  Trash2,
} from 'lucide-react'
import { transpileToX1zz } from '../transpiler/x1zzTranspiler'
import { DAG_DEFAULT_PARAMS, DAG_TOOLS, SEED_SCHEMA, seedFromStaticPipeline } from '../dag/dagTools'

// ── 커스텀 노드 ─────────────────────────────────────────────────────────────
function DagNode({ id, data, selected }) {
  const Icon = data.icon || Plus
  return (
    <div className={`dag-node ${data.category || 'prep'} ${selected ? 'is-selected' : ''}`}>
      {data?.source !== false && <Handle type="target" position={Position.Left} id="in" />}
      <div className="dag-node__body">
        <Icon size={13} aria-hidden="true" />
        <strong>{data?.label || id}</strong>
      </div>
      {data?.sink !== false && <Handle type="source" position={Position.Right} id="out" />}
    </div>
  )
}

const nodeTypes = { dag: DagNode }

// ── 노드 카테고리 색상 (CSS 변수와 연결) ────────────────────────────────────
const CATEGORY_LABEL = { inout: 'Data', prep: 'Preprocess', transform: 'Transform', ml: 'ML · Burn', security: 'Security' }

function DagEditor({ initialCode, onCodeChange }) {
  const seed = useMemo(() => {
    try {
      const saved = localStorage.getItem('xazz_dag')
      if (saved) {
        const parsed = JSON.parse(saved)
        if (parsed?.nodes?.length) return { nodes: parsed.nodes, edges: parsed.edges || [] }
      }
    } catch (_) {}
    return seedFromStaticPipeline()
  }, [])

  const [nodes, setNodes, onNodesChange] = useNodesState(seed.nodes)
  const [edges, setEdges, onEdgesChange] = useEdgesState(seed.edges)
  const [selectedId, setSelectedId] = useState(null)
  const wrapper = useRef(null)

  const generatedCode = useMemo(() => {
    try {
      return transpileToX1zz(nodes, edges)
    } catch (err) {
      return `// Transpilation error: ${err.message}`
    }
  }, [nodes, edges])

  // 부모(Workspace)로 코드 전달
  React.useEffect(() => {
    if (onCodeChange) onCodeChange(generatedCode)
  }, [generatedCode, onCodeChange])

  const onConnect = useCallback(
    (params) => setEdges((eds) => addEdge({ ...params, id: `e-${Date.now()}` }, eds)),
    [setEdges],
  )

  const addNode = (type) => {
    const tool = DAG_TOOLS.find((t) => t.id === type)
    const position = {
      x: 120 + Math.random() * 320,
      y: 80 + Math.random() * 200,
    }
    const id = `${type}-${Date.now().toString(36)}`
    const params = { ...(DAG_DEFAULT_PARAMS[type] || {}) }
    // fileInput은 기본 스키마 자동 주입
    if (type === 'fileInput') params.detectedSchema = JSON.parse(JSON.stringify(SEED_SCHEMA))
    setNodes((nds) => [
      ...nds,
      {
        id,
        type: 'dag',
        position,
        data: { label: tool?.name || type, category: tool?.category, icon: tool?.icon, source: type === 'fileInput', sink: type === 'fileInput', parameters: params },
      },
    ])
    setSelectedId(id)
  }

  const removeSelected = () => {
    if (!selectedId) return
    setNodes((nds) => nds.filter((n) => n.id !== selectedId))
    setEdges((eds) => eds.filter((e) => e.source !== selectedId && e.target !== selectedId))
    setSelectedId(null)
  }

  const saveDag = () => {
    localStorage.setItem('xazz_dag', JSON.stringify({ nodes, edges }))
  }

  return (
    <div className="dag-editor">
      {/* 좌측 도구 팔레트 */}
      <aside className="dag-palette">
        <div className="dag-palette__title">
          <span className="eyebrow">Tool Palette</span>
        </div>
        {Object.entries(
          DAG_TOOLS.reduce((acc, t) => {
            ;(acc[t.category] = acc[t.category] || []).push(t)
            return acc
          }, {}),
        ).map(([cat, tools]) => (
          <div key={cat} className="dag-palette__group">
            <span className={`dag-palette__cat dag-palette__cat--${cat}`}>{CATEGORY_LABEL[cat] || cat}</span>
            {tools.map((tool) => (
              <button key={tool.id} type="button" className={`dag-palette__tool dag-palette__tool--${cat}`} onClick={() => addNode(tool.id)} title={tool.description}>
                <ToolIcon id={tool.id} size={13} />
                <span>{tool.name}</span>
              </button>
            ))}
          </div>
        ))}
      </aside>

      {/* 중앙 캔버스 */}
      <div className="dag-canvas" ref={wrapper}>
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
          fitViewOptions={{ padding: 0.2 }}
          minZoom={0.3}
          maxZoom={1.5}
          proOptions={{ hideAttribution: true }}
        >
          <Background variant={BackgroundVariant.Dots} gap={18} size={1} color="rgba(255,255,255,0.06)" />
          <Controls showInteractive={false} />
          <MiniMap pannable zoomable nodeColor={() => '#3b82f6'} maskColor="rgba(28,39,51,0.7)" />
        </ReactFlow>
        <div className="dag-canvas__hint">
          캔버스를 드래그해 이동 · 우측 핸들로 노드 연결 · 노드 클릭 후 Delete 로 삭제
        </div>
      </div>

      {/* 우측 코드 + 선택 노드 파라미터 */}
      <aside className="dag-side">
        <div className="dag-side__section">
          <div className="dag-side__head">
            <span className="eyebrow">Generated Xazz</span>
            <button type="button" className="dag-mini-btn" onClick={() => navigator.clipboard.writeText(generatedCode)}>
              ⎘
            </button>
          </div>
          <pre className="dag-code">{generatedCode}</pre>
        </div>
        <div className="dag-side__section dag-side__section--params">
          <div className="dag-side__head">
            <span className="eyebrow">Node Params</span>
            {selectedId && (
              <button type="button" className="dag-mini-btn dag-mini-btn--danger" onClick={removeSelected}>
                <Trash2 size={12} /> Delete
              </button>
            )}
          </div>
          {selectedId ? (
            <NodeParamsEditor nodeId={selectedId} nodes={nodes} setNodes={setNodes} />
          ) : (
            <p className="dag-side__empty">노드를 선택해 파라미터를 편집하세요.</p>
          )}
        </div>
        <div className="dag-side__actions">
          <button type="button" className="dag-btn" onClick={saveDag}>
            Save DAG
          </button>
        </div>
      </aside>
    </div>
  )
}

// ── 선택 노드 파라미터 편집 ────────────────────────────────────────────────
function NodeParamsEditor({ nodeId, nodes, setNodes }) {
  const node = nodes.find((n) => n.id === nodeId)
  if (!node) return null
  const p = node.data?.parameters || {}
  const update = (patch) =>
    setNodes((nds) => nds.map((n) => (n.id === nodeId ? { ...n, data: { ...n.data, parameters: { ...p, ...patch } } } : n)))

  const fields = Object.entries(p)
    .filter(([k]) => k !== 'detectedSchema' && k !== 'columns' && k !== 'columnMapping')
    .map(([k, v]) => (
      <label key={k} className="dag-field">
        <span>{k}</span>
        <input type={typeof v === 'number' ? 'number' : 'text'} value={v ?? ''} onChange={(e) => update({ [k]: e.target.value })} />
      </label>
    ))

  return <div className="dag-params">{fields.length ? fields : <span className="dag-p__empty">편집할 파라미터가 없습니다.</span>}</div>
}

function ToolIcon({ id, size = 13 }) {
  const icons = { Database, Columns, Filter, PenLine, Trash2, Sort, Group, Hash, BarChart2, BrainCircuit, GraduationCap, Sparkles, ShieldCheck, Lock, Scissors }
  const I = icons[id] || Plus
  return <I size={size} aria-hidden="true" />
}

export default DagEditor