// Reference quartiles match Python's statistics.quantiles(method='inclusive') (NumPy's linear default); fences are Tukey 1.5×IQR.
import assert from 'node:assert/strict'
import { fiveNumber, histogram, numericValues, quantile } from '../src/chartStats.js'

assert.equal(quantile([1, 2, 3, 4], 0.25), 1.75)
assert.equal(quantile([1, 2, 3, 4], 0.5), 2.5)
assert.equal(quantile([1, 2, 3, 4], 0.75), 3.25)
assert.equal(quantile([7], 0.5), 7)

const box = fiveNumber([100, 1, 4, 2, 3])
assert.deepEqual(
  [box.min, box.q1, box.median, box.q3, box.max, box.whiskerLow, box.whiskerHigh],
  [1, 2, 3, 4, 100, 1, 4],
)
assert.deepEqual(box.outliers, [100], 'a point beyond Q3 + 1.5×IQR is an outlier, not a whisker end')

const values = [0, 0.5, 1, 1.5, 2, 2.5, 3, 3.5, 4]
const bins = histogram(values)
assert.equal(bins.length, Math.ceil(Math.log2(values.length)) + 1)
assert.equal(bins.reduce((sum, bin) => sum + bin.count, 0), values.length, 'every value lands in a bin')
assert.equal(bins.at(-1).end, 4)
assert.ok(bins.at(-1).count >= 1, 'the maximum is counted in the closed last bin')
assert.deepEqual(histogram([5, 5, 5]), [{ start: 5, end: 5, count: 3 }])

assert.deepEqual(numericValues([{ a: 1 }, { a: null }, { a: 'x' }, { a: 2.5 }], 'a'), {
  values: [1, 2.5],
  missing: 2,
})

console.log('chartStats: ok; quantile=type7; tukey fences; bins sum to n')
