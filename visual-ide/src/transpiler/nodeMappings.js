/**
 * nodeMappings.js — Xazz DSL 변환 규칙
 *
 * SUPPORTED_OPS: load, select, filter, count, groupBy, sum, mean, min, max,
 *                orderBy, take, dropNull, fillNull, join, withColumn, chart
 * XAZZ ML (v0.5): model, train, predict (Burn 딥러닝 컴파일)
 *
 * 출력은 Xazz 컴파일러(xazz-compiler)의 정확한 문법을 따른다:
 *   - select(["a","b"])  → select([a, b])          (bare ident)
 *   - withColumn("c", expr) → withColumn("c", col("x") * 2)
 *   - join  → join(other_var, on: "key")
 */

import { sanitizeFieldName } from './pathResolver.js';

// ─── 경로 정규화 ───────────────────────────────────────────────────────────────
/**
 * UI 원시 파일명 → Xazz DSL load 경로.
 * Xazz는 @data alias 대신 저장소 내 상대 경로(visual-ide/data/...)를 사용한다.
 * @param {string} path
 * @returns {string}
 */
export function normalizeLoadPath(path) {
  if (!path || typeof path !== 'string') return 'visual-ide/data/data.csv';
  if (path.startsWith('visual-ide/') || path.startsWith('examples/')) return path;
  if (/^[A-Za-z]:[\\\/]/.test(path) || path.startsWith('/')) return path;
  return `visual-ide/data/${path}`;
}

// ─── 컬럼 내부명 변환 ─────────────────────────────────────────────────────────
function resolveColumn(node, originalName) {
  const mapping = node.data?.parameters?.columnMapping || {};
  return mapping[originalName] ?? sanitizeFieldName(originalName);
}

// ─── 타입 변환 ────────────────────────────────────────────────────────────────
function mapTypeToXazz(rawType) {
  const t = (rawType || 'String').toLowerCase().replace(/\s+/g, '');
  if (t.includes('int')) return 'int';
  if (t.includes('float') || t.includes('double')) return 'float';
  if (t.includes('bool')) return 'bool';
  return 'string';
}

function buildSchemaFields(schema) {
  if (!schema || schema.length === 0) return '  _unknown: string';
  return schema
    .map(col => `  ${sanitizeFieldName(col.name)}: ${mapTypeToXazz(col.type)}`)
    .join(',\n');
}

// ─── 연산자 / 리터럴 ──────────────────────────────────────────────────────────
function mapOperator(op) {
  const MAP = { '==': '==', '!=': '!=', '>': '>', '>=': '>=', '<': '<', '<=': '<=' };
  return MAP[op] || '==';
}

function toLiteral(raw) {
  if (raw === '' || raw === undefined || raw === null) return '0';
  const s = String(raw).trim();
  if (s === 'true' || s === 'false') return s;
  if (/^-?[\d_]+(\.\d+)?$/.test(s)) return s;
  return `"${s}"`;
}

// ─── 변환 규칙 ─────────────────────────────────────────────────────────────────
export const NODE_MAPPINGS = {

  // ── fileInput ─────────────────────────────────────────────────────────────
  // type Schema_<var> = { ... }
  // v <var> = load("path") :: Schema_<var>
  fileInput: (node, varName) => {
    const params = node.data?.parameters || {};
    const rawPath = params.filePath || 'data.csv';
    const schema = params.detectedSchema || [];
    const schemaName = `Schema_${varName}`;
    const resolvedPath = normalizeLoadPath(rawPath);

    const columnMapping = {};
    schema.forEach(col => { columnMapping[col.name] = sanitizeFieldName(col.name); });
    if (node.data && node.data.parameters) {
      node.data.parameters.columnMapping = columnMapping;
    }

    const typeBlock = `type ${schemaName} = {\n${buildSchemaFields(schema)}\n}`;
    const loadStmt = `v ${varName} = load("${resolvedPath}") :: ${schemaName}`;

    return { type: 'source', lines: [typeBlock, loadStmt], varName, columnMapping };
  },

  // ── filter ────────────────────────────────────────────────────────────────
  // |> filter(col("col") op literal)
  filter: (node) => {
    const params = node.data?.parameters || {};
    const column = resolveColumn(node, params.column || '_col');
    const operator = mapOperator(params.operator || '==');
    const value = toLiteral(params.value ?? '');
    return { type: 'pipeline', lines: [`|> filter(col("${column}") ${operator} ${value})`] };
  },

// ── select ────────────────────────────────────────────────────────────────
  // |> select([a, b])   ← Xazz: bare ident (따옴표 없음)
  // columns 는 배열([{name,keep}] 또는 [string]) 또는 "a, b" 문자열을 허용한다.
  select: (node) => {
    const params = node.data?.parameters || {};
    let columns = params.columns || [];
    // 사용자가 문자열("a, b")로 입력한 경우 → 배열로 분리
    if (typeof columns === 'string') {
      columns = columns.split(',').map((s) => s.trim()).filter(Boolean);
    }
    const colNames = columns
      .map((c) => {
        const name = typeof c === 'string' ? c : (c.keep !== false ? c.name : null);
        if (!name) return null;
        return resolveColumn(node, name);
      })
      .filter(Boolean);
    if (colNames.length === 0) {
      return { type: 'pipeline', lines: ['|> select([_unknown])  // no columns configured'] };
    }
    return { type: 'pipeline', lines: [`|> select([${colNames.join(', ')}])`] };
  },

  // ── groupBy ───────────────────────────────────────────────────────────────
  // |> groupBy("group_col") |> count("group_col")  (agg=count)
  // |> groupBy("group_col") |> mean("agg_col")      (agg=sum/mean/min/max)
  groupBy: (node) => {
    const params = node.data?.parameters || {};
    const column = resolveColumn(node, params.column || '_col');
    const agg = params.agg || 'count';
    if (agg === 'count') {
      // Xazz 컴파일러는 groupBy 뒤 count 를 집계로 인정하지 않아 정적 분석 경고가 남는다.
      // 실행은 성공하지만, DIAGNOSTIC 경고를 피하려면 sum/mean 등 다른 집계를 쓰는 게 좋다.
      return {
        type: 'pipeline',
        lines: [
          `|> groupBy("${column}") |> count`,
          `// ⚠️ Xazz 는 groupBy 뒤 count 를 집계로 인식하지 못합니다. 정적 분석 경고가 남으면 sum/mean 등 다른 집계를 사용하세요.`,
        ],
      };
    }
    // 집계 대상 컬럼: aggColumn 지정 시 사용, 없으면 경고 주석과 함께 그룹 키 사용 회피
    const aggCol = params.aggColumn ? resolveColumn(node, params.aggColumn) : null;
    if (!aggCol) {
      return {
        type: 'pipeline',
        lines: [
          `|> groupBy("${column}") |> ${agg}("${column}")`,
          `// ⚠️ ${agg}() 대상 컬럼(aggColumn)을 지정하세요 — 그룹 키가 아니라 수치 컬럼에 집계를 적용해야 합니다`,
        ],
      };
    }
    return { type: 'pipeline', lines: [`|> groupBy("${column}") |> ${agg}("${aggCol}")`] };
  },

  // ── count ─────────────────────────────────────────────────────────────────
  count: (node) => {
    const params = node.data?.parameters || {};
    const column = params.column ? resolveColumn(node, params.column) : null;
    return { type: 'pipeline', lines: [column ? `|> count("${column}")` : '|> count'] };
  },

  sum: (node) => {
    const c = resolveColumn(node, node.data?.parameters?.column || '_col');
    return { type: 'pipeline', lines: [`|> sum("${c}")`] };
  },

  mean: (node) => {
    const c = resolveColumn(node, node.data?.parameters?.column || '_col');
    return { type: 'pipeline', lines: [`|> mean("${c}")`] };
  },

  min: (node) => {
    const c = resolveColumn(node, node.data?.parameters?.column || '_col');
    return { type: 'pipeline', lines: [`|> min("${c}")`] };
  },

  max: (node) => {
    const c = resolveColumn(node, node.data?.parameters?.column || '_col');
    return { type: 'pipeline', lines: [`|> max("${c}")`] };
  },

  // ── sort → orderBy ────────────────────────────────────────────────────────
  sort: (node) => {
    const params = node.data?.parameters || {};
    const column = resolveColumn(node, params.column || '_col');
    const desc = params.descending === true ? 'true' : 'false';
    return { type: 'pipeline', lines: [`|> orderBy("${column}", desc: ${desc})`] };
  },

  // ── take ──────────────────────────────────────────────────────────────────
  take: (node) => {
    const n = parseInt(node.data?.parameters?.count ?? node.data?.parameters?.n ?? 100, 10);
    const safeN = isNaN(n) || n < 1 ? 100 : n;
    return { type: 'pipeline', lines: [`|> take(${safeN})`] };
  },

  // ── dropNull ──────────────────────────────────────────────────────────────
  dropNull: (node) => {
    const c = resolveColumn(node, node.data?.parameters?.column || '_col');
    return { type: 'pipeline', lines: [`|> dropNull("${c}")`] };
  },

  // ── fillNull ──────────────────────────────────────────────────────────────
  fillNull: (node) => {
    const params = node.data?.parameters || {};
    const c = resolveColumn(node, params.column || '_col');
    const value = toLiteral(params.value ?? 0);
    return { type: 'pipeline', lines: [`|> fillNull("${c}", ${value})`] };
  },

  // ── join ──────────────────────────────────────────────────────────────────
  // Xazz 문법: join(other_var, on: "key")
  join: (node) => {
    const params = node.data?.parameters || {};
    const right = params.right || 'other';
    const on = resolveColumn(node, params.on || '_col');
    const how = params.joinType === 'left' ? ', how: "left"'
      : params.joinType === 'outer' ? ', how: "outer"'
      : '';
    return { type: 'pipeline', lines: [`|> join(${right}, on: "${on}"${how})`] };
  },

  // ── withColumn ────────────────────────────────────────────────────────────
  // Xazz 문법: withColumn("col", expr)
  withColumn: (node) => {
    const params = node.data?.parameters || {};
    const col = params.col || 'new_col';
    const expr = params.expr || 'col("_col")';
    return { type: 'pipeline', lines: [`|> withColumn("${col}", ${expr})`] };
  },

  // ── chart ─────────────────────────────────────────────────────────────────
  // |> chart { type: "bar", x: "col", y: "col", title: "..." }
  chart: (node) => {
    const params = node.data?.parameters || {};
    const chartType = params.chartType || 'bar';
    const x = resolveColumn(node, params.x || '_col');
    const y = resolveColumn(node, params.y || '_col');
    const title = params.title || '';
    const titlePart = title ? `, title: "${title}"` : '';
    return {
      type: 'pipeline',
      lines: [`|> chart { type: "${chartType}", x: "${x}", y: "${y}"${titlePart} }`],
    };
  },

  // ══════════════════════════════════════════════════════════════════════════
  // XAZZ ML (v0.5) — Burn 딥러닝 컴파일 노드
  // ══════════════════════════════════════════════════════════════════════════

  // ── model ─────────────────────────────────────────────────────────────────
  // model Name { Dense(32) -> ReLU() -> Dense(1) }
  model: (node) => {
    const params = node.data?.parameters || {};
    const name = params.name || 'Predictor';
    const layers = params.layers || 'Dense(32) -> ReLU() -> Dense(1)';
    return { type: 'model', lines: [`model ${name} { ${layers} }`] };
  },

  // ── train ─────────────────────────────────────────────────────────────────
  // v <MODEL_VAR> = <DATA_VAR> |> train(ModelName, target: "col", epochs: N, lr: 0.01)
  train: (node) => {
    const params = node.data?.parameters || {};
    const modelName = params.modelName || 'Predictor';
    const target = params.target || '_col';
    const epochs = parseInt(params.epochs ?? 10, 10) || 10;
    const lr = params.lr ?? 0.01;
    const modelVar = params.modelVar || `${modelName.toLowerCase()}_model`;
    return {
      type: 'modelbind',
      modelVar,
      // transpiler 가 <DATA_VAR> placeholder 를 데이터 변수명으로 치환한다.
      lines: [
        `v ${modelVar} = <DATA_VAR>`,
        `  |> train(${modelName}, target: "${target}", epochs: ${epochs}, lr: ${lr})`,
      ],
    };
  },

  // ── predict ───────────────────────────────────────────────────────────────
  // v <predictVar> = <dataVar> |> predict(model_var, as: "pred_col")
  predict: (node, varName) => {
    const params = node.data?.parameters || {};
    const modelVar = params.modelVar || 'trained';
    const as = params.as || `${params.target || 'target'}_pred`;
    return {
      type: 'predict',
      modelVar,
      as,
      // transpiler 가 <DATA_VAR> / <PREDICT_VAR> placeholder 를 치환한다.
      lines: [
        `v <PREDICT_VAR> = <DATA_VAR>`,
        `  |> predict(${modelVar}, as: "${as}")`,
      ],
    };
  },

  // ── guardrail (issue #2 — Policy-as-Code 정적 가드레일) ──────────────────
  // 아직 백엔드 미구현: 명확한 placeholder 주석으로 생성.
  // #2 완료 시 실제 API 호출/연산자로 교체하면 된다.
  guardrail: (node) => {
    const params = node.data?.parameters || {};
    const policy = params.policy || 'PII';
    const action = params.action || 'block';
    return {
      type: 'pipeline',
      lines: [
        `// [guardrail:${policy}] Policy-as-Code 정적 가드레일 — 위반 시 ${action}`,
        `// TODO(#2): sLM 코드 자동 보정 API 로 교체 예정`,
      ],
    };
  },

  // ── dp (issue #3 — 차등 프라이버시 노이즈) ──────────────────────────────
  // 플레이스홀더: #3 구현 시 Laplace/Gaussian 실제 메커니즘으로 교체.
  dp: (node) => {
    const params = node.data?.parameters || {};
    const mech = params.mechanism || 'laplace';
    const eps = params.epsilon ?? 1.0;
    return {
      type: 'pipeline',
      lines: [
        `// [dp:${mech}] Differential Privacy 노이즈 주입 · ε=${eps}`,
        `// TODO(#3): Polars-Burn 바인딩 시 실제 DP 메커니즘으로 교체 예정`,
      ],
    };
  },
};

// ─── 폴백 ─────────────────────────────────────────────────────────────────────
export function getFallbackMapping(node) {
  const typeName = node.type || 'unknown';
  return { type: 'pipeline', lines: [`// [${typeName}] unsupported node type – skipped`] };
}