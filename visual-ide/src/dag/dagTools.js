/**
 * dagTools.js — DAG 에디터에 사용할 수 있는 Xazz 연산자/ML/보안 노드 카탈로그.
 * 각 항목은 transpiler nodeMappings 와 연결된다.
 *
 * 카테고리:
 *   inout     — 데이터 입출력
 *   prep      — 전처리
 *   transform — 변환/집계
 *   ml        — Burn 딥러닝 (model/train/predict)
 *   security  — 정적 가드레일 · 차등 프라이버시 (issue #2 / #3)
 */
export const DAG_TOOLS = [
  { id: 'fileInput', name: 'File Input', category: 'inout', icon: 'Database', description: 'Load CSV data (schema inference)' },
  { id: 'select', name: 'Select', category: 'prep', icon: 'Columns', description: 'Select columns' },
  { id: 'filter', name: 'Filter', category: 'prep', icon: 'Filter', description: 'Filter rows by condition' },
  { id: 'fillNull', name: 'Fill Null', category: 'prep', icon: 'PenLine', description: 'Fill missing values' },
  { id: 'dropNull', name: 'Drop Null', category: 'prep', icon: 'Trash2', description: 'Drop rows with null' },
  { id: 'sort', name: 'Sort', category: 'prep', icon: 'ArrowUpDown', description: 'Sort rows (orderBy)' },
  { id: 'take', name: 'Take', category: 'prep', icon: 'Scissors', description: 'First N rows' },
  { id: 'groupBy', name: 'Group By', category: 'transform', icon: 'Group', description: 'Group and aggregate' },
  { id: 'count', name: 'Count', category: 'transform', icon: 'Hash', description: 'Row count' },
  { id: 'chart', name: 'Chart', category: 'transform', icon: 'BarChart2', description: 'Visualise' },

  // ML (Burn)
  { id: 'model', name: 'Model', category: 'ml', icon: 'BrainCircuit', description: 'Declare a model (model {})' },
  { id: 'train', name: 'Train', category: 'ml', icon: 'GraduationCap', description: 'Train with Burn' },
  { id: 'predict', name: 'Predict', category: 'ml', icon: 'Sparkles', description: 'Predict with a model' },

  // 보안 (Policy-as-Code · 차등 프라이버시) — #2·#3
  { id: 'guardrail', name: 'Guardrail', category: 'security', icon: 'ShieldCheck', description: 'Static guardrail · blocks personal data' },
  { id: 'dp', name: 'DP Noise', category: 'security', icon: 'Lock', description: 'Differential-privacy noise (Laplace/Gaussian)' },
]

// 노드별 기본 파라미터
export const DAG_DEFAULT_PARAMS = {
  fileInput: { filePath: 'visual-ide/data/seoul_air_quality.csv', fileType: 'csv', detectedSchema: [] },
  select: { columns: [] },
  filter: { column: '', operator: '>', value: '' },
  fillNull: { column: '', value: '0' },
  dropNull: { column: '' },
  sort: { column: '', descending: false },
  take: { n: 100 },
  groupBy: { column: '', agg: 'mean', aggColumn: '' },
  count: {},
  chart: { chartType: 'bar', x: '', y: '', title: '' },
  model: { name: 'Predictor', layers: 'Dense(32) -> ReLU() -> Dense(1)' },
  train: { modelName: 'Predictor', modelVar: 'predictor_model', target: '', epochs: 10, lr: 0.01 },
  predict: { modelVar: 'predictor_model', as: 'pred' },
  guardrail: { policy: 'PII', action: 'block' },
  dp: { mechanism: 'laplace', epsilon: 1.0, sensitivity: 1.0 },
}

/**
 * 노드별 파라미터 폼 정의 (직관적 UI용).
 * 각 필드: { key, label, type: 'text'|'number'|'select'|'checkbox', options?, placeholder?, hint? }
 * key 는 DAG_DEFAULT_PARAMS 와 대응한다.
 */
export const NODE_PARAM_FIELDS = {
  fileInput: [
    { key: 'filePath', label: 'File path', type: 'text', placeholder: 'visual-ide/data/...csv' },
  ],
  select: [
    { key: 'columns', label: 'Columns to keep (comma-separated)', type: 'text', placeholder: 'pm25, temperature_c' },
  ],
  filter: [
    { key: 'column', label: 'Column', type: 'text', placeholder: 'pm25' },
    { key: 'operator', label: 'Operator', type: 'select', options: ['==', '!=', '>', '>=', '<', '<='] },
    { key: 'value', label: 'Value', type: 'text', placeholder: '35' },
  ],
  fillNull: [
    { key: 'column', label: 'Column', type: 'text', placeholder: 'pm25' },
    { key: 'value', label: 'Fill value', type: 'text', placeholder: '31.0' },
  ],
  dropNull: [{ key: 'column', label: 'Column', type: 'text', placeholder: 'pm25' }],
  sort: [
    { key: 'column', label: 'Sort column', type: 'text', placeholder: 'pm25' },
    { key: 'descending', label: 'Descending', type: 'checkbox' },
  ],
  take: [{ key: 'n', label: 'Rows', type: 'number' }],
  groupBy: [
    { key: 'column', label: 'Group column', type: 'text', placeholder: 'district' },
    { key: 'agg', label: 'Aggregate', type: 'select', options: ['count', 'sum', 'mean', 'min', 'max'] },
    { key: 'aggColumn', label: 'Aggregate column', type: 'text', placeholder: 'pm25 (empty for count)' },
  ],
  count: [],
  chart: [
    { key: 'chartType', label: 'Chart type', type: 'select', options: ['bar', 'line', 'scatter', 'pie', 'area'] },
    { key: 'x', label: 'X axis', type: 'text', placeholder: 'district' },
    { key: 'y', label: 'Y axis', type: 'text', placeholder: 'pm25_pred' },
    { key: 'title', label: 'Title', type: 'text', placeholder: 'Chart title' },
  ],
  model: [
    { key: 'name', label: 'Model name', type: 'text', placeholder: 'AirPredictor' },
    { key: 'layers', label: 'Layers (Burn)', type: 'text', placeholder: 'Dense(32) -> ReLU() -> Dense(1)' },
  ],
  train: [
    { key: 'modelName', label: 'Model name', type: 'text', placeholder: 'AirPredictor' },
    { key: 'modelVar', label: 'Model variable', type: 'text', placeholder: 'predictor_model' },
    { key: 'target', label: 'Target column', type: 'text', placeholder: 'pm25' },
    { key: 'epochs', label: 'Epochs', type: 'number' },
    { key: 'lr', label: 'Learning rate', type: 'number', step: '0.001' },
  ],
  predict: [
    { key: 'modelVar', label: 'Model variable', type: 'text', placeholder: 'predictor_model' },
    { key: 'as', label: 'Prediction column', type: 'text', placeholder: 'pred' },
  ],
  guardrail: [
    { key: 'policy', label: 'Policy', type: 'select', options: ['PII', 'SQL', 'SECRET'] },
    { key: 'action', label: 'On violation', type: 'select', options: ['block', 'warn', 'mask'] },
  ],
  dp: [
    { key: 'mechanism', label: 'Mechanism', type: 'select', options: ['laplace', 'gaussian'] },
    { key: 'epsilon', label: 'Privacy Budget (ε)', type: 'number', step: '0.1' },
    { key: 'sensitivity', label: 'Sensitivity (Δf)', type: 'number', step: '0.1' },
  ],
}

// 파일 입력 노드의 기본 스키마 (미리 선택된 샘플)
export const SEED_SCHEMA = [
  { name: 'observed_at', type: 'string' },
  { name: 'district', type: 'string' },
  { name: 'pm25', type: 'float' },
  { name: 'temperature_c', type: 'float' },
]

/**
 * 정적 data.js pipeline (원본 시연 노드) → transpiler용 노드/엣지로 변환.
 * 원본 캔버스를 기본 편집 DAG로 시드하는 데 사용.
 */
// Laid out as two rows (preprocess, then ML) rather than one 1490px line: the
// edit canvas is roughly 390px wide, and fitView is floored by minZoom 0.3, so a
// single row left the right-hand nodes clipped out of view on first open.
export function seedFromStaticPipeline() {
  return {
    nodes: [
      {
        id: 'load',
        type: 'fileInput',
        position: { x: 20, y: 40 },
        data: {
          label: 'File Input',
          category: 'inout',
          icon: 'Database',
          source: true,
          parameters: {
            filePath: 'visual-ide/data/seoul_air_quality.csv',
            detectedSchema: JSON.parse(JSON.stringify(SEED_SCHEMA)),
          },
        },
      },
      {
        id: 'schema',
        type: 'select',
        position: { x: 250, y: 40 },
        data: {
          label: 'Select',
          category: 'prep',
          icon: 'Columns',
          parameters: { columns: [{ name: 'observed_at', keep: true }, { name: 'district', keep: true }, { name: 'pm25', keep: true }, { name: 'temperature_c', keep: true }] },
        },
      },
      {
        id: 'fill',
        type: 'fillNull',
        position: { x: 480, y: 40 },
        data: {
          label: 'Fill Null',
          category: 'prep',
          icon: 'PenLine',
          parameters: { column: 'pm25', value: '31.0' },
        },
      },
      {
        id: 'filter',
        type: 'filter',
        position: { x: 710, y: 40 },
        data: {
          label: 'Filter',
          category: 'prep',
          icon: 'Filter',
          parameters: { column: 'pm25', operator: '<=', value: '35' },
        },
      },
      {
        id: 'model',
        type: 'model',
        position: { x: 20, y: 260 },
        data: {
          label: 'Model',
          category: 'ml',
          icon: 'BrainCircuit',
          parameters: { name: 'AirPredictor', layers: 'Dense(32) -> ReLU() -> Dense(1)' },
        },
      },
      {
        id: 'train',
        type: 'train',
        position: { x: 250, y: 260 },
        data: {
          label: 'Train',
          category: 'ml',
          icon: 'GraduationCap',
          parameters: { modelName: 'AirPredictor', modelVar: 'airpredictor_model', target: 'pm25', epochs: 3, lr: 0.01 },
        },
      },
      {
        id: 'predict',
        type: 'predict',
        position: { x: 480, y: 260 },
        data: {
          label: 'Predict',
          category: 'ml',
          icon: 'Sparkles',
          parameters: { modelVar: 'airpredictor_model', as: 'pm25_pred' },
        },
      },
      {
        id: 'take',
        type: 'take',
        position: { x: 710, y: 260 },
        data: {
          label: 'Take',
          category: 'prep',
          icon: 'Scissors',
          parameters: { n: 5 },
        },
      },
    ],
    // React Flow keys edges by id; without one every seeded edge shares key undefined.
    edges: [
      ['load', 'schema'],
      ['schema', 'fill'],
      ['fill', 'filter'],
      ['filter', 'train'],
      ['filter', 'predict'],
      ['train', 'predict'],
      ['predict', 'take'],
    ].map(([source, target]) => ({ id: `e-${source}-${target}`, source, target })),
  }
}
/**
 * CSV bytes → text for the offline fallback. UTF-8 first (fatal, so a Korean
 * Windows export does not silently turn into U+FFFD), then EUC-KR — the WHATWG
 * "euc-kr" decoder is windows-949, so CP949 files decode too. Mirrors the server's
 * decode_bytes order.
 */
export async function decodeCsv(file) {
  const bytes = await file.arrayBuffer()
  try {
    return new TextDecoder('utf-8', { fatal: true }).decode(bytes)
  } catch {
    return new TextDecoder('euc-kr').decode(bytes)
  }
}

/**
 * CSV 텍스트에서 컬럼명/타입을 자동 감지한다. (브라우저 파일 선택용)
 * @param {string} text
 * @returns {{name:string, type:string}[]}
 */
export function detectCsvSchema(text) {
  const cleaned = text.replace(/^\uFEFF/, '')
  const lines = cleaned.split(/\r?\n/).filter((l) => l.trim() !== '')
  if (lines.length === 0) return []
  const parseRow = (line) => {
    const result = []
    let inQuotes = false
    let cur = ''
    for (let i = 0; i < line.length; i++) {
      const ch = line[i]
      if (ch === '"') {
        if (inQuotes && line[i + 1] === '"') { cur += '"'; i++ }
        else inQuotes = !inQuotes
      } else if (ch === ',' && !inQuotes) { result.push(cur); cur = '' }
      else cur += ch
    }
    result.push(cur)
    return result.map((v) => v.trim())
  }
  const headers = parseRow(lines[0])
  const sample = lines.slice(1, 21).map(parseRow)
  const inferType = (vals) => {
    const nz = vals.filter((v) => v !== '' && v !== null && v !== undefined)
    if (nz.length === 0) return 'string'
    if (nz.every((v) => /^-?\d+$/.test(v))) return 'int'
    if (nz.every((v) => /^-?\d+(\.\d+)?$/.test(v))) return 'float'
    if (nz.every((v) => ['true', 'false', '1', '0'].includes(v.toLowerCase()))) return 'bool'
    return 'string'
  }
  return headers.map((name, i) => ({
    name: name || `col_${i}`,
    type: inferType(sample.map((r) => r[i] || '')),
  }))
}
