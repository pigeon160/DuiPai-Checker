//! duipai-core：对拍输入 DSL 核心库（解析 / 序列化 / 表达式求值）。
//!
//! 纯逻辑实现，不依赖任何 UI 框架，供 Tauri 后端与未来的生成器 / NLP 模块复用。

pub mod ast;
pub mod check;
pub mod error;
pub mod expr;
pub mod generator;
pub mod parser;
pub mod runner;
pub mod serializer;
pub mod validate;

pub use ast::{Config, ElemType, GraphType, Item, MultiPart, RepeatMode, VarKind, Weight};
pub use check::{
    finish_summary, run_check, CheckEvent, CheckParams, CheckStats, GenMode, ProgMode,
    ProgramSpec,
};
pub use error::{DslError, DslResult};
pub use expr::{collect_names, eval_expr, parse_expr, tokenize, EnvValue, ExprNode, Tok};
pub use generator::{format_float, generate};
pub use parser::{parse, KNOWN_COMMANDS};
pub use runner::{
    compare, compile_cpp, normalize, parse_command, run_argv, run_argv_ex, run_program,
    run_program_ex, RunResult, RunStatus,
};
pub use serializer::serialize;
pub use validate::validate;
