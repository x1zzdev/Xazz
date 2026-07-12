use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// x1zzLang 통합 CLI — 컴파일러 · 정적 분석 · Rust 에밋 · 합성 데이터 생성기
#[derive(Parser, Debug)]
#[command(
    name = "x1zz",
    version,
    author,
    about = "x1zzLang unified toolchain: run, check, emit, and generate synthetic data"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// x1zz 데이터 분석 코드를 실행합니다
    ///
    /// 예: x1zz run examples/poc_script.xzz
    /// 예: x1zz run examples/poc_script.xzz --predict
    /// 예: x1zz run examples/pipeline.xzz --output result.csv
    Run {
        /// 실행할 .xzz 소스 파일 경로
        file: PathBuf,

        /// 릴리즈 모드 최적화 활성화
        #[arg(short, long)]
        release: bool,

        /// Verbose 모드: Lexer 토큰 스트림 및 AST 출력 활성화
        #[arg(short, long)]
        verbose: bool,

        /// NQP (Neural Query Planner) 시맨틱 실행 예측 모드
        /// — 코드를 실행하지 않고 NQP 모델을 통해 예측 결과를 반환합니다
        #[arg(long)]
        predict: bool,

        /// 실행 결과를 CSV 파일로 저장합니다
        ///
        /// 예: --output result.csv
        #[arg(long)]
        output: Option<PathBuf>,
    },

    /// sLM 기반 정적 상태 분석을 수행합니다 (Neural Query Planner)
    ///
    /// 예: x1zz check examples/poc_script.xzz
    Check {
        /// 분석할 .xzz 소스 파일 경로
        file: PathBuf,
    },

    /// .xzz 스크립트를 다른 언어/형식으로 변환 출력합니다
    ///
    /// 예: x1zz emit rust examples/poc_script.xzz --out output.rs
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
    /// 예: x1zz sde --rows 5000 --output data/pairs/pairs.jsonl
    Sde {
        /// 생성할 데이터 행 수
        #[arg(long, default_value_t = 10000)]
        rows: usize,

        /// 출력 파일 경로
        #[arg(long, default_value = "data/pairs/pairs.jsonl")]
        output: PathBuf,
    },

    /// 새 x1zzLang 프로젝트를 생성합니다
    ///
    /// 예: x1zz new my-project
    New {
        /// 생성할 프로젝트 이름
        name: String,
    },

    /// CSV 파일을 읽어 타입 정의 및 load 문을 main.xzz에 추가합니다
    ///
    /// 예: x1zz import data/seoul_air.csv
    Import {
        /// 가져올 CSV 파일 경로
        file: String,
    },

    /// x1zz 사용자 프로필을 분석하고 아이덴티티를 확인합니다
    ///
    /// 예: x1zz whoami
    Whoami,
}
