// Summary statistics for the result charts (#174). Pure functions over the rows a
// real Full Run returned — nothing here fabricates or smooths data.

/** Finite numbers of `column`, plus how many rows had none (null / text). */
export function numericValues(rows, column) {
  const values = []
  for (const row of rows) {
    const value = row[column]
    if (typeof value === 'number' && Number.isFinite(value)) values.push(value)
  }
  return { values, missing: rows.length - values.length }
}

/** Linear-interpolated quantile of ascending `sorted` (R type 7, NumPy's default). */
export function quantile(sorted, p) {
  const h = (sorted.length - 1) * p
  const low = Math.floor(h)
  const high = Math.min(low + 1, sorted.length - 1)
  return sorted[low] + (h - low) * (sorted[high] - sorted[low])
}

/** Tukey box-plot summary: quartiles, 1.5×IQR whiskers and the points beyond them. */
export function fiveNumber(values) {
  const sorted = [...values].sort((a, b) => a - b)
  const q1 = quantile(sorted, 0.25)
  const q3 = quantile(sorted, 0.75)
  const low = q1 - 1.5 * (q3 - q1)
  const high = q3 + 1.5 * (q3 - q1)
  const inside = sorted.filter((v) => v >= low && v <= high)
  return {
    n: sorted.length,
    min: sorted[0],
    q1,
    median: quantile(sorted, 0.5),
    q3,
    max: sorted.at(-1),
    whiskerLow: inside[0],
    whiskerHigh: inside.at(-1),
    outliers: sorted.filter((v) => v < low || v > high),
  }
}

/**
 * Equal-width bins over [min, max]; Sturges' bin count by default. The last bin is
 * closed so the maximum is counted. A constant column is one bin; no values, no bins.
 */
export function histogram(values, binCount = Math.ceil(Math.log2(values.length)) + 1) {
  if (values.length === 0) return []
  // reduce, not Math.min(...values): spreading a large result would overflow the call stack.
  const min = values.reduce((a, b) => Math.min(a, b))
  const max = values.reduce((a, b) => Math.max(a, b))
  if (min === max) return [{ start: min, end: max, count: values.length }]
  const width = (max - min) / binCount
  const bins = Array.from({ length: binCount }, (_, i) => ({
    start: min + i * width,
    end: i === binCount - 1 ? max : min + (i + 1) * width,
    count: 0,
  }))
  for (const value of values) bins[Math.min(binCount - 1, Math.floor((value - min) / width))].count += 1
  return bins
}
