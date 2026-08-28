# 메모리 모델 — Polars ↔ Burn 데이터 경계

상태: 구현됨 (v0.3.0) · 코드: [`xazz-exec/src/tensor_bridge.rs`](../../xazz-exec/src/tensor_bridge.rs)

---

## 배경

데이터 시스템(Polars, columnar)과 ML 시스템(Burn, tensor)을 연결하면 단순히
"값이 같다"는 것 이상의 문제가 생긴다. ownership · lifetime · dtype ·
alignment · contiguous layout · row/column major · device placement 행이
얽힌다. 이 문서는 Xazz 의 Polars → Burn 변환 경계에서 **무엇이 복사되고
무엇이 공유되는지**를 명시적으로 정의한다.

---

## 변환 경로 요약

```
Polars DataFrame (Arrow columnar)
        │
        ├─ Float64 연속(단일 청크·무결측) ──► cont_slice() → &[f64]  (무복사 읽기)
        ├─ Float32 연속 ─────────────────► cont_slice() → &[f32]  (무복사 읽기)
        ├─ 정수 / 결측 포함 ──────────────► cast → null→NaN 벡터    (1회 변환)
        │
        ▼
   columnar → [n, d] row-major 배치  (단일 사전할당 Vec)
        │
        ▼
   Burn TensorData::new(Vec<f32>, [n, d])
        │
        ▼
   Tensor::<B, 2>::from_data(...)  (host → device — Burn 책임)
```

---

## 복사 경계 (copy boundary)

완전한 무복사(zero-copy)는 Burn `TensorData` 가 owned `Vec<f32>` 를 요구하므로
불가능하다. Xazz 가 "제로카피"라 부르는 것은 **불필요한 중간 복사를 제거**한
것이다. 남는 복사는 셋뿐이다:

1. **f64 → f32 정밀도 강등**
   - Burn CPU 백엔드(NdArray)가 f32 이므로 발생.
   - 동일 f32 컬럼은 변환 없이 복사만 한다.
2. **columnar → row-major 재배치**
   - `[n, d]` 텐서 레이아웃 요구. 연속 컬럼은 raw 슬라이스에서 직접 읽어
     **컬럼별 중간 `Vec` 를 만들지 않고** 단일 버퍼에 한 번에 채운다.
3. **host → device 전송**
   - `Tensor::from_data` 의 책임. CPU 백엔드에서는 no-op.

### 제거된 복사 (v0.6 → v0.7)

| 항목 | v0.6 (예전) | v0.7 (현재) |
|------|-------------|-------------|
| Float64 컬럼 | `cast(Float64)` 전량 + `Option` 순회 | `cont_slice()` raw 버퍼 직접 읽기 |
| 컬럼별 중간 벡터 | 컬럼마다 `Vec<f32>` 생성 후 재평탄화 | raw 슬라이스에서 최종 row-major 로 직접 채움 |

---

## 규약

- **dtype 표준화**: 모든 숫자형(int/uint/float)은 f32 로 강등, 결측은 NaN.
  대체(평균 등)는 `dl` 표준화 단계의 책임이며 브리지는 값을 왜곡하지 않는다.
- **비숫자 컬럼**: 변환에서 제외 (에러 + cast 문구 안내).
- **null**: NaN 으로 전달 (원본 null 개수 보존은 dp/표준화가 각자 처리).
- **소유권**: 변환 중 Polars 가 원본을 소유하며, 슬라이스는 변환 함수 범위
  동안만 유효. 반환되는 `TensorData` 는 소유 벡터.

---

## 대용량 시 주의

데이터가 100GB/1TB 로 커지면 단일 f64→f32 벡터 + row-major 배치는 여전히
전체 복사다. 대규모 경로의 진짜 제로카피는 백엔드 수준에서 columnar 텐서
(Arrow 기반 tensor layout)를 Burn 이 직접 소비하게 하는 것이며, 이는 후속
마일스톤으로 남겨둔다. 현재 구현은 "최소 복사 + 단일 버퍼" 수준을 정직하게
제공한다. (README 의 성능 숫자는 이 모델을 기준으로 한다)