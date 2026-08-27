import React from 'react'
import {
  AlertTriangle,
  Boxes,
  Cpu,
  FlaskConical,
  ShieldAlert,
  ShieldCheck,
  ShieldQuestion,
} from 'lucide-react'
import { StatusBadge } from './Common'
import executeResponse from '../mock/execute-response.json'
import proposedTelemetry from '../mock/telemetry-proposed.json'

const fallbackFixture = executeResponse
const resources = proposedTelemetry.resource_efficiency

const NOT_AVAILABLE = 'Not available in this version'
const PROPOSED_SCOPE = 'Synthetic structure · not measured · proposed contract'

/**
 * Panel shell. `contract` is the whole point of this screen: an implemented panel and a
 * proposed panel must never be able to look alike, so the status drives surface, rule,
 * text colour, and plot hatching together — not colour alone.
 */
function MonitorPanel({ contract, icon: Icon, title, unit, maturity, scope, children }) {
  return (
    <section className={`monitor-panel monitor-panel--${contract}`} aria-label={title}>
      <header className="monitor-panel__head">
        <div className="monitor-panel__title">
          <Icon size={15} aria-hidden="true" />
          <div>
            <strong>{title}</strong>
            <span>{unit}</span>
          </div>
        </div>
        <StatusBadge axis="Maturity" tone={contract === 'implemented' ? 'info' : 'future'} compact>
          {maturity}
        </StatusBadge>
      </header>
      <p className="monitor-panel__scope">{scope}</p>
      {children}
    </section>
  )
}

function BurnPanel({ runState, training, model }) {
  const evaluatedInitial = runState === 'success'
  const failedInitial = runState === 'error'
  const pendingInitial = runState === 'running'

  const hasTraining = Boolean(training?.report)
  const report = hasTraining ? training.report : fallbackFixture.training.report
  const modelInfo = model ?? fallbackFixture.model
  const absent = fallbackFixture._absent_from_contract
  const isMeasured = hasTraining

  const evaluated = isMeasured && evaluatedInitial
  const failed = failedInitial && !hasTraining
  const pending = pendingInitial && !hasTraining

  const outcome = (value) =>
    evaluated
      ? value
      : failed
        ? 'Not available in failed run'
        : pending
          ? 'Unknown while Full Run is pending'
          : 'Not evaluated'

  const losses = [
    ['Final training loss', report.final_train_loss, 'train'],
    ...(Number.isFinite(report.final_val_loss)
      ? [['Final validation loss', report.final_val_loss, 'validation']]
      : []),
  ]
  const maxLoss = Math.max(...losses.map(([, value]) => value))

  return (
    <MonitorPanel
      contract={isMeasured ? 'measured' : 'implemented'}
      icon={Boxes}
      title="Burn compile and training"
      unit="mean squared error · parameters · rows"
      maturity={isMeasured ? 'Real' : 'Beta'}
      scope={
        isMeasured
          ? `Measured from a real Full Run · model ${report.model_name}`
          : 'Synthetic fixture · contract: implemented · fields mirror TrainReport'
      }
    >
      <dl className="monitor-facts">
        <div>
          <dt>Model</dt>
          <dd>{report.model_name}</dd>
        </div>
        <div>
          <dt>Target column</dt>
          <dd>{report.target}</dd>
        </div>
        <div>
          <dt>Shape</dt>
          <dd>
            in {report.input_dim} → out {report.output_dim}
          </dd>
        </div>
        <div>
          <dt>Parameters</dt>
          <dd>{report.num_params.toLocaleString('en-US')}</dd>
        </div>
        <div>
          <dt>Epochs · batch · lr</dt>
          <dd>
            {report.epochs} · {report.batch_size} · {report.learning_rate}
          </dd>
        </div>
        <div>
          <dt>Checkpoint</dt>
          <dd>{outcome(report.checkpoint_path)}</dd>
        </div>
      </dl>

      <div className="monitor-chart">
        <div className="monitor-chart__heading">
          <strong>Final loss comparison</strong>
          <span>two reported points · no interpolation</span>
        </div>
        {evaluated ? (
          <div
            className="monitor-bars"
            role="img"
            aria-label={`Final training loss ${report.final_train_loss}${
              Number.isFinite(report.final_val_loss)
                ? ` and final validation loss ${report.final_val_loss}`
                : '; no validation split configured'
            }, mean squared error. Reported points; no per-epoch history exists.`}
          >
            {losses.map(([label, value, series]) => (
              <div className="monitor-bars__row" key={series}>
                <span>{label}</span>
                <i
                  className={`monitor-bars__fill monitor-bars__fill--${series}`}
                  style={{ '--bar-width': `${(value / maxLoss) * 100}%` }}
                />
                <strong>{value.toFixed(4)}</strong>
              </div>
            ))}
          </div>
        ) : (
          <p className="monitor-empty">
            {failed
              ? 'The run failed before a training report was emitted.'
              : pending
                ? 'Training is in progress. No epoch event reaches stdout, so no progress value can be shown.'
                : 'No Full Run has produced a training report yet.'}
          </p>
        )}
        <details>
          <summary>Table alternative</summary>
          <table>
            <thead>
              <tr>
                <th scope="col">Series</th>
                <th scope="col">Loss (MSE)</th>
              </tr>
            </thead>
            <tbody>
              {losses.map(([label, value, series]) => (
                <tr key={series}>
                  <td>{label}</td>
                  <td>{evaluated ? value.toFixed(4) : outcome('')}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </details>
      </div>

      <div className="monitor-absent">
        <strong>Absent from the current contract</strong>
        <ul>
          <li>
            Per-epoch loss history — {NOT_AVAILABLE}. <span>{absent.per_epoch_loss_history}</span>
          </li>
          <li>
            Wall-clock duration — {NOT_AVAILABLE}. <span>{absent.wall_clock_duration}</span>
          </li>
          <li>
            Live epoch progress — {NOT_AVAILABLE}. <span>{absent.current_epoch_progress}</span>
          </li>
        </ul>
      </div>

      <details className="monitor-layers">
        <summary>
          Compiled Burn modules ({modelInfo.layers.length})
          {!model || ' · fixture, server does not echo the model marker'}
        </summary>
        <ol>
          {modelInfo.layers.map((layer, index) => (
            <li key={layer}>
              <code>{layer}</code>
              <span>{modelInfo.burn_code[index]}</span>
            </li>
          ))}
        </ol>
      </details>
    </MonitorPanel>
  )
}

function fmtNum(value) {
  const v = Number(value)
  if (!Number.isFinite(v)) return '—'
  return String(parseFloat(v.toPrecision(6)))
}

/**
 * Differential privacy budget. Two honest states, never blended:
 *   - measured  : a real [xazz:dp] report came back from the Full Run. A solid
 *                 fill and "Real" maturity show a quantity was consumed.
 *   - implemented/empty : no withDp(...) query ran this run, so nothing was
 *                 measured. The track stays empty so it never reads as one.
 */
function PrivacyBudgetPanel({ dp }) {
  const isMeasured = Boolean(
    dp && Number.isFinite(Number(dp.epsilon)) && Number(dp.budget_total) > 0,
  )

  if (!isMeasured) {
    return (
      <MonitorPanel
        contract="implemented"
        icon={ShieldQuestion}
        title="Differential privacy budget"
        unit="epsilon · none consumed"
        maturity="Beta"
        scope="Panel is implemented — no withDp(...) query ran this Full Run, so nothing is shown as measured."
      >
        <p className="monitor-gap">
          <FlaskConical size={13} aria-hidden="true" />
          No withDp(...) query ran in this Full Run, so nothing consumed the
          privacy budget. Add a DP aggregation step (e.g.{' '}
          <code>|&gt; groupBy("district") |&gt; mean("pm25") |&gt; withDp(epsilon: 1.0)</code>)
          to measure it.
        </p>

        <div className="monitor-chart">
          <div className="monitor-chart__heading">
            <strong>No epsilon spent this run</strong>
            <span>budget stays open until a withDp query runs</span>
          </div>
          <div
            className="monitor-budget"
            role="img"
            aria-label="No differential privacy budget consumed: the track is empty."
          >
            <div className="monitor-budget__track" />
            <div className="monitor-budget__legend">
              <span>Nothing spent · track is empty</span>
              <span>{NOT_AVAILABLE}</span>
            </div>
          </div>
        </div>
        <p className="monitor-caveat">
          The backend emits a <code>[xazz:dp]</code> report for every{' '}
          <code>withDp(...)</code> query. Without one there is no measured
          quantity to show, so the panel stays empty instead of inventing a fill.
        </p>
      </MonitorPanel>
    )
  }

  const spent = Number(dp.budget_spent ?? dp.epsilon)
  const total = Number(dp.budget_total ?? dp.epsilon)
  const pct = Math.min(100, Math.max(0, (spent / (total || 1)) * 100))
  const noiseLabel = dp.mechanism === 'gaussian' ? 'σ' : 'scale b'
  const noised = Array.isArray(dp.noised_columns) ? dp.noised_columns : []

  return (
    <MonitorPanel
      contract="measured"
      icon={ShieldCheck}
      title="Differential privacy budget"
      unit="epsilon · consumed this run"
      maturity="Real"
      scope={`Measured from a real Full Run · ${dp.mechanism} · ε ${fmtNum(dp.epsilon)}`}
    >
      <dl className="monitor-facts">
        <div>
          <dt>Mechanism</dt>
          <dd>{dp.mechanism}</dd>
        </div>
        <div>
          <dt>Epsilon (this query)</dt>
          <dd>{fmtNum(dp.epsilon)}</dd>
        </div>
        <div>
          <dt>Sensitivity Δf</dt>
          <dd>{fmtNum(dp.sensitivity)}</dd>
        </div>
        <div>
          <dt>Noise parameter</dt>
          <dd>
            {noiseLabel} {fmtNum(dp.noise_param)}
          </dd>
        </div>
        <div>
          <dt>Noised columns</dt>
          <dd>{noised.length ? noised.join(', ') : '—'}</dd>
        </div>
      </dl>

      <div className="monitor-chart">
        <div className="monitor-chart__heading">
          <strong>Epsilon consumed this session</strong>
          <span>
            {dp.mechanism} mechanism · applied to the aggregated output
          </span>
        </div>
        <div
          className="monitor-budget monitor-budget--measured"
          role="img"
          aria-label={`Privacy budget ${fmtNum(spent)} of ${fmtNum(total)} epsilon consumed by this run.`}
        >
          <div className="monitor-budget__track">
            <i style={{ '--bar-width': `${pct}%` }} />
          </div>
          <div className="monitor-budget__legend">
            <span>
              {fmtNum(spent)} of {fmtNum(total)} ε spent
            </span>
            <span>{pct.toFixed(1)}% of total budget</span>
          </div>
        </div>
        <p className="monitor-caveat">
          The budget is per execution session — each Full Run starts a fresh one.
          Repeated queries spend epsilon cumulatively, and a query that would push
          total over budget is refused to block noise-averaging reconstruction.
        </p>
      </div>
    </MonitorPanel>
  )
}

function ResourcePanel() {
  const maxCpu = Math.max(...resources.samples.map((sample) => sample.cpu_percent))
  const summary = `Illustrative only: this chart shows a proposed per-stage resource shape, not a measurement. No monitoring endpoint exists in the current implementation.`

  return (
    <MonitorPanel
      contract="proposed"
      icon={Cpu}
      title="Resource efficiency"
      unit="percent · megabytes · proposed units"
      maturity="Planned"
      scope={PROPOSED_SCOPE}
    >
      <p className="monitor-gap">
        <FlaskConical size={13} aria-hidden="true" />
        {resources.blocking_gap}
      </p>

      <div className="monitor-chart">
        <div className="monitor-chart__heading">
          <strong>Proposed per-stage utilisation</strong>
          <span>four pipeline stages</span>
        </div>
        <div className="monitor-bars" role="img" aria-label={summary}>
          {resources.samples.map((sample) => (
            <div className="monitor-bars__row" key={sample.stage}>
              <span>{sample.stage}</span>
              <i
                className="monitor-bars__fill monitor-bars__fill--proposed"
                style={{ '--bar-width': `${(sample.cpu_percent / maxCpu) * 100}%` }}
              />
              <strong>{NOT_AVAILABLE}</strong>
            </div>
          ))}
        </div>
        <p className="monitor-caveat">{summary}</p>
        <details>
          <summary>Table alternative · proposed samples</summary>
          <table>
            <thead>
              <tr>
                <th scope="col">Stage</th>
                <th scope="col">Proposed CPU</th>
                <th scope="col">Proposed memory</th>
                <th scope="col">Measured</th>
              </tr>
            </thead>
            <tbody>
              {resources.samples.map((sample) => (
                <tr key={sample.stage}>
                  <td>
                    <code>{sample.stage}</code>
                  </td>
                  <td>{sample.cpu_percent}%</td>
                  <td>{sample.memory_mb} MB</td>
                  <td>{NOT_AVAILABLE}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </details>
      </div>

      <p className="monitor-endpoint">
        Proposed endpoint <code>{resources.proposed_endpoint}</code> · GPU {resources.gpu_reason}
      </p>
    </MonitorPanel>
  )
}

/**
 * Policy-as-Code guardrail. Two honest states, never blended:
 *   - measured : a real /security/policy/check or blocked /execute came back
 *                with a PolicyReport. Pass/blocked/warnings are shown as facts.
 *   - implemented/empty : no check has run this session, so nothing is shown
 *                as measured. The panel stays hollow so it never reads as one.
 *
 * This is a *measured* capability (issue #2 / #8 shipped the backend), so unlike
 * the proposed panels it may render success and warning tones — but only when a
 * real report is present.
 */
function GuardrailCard({ v }) {
  const tone =
    v.severity === 'block' ? 'guardrail-card--block' : v.severity === 'warn' ? 'guardrail-card--warn' : 'guardrail-card--info'
  return (
    <div className={`guardrail-card ${tone}`}>
      <div className="guardrail-card__head">
        <code>{v.rule_id}</code>
        <strong>{v.rule_name}</strong>
        <span>{v.severity}</span>
      </div>
      <p className="guardrail-card__message">{v.message}</p>
      {Array.isArray(v.columns) && v.columns.length > 0 && (
        <p className="guardrail-card__cols">
          Columns: <code>{v.columns.join(', ')}</code>
        </p>
      )}
      {v.remediation_hint && (
        <p className="guardrail-card__hint">Fix: {v.remediation_hint}</p>
      )}
      {v.source_ref && (
        <p className="guardrail-card__ref">
          Basis: <code>{v.source_ref}</code>
        </p>
      )}
    </div>
  )
}

function RemediationDiff({ originalCode, remediation }) {
  const original = (originalCode || '').split('\n')
  const fixed = (remediation?.code || '').split('\n')
  const length = Math.max(original.length, fixed.length)
  const verified = remediation?.verified !== false
  const residual = Array.isArray(remediation?.residual) ? remediation.residual : []

  return (
    <div className="guardrail-remediation">
      <div className="guardrail-remediation__head">
        <div>
          <strong>Automatic remediation</strong>
          <span>
            strategy: <code>{remediation?.strategy}</code>
          </span>
        </div>
        <StatusBadge
          axis="Verified"
          tone={verified ? 'success' : 'warning'}
          compact
        >
          {verified ? 'Policy verified' : 'Manual review required'}
        </StatusBadge>
      </div>

      {!verified && (
        <p className="guardrail-remediation__residual">
          <AlertTriangle size={14} aria-hidden="true" />
          This fix is <strong>not safe on its own</strong> — {residual.length} residual
          violation(s) need human handling before the code may run.
        </p>
      )}

      <div className="guardrail-diff">
        <div className="guardrail-diff__col">
          <span className="guardrail-diff__label">Original</span>
          <div className="guardrail-diff__body">
            {Array.from({ length }, (_, i) => (
              <pre
                key={`o-${i}`}
                className={original[i] !== fixed[i] ? 'is-changed' : ''}
              >
                {original[i] ?? ' '}
              </pre>
            ))}
          </div>
        </div>
        <div className="guardrail-diff__col">
          <span className="guardrail-diff__label">Remediated</span>
          <div className="guardrail-diff__body">
            {Array.from({ length }, (_, i) => (
              <pre
                key={`f-${i}`}
                className={original[i] !== fixed[i] ? 'is-changed' : ''}
              >
                {fixed[i] ?? ' '}
              </pre>
            ))}
          </div>
        </div>
      </div>

      {Array.isArray(remediation?.applied) && remediation.applied.length > 0 && (
        <ul className="guardrail-applied">
          {remediation.applied.map((fix, index) => (
            <li key={index}>
              <code>{fix.rule_id}</code>
              {fix.description}
            </li>
          ))}
        </ul>
      )}

      {residual.length > 0 && (
        <div className="guardrail-residual">
          <strong>Residual · human handling</strong>
          {residual.map((v, index) => (
            <GuardrailCard key={index} v={v} />
          ))}
        </div>
      )}

      {Array.isArray(remediation?.notes) && remediation.notes.length > 0 && (
        <ul className="guardrail-notes">
          {remediation.notes.map((note, index) => (
            <li key={index}>{note}</li>
          ))}
        </ul>
      )}
    </div>
  )
}

function GuardrailPanel({ policy, remediation, originalCode }) {
  const isMeasured = Boolean(policy)

  if (!isMeasured) {
    return (
      <MonitorPanel
        contract="implemented"
        icon={ShieldQuestion}
        title="Policy-as-Code guardrail"
        unit="static check · none run"
        maturity="Beta"
        scope="Panel is implemented — no policy check has run this session, so nothing is shown as measured."
      >
        <p className="monitor-gap">
          <FlaskConical size={13} aria-hidden="true" />
          Run a policy check (or a Full Run) to inspect violations and warnings
          before anything executes. The backend performs the static check via{' '}
          <code>/security/policy/check</code>.
        </p>
      </MonitorPanel>
    )
  }

  const blocked = !policy.safe_to_execute
  const violations = Array.isArray(policy.violations) ? policy.violations : []
  const warnings = Array.isArray(policy.warnings) ? policy.warnings : []
  const parseError = policy.parse_error

  return (
    <MonitorPanel
      contract="measured"
      icon={blocked ? ShieldAlert : ShieldCheck}
      title="Policy-as-Code guardrail"
      unit={`${policy.policy_id} v${policy.policy_version}`}
      maturity="Real"
      scope={`${policy.domain} · risk ${policy.risk_level} · ${policy.scanned_statements} statement(s) scanned`}
    >
      {blocked ? (
        <p className="guardrail-result guardrail-result--blocked">
          <ShieldAlert size={15} aria-hidden="true" />
          Policy check blocked execution · {violations.length} violation(s)
        </p>
      ) : (
        <p className="guardrail-result guardrail-result--pass">
          <ShieldCheck size={15} aria-hidden="true" />
          Guardrail check passed
          {warnings.length > 0 ? ` · ${warnings.length} warning(s)` : ' · no warnings'}
        </p>
      )}

      {parseError && (
        <p className="guardrail-parse-error">
          <AlertTriangle size={14} aria-hidden="true" />
          Parse error — failing closed: {parseError}
        </p>
      )}

      {violations.length > 0 && (
        <div className="guardrail-section">
          <strong>Violations</strong>
          {violations.map((v, index) => (
            <GuardrailCard key={index} v={v} />
          ))}
        </div>
      )}

      {warnings.length > 0 && (
        <div className="guardrail-section guardrail-section--warnings">
          <strong>Warnings</strong>
          {warnings.map((w, index) => (
            <GuardrailCard key={index} v={w} />
          ))}
        </div>
      )}

      {remediation && (
        <RemediationDiff originalCode={originalCode} remediation={remediation} />
      )}
    </MonitorPanel>
  )
}

export function MonitorView({ runState, training, model, dp, policy, remediation, originalCode }) {
  return (
    <div className="monitor-view" aria-label="Run monitoring">
      <div className="monitor-view__rail" aria-hidden="true" />
      <div className="monitor-view__panels">
        <BurnPanel runState={runState} training={training} model={model} />
        <div className="monitor-view__pair">
          <PrivacyBudgetPanel dp={dp} />
          <ResourcePanel />
        </div>
        <GuardrailPanel
          policy={policy}
          remediation={remediation}
          originalCode={originalCode}
        />
      </div>
    </div>
  )
}
