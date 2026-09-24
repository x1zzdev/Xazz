# Semiconductor etch SPC example

A `.xzz` project that runs statistical process control (SPC) on wafer metrology from an
etch step. The data is synthetic and generated from a fixed seed. It contains no fab,
tool-vendor or customer data.

| File | What it is |
|---|---|
| `generate_wafer_data.py` | Writes `data/wafer_metrology.csv`: 12 lots × 25 wafers × 5 sites = 1,500 readings |
| `main.xzz` | Clean → particles per tool → X-bar chart → CD summary → readings above the control limit |
| `xazz.toml` | Project file, same layout as `xazz new` |

The CSV is generated rather than committed. The repository tracks `*.csv` with Git LFS,
and the security example handles its data the same way.

## Run

```bash
cd examples/semiconductor
python3 generate_wafer_data.py      # data/wafer_metrology.csv (seed 173)
xazz check main.xzz                 # static analysis passes with 0 errors
xazz run main.xzz                   # prints the last pipeline, writes *_chart.html
```

Run it from this directory, because `load("data/...")` is relative to the project.

## What the data is built to show

- **Missing sites.** About 1.5% of `thickness_nm` values are empty (`Option<float>`). The
  type checker knows the column can be null, and `dropNull` removes these rows.
- **Sensor glitches.** About 0.5% of readings are `0.0` or `999.9` nm, which no film can
  produce. Two `filter` steps remove them. 1,500 rows become 1,466.
- **A drifting tool.** Chamber B of ETCH-02 moves its critical dimension (CD) up by
  0.12 nm per lot. The other tools stay on the 45.00 nm target.

## Results (seed 173)

X-bar chart `etch02_xbar_chart.html` shows the lot-mean CD on ETCH-02. It climbs from
44.96 nm (LOT-2609-01) to 45.68 nm (LOT-2609-12).

CD summary per tool (`cd_summary`, from `agg([mean, std, count])`):

| tool | cd_nm_mean | cd_nm_std | cd_nm_count |
|---|---:|---:|---:|
| ETCH-01 | 45.024 | 0.350 | 491 |
| ETCH-02 | 45.336 | 0.579 | 488 |
| ETCH-03 | 44.998 | 0.348 | 487 |

Readings above the upper control limit (`above_ucl`, the printed result):

| tool | readings above UCL 46.05 nm |
|---|---:|
| ETCH-01 | 1 |
| ETCH-02 | 72 |

ETCH-03 has no reading above the limit, so it has no row. One reading on ETCH-01 is
within the false-alarm rate of a 3σ limit, which is about 0.13% of 491 readings.

The control limits come from the qualified baseline: target 45.00 nm and σ 0.35 nm, so
UCL = 46.05 and LCL = 43.95. They stay fixed while production is monitored.
Recomputing them from the drifting data itself would widen the limits and hide the drift,
which is why ETCH-02's σ of 0.579 is a symptom, not a limit.

## In the Visual IDE

The IDE opens its built-in air-quality sample, and it cannot open a `.xzz` project
yet. To see this data there, generate the CSV and follow these steps:

1. Open **Edit**.
2. Select the **File Input** node.
3. Choose `data/wafer_metrology.csv`.

xazz-server infers the schema. `thickness_nm` comes back as `float`, and the node is
filled in.

## 한국어 요약

식각 공정 계측 데이터로 SPC를 수행하는 예제입니다. 합성 데이터는 고정 시드로 만들며
실제 기업 데이터는 쓰지 않습니다. 파이프라인은 다음 순서로 진행합니다.

1. 결측 사이트와 센서 글리치를 제거합니다.
2. 장비별 파티클을 집계합니다.
3. ETCH-02의 로트 평균 CD로 X-bar 관리도를 그립니다.
4. 장비별 CD 평균과 표준편차를 요약합니다.
5. 기준 공정으로 정한 관리 한계(UCL 46.05nm)를 넘은 측정값을 셉니다.

ETCH-02 B 챔버의 드리프트 때문에 이 장비에서만 초과 측정이 72건 나옵니다.
