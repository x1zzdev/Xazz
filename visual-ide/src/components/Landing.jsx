import React, { useState } from 'react'
import {
  ArrowRight,
  Braces,
  Check,
  ChevronRight,
  CircleAlert,
  Code2,
  Database,
  FileInput,
  FolderOpen,
  Gauge,
  Hash,
  Layers3,
  LockKeyhole,
  Play,
  ScanSearch,
  ShieldCheck,
  Table2,
  Workflow,
} from 'lucide-react'
import { Brand, GitHubLink, StatusBadge } from './Common'
import { resultRows, scenario } from '../data'

const proofNodes = [
  { label: 'CSV', meta: '100 rows', tone: 'default' },
  { label: 'Schema', meta: '2 checks', tone: 'warning' },
  { label: 'Fill null', meta: `${scenario.sourceNulls} → 0`, tone: 'selected' },
  { label: 'Filter', meta: `→ ${scenario.resultCount}`, tone: 'default' },
  { label: 'Result', meta: `${scenario.resultCount} rows`, tone: 'success' },
]

function LandingPipeline() {
  return (
    <div className="landing-pipeline" aria-label="Sample pipeline proof">
      <div className="landing-pipeline__toolbar">
        <span className="window-dots" aria-hidden="true">
          <i />
          <i />
          <i />
        </span>
        <span>air-quality.xzz</span>
        <StatusBadge axis="Maturity" tone="success" compact>
          Available
        </StatusBadge>
      </div>
      <div className="landing-pipeline__canvas">
        <div className="pipeline-rail" aria-hidden="true" />
        {proofNodes.map((node, index) => (
          <div
            className={`proof-node proof-node--${node.tone}`}
            key={node.label}
            style={{ '--node-index': index }}
          >
            <span className="proof-node__stage">0{index + 1}</span>
            <strong>{node.label}</strong>
            <span>{node.meta}</span>
          </div>
        ))}
      </div>
      <div className="landing-pipeline__evidence">
        <span>
          <Check size={15} aria-hidden="true" />
          Live Check demo · Future contract · 100-row synthetic fixture
        </span>
        <span>
          <ShieldCheck size={15} aria-hidden="true" />
          Side-effect free
        </span>
      </div>
    </div>
  )
}

function ResultProof() {
  const rows = resultRows
    .slice(0, 3)
    .map(({ observed_at, district, pm25, temperature_c }) => [
      observed_at,
      district,
      pm25.toFixed(1),
      temperature_c.toFixed(1),
    ])
  return (
    <div className="result-proof">
      <div className="result-proof__head">
        <div>
          <span className="eyebrow">Preview result</span>
          <strong>{scenario.resultCount} valid rows</strong>
        </div>
        <StatusBadge axis="Pipeline" tone="info" compact>
          Sample only
        </StatusBadge>
      </div>
      <div className="mini-table" role="table" aria-label="Synthetic result preview">
        <div className="mini-table__row mini-table__row--head" role="row">
          {['observed_at', 'district', 'pm25', 'temperature_c'].map((cell) => (
            <span role="columnheader" key={cell}>
              {cell}
            </span>
          ))}
        </div>
        {rows.map((row) => (
          <div className="mini-table__row" role="row" key={row.join('-')}>
            {row.map((cell) => (
              <span role="cell" key={cell}>
                {cell}
              </span>
            ))}
          </div>
        ))}
      </div>
      <div className="result-proof__delta">
        <span>
          <strong>−{scenario.sourceNulls}</strong> null values
        </span>
        <span>
          <strong>−{scenario.removedCount}</strong> out-of-range rows
        </span>
        <span>
          <strong>0</strong> hidden side effects
        </span>
      </div>
    </div>
  )
}

export function Landing({ onOpenSample }) {
  return (
    <div className="landing-page">
      <a className="skip-link" href="#main-content">
        Skip to main content
      </a>
      <header className="landing-nav">
        <div className="landing-container landing-nav__inner">
          <Brand />
          <nav aria-label="Primary navigation">
            <a href="#how-it-works">How it works</a>
            <a href="#capabilities">What is real</a>
            <GitHubLink />
            <button className="button button--primary button--small" type="button" onClick={onOpenSample}>
              Open sample
              <ArrowRight size={16} aria-hidden="true" />
            </button>
          </nav>
        </div>
      </header>

      <main id="main-content">
        <section className="landing-hero">
          <div className="landing-container landing-hero__grid">
            <div className="landing-hero__copy">
              <div className="hero-kicker">
                <span className="hero-kicker__rail" aria-hidden="true" />
                Typed pipeline workbench
                <StatusBadge axis="Maturity" tone="success" compact>
                  Available
                </StatusBadge>
              </div>
              <h1>Catch data errors before training starts.</h1>
              <p className="hero-lede">
                Build one typed <code>.xzz</code> pipeline, inspect every transformation,
                and run it locally with Polars—without hiding what the runtime can and
                cannot prove.
              </p>
              <div className="hero-actions">
                <button className="button button--primary button--large" type="button" onClick={onOpenSample}>
                  <Play size={18} fill="currentColor" aria-hidden="true" />
                  Open a sample pipeline
                </button>
                <a className="button button--secondary button--large" href="#how-it-works">
                  Read the .xzz guide
                  <ChevronRight size={18} aria-hidden="true" />
                </a>
              </div>
              <p className="hero-assurance">
                Synthetic data · no account · no install · local prototype
              </p>
            </div>
            <div className="landing-hero__proof">
              <LandingPipeline />
              <div className="proof-caption">
                <span>Core + demo preview</span>
                <strong>Data → type review → synthetic result preview</strong>
              </div>
            </div>
          </div>
        </section>

        <section className="proof-section" aria-labelledby="proof-title">
          <div className="landing-container proof-section__grid">
            <div className="proof-section__copy">
              <span className="section-index">01 · First proof</span>
              <h2 id="proof-title">See the result before learning the tool.</h2>
              <p>
                The sample opens with data, graph, and code already connected. Select a
                step to see which rows, nulls, and types changed—then decide whether a
                full run is justified.
              </p>
              <ul className="evidence-list">
                <li>
                  <ScanSearch aria-hidden="true" />
                  Live Check is a 100-row synthetic demo with a Future backend contract,
                  visibly separate from Full Run.
                </li>
                <li>
                  <Workflow aria-hidden="true" />
                  Graph selection traces the matching .xzz statement and lineage.
                </li>
                <li>
                  <Table2 aria-hidden="true" />
                  Result claims carry row counts, units, and sample scope.
                </li>
              </ul>
            </div>
            <ResultProof />
          </div>
        </section>

        <section className="method-section" id="how-it-works" aria-labelledby="method-title">
          <div className="landing-container">
            <div className="section-heading">
              <span className="section-index">02 · Compiler Canvas</span>
              <h2 id="method-title">One flow, three kinds of confidence.</h2>
              <p>
                Xazz does not treat a green process exit as proof that every operation and
                artifact succeeded.
              </p>
            </div>
            <ol className="method-steps">
              <li>
                <span className="method-steps__index">01</span>
                <Braces aria-hidden="true" />
                <h3>Inspect the contract</h3>
                <p>Review inferred fields, nullability, and pending source changes.</p>
              </li>
              <li>
                <span className="method-steps__index">02</span>
                <Gauge aria-hidden="true" />
                <h3>Measure the impact</h3>
                <p>Trace row, null, and type deltas at the operation that caused them.</p>
              </li>
              <li>
                <span className="method-steps__index">03</span>
                <Hash aria-hidden="true" />
                <h3>Keep the evidence</h3>
                <p>Separate process, pipeline, artifact, and code-hash observations.</p>
              </li>
            </ol>
          </div>
        </section>

        <section className="truth-section" id="capabilities" aria-labelledby="truth-title">
          <div className="landing-container">
            <div className="truth-section__intro">
              <span className="section-index">03 · Capability truth</span>
              <h2 id="truth-title">Know what is usable now—and what is still research.</h2>
              <p>
                Maturity is part of the interface, not a footnote. The current Core and
                future trust controls never share the same availability label.
              </p>
            </div>
            <div className="capability-map">
              <div className="capability-map__column">
                <div className="capability-map__heading">
                  <StatusBadge axis="Maturity" tone="success">
                    Available
                  </StatusBadge>
                  <span>Current Core</span>
                </div>
                <ul>
                  <li>
                    <Database aria-hidden="true" />
                    Polars-backed local data transforms
                  </li>
                  <li>
                    <Code2 aria-hidden="true" />
                    Parser, AST, and .xzz execution path
                  </li>
                  <li>
                    <Hash aria-hidden="true" />
                    SHA-256 code hash compute and verify
                  </li>
                </ul>
              </div>
              <div className="capability-map__divider" aria-hidden="true">
                <span>clear boundary</span>
              </div>
              <div className="capability-map__column capability-map__column--future">
                <div className="capability-map__heading">
                  <strong>Future Labs</strong>
                  <span>Per-capability maturity</span>
                </div>
                <ul>
                  <li>
                    <LockKeyhole aria-hidden="true" />
                    <span>Policy-as-Code and durable audit ledger</span>
                    <StatusBadge axis="Maturity" tone="future" compact>
                      Research
                    </StatusBadge>
                  </li>
                  <li>
                    <Layers3 aria-hidden="true" />
                    <span>Burn model execution and training</span>
                    <StatusBadge axis="Maturity" tone="neutral" compact>
                      Planned
                    </StatusBadge>
                  </li>
                  <li>
                    <ShieldCheck aria-hidden="true" />
                    <span>Differential-privacy budget</span>
                    <StatusBadge axis="Maturity" tone="future" compact>
                      Research
                    </StatusBadge>
                  </li>
                  <li>
                    <ShieldCheck aria-hidden="true" />
                    <span>Sandboxing and on-device sLM correction</span>
                    <StatusBadge axis="Maturity" tone="future" compact>
                      Research
                    </StatusBadge>
                  </li>
                  <li>
                    <Gauge aria-hidden="true" />
                    <span>Run and resource monitoring</span>
                    <StatusBadge axis="Maturity" tone="neutral" compact>
                      Planned
                    </StatusBadge>
                  </li>
                </ul>
              </div>
            </div>
            <div className="truth-note">
              <CircleAlert aria-hidden="true" />
              <p>
                Current integrity endpoints compute or compare a code hash. They do not
                persist a durable audit record, so the product says <strong>Computed</strong>,
                not <strong>Audited</strong>.
              </p>
            </div>
          </div>
        </section>

        <section className="final-cta">
          <div className="landing-container final-cta__inner">
            <div>
              <span className="section-index">Start with evidence</span>
              <h2>Open the pipeline. Find the change. Decide to run.</h2>
            </div>
            <button className="button button--primary button--large" type="button" onClick={onOpenSample}>
              Open the air-quality sample
              <ArrowRight size={18} aria-hidden="true" />
            </button>
          </div>
        </section>
      </main>
      <footer className="landing-footer">
        <div className="landing-container">
          <Brand />
          <p>Open-source product UX prototype · synthetic data only</p>
          <GitHubLink />
        </div>
      </footer>
    </div>
  )
}

const projectStartCopy = {
  en: {
    skip: 'Skip to project choices',
    mode: 'Local prototype',
    eyebrow: 'Start a pipeline',
    title: 'Begin with something you can inspect.',
    lede:
      'Xazz opens the evidence before asking you to configure a runtime. The sample is synthetic and never leaves this browser.',
    aria: 'Project start choices',
    dismiss: 'Dismiss',
    noticeImport:
      'CSV import review is represented in the design system, but file access is disabled in this synthetic prototype.',
    noticeProject: 'Local project access is disabled in this synthetic prototype.',
    asideTitle: 'What opens',
    asideItems: [
      ['Connected graph + code', 'Five operations mapped to exact .xzz lines'],
      [
        'Live Check demo',
        'Future contract · 100-row synthetic fixture · no backend call',
      ],
      ['Explicit Full Run', 'Runtime readiness and output path before execution'],
    ],
    data: 'Data',
    dataset: 'Synthetic Seoul air quality',
    options: [
      {
        id: 'sample',
        icon: Play,
        title: 'Run the air-quality sample',
        description: 'Open a ready pipeline with 100 deterministic synthetic rows.',
        meta: ['Recommended', 'No install', 'About 60 seconds'],
      },
      {
        id: 'import',
        icon: FileInput,
        title: 'Import a CSV',
        description: 'Review 100-row inference, encoding, and the pending .xzz change.',
        meta: ['Prototype unavailable', 'Review before write'],
      },
      {
        id: 'project',
        icon: FolderOpen,
        title: 'Open an existing .xzz project',
        description: 'Choose a local project and inspect its execution dependencies.',
        meta: ['Prototype unavailable', 'Local path'],
      },
    ],
  },
  ko: {
    skip: '프로젝트 선택으로 건너뛰기',
    mode: '로컬 프로토타입',
    eyebrow: '파이프라인 시작',
    title: '직접 확인할 수 있는 것부터 시작하세요.',
    lede:
      'Xazz는 런타임 설정을 요구하기 전에 근거부터 보여줍니다. 이 샘플은 합성 데이터이며 브라우저 밖으로 나가지 않습니다.',
    aria: '프로젝트 시작 선택',
    dismiss: '닫기',
    noticeImport:
      'CSV 가져오기 검토 흐름은 디자인 시스템에 포함되어 있지만, 이 합성 프로토타입에서는 파일 접근을 사용하지 않습니다.',
    noticeProject: '이 합성 프로토타입에서는 로컬 프로젝트 접근을 사용하지 않습니다.',
    asideTitle: '열리는 화면',
    asideItems: [
      ['연결된 그래프와 코드', '5개 연산을 정확한 .xzz 코드 줄과 연결'],
      [
        'Live Check 데모',
        '향후 계약 · 합성 데이터 100행 · 백엔드 호출 없음',
      ],
      ['명시적인 Full Run', '실행 전 런타임 준비 상태와 출력 경로 확인'],
    ],
    data: '데이터',
    dataset: '서울 대기질 합성 데이터',
    options: [
      {
        id: 'sample',
        icon: Play,
        title: '대기질 샘플 실행',
        description: '결정론적으로 생성된 합성 데이터 100행으로 준비된 파이프라인을 엽니다.',
        meta: ['추천', '설치 불필요', '약 60초'],
      },
      {
        id: 'import',
        icon: FileInput,
        title: 'CSV 가져오기',
        description: '100행 타입 추론, 인코딩, 예정된 .xzz 변경을 실행 전에 검토합니다.',
        meta: ['프로토타입 미지원', '쓰기 전 검토'],
      },
      {
        id: 'project',
        icon: FolderOpen,
        title: '기존 .xzz 프로젝트 열기',
        description: '로컬 프로젝트를 선택하고 실행 의존성을 확인합니다.',
        meta: ['프로토타입 미지원', '로컬 경로'],
      },
    ],
  },
}

export function ProjectStart({
  onBack,
  onOpenWorkspace,
  language = 'en',
  onLanguageChange,
}) {
  const [notice, setNotice] = useState(null)
  const copy = projectStartCopy[language]

  const selectOption = (id) => {
    if (id === 'sample') {
      onOpenWorkspace()
      return
    }
    setNotice(id === 'import' ? copy.noticeImport : copy.noticeProject)
  }

  return (
    <div className="start-page">
      <a className="skip-link" href="#start-main">
        {copy.skip}
      </a>
      <header className="start-nav">
        <Brand onHome={onBack} />
        <div>
          <div className="locale-switch" aria-label="Language validation">
            <button
              className={language === 'en' ? 'is-active' : ''}
              type="button"
              onClick={() => onLanguageChange('en')}
              aria-pressed={language === 'en'}
            >
              EN
            </button>
            <button
              className={language === 'ko' ? 'is-active' : ''}
              type="button"
              onClick={() => onLanguageChange('ko')}
              aria-pressed={language === 'ko'}
            >
              한국어
            </button>
          </div>
          <StatusBadge axis="Mode" tone="info">
            {copy.mode}
          </StatusBadge>
          <GitHubLink />
        </div>
      </header>
      <main id="start-main" className="start-main">
        <section className="start-intro">
          <span className="section-index">{copy.eyebrow}</span>
          <h1>{copy.title}</h1>
          <p>{copy.lede}</p>
        </section>
        <section className="start-grid" aria-label={copy.aria}>
          <div className="start-options">
            {copy.options.map(({ id, icon: Icon, title, description, meta }) => (
              <button
                className={`start-option ${id === 'sample' ? 'start-option--recommended' : ''}`}
                type="button"
                key={id}
                onClick={() => selectOption(id)}
              >
                <span className="start-option__icon">
                  <Icon aria-hidden="true" />
                </span>
                <span className="start-option__body">
                  <strong>{title}</strong>
                  <span>{description}</span>
                  <span className="start-option__meta">
                    {meta.map((item) => (
                      <i key={item}>{item}</i>
                    ))}
                  </span>
                </span>
                <ChevronRight className="start-option__arrow" aria-hidden="true" />
              </button>
            ))}
            {notice && (
              <div className="inline-notice" role="status">
                <CircleAlert aria-hidden="true" />
                <p>{notice}</p>
                <button type="button" onClick={() => setNotice(null)}>
                  {copy.dismiss}
                </button>
              </div>
            )}
          </div>
          <aside className="start-context" aria-labelledby="opens-title">
            <span className="start-context__index">01 / 03</span>
            <h2 id="opens-title">{copy.asideTitle}</h2>
            <ol>
              {copy.asideItems.map(([title, description], index) => {
                const Icon = [Workflow, ScanSearch, ShieldCheck][index]
                return (
                  <li key={title}>
                    <Icon aria-hidden="true" />
                    <span>
                      <strong>{title}</strong>
                      {description}
                    </span>
                  </li>
                )
              })}
            </ol>
            <div className="start-context__foot">
              <span>{copy.data}</span>
              <strong>{copy.dataset}</strong>
            </div>
          </aside>
        </section>
      </main>
    </div>
  )
}
