# Xazz 시연 명령어 (복붙용 · 명세서 순서 1~33)

> 명세서 `기능 시험 항목` 번호 순서. 화면공유용.

## 준비
```bash
cd /home/x1zz/Xazz
bash demo/preflight.sh
```

---

## 컴파일러

**(1) 정적 분석 정상 통과**
```bash
./target/release/xazz check demo/preprocess_chart.xzz
```

**(2) 오류 검출 — Did-you-mean**
```bash
sed 's/mean("pm25")/mean("pm52")/' demo/preprocess_chart.xzz > /tmp/bad.xzz
./target/release/xazz check /tmp/bad.xzz
```

**(3) 코드 생성 — emit rust**
```bash
./target/release/xazz emit rust demo/deep_learning.xzz | head -40
```

**(4) 모듈 시스템 — stdlib import**
```bash
./target/release/xazz run examples/stdlib_import.xzz
```

**(5) Typed IR 최적화 — --opt**
```bash
./target/release/xazz run demo/preprocess_chart.xzz --opt
```

**(6) 실패 안전 — fail-closed**
```bash
printf 'type Air = { district: string, pm25: float }\nv x = load("demo/data/seoul_air_sample.csv") :: Air\nv c = x |> chart { kind: bar }\n' > /tmp/bad2.xzz
./target/release/xazz run /tmp/bad2.xzz; echo "exit=$?"
```

---

## 데이터

**(7) 스키마 자동 추론 — import**
```bash
rm -rf /tmp/imp && mkdir -p /tmp/imp && cd /tmp/imp
/home/x1zz/Xazz/target/release/xazz import /home/x1zz/Xazz/visual-ide/data/seoul_air_quality.csv
cat main.xzz
cd /home/x1zz/Xazz
```

**(8) 전처리·차트 — run**
```bash
./target/release/xazz run demo/preprocess_chart.xzz
```

**(9) 결과 저장 — --output**
```bash
./target/release/xazz run demo/preprocess_chart.xzz --output /tmp/out.csv
```

**(10) parquet 저장**
```bash
cat > /tmp/pq.xzz <<'EOF'
type Air = { observed_at: string, district: string, pm10: Option<float>, pm25: Option<float> }
v raw = load("demo/data/seoul_air_sample.csv") :: Air
v s = raw |> groupBy("district") |> mean("pm25") |> take(5)
v saved = s |> save("/tmp/out.parquet", format: "parquet")
EOF
./target/release/xazz run /tmp/pq.xzz
```

**(11) 차트 종류 — pie / scatter**
```bash
cat > /tmp/piescat.xzz <<'EOF'
type Air = { observed_at: string, district: string, pm10: Option<float>, pm25: Option<float> }
v raw = load("demo/data/seoul_air_sample.csv") :: Air
v c = raw |> fillNull("pm25", strategy: "mean")
v s = c |> groupBy("district") |> mean("pm25") |> take(5)
v pie = s |> chart { type: pie, label: district, value: pm25, title: "PM25 pie" }
v scatter = c |> take(30) |> chart { type: scatter, x: pm10, y: pm25, title: "pm10 vs pm25" }
EOF
./target/release/xazz run /tmp/piescat.xzz
```

**(12) 기계판독 출력 — --json**
```bash
./target/release/xazz run demo/preprocess_chart.xzz --json
```

**(13) DuckDB 커넥터**
```bash
./target/release/xazz run examples/duckdb/hello_duckdb.xzz
```

**(14) PostgreSQL 커넥터**
```bash
./target/release/xazz run examples/duckdb/postgres_demo.xzz
```

---

## AI

**(15) 학습 — train**
```bash
./target/release/xazz run demo/deep_learning.xzz
```

**(16) 추론 — predict**
```bash
./target/release/xazz run examples/deep_learning/air_pipeline.xzz
```

**(17) 표준모델**
```bash
./target/release/xazz run examples/stdlib_import.xzz
```

---

## 보안

**(18) PII 차단 — policy**
```bash
./target/release/xazz policy examples/security/patient_unsafe.xzz; echo "exit=$?"
```

**(19) 정상 통과**
```bash
./target/release/xazz policy examples/security/patient_safe.xzz
```

**(20) 자동 수정 — --fix**
```bash
./target/release/xazz policy examples/security/patient_unsafe.xzz --fix --out /tmp/safe.xzz
```

**(21) 기계판독 리포트 — --json**
```bash
./target/release/xazz policy examples/security/patient_unsafe.xzz --json
```

**(22) 차등 프라이버시 — laplace**
```bash
./target/release/xazz run demo/dp.xzz
```

**(23) DP gaussian · 예산 회계**
```bash
./target/release/xazz run examples/privacy/dp_aggregation.xzz
```

**(24) 데이터 정화 — sanitize**
```bash
./target/release/xazz sanitize examples/security/data/patients.csv --json
```

---

## 서버·감사

**(25) 감사 로그 · 해시 체인**
```bash
curl -s -X POST http://127.0.0.1:8005/security/audit -H 'Content-Type: application/json' -d '{"code":"probe"}'
curl -s http://127.0.0.1:8005/security/audit/chain
```

**(26) health**
```bash
curl -s http://127.0.0.1:8005/health
```

**(27) execute**
```bash
curl -s -X POST http://127.0.0.1:8005/execute -H 'Content-Type: application/json' \
  --data-binary '{"code":"type Air = { observed_at: string, district: string, pm10: Option<float>, pm25: Option<float> }\nv r = load(\"demo/data/seoul_air_sample.csv\") :: Air\nv t = r |> take(2)"}'
```

---

## CLI · IDE · 커뮤니티

**(28) new · run**
```bash
rm -rf /tmp/newproj && mkdir -p /tmp/newproj && cd /tmp/newproj
/home/x1zz/Xazz/target/release/xazz new my-project
cd my-project && /home/x1zz/Xazz/target/release/xazz run example.xzz
cd /home/x1zz/Xazz
```

**(29) 레지스트리**
```bash
./target/release/xazz registry list
```

**(30) sde — 합성 데이터**
```bash
./target/release/xazz sde --rows 5 --output /tmp/pairs.jsonl
cat /tmp/pairs.jsonl
```

**(31) Visual IDE**
```
http://127.0.0.1:5173
```

**(32) 라이선스 검증**
```bash
cargo deny check licenses
```

**(33) SBOM**
```bash
ls sbom/xazz.cdx.json THIRD_PARTY_NOTICES.md
```

---

## 정리
```bash
bash demo/preflight.sh stop
```
