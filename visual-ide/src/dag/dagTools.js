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
  { id: 'fileInput', name: 'File Input', category: 'inout', icon: 'Database', description: 'CSV 데이터 로드 (스키마 추론)' },
  { id: 'select', name: 'Select', category: 'prep', icon: 'Columns', description: '컬럼 선택' },
  { id: 'filter', name: 'Filter', category: 'prep', icon: 'Filter', description: '조건부 필터' },
  { id: 'fillNull', name: 'Fill Null', category: 'prep', icon: 'PenLine', description: '결측치 채우기' },
  { id: 'dropNull', name: 'Drop Null', category: 'prep', icon: 'Trash2', description: 'null 행 제거' },
  { id: 'sort', name: 'Sort', category: 'prep', icon: 'ArrowUpDown', description: '정렬 (orderBy)' },
  { id: 'take', name: 'Take', category: 'prep', icon: 'Scissors', description: '상위 N 행' },
  { id: 'groupBy', name: 'Group By', category: 'transform', icon: 'Group', description: '그룹 집계' },
  { id: 'count', name: 'Count', category: 'transform', icon: 'Hash', description: '행 수' },
  { id: 'chart', name: 'Chart', category: 'transform', icon: 'BarChart2', description: '시각화' },

  // ML (Burn)
  { id: 'model', name: 'Model', category: 'ml', icon: 'BrainCircuit', description: '모델 선언 (model {})' },
  { id: 'train', name: 'Train', category: 'ml', icon: 'GraduationCap', description: 'Burn 학습' },
  { id: 'predict', name: 'Predict', category: 'ml', icon: 'Sparkles', description: '모델 예측' },

  // 보안 (Policy-as-Code · 차등 프라이버시) — #2·#3
  { id: 'guardrail', name: 'Guardrail', category: 'security', icon: 'ShieldCheck', description: '정적 가드레일 · 개인정보 차단' },
  { id: 'dp', name: 'DP Noise', category: 'security', icon: 'Lock', description: '차등 프라이버시 노이즈 (Laplace/Gaussian)' },
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
  dp: { mechanism: 'laplace', epsilon: 1.0 },
}

/**
 * 노드별 파라미터 폼 정의 (직관적 UI용).
 * 각 필드: { key, label, type: 'text'|'number'|'select'|'checkbox', options?, placeholder?, hint? }
 * key 는 DAG_DEFAULT_PARAMS 와 대응한다.
 */
export const NODE_PARAM_FIELDS = {
  fileInput: [
    { key: 'filePath', label: '파일 경로', type: 'text', placeholder: 'visual-ide/data/...csv' },
  ],
  select: [
    { key: 'columns', label: '선택할 컬럼 (쉼표 구분)', type: 'text', placeholder: 'pm25, temperature_c' },
  ],
  filter: [
    { key: 'column', label: '컬럼', type: 'text', placeholder: 'pm25' },
    { key: 'operator', label: '연산자', type: 'select', options: ['==', '!=', '>', '>=', '<', '<='] },
    { key: 'value', label: '값', type: 'text', placeholder: '35' },
  ],
  fillNull: [
    { key: 'column', label: '컬럼', type: 'text', placeholder: 'pm25' },
    { key: 'value', label: '채울 값', type: 'text', placeholder: '31.0' },
  ],
  dropNull: [{ key: 'column', label: '컬럼', type: 'text', placeholder: 'pm25' }],
  sort: [
    { key: 'column', label: '정렬 컬럼', type: 'text', placeholder: 'pm25' },
    { key: 'descending', label: '내림차순', type: 'checkbox' },
  ],
  take: [{ key: 'n', label: '행 수', type: 'number' }],
  groupBy: [
    { key: 'column', label: '그룹 컬럼', type: 'text', placeholder: 'district' },
    { key: 'agg', label: '집계', type: 'select', options: ['count', 'sum', 'mean', 'min', 'max'] },
    { key: 'aggColumn', label: '집계 대상 컬럼', type: 'text', placeholder: 'pm25 (count면 비워도 됨)' },
  ],
  count: [],
  chart: [
    { key: 'chartType', label: '차트 유형', type: 'select', options: ['bar', 'line', 'scatter', 'pie', 'area'] },
    { key: 'x', label: 'X 축', type: 'text', placeholder: 'district' },
    { key: 'y', label: 'Y 축', type: 'text', placeholder: 'pm25_pred' },
    { key: 'title', label: '제목', type: 'text', placeholder: '차트 제목' },
  ],
  model: [
    { key: 'name', label: '모델 이름', type: 'text', placeholder: 'AirPredictor' },
    { key: 'layers', label: '레이어 (Burn)', type: 'text', placeholder: 'Dense(32) -> ReLU() -> Dense(1)' },
  ],
  train: [
    { key: 'modelName', label: '모델 이름', type: 'text', placeholder: 'AirPredictor' },
    { key: 'modelVar', label: '모델 변수', type: 'text', placeholder: 'predictor_model' },
    { key: 'target', label: '목표 컬럼', type: 'text', placeholder: 'pm25' },
    { key: 'epochs', label: '에폭', type: 'number' },
    { key: 'lr', label: '학습률', type: 'number', step: '0.001' },
  ],
  predict: [
    { key: 'modelVar', label: '모델 변수', type: 'text', placeholder: 'predictor_model' },
    { key: 'as', label: '예측 컬럼명', type: 'text', placeholder: 'pred' },
  ],
  guardrail: [
    { key: 'policy', label: '정책', type: 'select', options: ['PII', 'SQL', 'SECRET'] },
    { key: 'action', label: '위반 동작', type: 'select', options: ['block', 'warn', 'mask'] },
  ],
  dp: [
    { key: 'mechanism', label: '메커니즘', type: 'select', options: ['laplace', 'gaussian'] },
    { key: 'epsilon', label: 'Privacy Budget (ε)', type: 'number', step: '0.1' },
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
export function seedFromStaticPipeline() {
  return {
    nodes: [
      {
        id: 'load',
        type: 'fileInput',
        position: { x: 20, y: 60 },
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
        position: { x: 250, y: 60 },
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
        position: { x: 480, y: 60 },
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
        position: { x: 770, y: 60 },
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
        position: { x: 480, y: 220 },
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
        position: { x: 1060, y: 120 },
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
        position: { x: 1060, y: 40 },
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
        position: { x: 1350, y: 40 },
        data: {
          label: 'Take',
          category: 'prep',
          icon: 'Scissors',
          parameters: { n: 5 },
        },
      },
    ],
    edges: [
      { source: 'load', target: 'schema' },
      { source: 'schema', target: 'fill' },
      { source: 'fill', target: 'filter' },
      { source: 'filter', target: 'train' },
      { source: 'filter', target: 'predict' },
      { source: 'train', target: 'predict' },
      { source: 'predict', target: 'take' },
    ],
  }
}