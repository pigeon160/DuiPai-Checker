//! Tauri 薄胶水层：仅定义 IPC 命令，逻辑全部委托给 duipai-core。

use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};

use duipai_core::{eval_expr, parse, serialize, validate, Config, DslError};
use serde::Serialize;

/// 捕获命令内部 panic，转成用户可见错误（防止任何输入导致整个应用闪退）。
fn safe<T>(f: impl FnOnce() -> Result<T, DslError>) -> Result<T, DslError> {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| {
        Err(DslError::bare(
            "内部错误：处理该输入时发生未预期异常（请将 DSL 内容反馈给开发者）",
        ))
    })
}

/// 连通性检查。
#[tauri::command]
pub fn ping() -> String {
    "pong".into()
}

/// DSL 文本 -> IR 配置。失败返回带行号的错误。
#[tauri::command]
pub fn dsl_parse(text: String) -> Result<Config, DslError> {
    safe(|| parse(&text))
}

/// 解析 + 静态校验的结果（校验错误不阻断 IR 加载，供前端高亮错误行）。
#[derive(Serialize)]
pub struct ParseChecked {
    pub config: Config,
    pub errors: Vec<DslError>,
}

/// DSL 文本 -> IR 配置 + 校验错误列表。语法错误仍以 Err 返回。
#[tauri::command]
pub fn dsl_parse_checked(text: String) -> Result<ParseChecked, DslError> {
    safe(|| {
        let config = parse(&text)?;
        let errors = validate(&config);
        Ok(ParseChecked { config, errors })
    })
}

/// IR 配置 -> DSL 文本（规范化）。
#[tauri::command]
pub fn dsl_serialize(config: Config) -> Result<String, DslError> {
    safe(|| serialize(&config))
}

/// 求值一个表达式（Phase 1 验证用；环境为 {变量名: 值}）。
/// 随机调用（int/float）使用线程随机数，Phase 3 引入种子。
#[tauri::command]
pub fn expr_eval(expr: String, env: HashMap<String, f64>) -> Result<f64, DslError> {
    safe(|| {
        let mut rng = rand::rng();
        eval_expr(&expr, &env, &mut rng)
    })
}
