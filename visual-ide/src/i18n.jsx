import React, { createContext, useCallback, useContext, useEffect, useMemo, useState } from 'react'

/**
 * Two-language UI copy for the Xazz workspace.
 *
 * What gets translated and what does not is a deliberate line, not an oversight:
 *
 *   Translated — the prose that explains a step: intents, impacts, headings,
 *   empty states, hints, button labels. This is what a Korean-reading developer
 *   needs in order to follow what the pipeline is doing.
 *
 *   Left in English — ML and language terms (`train`, `predict`, `epochs`, `lr`,
 *   `loss`, Burn, TrainReport), column names (`pm25`), .xzz types (`Float`),
 *   file paths, and the generated code. Translating these would make the screen
 *   disagree with the code the user is reading beside it.
 *
 *   Also left in English — the status axis vocabulary (`Available`, `Beta`,
 *   `Research`, `Planned`, `Partial`, `Succeeded`, `Not configured`, …).
 *   docs/design/state-contract.md defines these as an exact contract vocabulary
 *   with distinct meanings per axis; a translation would blur distinctions the
 *   contract exists to keep apart. Their surrounding prose is translated.
 */

export const LANGUAGES = ['en', 'ko']
const STORAGE_KEY = 'xazz_lang'

const en = {
  toggle: { label: 'Interface language', en: 'EN', ko: '한국어' },
  topbar: {
    liveCheck: 'Live Check',
    liveCheckHint: 'Server health',
    fullRun: 'Full Run',
    connectHint: 'Connect to xazz-server to execute',
  },
  rail: {
    search: 'Search project',
    project: 'Project',
    operations: 'Pipeline operations',
    operationsHelp: 'Keyboard-selectable mirror of the canvas.',
    selected: 'Selected',
    upstream: 'Upstream',
    downstream: 'Downstream',
    labsTitle: 'Models & policy',
    labsBody: 'Separated from current Core.',
  },
  canvas: {
    title: 'Compiler Canvas',
    views: { edit: 'Edit', graph: 'Graph', split: 'Split', code: 'Code', monitor: 'Monitor' },
    scope: 'Structural pipeline canvas · evidence comes from the Full Run response',
    staleScope: 'Last result · stale while Full Run is pending',
    bands: { PREPROCESS: 'PREPROCESS', 'ML COMPILE': 'ML COMPILE' },
  },
  inspector: {
    selected: 'Selected operation',
    intent: 'Intent',
    impact: 'Impact',
    rows: 'Rows',
    nulls: 'Nulls',
    schema: 'Schema',
    duration: 'Duration',
    artifact: 'Artifact',
    lineage: 'Lineage',
    upstream: 'upstream',
    downstream: 'downstream',
    note: 'Metrics the current runtime does not emit remain labelled “Not emitted,” rather than inferred.',
    rowsReturned: '{n} rows returned',
    fieldCount: '{n} fields',
    stale: 'Last result · stale while Full Run is pending.',
    fromRun: 'From the last real Full Run response.',
    structural: 'Canvas is structural · per-node metrics come from the run response.',
  },
  dag: {
    palette: 'Tool Palette',
    paletteHelp: 'Drag a node onto the canvas, or click to add it',
    categories: {
      inout: 'Data',
      prep: 'Preprocess',
      transform: 'Transform',
      ml: 'ML · Burn',
      security: 'Security',
    },
    generated: 'Generated Xazz',
    copy: 'Copy code',
    copied: 'Copied',
    params: 'Node Params',
    paramsEmpty: 'Select a node on the canvas to edit its parameters.',
    noParams: 'This node takes no parameters.',
    delete: 'Delete',
    save: 'Save DAG',
    reset: 'Reset',
    hint: 'Drag from a node’s right ○ to another node’s left ○ to connect · select a node and press Delete to remove it',
    filePick: 'Choose a file (schema auto-detected)',
    detectedSchema: 'Detected schema',
    detected: '{n} columns detected',
    readFailed: 'Could not read the file',
  },
  dock: {
    tabs: { preview: 'Preview', delta: 'Delta', chart: 'Chart', logs: 'Logs', receipt: 'Receipt' },
    notRun: 'Not run yet',
    rows: 'Rows',
    columns: 'Columns',
    emptyTitle: 'No real result yet',
    emptyBody: 'Run Full Run to execute example.xzz against xazz-server and preview the returned rows.',
  },
}

const ko = {
  toggle: { label: '인터페이스 언어', en: 'EN', ko: '한국어' },
  topbar: {
    liveCheck: '연결 확인',
    liveCheckHint: '서버 상태',
    fullRun: '전체 실행',
    connectHint: '실행하려면 xazz-server에 연결하세요',
  },
  rail: {
    search: '프로젝트 검색',
    project: '프로젝트',
    operations: '파이프라인 단계',
    operationsHelp: '캔버스를 키보드로 이동할 수 있는 목록입니다.',
    selected: '선택됨',
    upstream: '앞 단계',
    downstream: '뒤 단계',
    labsTitle: '모델 · 정책',
    labsBody: '현재 Core와 분리되어 있습니다.',
  },
  canvas: {
    title: '컴파일러 캔버스',
    views: { edit: '편집', graph: '그래프', split: '분할', code: '코드', monitor: '모니터' },
    scope: '구조를 보여주는 캔버스입니다 · 수치는 전체 실행 응답에서 옵니다',
    staleScope: '이전 결과입니다 · 전체 실행이 끝날 때까지 최신이 아닙니다',
    bands: { PREPROCESS: '전처리', 'ML COMPILE': 'ML 컴파일' },
  },
  inspector: {
    selected: '선택한 단계',
    intent: '이 단계가 하는 일',
    impact: '영향',
    rows: '행',
    nulls: '결측치',
    schema: '스키마',
    duration: '소요 시간',
    artifact: '산출물',
    lineage: '연결',
    upstream: '앞 단계',
    downstream: '뒤 단계',
    note: '현재 런타임이 내보내지 않는 값은 추정하지 않고 “측정되지 않음”으로 표시합니다.',
    rowsReturned: '{n}행 반환됨',
    fieldCount: '{n}개 필드',
    stale: '이전 결과입니다 · 전체 실행이 끝날 때까지 최신이 아닙니다.',
    fromRun: '마지막 실제 전체 실행 응답에서 가져왔습니다.',
    structural: '캔버스는 구조를 보여줍니다 · 단계별 수치는 실행 응답에서 옵니다.',
  },
  dag: {
    palette: '도구 팔레트',
    paletteHelp: '노드를 캔버스로 끌어놓거나 클릭해 추가하세요',
    categories: {
      inout: '데이터',
      prep: '전처리',
      transform: '변환',
      ml: 'ML · Burn',
      security: '보안',
    },
    generated: '생성된 Xazz 코드',
    copy: '코드 복사',
    copied: '복사됨',
    params: '노드 설정',
    paramsEmpty: '캔버스에서 노드를 선택하면 설정을 편집할 수 있습니다.',
    noParams: '이 노드에는 설정할 값이 없습니다.',
    delete: '삭제',
    save: 'DAG 저장',
    reset: '초기화',
    hint: '노드 오른쪽 ○ 에서 다른 노드 왼쪽 ○ 으로 끌면 연결됩니다 · 노드를 선택하고 Delete 를 누르면 삭제됩니다',
    filePick: '파일 선택 (스키마 자동 감지)',
    detectedSchema: '감지된 스키마',
    detected: '{n}개 컬럼 감지됨',
    readFailed: '파일을 읽지 못했습니다',
  },
  dock: {
    tabs: { preview: '미리보기', delta: '변화량', chart: '차트', logs: '로그', receipt: '실행 기록' },
    notRun: '아직 실행하지 않음',
    rows: '행',
    columns: '열',
    emptyTitle: '아직 실제 결과가 없습니다',
    emptyBody: '전체 실행을 누르면 example.xzz 를 xazz-server 에서 실행하고 반환된 행을 보여줍니다.',
  },
}

/**
 * Per-step copy, keyed by pipeline node id. English lives in src/data.js as the
 * single source of truth for the fixture; only the Korean rendering lives here,
 * so a value can never drift from its English original without being noticed.
 * ML terms, column names and .xzz types stay verbatim.
 */
const stepsKo = {
  load: {
    label: 'CSV 불러오기',
    evidence: '100행 · UTF-8',
    intent: '합성 대기질 샘플을 읽습니다.',
    nulls: `결측치 6개 발견`,
    schema: '4개 필드 추론됨',
    duration: '현재 런타임이 측정하지 않음',
    artifact: '없음',
  },
  schema: {
    label: '스키마 확인',
    evidence: 'pm25 · Float?',
    intent: '생성된 .xzz 를 받아들이기 전에 추론된 타입을 검토합니다.',
    nulls: '널 허용 값 6개',
    schema: 'pm25: Float? · 검토 필요',
    duration: '현재 런타임이 측정하지 않음',
    artifact: 'main.xzz 변경 대기 중',
  },
  fill: {
    label: '결측치 채우기',
    evidence: '결측치 6 → 0',
    intent: '비어 있는 PM2.5 값을 고정 demo 값 31.0 으로 채웁니다.',
    nulls: '−6',
    schema: 'pm25 는 Float 유지',
    duration: '현재 런타임이 측정하지 않음',
    artifact: '없음',
  },
  filter: {
    label: '임계값 필터',
    evidence: '100 → 41행',
    intent: '35 μg/m³ 이하 관측만 남깁니다.',
    nulls: '0 → 0',
    schema: '타입 변화 없음',
    duration: '현재 런타임이 측정하지 않음',
    artifact: '없음',
  },
  result: {
    label: '결과',
    evidence: '41행',
    intent: '범위를 한정한 결과 미리보기를 제공하고, 원하면 브라우저에서 내보낼 수 있습니다.',
    nulls: '0',
    schema: '4개 필드',
    duration: '현재 런타임이 측정하지 않음',
    artifact: '요청되지 않음 · 결과 이후 선택적 내보내기',
  },
  compile: {
    label: 'AirNet 컴파일',
    evidence: '5개 레이어 · 209 params',
    intent: '선언한 레이어 스택을 Burn 모듈 설정으로 낮춥니다.',
    rows: '입력 행 없음 · 선언',
    nulls: '해당 없음',
    duration: '이 버전에서는 제공되지 않음',
    artifact: '없음',
  },
  train: {
    label: '모델 학습',
    evidence: '40 epochs · loss 0.0417',
    intent: '필터링된 행으로 pm25 를 타깃 삼아 AirNet 을 학습합니다.',
    nulls: '남은 결측치 0',
    duration: '이 버전에서는 제공되지 않음',
  },
  predict: {
    label: '예측',
    evidence: '+1개 열 · 41행',
    intent: '학습한 모델을 같은 행에 적용해 새 열로 만듭니다.',
    nulls: '0 → 0',
    schema: '+ pm25_pred: Float',
    duration: '이 버전에서는 제공되지 않음',
    artifact: '없음',
  },
}

const DICTIONARIES = { en, ko }

function lookup(dictionary, key) {
  return key.split('.').reduce((node, part) => (node == null ? undefined : node[part]), dictionary)
}

function readStoredLanguage() {
  try {
    const stored = localStorage.getItem(STORAGE_KEY)
    if (LANGUAGES.includes(stored)) return stored
  } catch (error) {
    /* private mode or blocked storage — fall through to the default */
  }
  return null
}

const LanguageContext = createContext(null)

export function LanguageProvider({
  initialLanguage = 'en',
  // An explicit ?lang= in the URL is a deliberate request and outranks the
  // remembered choice; without one, the remembered choice wins.
  forceInitial = false,
  onLanguageChange,
  children,
}) {
  const [language, setLanguageState] = useState(() => {
    const requested = LANGUAGES.includes(initialLanguage) ? initialLanguage : 'en'
    if (forceInitial) return requested
    return readStoredLanguage() ?? requested
  })

  useEffect(() => {
    document.documentElement.lang = language
    try {
      localStorage.setItem(STORAGE_KEY, language)
    } catch (error) {
      /* the choice simply will not persist */
    }
  }, [language])

  const setLanguage = useCallback(
    (next) => {
      if (!LANGUAGES.includes(next)) return
      setLanguageState(next)
      if (onLanguageChange) onLanguageChange(next)
    },
    [onLanguageChange],
  )

  const value = useMemo(() => {
    const t = (key) => {
      const translated = lookup(DICTIONARIES[language], key)
      if (translated !== undefined) return translated
      // An untranslated key falls back to English rather than rendering the key
      // itself, so a gap degrades to a readable screen instead of a broken one.
      return lookup(DICTIONARIES.en, key) ?? key
    }
    return { language, setLanguage, t }
  }, [language, setLanguage])

  return <LanguageContext.Provider value={value}>{children}</LanguageContext.Provider>
}

export function useLanguage() {
  const context = useContext(LanguageContext)
  if (context) return context
  // Components rendered outside a provider (tests, Storybook) still work in English.
  return { language: 'en', setLanguage: () => {}, t: (key) => lookup(en, key) ?? key }
}

/**
 * Localise one pipeline step. English stays authoritative; Korean overrides only
 * the fields it actually provides, so an untranslated field shows its original.
 */
export function localizeStep(node, language) {
  if (language !== 'ko') return node
  const override = stepsKo[node.id]
  if (!override) return node
  const { label, evidence, ...detail } = override
  return {
    ...node,
    label: label ?? node.label,
    evidence: evidence ?? node.evidence,
    detail: { ...node.detail, ...detail },
  }
}

export function LocaleSwitch({ compact = false }) {
  const { language, setLanguage, t } = useLanguage()
  return (
    <div
      className={`locale-switch ${compact ? 'locale-switch--compact' : ''}`}
      aria-label={t('toggle.label')}
    >
      {LANGUAGES.map((code) => (
        <button
          key={code}
          type="button"
          className={language === code ? 'is-active' : ''}
          aria-pressed={language === code}
          onClick={() => setLanguage(code)}
        >
          {t(`toggle.${code}`)}
        </button>
      ))}
    </div>
  )
}
