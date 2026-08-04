//! Tauri 薄胶水层：仅定义 IPC 命令，逻辑全部委托给 duipai-core。

use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use duipai_core::{
    eval_expr, generate, parse, run_check, serialize, validate, CheckEvent, CheckParams, Config,
    DslError,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

/// 捕获命令内部 panic，转成用户可见错误（防止任何输入导致整个应用闪退）。
fn safe<T>(f: impl FnOnce() -> Result<T, DslError>) -> Result<T, DslError> {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| {
        Err(DslError::bare(
            "内部错误：处理该输入时发生未预期异常（请将 DSL 内容反馈给开发者）",
        ))
    })
}

/// 共享状态：对拍取消标志 + 运行标志。
pub struct AppState {
    pub cancel: Arc<AtomicBool>,
    pub running: Arc<AtomicBool>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            cancel: Arc::new(AtomicBool::new(false)),
            running: Arc::new(AtomicBool::new(false)),
        }
    }
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

/// 求值一个表达式（验证用；环境为 {变量名: 值}）。
#[tauri::command]
pub fn expr_eval(expr: String, env: HashMap<String, f64>) -> Result<f64, DslError> {
    safe(|| {
        let mut rng = rand::rng();
        eval_expr(&expr, &env, &mut rng)
    })
}

/// 生成数据预览。seed 为 None 时随机。
#[tauri::command]
pub fn generate_data(config: Config, seed: Option<u64>) -> Result<String, DslError> {
    safe(|| {
        let lines = generate(&config, seed)?;
        Ok(lines.join("\n"))
    })
}

/// 保存文本到指定路径（导出数据）。
#[tauri::command]
pub fn save_text_file(path: String, content: String) -> Result<(), String> {
    std::fs::write(&path, content).map_err(|e| e.to_string())
}

/// 读取文本文件（源码预览）。
#[tauri::command]
pub fn read_text_file(path: String) -> Result<String, String> {
    std::fs::read_to_string(&path).map_err(|e| e.to_string())
}

/// 编译 C++ 源码。
#[tauri::command]
pub fn compile_program(
    source: String,
    workdir: String,
    name: String,
    compiler: String,
    flags: String,
) -> Result<String, DslError> {
    safe(|| duipai_core::compile_cpp(&source, &workdir, &name, &compiler, &flags))
}

/// 运行单个程序（试运行用）。
#[tauri::command]
pub fn run_program_ipc(
    cmd: String,
    dir: String,
    input: String,
    timeout: f64,
    memory_limit_mb: Option<u64>,
) -> Result<duipai_core::RunResult, String> {
    let r = duipai_core::run_program_ex(&cmd, &dir, input.as_bytes(), timeout, memory_limit_mb);
    Ok(r)
}

const CHECK_EVENT: &str = "check://event";

/// 启动对拍：后台线程跑循环，事件经 `check://event` 推送。
#[tauri::command]
pub fn duipai_start(
    app: AppHandle,
    state: State<'_, AppState>,
    params: CheckParams,
) -> Result<(), String> {
    if state.running.swap(true, Ordering::Relaxed) {
        return Err("对拍已在运行中，请先停止".to_string());
    }
    state.cancel.store(false, Ordering::Relaxed);
    let cancel = state.cancel.clone();
    let running = state.running.clone();
    let handler = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut emit = |e: CheckEvent| {
            let _ = handler.emit(CHECK_EVENT, e);
        };
        emit(CheckEvent::Log {
            msg: format!(
                "开始对拍：共 {} 组，超时 {}s",
                if params.total == -1 {
                    "无限".to_string()
                } else {
                    params.total.to_string()
                },
                params.timeout
            ),
        });
        emit(CheckEvent::Log {
            msg: format!("正解：{}", params.sol.cmd),
        });
        emit(CheckEvent::Log {
            msg: format!("暴力：{}", params.brute.cmd),
        });
        if let Some(ext) = &params.ext {
            emit(CheckEvent::Log {
                msg: format!("数据：外置生成器 {}", ext.cmd),
            });
        }
        run_check(&params, cancel.clone(), &mut emit);
        running.store(false, Ordering::Relaxed);
    });
    Ok(())
}

/// 请求停止对拍。
#[tauri::command]
pub fn duipai_cancel(state: State<'_, AppState>) {
    state.cancel.store(true, Ordering::Relaxed);
}

/// 对拍是否在运行。
#[tauri::command]
pub fn duipai_running(state: State<'_, AppState>) -> bool {
    state.running.load(Ordering::Relaxed)
}
