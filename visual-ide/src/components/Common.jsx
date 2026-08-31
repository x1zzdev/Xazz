import React from 'react'
import {
  ArrowLeft,
  Check,
  CircleAlert,
  CircleDashed,
  FlaskConical,
  GitFork,
} from 'lucide-react'

export function Brand({ onHome, inverse = false }) {
  const content = (
    <>
      <span className="brand-mark" aria-hidden="true">
        <span>[</span>
        <strong>x</strong>
        <span>]</span>
      </span>
      <span className="brand-name">Xazz</span>
    </>
  )

  if (!onHome) {
    return <div className={`brand ${inverse ? 'brand--inverse' : ''}`}>{content}</div>
  }

  return (
    <button
      className={`brand brand--button ${inverse ? 'brand--inverse' : ''}`}
      type="button"
      onClick={onHome}
      aria-label="Go to Xazz landing page"
    >
      {content}
    </button>
  )
}

const toneIcons = {
  success: Check,
  warning: CircleAlert,
  danger: CircleAlert,
  future: FlaskConical,
  neutral: CircleDashed,
  info: CircleDashed,
}

export function StatusBadge({ axis, tone = 'neutral', children, compact = false }) {
  const Icon = toneIcons[tone] ?? CircleDashed
  return (
    <span
      className={`status-badge status-badge--${tone} ${
        compact ? 'status-badge--compact' : ''
      }`}
      aria-label={`${axis}: ${children}`}
    >
      <Icon aria-hidden="true" size={compact ? 12 : 14} strokeWidth={2} />
      {!compact && <span className="status-badge__axis">{axis}</span>}
      <span>{children}</span>
    </span>
  )
}

export function BackButton({ onClick, label = 'Back' }) {
  return (
    <button className="button button--quiet" type="button" onClick={onClick}>
      <ArrowLeft size={17} aria-hidden="true" />
      {label}
    </button>
  )
}

export function GitHubLink({ inverse = false }) {
  return (
    <a
      className={`button button--quiet ${inverse ? 'button--inverse' : ''}`}
      href="https://github.com/xazzdev/Xazz"
      target="_blank"
      rel="noreferrer"
    >
      <GitFork size={17} aria-hidden="true" />
      GitHub
    </a>
  )
}

export function InlineIcon({ icon: Icon, children }) {
  return (
    <span className="inline-icon">
      <Icon size={15} aria-hidden="true" />
      {children}
    </span>
  )
}
