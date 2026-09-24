import React from 'react'
import { RotateCcw, TriangleAlert } from 'lucide-react'
import { useLanguage } from '../i18n'

function PanelFallback({ name, error, onRetry, resetLabel, onResetAndRetry }) {
  const { t } = useLanguage()
  return (
    <div className="panel-error" role="alert">
      <TriangleAlert size={16} aria-hidden="true" />
      <div className="panel-error__body">
        <strong>{t('errors.panelTitle').replace('{name}', name)}</strong>
        <p>{t('errors.panelBody')}</p>
        <code>{String(error?.message ?? error)}</code>
        <div className="panel-error__actions">
          <button className="button button--tool-secondary" type="button" onClick={onRetry}>
            <RotateCcw size={14} aria-hidden="true" />
            {t('errors.retry')}
          </button>
          {resetLabel && (
            <button className="button button--tool-secondary" type="button" onClick={onResetAndRetry}>
              {resetLabel}
            </button>
          )}
        </div>
      </div>
    </div>
  )
}

/**
 * Keeps one crashed panel from taking the whole screen down. React only catches
 * render/lifecycle errors in a class component, so this stays a class.
 * `onReset` is an explicit, user-chosen recovery (e.g. discarding a corrupt saved
 * DAG) — never run automatically, because it may throw away the user's work.
 */
export class ErrorBoundary extends React.Component {
  state = { error: null }

  static getDerivedStateFromError(error) {
    return { error }
  }

  retry = () => this.setState({ error: null })

  resetAndRetry = () => {
    this.props.onReset?.()
    this.retry()
  }

  render() {
    if (!this.state.error) return this.props.children
    return (
      <PanelFallback
        name={this.props.name}
        error={this.state.error}
        onRetry={this.retry}
        resetLabel={this.props.onReset ? this.props.resetLabel : null}
        onResetAndRetry={this.resetAndRetry}
      />
    )
  }
}
