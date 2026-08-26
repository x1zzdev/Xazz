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
]

export const chartData = districts.map((district) => {
  const rows = resultRows.filter((row) => row.district === district)
  const mean = rows.reduce((total, row) => total + row.pm25, 0) / rows.length
  return { district, mean: Number(mean.toFixed(1)), count: rows.length }
})
