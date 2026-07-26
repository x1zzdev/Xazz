// xazz-compiler/src/error.rs — xazz-core::error 완전 재노출
//
// 에러 타입 정의는 xazz-core 크레이트로 이동했습니다.
// 내부 모듈(lexer, parser)의 `crate::error::*` import는
// 이 파일을 통해 그대로 동작합니다.
pub use xazz_core::error::*;
