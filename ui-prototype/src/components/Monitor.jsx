import React from 'react'
import {
  Boxes,
  Cpu,
  FlaskConical,
  ShieldQuestion,
} from 'lucide-react'
import { StatusBadge } from './Common'
import executeResponse from '../mock/execute-response.json'
import proposedTelemetry from '../mock/telemetry-proposed.json'

const fallbackFixture = executeResponse
const budget = proposedTelemetry.privacy_budget
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

function PrivacyBudgetPanel() {
  const spentPercent = (budget.spent / budget.total) * 100
  const summary = `Illustrative only: this chart shows a proposed epsilon ledger structure, not a measurement. No differential-privacy accountant exists in the current implementation.`

  return (
    <MonitorPanel
      contract="proposed"
      icon={ShieldQuestion}
      title="Differential privacy budget"
      unit="epsilon · proposed unit"
      maturity="Research"
      scope={PROPOSED_SCOPE}
    >
      <p className="monitor-gap">
        <FlaskConical size={13} aria-hidden="true" />
        {budget.blocking_gap}
      </p>

      <div className="monitor-chart">
        <div className="monitor-chart__heading">
          <strong>Proposed epsilon consumption</strong>
          <span>
            {budget.mechanism} mechanism · delta {budget.delta}
          </span>
        </div>
        <div className="monitor-budget" role="img" aria-label={summary}>
          <div className="monitor-budget__track">
            <i style={{ '--bar-width': `${spentPercent}%` }} />
          </div>
          <div className="monitor-budget__legend">
            <span>
              Structure shows {budget.spent} of {budget.total} epsilon
            </span>
            <span>{NOT_AVAILABLE}</span>
          </div>
        </div>
        <p className="monitor-caveat">{summary}</p>
        <details>
          <summary>Table alternative · proposed per-operation ledger</summary>
          <table>
            <thead>
              <tr>
                <th scope="col">Operation</th>
                <th scope="col">Proposed epsilon</th>
                <th scope="col">Measured</th>
              </tr>
            </thead>
            <tbody>
              {budget.ledger.map((entry) => (
                <tr key={entry.op}>
                  <td>
                    <code>{entry.op}</code>
                    {entry.note && <span> · {entry.note}</span>}
                  </td>
                  <td>{entry.epsilon.toFixed(2)}</td>
                  <td>{NOT_AVAILABLE}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </details>
      </div>

      <p className="monitor-endpoint">
        Proposed endpoint <code>{budget.proposed_endpoint}</code>
      </p>
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

export function MonitorView({ runState, training, model }) {
  return (
    <div className="monitor-view" aria-label="Run monitoring">
      <div className="monitor-view__rail" aria-hidden="true" />
      <div className="monitor-view__panels">
        <BurnPanel runState={runState} training={training} model={model} />
        <div className="monitor-view__pair">
          <PrivacyBudgetPanel />
          <ResourcePanel />
        </div>
      </div>
    </div>
  )
}
