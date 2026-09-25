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
    inferring: 'Inferring schema on xazz-server…',
    detectedServer: '{n} columns inferred by xazz-server · 100-row sample · UTF-8 or EUC-KR',
    detectedBrowser: '{n} columns detected in this browser · 20-row sample · xazz-server offline',
    tooLarge: 'The file is larger than 50 MB, which xazz-server refuses.',
    serverRejected: 'xazz-server rejected the file',
    uploadNote: 'Server copy {path} · may be removed after about an hour',
    guardrail: { blocked: 'Guardrail: blocked', passed: 'Guardrail: passed' },
  },
  dock: {
    tabs: { preview: 'Preview', delta: 'Delta', chart: 'Chart', logs: 'Logs', receipt: 'Receipt', history: 'History' },
    notRun: 'Not run yet',
    rows: 'Rows',
    columns: 'Columns',
    emptyTitle: 'No real result yet',
    emptyBody: 'Run Full Run to execute example.xzz against xazz-server and preview the returned rows.',
  },  errors: {
    panelTitle: '{name} stopped working',
    panelBody: 'The rest of the workspace still works. Try again, or switch to another view.',
    retry: 'Try again',
    discardDag: 'Discard saved DAG',
    panels: {
      canvas: 'Canvas',
      inspector: 'Inspector',
      results: 'Result dock',
      workspace: 'Workspace',
      page: 'This page',
    },
  },
  server: {
    loading: 'Loading from xazz-server',
    offlineTitle: 'xazz-server is not reachable',
    offlineBody: 'Start xazz-server, then retry. Nothing here is shown until the server answers.',
    errorTitle: 'xazz-server answered {status}',
    retry: 'Retry',
  },
  history: {
    listLabel: 'Recorded runs, newest first',
    limit: 'Newest 50 runs · server limit',
    rowsShort: '{n} rows',
    pick: 'Select a run to reopen its receipt.',
    emptyTitle: 'No runs recorded for this tenant yet',
    emptyBody: 'Each Full Run that reaches the engine is recorded here and survives a server restart.',
    detailLabel: 'Run {id} receipt',
    created: 'Recorded at',
    rows: 'Result rows',
    verdictNotStored: 'Not stored · the server records the process exit only',
    sameCode: 'same code as the editor',
    otherCode: 'differs from the editor’s current code',
    audit: 'Audit trail',
    auditRecords: '{n} audit record(s) carry this code hash · outcomes: {outcomes}',
    auditUnavailable: 'Audit log unavailable',
    error: 'Error',
    restore: 'Restore result in Preview',
    metadataOnly: 'xazz-server keeps run metadata only. Rows return only in the /execute response, and this browser session does not hold this run’s response.',
    notFound: 'Run {id} was not found for this tenant',
    notFoundBody: 'Runs of another tenant answer 404 exactly like deleted ones, by design.',
  },
  progress: {
    eyebrow: 'Process running · xazz-server',
    title: 'Waiting for xazz-exec to return evidence',
    elapsed: 'Elapsed {s}s · measured in this browser',
    steps: {
      sent: 'Sent to xazz-server',
      server: 'Executing on xazz-server',
      result: 'Structured result',
    },
    epochs: 'Epoch progress · Not available in this version — xazz-server answers only after the process exits.',
    viewLogs: 'View logs',
    stop: 'Stop waiting',
    stopNote: 'Stopping drops this request. xazz-server has no cancel API and keeps executing, but a dropped request is currently not recorded in History, the audit chain or the ε ledger.',
    stoppedTitle: 'Stopped waiting for xazz-server',
    timedOutTitle: 'Stopped waiting after 5 minutes',
    stoppedBody: 'xazz-server may still execute the code, but a dropped request is currently not recorded in History or the audit chain. Run it again to get a recorded result.',
    runAgain: 'Run again',
    waitingRows: 'Waiting for rows from xazz-server',
  },
  lineage: {
    title: 'Column lineage',
    trace: 'Trace columns',
    retrace: 'Trace again',
    stale: 'Code changed since the last trace.',
    hint: 'Static compile via POST /catalog · nothing executes.',
    table: 'Table',
    diagram: 'Diagram',
    pipeline: 'Pipeline',
    output: 'Output',
    from: 'From',
    step: 'Step',
    source: 'source',
    derived: 'derived · no source column',
    empty: 'This code declares no pipeline.',
    select: 'Select an output column to highlight its sources.',
  },
  charts: {
    modes: 'Chart type',
    mode: { mean: 'Mean by group', distribution: 'Distribution', box: 'Box plot', training: 'Training curves' },
    column: 'Numeric column',
    groupBy: 'Group by',
    allRows: 'All rows',
    rows: 'rows',
    table: 'Table alternative',
    fromRun: 'From the rows the last Full Run returned',
    meanTitle: 'Mean {y} by {x}',
    distributionTitle: 'Distribution of {column}',
    distributionLabel: 'Histogram of {column}: {n} values in {bins} equal-width bins.',
    sample: '{n} values · {missing} row(s) without a number left out · Sturges bins',
    boxTitle: '{column} by {group}',
    boxTitleAll: '{column} · all rows',
    boxNote: 'Box Q1–Q3 · line median · whiskers 1.5×IQR · dots beyond',
    groupsShown: 'Largest {shown} of {total} groups · box Q1–Q3 · whiskers 1.5×IQR',
    noNumeric: 'The returned rows have no numeric column to chart.',
    noValues: '{column} has no numeric value in the returned rows, so there is nothing to draw.',
    noGroupColumn: 'Mean by group needs a text column to group by; the returned rows have none.',
    negativeMeans: 'Some group means are negative; bars from zero cannot show them, so see the table.',
    barsShown: 'First {shown} of {total} groups · see the table for the rest',
    trainingNa: 'Not available in this version. Loss curves need per-epoch history, which TrainReport does not carry yet (#117, #129); ROC, PR and a confusion matrix need classification metrics (#163). The final training and validation loss are in Monitor.',
  },
  mobile: {
    title: 'Compiler Canvas is a desktop tool.',
    body: 'The mobile landing experience is supported. Open this workspace at 1024px or wider to inspect graph, code, and evidence together.',
    back: 'Return to landing',
  },
  gov: {
    heading: 'Governance · live from xazz-server',
    subheading: 'What this tenant allows and what the server recorded. Independent of the last Full Run.',
    defaultTenant: 'the default tenant',
    namedTenant: 'tenant “{tenant}”',
    cancel: 'Cancel',
    refresh: 'Refresh',
    access: {
      title: 'Server access',
      tenant: 'Tenant (X-Xazz-Tenant)',
      token: 'Bearer token',
      actor: 'Admin actor (X-Xazz-Actor)',
      note: 'Kept in memory only and cleared on reload. In local mode (no XAZZ_*_TOKEN set) xazz-server ignores all three and every request belongs to the default tenant.',
      apply: 'Apply and reload panels',
    },
    dp: {
      title: 'Differential-privacy ledger',
      unit: 'epsilon · delta · tenant-wide',
      scope: 'Live GET /dp/budget · {tenant} · carries across runs',
      scopeEmpty: 'Live GET /dp/budget · nothing shown until the server answers',
      trackLabel: '{spent} of {total} epsilon spent by this tenant.',
      spent: 'spent',
      left: 'left',
      window: 'Window',
      noWindow: 'No rolling window',
      windowInput: 'Window length (seconds)',
      windowSave: 'Save window',
      windowClear: 'Use global window',
      windowNote: '0 keeps a tenant-specific cumulative budget. Use global window removes the override and restores the server default.',
      windowInvalid: 'Enter a non-negative whole number of seconds.',
      windowSaved: 'Window updated from the server response.',
      history: 'Budget reset history',
      historyWhen: 'When',
      historyActor: 'Actor',
      historyBefore: 'Before reset',
      historyEmpty: 'No budget reset recorded for this tenant.',
      resetsAt: 'Resets at',
      manualOnly: 'Only on an explicit reset',
      reserved: 'In-flight reservation',
      reservedNa: 'Not available in this version (#123)',
      reset: 'Reset budget',
      resetDone: 'Reset recorded · spent ε now {spent} (value re-read from the server below)',
      resetFailed: 'Reset failed',
      confirmTitle: 'Reset this tenant’s privacy budget?',
      confirmBody: 'Spent ε and δ for {tenant} return to 0 and queries that were refused become possible again. Use this only when a new accounting period starts.',
    },
    audit: {
      title: 'Audit hash chain',
      scope: 'Live GET /security/audit/log + /chain · the server verdict is authoritative',
      intact: '{n} record(s) · every prev_hash links and every record_hash recomputes on xazz-server.',
      break: {
        link: 'The chain breaks at record #{index}: its prev_hash does not match the record before it (a record was removed, inserted or reordered).',
        hash: 'The chain breaks at record #{index}: its contents changed after it was written (record_hash does not recompute).',
      },
      breakUnlocated: 'xazz-server reports a broken chain; the browser replay could not locate the record.',
      replayUnavailable: 'xazz-server reports a broken chain. The browser replay that names the record needs HTTPS or localhost (Web Crypto) and is unavailable here.',
      replayDisagrees: 'Browser replay disagrees at record #{index}. The server verdict above stands; the IDE and server versions may differ.',
      empty: 'No audit record yet. Every /execute and inference check appends one.',
      when: 'When',
      outcome: 'Outcome',
      inference: 'inference',
      truncated: 'Showing the newest 50 of {n} records.',
      inferenceNote: 'Inference records keep prompt, response and model hashes only — raw text is never stored.',
      lookup: 'Look up a code hash',
      lookupAction: 'Look up',
      lookupFound: '{n} record(s) carry this code hash.',
      lookupNone: 'No audit record carries this code hash.',
      verify: 'Verify chain',
    },
    inference: {
      title: 'Inference output gate',
      scope: 'Check one model response with xazz-server before emitting it.',
      privacyNote: 'The server records only prompt and response hashes, never their raw text. Inputs stay in this tab’s memory and are not written to browser storage.',
      code: '.xzz source code',
      prompt: 'Prompt',
      response: 'Model response',
      model_fingerprint: 'Model fingerprint (optional)',
      check: 'Check response',
      clear: 'Clear inputs and result',
      allowed: 'Safe to emit',
      blocked: 'Blocked',
      auditIndex: 'Audit index',
      chain: 'Audit chain',
      valid: 'Valid',
      invalid: 'Invalid',
      promptHash: 'Prompt hash',
      responseHash: 'Response hash',
      findings: 'Findings · rule, line:column, redacted value',
      noFindings: 'No runtime finding in this response.',
    },
    policy: {
      title: 'Policy packs',
      scope: 'Applies to {tenant} only · other tenants keep their own pack',
      failClosed: 'The policy pack failed to load, so xazz-server denies every execution until it does.',
      active: 'Active pack',
      source: 'Source',
      tenantPack: 'Tenant pack',
      globalPolicy: 'Global policy',
      packJson: 'Policy pack JSON',
      chooseFile: 'Choose .json',
      fromActive: 'Start from active',
      install: 'Install pack',
      remove: 'Remove pack',
      adminNote: 'Admin changes need a bearer admin token plus X-Xazz-Tenant; history records the actor as changed_by.',
      history: 'Change history',
      historyEmpty: 'No change recorded for this tenant.',
      ttl: 'History retention',
      ttlPlaceholder: 'seconds · 0 keeps forever',
      ttlSave: 'Save retention',
      ttlClear: 'Use global default',
      ttlSaved: 'Retention saved for this tenant.',
      ttlCleared: 'Retention reverted to the global default.',
      ttlInvalid: 'Retention must be a whole number of seconds, 0 or more.',
      ttlHistory: 'Retention change history',
      ttlHistoryEmpty: 'No retention change recorded for this tenant.',
      ttlWhen: 'When',
      ttlActor: 'Actor',
      ttlAction: 'Action',
      ttlOld: 'Previous (s)',
      ttlNew: 'New (s)',
      ttlPage: 'Page {n}',
      previous: 'Previous',
      next: 'Next',
      invalidJson: 'Not valid JSON',
      installed: 'Installed {pack} for this tenant.',
      removed: 'Pack removed · this tenant now uses the global policy.',
      nothingRemoved: 'This tenant had no pack to remove.',
      confirmTitle: 'Remove this tenant’s policy pack?',
      confirmBody: 'On the next check or run, {tenant} falls back to the global policy. The removal is recorded in the change history.',
    },
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
    inferring: 'xazz-server에서 스키마를 추론하는 중…',
    detectedServer: 'xazz-server가 {n}개 컬럼 추론 · 100행 표본 · UTF-8 또는 EUC-KR',
    detectedBrowser: '이 브라우저에서 {n}개 컬럼 감지 · 20행 표본 · xazz-server 오프라인',
    tooLarge: '50MB를 넘는 파일은 xazz-server가 거부합니다.',
    serverRejected: 'xazz-server가 파일을 거부했습니다',
    uploadNote: '서버 사본 {path} · 약 1시간 뒤 삭제될 수 있음',
    guardrail: { blocked: '가드레일: 차단됨', passed: '가드레일: 통과' },
  },
  dock: {
    tabs: { preview: '미리보기', delta: '변화량', chart: '차트', logs: '로그', receipt: '실행 기록', history: '히스토리' },
    notRun: '아직 실행하지 않음',
    rows: '행',
    columns: '열',
    emptyTitle: '아직 실제 결과가 없습니다',
    emptyBody: '전체 실행을 누르면 example.xzz 를 xazz-server 에서 실행하고 반환된 행을 보여줍니다.',
  },  errors: {
    panelTitle: '{name} 패널이 멈췄습니다',
    panelBody: '나머지 작업 공간은 그대로 쓸 수 있습니다. 다시 시도하거나 다른 보기로 전환하세요.',
    retry: '다시 시도',
    discardDag: '저장된 DAG 버리기',
    panels: {
      canvas: '캔버스',
      inspector: '인스펙터',
      results: '결과 독',
      workspace: '작업 공간',
      page: '이 화면',
    },
  },
  server: {
    loading: 'xazz-server에서 불러오는 중',
    offlineTitle: 'xazz-server에 연결할 수 없습니다',
    offlineBody: 'xazz-server를 켠 뒤 다시 시도하세요. 서버가 응답하기 전에는 아무 값도 보여주지 않습니다.',
    errorTitle: 'xazz-server가 {status}로 응답했습니다',
    retry: '다시 시도',
  },
  history: {
    listLabel: '기록된 실행, 최신순',
    limit: '최근 50개 실행 · 서버 한도',
    rowsShort: '{n}행',
    pick: '실행을 고르면 그 실행 기록을 다시 엽니다.',
    emptyTitle: '이 테넌트에 기록된 실행이 아직 없습니다',
    emptyBody: '엔진까지 도달한 전체 실행은 여기에 기록되고 서버를 다시 켜도 남습니다.',
    detailLabel: '실행 {id} 기록',
    created: '기록 시각',
    rows: '결과 행',
    verdictNotStored: '저장되지 않음 · 서버는 프로세스 종료 여부만 기록합니다',
    sameCode: '편집기 코드와 같음',
    otherCode: '편집기의 현재 코드와 다름',
    audit: '감사 기록',
    auditRecords: '이 코드 해시를 가진 감사 레코드 {n}개 · 결과: {outcomes}',
    auditUnavailable: '감사 로그를 불러올 수 없음',
    error: '오류',
    restore: '미리보기에 결과 복원',
    metadataOnly: 'xazz-server는 실행 메타데이터만 보관합니다. 행 데이터는 /execute 응답에만 담기며, 이 브라우저 세션에는 이 실행의 응답이 남아 있지 않습니다.',
    notFound: '이 테넌트에서 실행 {id}을(를) 찾지 못했습니다',
    notFoundBody: '다른 테넌트의 실행은 삭제된 실행과 똑같이 404로 응답합니다. 의도된 설계입니다.',
  },
  progress: {
    eyebrow: '프로세스 실행 중 · xazz-server',
    title: 'xazz-exec가 결과를 돌려주기를 기다리는 중',
    elapsed: '경과 {s}초 · 이 브라우저에서 측정',
    steps: {
      sent: 'xazz-server로 전송됨',
      server: 'xazz-server에서 실행 중',
      result: '구조화된 결과',
    },
    epochs: '에폭 진행률 · 이 버전에서는 제공되지 않음 — xazz-server는 프로세스가 끝난 뒤에만 응답합니다.',
    viewLogs: '로그 보기',
    stop: '기다리기 중단',
    stopNote: '중단하면 이 요청이 끊깁니다. xazz-server에는 취소 API가 없어 실행은 계속되지만, 끊긴 요청은 현재 히스토리·감사 체인·ε 원장에 기록되지 않습니다.',
    stoppedTitle: 'xazz-server 응답 기다리기를 중단했습니다',
    timedOutTitle: '5분이 지나 기다리기를 중단했습니다',
    stoppedBody: 'xazz-server는 코드를 계속 실행할 수 있지만, 끊긴 요청은 현재 히스토리와 감사 체인에 기록되지 않습니다. 기록이 남는 결과가 필요하면 다시 실행하세요.',
    runAgain: '다시 실행',
    waitingRows: 'xazz-server에서 행을 기다리는 중',
  },
  lineage: {
    title: '컬럼 계보',
    trace: '컬럼 추적',
    retrace: '다시 추적',
    stale: '마지막 추적 이후 코드가 바뀌었습니다.',
    hint: 'POST /catalog 정적 컴파일 · 아무것도 실행하지 않습니다.',
    table: '표',
    diagram: '다이어그램',
    pipeline: '파이프라인',
    output: '출력',
    from: '출처',
    step: '단계',
    source: '원본',
    derived: '파생 · 원본 컬럼 없음',
    empty: '이 코드에는 파이프라인이 없습니다.',
    select: '출력 컬럼을 고르면 원본 컬럼이 강조됩니다.',
  },
  charts: {
    modes: '차트 종류',
    mode: { mean: '그룹별 평균', distribution: '분포', box: '박스플롯', training: '학습 곡선' },
    column: '숫자 컬럼',
    groupBy: '그룹 기준',
    allRows: '전체 행',
    rows: '행',
    table: '표로 보기',
    fromRun: '마지막 전체 실행이 반환한 행에서 계산했습니다',
    meanTitle: '{x}별 {y} 평균',
    distributionTitle: '{column} 분포',
    distributionLabel: '{column} 히스토그램: 값 {n}개, 같은 폭 구간 {bins}개.',
    sample: '값 {n}개 · 숫자가 없는 {missing}행 제외 · Sturges 구간',
    boxTitle: '{group}별 {column}',
    boxTitleAll: '{column} · 전체 행',
    boxNote: '상자 Q1–Q3 · 선 중앙값 · 수염 1.5×IQR · 점은 그 밖',
    groupsShown: '그룹 {total}개 중 큰 {shown}개 · 상자 Q1–Q3 · 수염 1.5×IQR',
    noNumeric: '반환된 행에 차트로 그릴 숫자 컬럼이 없습니다.',
    noValues: '반환된 행의 {column}에 숫자 값이 없어 그릴 것이 없습니다.',
    noGroupColumn: '그룹별 평균에는 묶을 텍스트 컬럼이 필요한데, 반환된 행에는 없습니다.',
    negativeMeans: '음수인 그룹 평균이 있어 0에서 시작하는 막대로는 보일 수 없습니다. 표를 보세요.',
    barsShown: '그룹 {total}개 중 처음 {shown}개 · 나머지는 표에서 확인',
    trainingNa: '이 버전에서는 제공되지 않습니다. 손실 곡선에는 에폭별 이력이 필요한데 TrainReport에 아직 없습니다(#117, #129). ROC·PR 곡선과 혼동행렬에는 분류 지표가 필요합니다(#163). 최종 학습·검증 손실은 모니터에서 볼 수 있습니다.',
  },
  mobile: {
    title: '컴파일러 캔버스는 데스크톱 도구입니다.',
    body: '모바일에서는 랜딩 화면을 지원합니다. 그래프, 코드, 근거를 함께 보려면 1024px 이상 화면에서 작업 공간을 여세요.',
    back: '랜딩으로 돌아가기',
  },
  gov: {
    heading: '거버넌스 · xazz-server 실시간',
    subheading: '이 테넌트에 허용된 것과 서버가 기록한 것입니다. 마지막 전체 실행과 무관합니다.',
    defaultTenant: '기본 테넌트',
    namedTenant: '테넌트 “{tenant}”',
    cancel: '취소',
    refresh: '새로고침',
    access: {
      title: '서버 접근',
      tenant: '테넌트 (X-Xazz-Tenant)',
      token: 'Bearer 토큰',
      actor: '관리자 대리인 (X-Xazz-Actor)',
      note: '메모리에만 보관하며 새로고침하면 지워집니다. 로컬 모드(XAZZ_*_TOKEN 미설정)에서는 xazz-server가 세 값을 모두 무시하고 모든 요청을 기본 테넌트로 처리합니다.',
      apply: '적용하고 패널 다시 불러오기',
    },
    dp: {
      title: '차등 프라이버시 원장',
      unit: 'epsilon · delta · 테넌트 전체',
      scope: 'GET /dp/budget 실시간 · {tenant} · 실행 간 누적',
      scopeEmpty: 'GET /dp/budget 실시간 · 서버가 응답하기 전에는 표시하지 않음',
      trackLabel: '이 테넌트가 {total} epsilon 중 {spent}를 사용했습니다.',
      spent: '사용',
      left: '남음',
      window: '윈도',
      noWindow: '롤링 윈도 없음',
      windowInput: '윈도 길이 (초)',
      windowSave: '윈도 저장',
      windowClear: '전역 윈도 사용',
      windowNote: '0은 이 테넌트의 누적 예산 설정입니다. 전역 윈도 사용은 재정의를 지우고 서버 기본값으로 돌아갑니다.',
      windowInvalid: '0 이상의 정수(초)를 입력하세요.',
      windowSaved: '서버 응답에 따라 윈도를 갱신했습니다.',
      history: '예산 초기화 이력',
      historyWhen: '시각',
      historyActor: '실행자',
      historyBefore: '초기화 전',
      historyEmpty: '이 테넌트의 예산 초기화 기록이 없습니다.',
      resetsAt: '초기화 시각',
      manualOnly: '명시적 초기화 때만',
      reserved: '진행 중 예약분',
      reservedNa: '이 버전에서는 제공되지 않음 (#123)',
      reset: '예산 초기화',
      resetDone: '초기화 기록됨 · 사용 ε {spent} (아래 값은 서버에서 다시 읽은 값)',
      resetFailed: '초기화 실패',
      confirmTitle: '이 테넌트의 프라이버시 예산을 초기화할까요?',
      confirmBody: '{tenant}의 사용 ε와 δ가 0으로 돌아가고, 거부되던 쿼리가 다시 가능해집니다. 새 회계 기간이 시작될 때만 사용하세요.',
    },
    audit: {
      title: '감사 해시 체인',
      scope: 'GET /security/audit/log + /chain 실시간 · 서버 판정이 기준',
      intact: '레코드 {n}개 · xazz-server에서 모든 prev_hash가 이어지고 모든 record_hash가 재계산됩니다.',
      break: {
        link: '레코드 #{index}에서 체인이 끊깁니다: prev_hash가 앞 레코드와 맞지 않습니다 (레코드 삭제·삽입·순서 변경).',
        hash: '레코드 #{index}에서 체인이 끊깁니다: 기록된 뒤 내용이 바뀌었습니다 (record_hash가 재계산되지 않음).',
      },
      breakUnlocated: 'xazz-server는 체인이 깨졌다고 판정했지만, 브라우저 재검증으로는 위치를 찾지 못했습니다.',
      replayUnavailable: 'xazz-server는 체인이 깨졌다고 판정했습니다. 레코드 위치를 찾는 브라우저 재검증은 HTTPS나 localhost(Web Crypto)에서만 동작해 여기서는 쓸 수 없습니다.',
      replayDisagrees: '브라우저 재검증 결과가 레코드 #{index}에서 다릅니다. 위 서버 판정이 기준이며, IDE와 서버 버전이 다를 수 있습니다.',
      empty: '아직 감사 레코드가 없습니다. /execute와 추론 검사가 실행될 때마다 하나씩 추가됩니다.',
      when: '시각',
      outcome: '결과',
      inference: '추론',
      truncated: '레코드 {n}개 중 최신 50개를 표시합니다.',
      inferenceNote: '추론 레코드는 프롬프트·응답·모델의 해시만 보관합니다. 원문은 저장하지 않습니다.',
      lookup: '코드 해시 조회',
      lookupAction: '조회',
      lookupFound: '이 코드 해시를 가진 레코드 {n}개.',
      lookupNone: '이 코드 해시를 가진 감사 레코드가 없습니다.',
      verify: '체인 검증',
    },
    inference: {
      title: '추론 출력 게이트',
      scope: '모델 응답을 내보내기 전에 xazz-server에서 한 번 점검합니다.',
      privacyNote: '서버는 프롬프트와 응답의 해시만 기록하고 원문은 저장하지 않습니다. 입력은 현재 탭의 메모리에만 있으며 브라우저 저장소에 기록하지 않습니다.',
      code: '.xzz 소스 코드',
      prompt: '프롬프트',
      response: '모델 응답',
      model_fingerprint: '모델 지문 (선택)',
      check: '응답 점검',
      clear: '입력과 결과 지우기',
      allowed: '출력 가능',
      blocked: '차단',
      auditIndex: '감사 인덱스',
      chain: '감사 체인',
      valid: '유효',
      invalid: '무효',
      promptHash: '프롬프트 해시',
      responseHash: '응답 해시',
      findings: '탐지 결과 · 규칙, 행:열, 마스킹 값',
      noFindings: '이 응답에서 탐지된 항목이 없습니다.',
    },
    policy: {
      title: '정책 팩',
      scope: '{tenant}에만 적용 · 다른 테넌트는 각자의 팩을 유지',
      failClosed: '정책 팩을 불러오지 못해, 불러올 때까지 xazz-server가 모든 실행을 거부합니다.',
      active: '활성 팩',
      source: '출처',
      tenantPack: '테넌트 팩',
      globalPolicy: '전역 정책',
      packJson: '정책 팩 JSON',
      chooseFile: '.json 선택',
      fromActive: '활성 정책에서 시작',
      install: '팩 설치',
      remove: '팩 제거',
      adminNote: '관리자 변경에는 관리자 Bearer 토큰과 X-Xazz-Tenant가 필요하며, 이력에는 대리인이 changed_by로 남습니다.',
      history: '변경 이력',
      historyEmpty: '이 테넌트에 기록된 변경이 없습니다.',
      ttl: '이력 보존 기간',
      ttlPlaceholder: '초 · 0이면 영구 보존',
      ttlSave: '보존 기간 저장',
      ttlClear: '전역 기본값 사용',
      ttlSaved: '이 테넌트의 보존 기간을 저장했습니다.',
      ttlCleared: '보존 기간을 전역 기본값으로 되돌렸습니다.',
      ttlInvalid: '보존 기간은 0 이상의 정수(초)여야 합니다.',
      ttlHistory: '보존 기간 변경 이력',
      ttlHistoryEmpty: '이 테넌트의 보존 기간 변경 기록이 없습니다.',
      ttlWhen: '시각',
      ttlActor: '실행자',
      ttlAction: '동작',
      ttlOld: '이전 값 (초)',
      ttlNew: '새 값 (초)',
      ttlPage: '{n}페이지',
      previous: '이전',
      next: '다음',
      invalidJson: '올바른 JSON이 아닙니다',
      installed: '이 테넌트에 {pack}을(를) 설치했습니다.',
      removed: '팩을 제거했습니다 · 이 테넌트는 이제 전역 정책을 씁니다.',
      nothingRemoved: '이 테넌트에는 제거할 팩이 없었습니다.',
      confirmTitle: '이 테넌트의 정책 팩을 제거할까요?',
      confirmBody: '다음 검사나 실행부터 {tenant}은(는) 전역 정책을 따릅니다. 제거 사실은 변경 이력에 남습니다.',
    },
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

/**
 * DAG editor copy (#115). English lives in src/dag/dagTools.js; only the Korean
 * rendering lives here, keyed by `<tool id>.<field key>` because the same key means
 * different things per tool (a `column` to sort vs a `column` to group by).
 * Tool names stay English — they are the .xzz operation names.
 */
const dagFieldsKo = {
  'fileInput.filePath': '파일 경로',
  'select.columns': '남길 컬럼 (쉼표 구분)',
  'filter.column': '컬럼',
  'filter.operator': '연산자',
  'filter.value': '값',
  'fillNull.column': '컬럼',
  'fillNull.value': '채울 값',
  'dropNull.column': '컬럼',
  'sort.column': '정렬 컬럼',
  'sort.descending': '내림차순',
  'take.n': '행 수',
  'groupBy.column': '그룹 컬럼',
  'groupBy.agg': '집계',
  'groupBy.aggColumn': ['집계 대상 컬럼', 'pm25 (count면 비워도 됨)'],
  'chart.chartType': '차트 유형',
  'chart.x': 'X 축',
  'chart.y': 'Y 축',
  'chart.title': ['제목', '차트 제목'],
  'model.name': '모델 이름',
  'model.layers': '레이어 (Burn)',
  'train.modelName': '모델 이름',
  'train.modelVar': '모델 변수',
  'train.target': '목표 컬럼',
  'train.epochs': '에폭',
  'train.lr': '학습률',
  'predict.modelVar': '모델 변수',
  'predict.as': '예측 컬럼명',
  'guardrail.policy': '정책',
  'guardrail.action': '위반 동작',
  'dp.mechanism': '메커니즘',
  'dp.epsilon': 'Privacy Budget (ε)',
  'dp.sensitivity': '민감도 (Δf)',
}

const dagToolsKo = {
  fileInput: 'CSV 데이터 로드 (스키마 추론)',
  select: '컬럼 선택',
  filter: '조건부 필터',
  fillNull: '결측치 채우기',
  dropNull: 'null 행 제거',
  sort: '정렬 (orderBy)',
  take: '상위 N 행',
  groupBy: '그룹 집계',
  count: '행 수',
  chart: '시각화',
  model: '모델 선언 (model {})',
  train: 'Burn 학습',
  predict: '모델 예측',
  guardrail: '정적 가드레일 · 개인정보 차단',
  dp: '차등 프라이버시 노이즈 (Laplace/Gaussian)',
}

export function localizeField(toolId, field, language) {
  if (language !== 'ko') return field
  const override = dagFieldsKo[`${toolId}.${field.key}`]
  if (override === undefined) return field
  const [label, placeholder] = Array.isArray(override) ? override : [override]
  return { ...field, label, placeholder: placeholder ?? field.placeholder }
}

export function localizeToolDescription(tool, language) {
  return language === 'ko' ? (dagToolsKo[tool.id] ?? tool.description) : tool.description
}

// Exported for the key-parity check in tests/contract.mjs.
export const DICTIONARIES = { en, ko }

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
