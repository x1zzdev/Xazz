const districts = ['Jongno', 'Mapo', 'Gangnam', 'Songpa', 'Yongsan']

export const demoFillValue = 31

export const sourceRows = Array.from({ length: 100 }, (_, index) => {
  const pm25 = index % 17 === 0 ? null : 12 + ((index * 7) % 65)
  return {
    observed_at: `2026-07-${String(1 + (index % 26)).padStart(2, '0')} ${String(
      8 + (index % 12),
    ).padStart(2, '0')}:00`,
    district: districts[index % districts.length],
    pm25,
    temperature_c: 21 + ((index * 3) % 14) / 2,
  }
})

export const filledRows = sourceRows.map((row) => ({
  ...row,
  pm25: row.pm25 ?? demoFillValue,
}))

export const resultRows = filledRows.filter((row) => row.pm25 <= 35)

export const scenario = {
  fillValue: demoFillValue,
  sampleLimit: sourceRows.length,
  sourceNulls: sourceRows.filter((row) => row.pm25 === null).length,
  resultNulls: resultRows.filter((row) => row.pm25 === null).length,
  resultCount: resultRows.length,
  removedCount: sourceRows.length - resultRows.length,
}

export const pipeline = [
  {
    id: 'load',
    band: 'PREPROCESS',
    from: [],
    position: { x: 16,  y: 40 },
    label: 'Load CSV',
    stage: 'SOURCE',
    evidence: '100 rows · UTF-8',
    codeLine: 8,
    detail: {
      intent: 'Read the synthetic air-quality sample.',
      rows: '100 → 100',
      nulls: `${scenario.sourceNulls} found`,
      schema: '4 fields inferred',
      duration: 'Not emitted by current runtime',
      artifact: 'None',
    },
  },
  {
    id: 'schema',
    band: 'PREPROCESS',
    from: ['load'],
    position: { x: 188, y: 40 },
    label: 'Check schema',
    stage: 'TYPE',
    evidence: 'pm25 · Float?',
    codeLine: 4,
    detail: {
      intent: 'Review the inferred type before accepting generated .xzz.',
      rows: '100 → 100',
      nulls: `${scenario.sourceNulls} nullable values`,
      schema: 'pm25: Float? · review required',
      duration: 'Not emitted by current runtime',
      artifact: 'main.xzz change pending',
    },
  },
  {
    id: 'fill',
    band: 'PREPROCESS',
    from: ['schema'],
    position: { x: 360, y: 40 },
    label: 'Fill null',
    stage: 'TRANSFORM',
    evidence: `${scenario.sourceNulls} → 0 nulls`,
    codeLine: 9,
    detail: {
      intent: `Replace missing PM2.5 values with the fixed synthetic demo value ${demoFillValue.toFixed(1)}.`,
      rows: '100 → 100',
      nulls: `−${scenario.sourceNulls}`,
      schema: 'pm25 stays Float',
      duration: 'Not emitted by current runtime',
      artifact: 'None',
    },
  },
  {
    id: 'filter',
    band: 'PREPROCESS',
    from: ['fill'],
    position: { x: 532, y: 40 },
    label: 'Filter threshold',
    stage: 'TRANSFORM',
    evidence: `100 → ${scenario.resultCount} rows`,
    codeLine: 10,
    detail: {
      intent: 'Keep observations at or below 35 μg/m³.',
      rows: `100 → ${scenario.resultCount}`,
      nulls: '0 → 0',
      schema: 'No type change',
      duration: 'Not emitted by current runtime',
      artifact: 'None',
    },
  },
  {
    id: 'result',
    band: 'PREPROCESS',
    from: ['filter'],
    position: { x: 704, y: 40 },
    label: 'Result',
    stage: 'OUTPUT',
    evidence: `${scenario.resultCount} rows`,
    codeLine: 11,
    detail: {
      intent: 'Expose a bounded result preview with an optional browser export.',
      rows: `${scenario.resultCount} rows`,
      nulls: '0',
      schema: '4 fields',
      duration: 'Not emitted by current runtime',
      artifact: 'Not requested · optional browser export',
    },
  },
  {
    id: 'compile',
    band: 'ML COMPILE',
    from: [],
    position: { x: 360, y: 190 },
    label: 'Compile AirNet',
    stage: 'MODEL',
    evidence: '5 layers · 209 params',
    codeLine: 13,
    detail: {
      intent: 'Lower the declared layer stack to Burn module configuration.',
      rows: 'No rows consumed · declaration',
      nulls: 'Not applicable',
      schema: 'in 3 → out 1',
      duration: 'Not available in this version',
      artifact: 'None',
    },
  },
  {
    id: 'train',
    band: 'ML COMPILE',
    from: ['result', 'compile'],
    position: { x: 704, y: 190 },
    label: 'Train model',
    stage: 'TRAIN',
    evidence: '40 epochs · loss 0.0417',
    codeLine: 19,
    detail: {
      intent: 'Fit AirNet on the filtered rows with pm25 as the target column.',
      rows: `${scenario.resultCount} rows · batch 16`,
      nulls: '0 remaining',
      schema: 'features temperature_c, district_idx, hour',
      duration: 'Not available in this version',
      artifact: 'artifacts/airnet.mpk',
    },
  },
  {
    id: 'predict',
    band: 'ML COMPILE',
    from: ['result', 'train'],
    position: { x: 876, y: 115 },
    label: 'Predict',
    stage: 'PREDICT',
    evidence: `+1 column · ${scenario.resultCount} rows`,
    codeLine: 20,
    detail: {
      intent: 'Apply the trained model back onto the rows as a new column.',
      rows: `${scenario.resultCount} → ${scenario.resultCount}`,
      nulls: '0 → 0',
      schema: '+ pm25_pred: Float',
      duration: 'Not available in this version',
      artifact: 'None',
    },
  },
]

export const codeLines = [
  'type AirRow {',
  '  observed_at: String,',
  '  district: String,',
  '  pm25: Float?,',
  '  temperature_c: Float',
  '}',
  '',
  'air = load<AirRow>("data/seoul_air_quality.csv")',
  'clean = air |> fillNull(pm25, 31.0)',
  'safe = clean |> filter(pm25 <= 35.0)',
  'result = safe |> select(observed_at, district, pm25, temperature_c)',
  '',
  'model AirNet {',
  '  Dense(16) ReLU',
  '  Dense(8) ReLU',
  '  Dense(1)',
  '}',
  '',
  'trained = result |> train(AirNet, target: "pm25", epochs: 40, lr: 0.01)',
  'scored = result |> predict(trained, as: "pm25_pred")',
]

export const chartData = districts.map((district) => {
  const rows = resultRows.filter((row) => row.district === district)
  const mean = rows.reduce((total, row) => total + row.pm25, 0) / rows.length
  return { district, mean: Number(mean.toFixed(1)), count: rows.length }
})
