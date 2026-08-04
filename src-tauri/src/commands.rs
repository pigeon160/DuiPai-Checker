//! Tauri 薄胶水层：仅定义 IPC 命令，逻辑全部委托给 duipai-core。

use std::collections::HashMap;

use duipai_core::{eval_expr, parse, serialize, Config, DslError};

/// 连通性检查。
#[tauri::command]
pub fn ping() -> String {
    "pong".into()
}

/// DSL 文本 -> IR 配置。失败返回带行号的错误。
#[tauri::command]
pub fn dsl_parse(text: String) -> Result<Config, DslError> {
    parse(&text)
}

/// IR 配置 -> DSL 文本（规范化）。
#[tauri::command]
pub fn dsl_serialize(config: Config) -> Result<String, DslError> {
    serialize(&config)
}

/// 求值一个表达式（Phase 1 验证用；环境为 {变量名: 值}）。
/// 随机调用（int/float）使用线程随机数，Phase 3 引入种子。
#[tauri::command]
pub fn expr_eval(expr: String, env: HashMap<String, f64>) -> Result<f64, DslError> {
    let mut rng = rand::rng();
    eval_expr(&expr, &env, &mut rng)
}
