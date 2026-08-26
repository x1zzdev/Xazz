import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import { fileURLToPath } from 'node:url'
import { dirname, resolve } from 'node:path'

const here = dirname(fileURLToPath(import.meta.url))
const css = await readFile(resolve(here, '../src/styles.css'), 'utf8')

const token = (name) => {
  const match = css.match(new RegExp(`--${name}:\\s*(#[0-9a-f]{6})`, 'i'))
  assert.ok(match, `missing color token --${name}`)
  return match[1]
}

const channels = (hex) =>
  hex
    .slice(1)
    .match(/.{2}/g)
    .map((channel) => Number.parseInt(channel, 16) / 255)

const luminance = (hex) =>
  channels(hex)
    .map((channel) =>
      channel <= 0.04045
        ? channel / 12.92
        : ((channel + 0.055) / 1.055) ** 2.4,
    )
    .reduce(
      (sum, channel, index) => sum + channel * [0.2126, 0.7152, 0.0722][index],
      0,
    )

const ratio = (foreground, background) => {
  const values = [luminance(foreground), luminance(background)].sort(
    (a, b) => b - a,
  )
  return (values[0] + 0.05) / (values[1] + 0.05)
}

const checks = [
  ['light primary text', 'light-text', 'light-canvas', 4.5],
  ['light secondary text', 'light-text-2', 'light-canvas', 4.5],
  ['dark primary text', 'dark-text', 'dark-surface', 4.5],
  ['dark secondary text', 'dark-text-2', 'dark-surface', 4.5],
  ['dark tertiary text', 'dark-text-3', 'dark-surface', 4.5],
  ['light control boundary', 'light-control-border', 'light-surface', 3],
  ['dark control boundary', 'dark-control-border', 'dark-surface', 3],
  ['dark primary action', 'brand-dark', 'dark-surface', 3],
  ['dark information', 'info-dark', 'dark-surface', 4.5],
  ['dark warning', 'warning-dark', 'dark-surface', 4.5],
  ['dark danger', 'danger-dark', 'dark-surface', 4.5],
]

for (const [label, foreground, background, minimum] of checks) {
  const measured = ratio(token(foreground), token(background))
  assert.ok(
    measured >= minimum,
    `${label} contrast ${measured.toFixed(2)}:1 is below ${minimum}:1`,
  )
}

console.log(`contrast: ok; pairs=${checks.length}; text>=4.5; controls>=3.0`)
