use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// Xazz 통합 CLI — 컴파일러 · 정적 분석 · Rust 에밋 · 합성 데이터 생성기
#[derive(Parser, Debug)]
#[command(
    name = "xazz",
    version,
    author,
    about = "Xazz unified toolchain: run, check, emit, and generate synthetic data"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// xazz 데이터 분석 코드를 실행합니다
    ///
    /// 예: xazz run examples/poc_script.xzz
    /// 예: xazz run examples/pipeline.xzz --output result.csv
    Run {
        /// 실행할 .xzz 소스 파일 경로
        file: PathBuf,

        /// 릴리즈 모드 최적화 활성화
        #[arg(short, long)]
        release: bool,

        /// Verbose 모드: Lexer 토큰 스트림 및 AST 출력 활성화
        #[arg(short, long)]
        verbose: bool,

        /// 실행 결과를 CSV 파일로 저장합니다
        ///
        /// 예: --output result.csv
        #[arg(long)]
        output: Option<PathBuf>,

        /// 구조화된 JSON 실행 결과를 출력합니다 (기계 판독용)
        ///
        /// 예: xazz run examples/poc_script.xzz --json
        #[arg(long)]
        json: bool,
    },

    /// .xzz 코드를 실행 전에 정적 의미 분석(Type Checker)합니다
    ///
    /// 미선언 변수·모델·스키마, 스키마에 없는 컬럼, 타입 불일치 등을
    /// 실행 전에 검출합니다.
    ///
    /// 예: xazz check examples/poc_script.xzz
    /// 예: xazz check examples/poc_script.xzz --json
    Check {
        /// 분석할 .xzz 소스 파일 경로
        file: PathBuf,

        /// 구조화된 JSON 진단 결과를 출력합니다 (기계 판독용)
        #[arg(long)]
        json: bool,
    },

    /// .xzz 코드를 Policy-as-Code 보안 가드레일로 검사합니다 (issue #2)
    ///
    /// 개인정보 직접 노출·재식별 위험·하드코딩된 비밀키를 실행 전에 탐지하고,
    /// --fix 를 주면 안전한 대체 코드까지 함께 제안합니다.
    ///
    /// 예: xazz policy examples/security/patient_unsafe.xzz
    /// 예: xazz policy examples/security/patient_unsafe.xzz --fix
    /// 예: xazz policy pipeline.xzz --fix --out safe.xzz --json
    Policy {
        /// 검사할 .xzz 소스 파일 경로
        file: PathBuf,

        /// 구조화된 JSON 리포트를 출력합니다 (기계 판독용)
        #[arg(long)]
        json: bool,

        /// 위반을 자동 보정한 안전한 대체 코드를 함께 제안합니다
        #[arg(long)]
        fix: bool,

        /// 보정된 코드를 저장할 경로 (--fix 와 함께 사용)
        #[arg(long)]
        out: Option<PathBuf>,
    },

    /// .xzz 스크립트를 다른 언어/형식으로 변환 출력합니다
    ///
    /// 예: xazz emit rust examples/poc_script.xzz --out output.rs
    Emit {
        /// 출력 형식 (현재 지원: rust)
        format: String,

        /// 변환할 .xzz 소스 파일 경로
        file: PathBuf,

        /// 출력 파일 경로 (미지정 시 stdout으로 출력)
        #[arg(short, long)]
        out: Option<PathBuf>,
    },

    /// 합성 학습 데이터 쌍(pairs)을 자동 생성합니다
    ///
    /// 예: xazz sde --rows 5000 --output data/pairs/pairs.jsonl
    Sde {
        /// 생성할 데이터 행 수
        #[arg(long, default_value_t = 10000)]
        rows: usize,

        /// 출력 파일 경로
        #[arg(long, default_value = "data/pairs/pairs.jsonl")]
        output: PathBuf,
    },

    /// 새 Xazz 프로젝트를 생성합니다
    ///
    /// 예: xazz new my-project
    New {
        /// 생성할 프로젝트 이름
        name: String,
    },

    /// CSV 파일을 읽어 타입 정의 및 load 문을 main.xzz에 추가합니다
    ///
    /// 예: xazz import data/seoul_air.csv
    Import {
        /// 가져올 CSV 파일 경로
        file: String,
    },

    /// xazz 사용자 프로필을 분석하고 아이덴티티를 확인합니다
    ///
    /// 예: xazz whoami
    Whoami,
}
