# #166 Windows CPU 측정과 스트리밍 실패 처리

2026-10-06, PR #285의 `a3ec9a91d2f7874b8adf9fd7112abacaa04df35a`에서 이어서 작업했다. 이 문서는 최초 CPU 실측 당시의 검증 기록이다. 같은 날 진행한 CSV 저장 오류·감사로그 경합 수정과 추가 검증은 [후속 기록](benchmark-scale-166-windows-followup-2026-10-06.md)에 구분했다. 커밋·푸시는 하지 않았다.

**스트리밍 실패 재시도를 제거하고 200,371,584행 CPU 실측·전체 입력 대조를 완료했다.** [#166](https://github.com/x1zzdev/Xazz/issues/166)의 200M 수치와 재현 절차를 별도 표·결과 파일로 기록했다. 이 단계의 Rust 전체 병렬 검사에서는 서버 감사 체인 테스트 2개가 실패했다. 후속 수정 후 전체 병렬 검사는 통과했으며, 아래에는 최초 실패와 직렬 비교 결과를 그대로 남긴다.

## 시작 시 확인한 PR과 작업 범위

[PR #285](https://github.com/x1zzdev/Xazz/pull/285)는 draft/open이었다. 위 head의 검사는 성공 11개, 조건에 따른 제외 1개, 실패·진행 중 0개였다. [CI 실행](https://github.com/x1zzdev/Xazz/actions/runs/37316794896)의 Windows/Linux/macOS 포맷·Clippy·워크스페이스 테스트가 성공했다. 10:39 KST 재확인에서도 head·검사·GPU 지시 범위는 그대로였으며 리뷰와 댓글은 없었다. 이는 이번 미커밋 변경에 대한 원격 검사가 아니다.

문서 배포는 main push 조건이 맞지 않아 제외됐다. 문서 빌드 및 정책 검사 로그에는 기존 액션의 Node 20 지원 종료 경고가 있었다. 최초 실측 당시 Python 벤치마크 테스트는 CI에 연결돼 있지 않아 별도로 실행했다. 후속 변경에는 CI 연결을 추가했지만 아직 원격으로 실행하지 않았다.

[팀장 최신 범위 코멘트](https://github.com/x1zzdev/Xazz/issues/236#issuecomment-5926175456)(2026-10-01 15:43:54 KST)의 남은 GPU 범위는 Windows MSVC에서 `onnx`·`onnx-cuda` 및 `XAZZ_ORT_EP=cuda` 실패 전파 검증이다. CUDA 재측정은 불필요하고 probe panic 경고 개선은 선택 사항이다. 이번 CPU 작업에서는 범위만 확인했으며 GPU 기능을 빌드하거나 측정하지 않았다. [#127](https://github.com/x1zzdev/Xazz/issues/127#issuecomment-5834268145)·[#129](https://github.com/x1zzdev/Xazz/issues/129#issuecomment-5834268356)는 보류를 유지한다.

## 환경과 자원 판단

- Windows 11 Enterprise, build 26200, x86_64.
- Intel Core Ultra 9 185H, 물리 16코어 / 논리 22개.
- OS가 보고한 RAM 68,186,861,568바이트(약 63.5GiB), 시작 시 가용 약 45.3GiB.
- C: 시작 시 여유 1,677,242,372,096바이트(약 1.53TiB).
- NVIDIA GeForce RTX 4070 Laptop GPU, VRAM 8,188MiB, 드라이버 591.44. CPU 측정의 메모리 판단에는 시스템 RAM을 사용했다.
- Rust 1.98.0, `stable-x86_64-pc-windows-gnu`, 기본 CPU 기능. Python 3.12.12, pandas 2.3.3, NumPy 2.3.5, psutil 7.2.2.

맥의 4M RSS를 단순 환산한 pandas 약 59GiB만으로는 실행 여유가 부족했다. 이 PC에서 자원 검토용으로 4,089,216행과 이를 5회 반복한 20,446,080행을 읽었다. 후자의 관측 RSS는 약 2,898MiB였고, 200M까지 단순 환산하면 약 27.7GiB였다. 이 계산은 실제 메모리 상한이나 200M 성능 결과가 아니다. 자원 검토는 Rust 빌드와 겹쳤으므로 그 시간 수치는 성능 비교에 사용하지 않는다.

Xazz도 같은 20,446,080행으로 on/off 각각 워밍업 1회·측정 1회를 실행했다. 워밍업을 포함한 최대 관측 RSS는 on 1,787.7MiB, off 2,300.6MiB였다. 200M까지 선형 환산한 약 17.1GiB·22.0GiB도 실행 여부를 판단하기 위한 추정일 뿐이다.

수정 검증과 빌드가 끝난 뒤 200M 시작 직전 가용 RAM은 46,307,479,552바이트(약 43.1GiB), C: 여유는 약 1.50TiB였다. 실행당 프로세스 트리 RSS 32,768MiB, 시간 900초의 감시 제한을 적용했다. 3ms 폴링 제한은 OS의 강제 메모리 격리가 아니며 순간 초과를 완전히 막지 못한다. Windows의 RSS는 working set이므로 가상 메모리·commit 크기나 전체 메모리 사용량 상한을 뜻하지 않는다.

## 수정한 실패 계약

1. 스트리밍 collect가 실패하면 원래 오류를 반환한다. 일반 메모리 collect로 재시도하지 않는다. 자동 모드에서 스트리밍을 선택한 경우도 같다.
2. 파이프라인 노드 오류는 첫 실패에서 실패 종료 코드로 전파하며 뒤 단계와 성공 타이밍·최종 결과 출력을 중단한다. 오류 전에 이미 수행한 외부 저장 등의 부수효과까지 취소하는 트랜잭션은 아니다. 별도 `--output` 저장 오류를 포함한 모든 오류 경로를 수정한 것은 아니다.
3. `XAZZ_STREAMING`은 미설정/`auto`, `1`/`true`, `0`/`false`를 허용한다. 잘못된 설정을 조용히 off로 바꾸지 않는다.
4. `POLARS_AUTO_NEW_STREAMING=1` 또는 `POLARS_FORCE_NEW_STREAMING=1`은 Polars 내부에서 선택한 엔진을 우회할 수 있어 런타임과 측정기 모두 명시적으로 거부한다. 환경값을 몰래 지우거나 바꾸지 않는다.
5. `XAZZ_COLLECT_DIAGNOSTICS=1`이면 성공한 수집마다 `[xazz:collect]` JSON을 출력한다. 측정기는 워밍업과 본 측정 모두 정확히 네 개의 마커와 요청 모드·엔진·성공 상태를 검사한다. 누락·잘못된 JSON·중복 키·모드 불일치도 실패다.
6. 마커는 런타임이 호출하고 완료한 Polars 수집 API의 증거다. Polars 내부의 모든 노드가 스트리밍으로 실행됐다는 뜻이 아니므로 `streaming_actual_engine_verified=false`를 유지한다. 이름 붙은 P2 결과도 여전히 메모리에 보관한다.
7. Windows 로그를 UTF-8로 읽고 pandas 자식 출력도 UTF-8로 지정한다. 테스트 실행 파일의 `.exe` 경로와 UTF-8 결과 읽기를 수정했다.

성공 JSON은 모든 요청 측정이 성공해야 교체한다. 원시 stdout/stderr, 프로세스 측정 JSON, 실패 기록은 `target/qa-166/`에 보관한다. 내부 실행시간 대신 wall-clock을 쓰거나 실패 후 낮은 스케일의 수치를 대체 결과로 기록하지 않는다.

## 검증과 실측 상태

최초 실측 당시 소스에서 기본 CPU release 실행 파일 세 개를 새로 빌드했다. 빌드 소요 시간은 19분 47초였다. 버전 문자열은 `xazz 0.3.1`이지만 공개 배포 실행 파일과 구분해야 한다. 결과 JSON에는 미커밋 작업 트리 상태, 런타임 소스·측정 코드·파이프라인·입력 CSV·세 실행 파일의 SHA-256을 기록했다. 아래 성능 표는 후속 CSV 저장 오류·서버 수정을 포함한 최신 소스를 재측정한 결과가 아니다.

GNU 기본 debug 빌드의 실행 엔진 테스트 파일은 3,509,291,924바이트였고 Windows 로더 오류 193으로 실행되지 않았다. [기존 환경 기록](design/gpu-backend-acceptance.md)의 동일 문제와 대조한 뒤 `CARGO_PROFILE_DEV_DEBUG=0`으로 디버그 정보만 제외해 재빌드했다. 테스트와 debug assertion은 유지하며 실행 파일을 임의로 패치하지 않았다. 최초 C++ 컴파일러 권한 거부, 병렬도 조정에 따른 중단, 기본 debug 실행 실패 기록도 보존했다.

- Rust 전체 병렬 테스트: **669개 통과, 서버 감사 체인 검사 2개 실패, 제외 0개**. `inference_check_blocks_leaked_secret`·`inference_check_passes_clean_response`가 실패했다. 이번에 수정한 스트리밍 테스트 5개, 오류 전파 통합 테스트 2개, DP 검사 5개는 모두 통과했다.
- 같은 전체 테스트를 `--test-threads=1`로 비교 실행: **671개 통과, 실패·제외 0개**. 이 결과로 병렬 실패가 해결됐다고 주장하지 않는다. 별도 임시 파일에서는 배타적 파일 잠금 중 읽기가 Windows 오류 33으로 실패하고 잠금 해제 후 성공하는 경합을 재현했다. 서버가 읽기 오류를 `unwrap_or(false)`로 체인 손상과 합치는 경로도 확인했다. 다만 최초 두 실패의 원래 읽기 오류는 보존되지 않아 그 원인을 오류 33으로 확정하지 않는다. 서버 코드와 기존 감사로그를 이 진단을 위해 수정·삭제하지 않았다.
- `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check` 통과. Clippy와 테스트에는 위 디버그 정보 설정을 적용했다.

Python 최종 전체 테스트는 61개 중 60개 통과, 실패 0개였다. Windows에 적용할 수 없는 기존 POSIX 프로세스 그룹 테스트 1개는 제외됐다. Windows에서도 관측한 자식 프로세스 종료, 제한시간·RSS 초과, 잔존 자식, 실패 파일 보존 검사를 실행했다. 초기 실행의 Windows 경로·인코딩 실패 로그는 수정 후 로그와 함께 보존했다. 원본 결과 JSON을 수정하지 않고 보관 로그를 다른 경로에서 대조하는 `--logs-dir` 옵션도 검사했다.

독립 대조기는 모든 CSV 행·문자열·해시·집계를 검사한다. 정확히 같은 숫자 문자열의 엄격한 파싱 결과만 최대 8,192개 캐시에 재사용하며 숫자를 보정하지 않는다. 이 최적화 전후 12행 고정 정답과 실제 small 전체 계산 JSON이 완전히 동일함을 확인했다. 시간·RSS·중앙값과 수집 API 진단 검사는 측정기가 담당하며, 독립 대조기의 `status=ok`를 이 모든 항목의 재검증으로 확대하지 않는다.

Debug 실행 파일로 227,760행을 워밍업 1회·측정 1회씩 pandas/on/off, 총 6회 실행해 CLI→수집 마커→측정기→독립 대조기의 연결을 확인했다. 이 실행은 release 빌드와 겹쳤고 최적화 설정도 달라 성능 결과로 사용하지 않는다. 첫 독립 대조는 샌드박스의 로그 디렉터리 접근 거부로 실패했으며 실패 보고서를 보존했다. 같은 사용자 권한에서 대조한 결과는 통과했다. 대조기는 디렉터리 접근 오류도 원래 오류로 보고하도록 수정했다.

### Release 실측

각 입력·엔진별 워밍업 1회 뒤 3회를 측정한 중앙값이다. 시간은 CSV 읽기·계산을 포함한 각 엔진의 내부 실행시간이며 프로세스 시작·import·결과 출력 시간은 제외한다. pandas 입력 형 검사도 시간에 포함된다. RSS는 실행별 프로세스 트리 RSS의 3ms 폴링 최댓값을 다시 3회 중앙값으로 요약했다. 시간과 RSS의 중앙값이 같은 실행에서 나온다는 뜻은 아니다.

기존 파이프라인의 읽기 구조를 유지했다. Xazz는 P2와 P7에서 각각 CSV를 읽고, pandas는 한 번 읽은 원시 프레임으로 네 결과를 만든다. 두 구현 모두 P2 결과를 보관한다. 이 비교가 같은 I/O 횟수나 입력 전체를 메모리에 보관하지 않는 실행을 뜻하지 않는다.

프로세스 전체 대기 시간은 표의 내부 계산 시간보다 길 수 있다. 런타임은 계산 직후 시간을 확정하고 결과를 출력한 뒤 함수 종료 시 큰 중간 프레임을 해제한다. 200M on 첫 본 측정은 내부 223.173초, 전체 프로세스 307.0초였다. 결과 출력 뒤 RSS가 감소하는 동안 프로세스가 살아 있는 것도 관측했다. 추가 시간에는 해제·출력·종료 등이 포함될 수 있으나 구간별 프로파일링을 하지 않았으므로 전부를 프레임 해제 비용으로 단정하지 않는다. P7 계산 자체의 지연 원인도 이번 측정으로 확정하지 않는다.

| 입력 행 수 | 엔진 / 요청 모드 | 시간(ms) | 관측 RSS(MiB) |
| ---: | --- | ---: | ---: |
| 227,760 | pandas | 115.9 | 102.0 |
| 227,760 | Xazz on | 59.2 | 65.1 |
| 227,760 | Xazz off | 63.4 | 61.2 |
| 911,662 | pandas | 416.1 | 197.7 |
| 911,662 | Xazz on | 123.9 | 125.7 |
| 911,662 | Xazz off | 133.3 | 140.4 |
| 4,089,216 | pandas | 1,849.0 | 639.8 |
| 4,089,216 | Xazz on | 936.1 | 403.8 |
| 4,089,216 | Xazz off | 1,050.5 | 481.4 |
| 200,371,584 | pandas | 91,344.0 | 27,684.5 |
| 200,371,584 | Xazz on | 237,889.2 | 16,796.2 |
| 200,371,584 | Xazz off | 203,319.0 | 22,305.7 |

small/medium/large 세 입력의 전체 행을 다시 계산하고 워밍업을 포함한 36회 실행을 대조했다. P2 행 수는 각각 151,346·619,170·2,983,785였으며 P3 26행, P4 10행, P7 5행과 Xazz 최종 P7 셀이 모두 일치했다. P2/P3/P4 전체 셀과 pandas P7 셀은 원시 출력에 없어 대조하지 않았다.

200,371,584행은 정규화한 large를 49회 반복한 6,505,447,832바이트 입력이다. 이 스케일의 워밍업 3회와 본 측정 9회가 모두 제한 안에서 성공했다. 내부 계산의 본 측정 범위는 pandas 87.590~95.285초, on 223.173~250.345초, off 190.767~210.268초였다. 실패 후 재실행하거나 느린 본 측정을 제외하지 않았다. 20M 자원 사전 측정의 시간을 200M 성능으로 대신 쓰지 않았다.

이 PC의 200M 결과에서는 pandas가 가장 빨랐다. on은 off보다 느렸지만 관측 RSS가 적었다. on의 RSS 중앙값은 약 16.4GiB, off는 약 21.8GiB, pandas는 약 27.0GiB였다. 일반 데스크톱 환경에서 얻은 CPU 수치이며 플랫폼·입력 검사가 다른 기존 README나 맥 수치와 직접 비교하지 않는다. OS 페이지 캐시나 열 상태를 고정한 전용 서버 측정도 아니다.

측정 직후 가용 RAM은 약 43.3GiB였다. 호스트 전체에서 psutil이 보고한 페이지 파일 사용량은 시작 전 89,174,016바이트에서 끝난 뒤 216,346,624바이트로 늘었다. 이 값만으로 측정 프로세스의 실제 page-out 양을 알 수 없으며, Windows에서 반환한 swap-in/out 0도 페이지 입출력이 없었다는 증거로 사용하지 않는다.

독립 대조기로 200,371,584행 전체를 다시 읽어 바이트 수·SHA-256·행 수·집계를 계산하고 12회 실행 로그와 대조했다. P2 146,205,465행, P3 26행, P4 10행, P7 5행과 모든 Xazz 최종 P7 셀이 일치했다. 출력되지 않는 P2/P3/P4 전체 셀과 pandas P7 셀은 대조하지 않았다. 워밍업과 본 측정의 네 수집 API 완료 마커는 측정기가 검증했다.

전체 네 스케일의 48회 실행에 해당하는 stdout/stderr 96개도 확인했다. 경고·오류 행이 없었고, 정책 JSON의 warnings·violations는 비어 있으며 safe_to_execute는 true였다. 측정에 기록한 소스 네 개와 실행 파일 세 개의 해시가 실측 직후 파일과 일치하는 것도 확인했다. 이후 수정한 소스와 현재 debug 실행 파일의 해시는 후속 기록에 별도로 남긴다.

## 보관한 결과와 검증 근거

- [227,760~4,089,216행 결과](../benches/results/windows-cpu-2026-10-06.json)와 [독립 대조 보고서](../benches/results/windows-cpu-2026-10-06.verification.json).
- [200,371,584행 결과](../benches/results/windows-cpu-200m-2026-10-06.json)와 [독립 대조 보고서](../benches/results/windows-cpu-200m-2026-10-06.verification.json).
- [원시 실행·검증 로그 묶음](../benches/results/windows-cpu-2026-10-06-evidence.zip): `small-large/`·`200m/`에 48회 실행 근거를, `validation/`에 초기 실패·중간 검사·최종 검증 로그를 보관했다. 189개 원본 파일을 바이트 변경 없이 압축했으며, `manifest.json`에 각 SHA-256과 독립 대조기 해시를 기록했다. 압축 무결성과 모든 원본 바이트의 일치를 확인했다.

결과 JSON은 측정 당시 절대경로와 작업 트리 상태를 그대로 보존한다. 다른 PC에서는 원본 JSON을 편집하는 대신 아래 `--logs-dir`로 압축을 푼 로그 경로를 지정한다. 보고서의 `raw_logs`는 실제 대조 경로이고 `recorded_raw_logs`는 원본 기록 경로다.

## 재현 명령

저장소 루트의 PowerShell에서 실제 LFS CSV가 준비된 상태로 실행한다. 아래 `python`은 Python 3.12.12를 뜻한다. 현재 변경을 포함해 새 실행 파일 세 개를 같은 디렉터리에 빌드해야 한다. 이전 실행 파일은 새 수집 마커가 없으면 측정기가 거부한다.

```powershell
python -m venv .venv-bench
./.venv-bench/Scripts/python.exe -m pip install pandas==2.3.3 numpy==2.3.5 psutil==7.2.2
$env:PYTHONIOENCODING = 'utf-8'
./.venv-bench/Scripts/python.exe -m unittest discover -s benches -p 'test_*.py' -v
cargo fmt --all -- --check
$env:CARGO_PROFILE_DEV_DEBUG = '0' # GNU Windows 실행 파일 크기 문제: 디버그 정보만 제외
cargo clippy --locked --workspace --all-targets -j 8 -- -D warnings
cargo test --locked --workspace -j 8
# 병렬 실패와 구분해서 기록한 비교 진단. 위 실패를 이 결과로 덮어쓰지 않는다.
cargo test --locked --workspace -j 8 -- --test-threads=1
Remove-Item Env:CARGO_PROFILE_DEV_DEBUG
cargo build --release --locked -p xazz -p xazz-runner -p xazz-exec -j 8
./.venv-bench/Scripts/python.exe benches/make_scale_data.py --xlarge --source-dir examples/data --chunk-rows 100000
./.venv-bench/Scripts/python.exe benches/run_readme_benchmark.py --streaming both --runs 3 --timeout-seconds 120 --max-memory-mb 4096 --out target/qa-166/windows-cpu-small-large.json
# 가용 RAM을 다시 확인한 뒤 별도로 실행한다.
./.venv-bench/Scripts/python.exe benches/run_readme_benchmark.py --scale xlarge --streaming both --runs 3 --timeout-seconds 900 --max-memory-mb 32768 --out target/qa-166/windows-cpu-200m.json
./.venv-bench/Scripts/python.exe benches/verify_benchmark_results.py --results target/qa-166/windows-cpu-small-large.json
./.venv-bench/Scripts/python.exe benches/verify_benchmark_results.py --results target/qa-166/windows-cpu-200m.json
```

보관된 실측 근거만 다시 대조하려면 동일한 입력 CSV를 생성한 뒤 다음을 실행한다. 원시 로그에는 측정 당시의 stdout/stderr와 프로세스 기록이 있으며 CSV와 실행 파일 자체는 압축에 넣지 않았다.

```powershell
Expand-Archive benches/results/windows-cpu-2026-10-06-evidence.zip -DestinationPath target/qa-166/restored-evidence
./.venv-bench/Scripts/python.exe benches/verify_benchmark_results.py --results benches/results/windows-cpu-2026-10-06.json --logs-dir target/qa-166/restored-evidence/small-large --out target/qa-166/restored-small-large.verification.json
./.venv-bench/Scripts/python.exe benches/verify_benchmark_results.py --results benches/results/windows-cpu-200m-2026-10-06.json --logs-dir target/qa-166/restored-evidence/200m --out target/qa-166/restored-200m.verification.json
```
