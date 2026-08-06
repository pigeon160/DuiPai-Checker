//! Tauri 薄胶水层：仅定义 IPC 命令，逻辑全部委托给 duipai-core。

use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use duipai_core::{
    check_model_path, eval_expr, generate, nl_to_dsl, parse, run_check, serialize, validate,
    CheckEvent, CheckParams, Config, DslError, ModelConfig, ModelStatus,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

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
        let env: HashMap<String, duipai_core::EnvValue> =
            env.into_iter().map(|(k, v)| (k, duipai_core::EnvValue::Scalar(v))).collect();
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

/// 用资源管理器打开目录（对拍失败现场一键查看）。
#[tauri::command]
pub fn open_dir(path: String) -> Result<(), String> {
    let p = std::path::Path::new(&path);
    if !p.exists() {
        return Err(format!("目录不存在：{path}"));
    }
    if !p.is_dir() {
        return Err(format!("不是目录：{path}"));
    }
    let mut cmd = std::process::Command::new("explorer");
    cmd.arg(&path);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("打开目录失败：{e}"))
}

/// 编译 C++ 源码（编译耗时可能数秒，放后台线程避免卡界面）。
#[tauri::command]
pub async fn compile_program(
    source: String,
    workdir: String,
    name: String,
    compiler: String,
    flags: String,
) -> Result<String, DslError> {
    tauri::async_runtime::spawn_blocking(move || {
        safe(|| duipai_core::compile_cpp(&source, &workdir, &name, &compiler, &flags))
    })
    .await
    .map_err(|e| DslError::bare(format!("编译线程异常：{e}")))?
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

// --------------------------------------------------------------------------- //
// 自然语言 → DSL
// --------------------------------------------------------------------------- //

/// 自然语言描述 -> DSL。`model_only=true` 时跳过规则引擎直接走模型。
/// 推理耗时长（3B 模型 10~30s），必须在后台线程执行，避免冻结界面。
#[tauri::command]
pub async fn nl_to_dsl_ipc(
    app: AppHandle,
    text: String,
    model_only: Option<bool>,
) -> Result<duipai_core::NlResult, String> {
    let cfg = load_config(&app)?;
    duipai_core::set_infer_threads(cfg.threads);
    tauri::async_runtime::spawn_blocking(move || {
        safe(|| Ok(duipai_core::nl_to_dsl_opt(&text, model_only.unwrap_or(false))))
            .map_err(|e| e.message)
    })
    .await
    .map_err(|e| format!("转换线程异常：{e}"))?
}

/// 模型配置 JSON 路径（应用配置目录 models/config.json）。
fn models_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("无法获取应用配置目录：{e}"))?
        .join("models");
    std::fs::create_dir_all(&dir).map_err(|e| format!("无法创建模型目录：{e}"))?;
    Ok(dir)
}

fn config_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(models_dir(app)?.join("config.json"))
}

fn load_config(app: &AppHandle) -> Result<ModelConfig, String> {
    let p = config_path(app)?;
    if !p.exists() {
        return Ok(ModelConfig::default());
    }
    let raw = std::fs::read_to_string(&p).map_err(|e| format!("读取模型配置失败：{e}"))?;
    serde_json::from_str(&raw).map_err(|e| format!("模型配置损坏：{e}"))
}

fn save_config(app: &AppHandle, cfg: &ModelConfig) -> Result<(), String> {
    let p = config_path(app)?;
    let raw = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    std::fs::write(&p, raw).map_err(|e| format!("保存模型配置失败：{e}"))
}

fn current_status(app: &AppHandle) -> Result<ModelStatus, String> {
    let cfg = load_config(app)?;
    let mut st = ModelStatus::default();
    st.path = cfg.path.clone();
    st.threads = cfg.threads;
    st.loaded = duipai_core::model_loaded();
    Ok(st)
}

/// 模型通道状态。
#[tauri::command]
pub fn model_status(app: AppHandle) -> Result<ModelStatus, String> {
    current_status(&app)
}

/// 设置模型文件路径并持久化。
#[tauri::command]
pub fn model_set_path(app: AppHandle, path: String) -> Result<ModelStatus, String> {
    if !path.trim().is_empty() && !check_model_path(&path) {
        return Err(format!("模型文件不存在：{path}"));
    }
    let mut cfg = load_config(&app)?;
    cfg.path = if path.trim().is_empty() { None } else { Some(path.trim().to_string()) };
    save_config(&app, &cfg)?;
    current_status(&app)
}

/// 设置推理线程数并持久化（None=自动留 2 核；Some(0)=全部核；Some(n)=指定 n）。即时生效。
#[tauri::command]
pub fn model_set_threads(app: AppHandle, threads: Option<u32>) -> Result<ModelStatus, String> {
    let mut cfg = load_config(&app)?;
    cfg.threads = threads;
    save_config(&app, &cfg)?;
    duipai_core::set_infer_threads(threads);
    current_status(&app)
}

/// 加载模型（nl-model 未启用时报错；启用后加载 GGUF）。
/// 加载 3B 模型需 10~30s，必须在后台线程执行，避免冻结界面。
#[tauri::command]
pub async fn model_load(app: AppHandle) -> Result<ModelStatus, String> {
    let cfg = load_config(&app)?;
    let path = cfg.path.clone().ok_or("未设置模型路径，请先设置或下载模型")?;
    if !check_model_path(&path) {
        return Err(format!("模型文件不存在：{path}"));
    }
    if !duipai_core::MODEL_AVAILABLE {
        return Err(
            "模型通道未编译启用（nl-model 特性）：当前仅规则引擎可用；模型推理需启用该特性并重新编译"
                .to_string(),
        );
    }
    tauri::async_runtime::spawn_blocking(move || duipai_core::model_load(&path))
        .await
        .map_err(|e| format!("加载线程异常：{e}"))??;
    current_status(&app)
}

const MODEL_EVENT: &str = "model://progress";

/// 下载模型文件（GitHub Releases 等）到 models/ 目录。
///
/// 用系统 curl（Windows 10+ 自带）。进度经 `model://progress` 推送：
/// `{stage: "start"|"progress"|"done"|"error", file, pct?, message}`。
#[tauri::command]
pub fn model_download(app: AppHandle, url: String) -> Result<String, String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("下载地址必须是 http(s):// 开头".to_string());
    }
    let dir = models_dir(&app)?;
    let name = url
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("model.gguf")
        .to_string();
    let dest = dir.join(&name);
    let dest_str = dest.to_string_lossy().to_string();
    let handler = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        use std::io::{BufRead, BufReader};
        use std::process::Stdio;
        let emit = |stage: &str, pct: Option<u64>, msg: &str| {
            let mut payload = serde_json::json!({ "stage": stage, "file": name, "message": msg });
            if let Some(p) = pct {
                payload["pct"] = serde_json::json!(p);
            }
            let _ = handler.emit(MODEL_EVENT, payload);
        };
        emit("start", None, &format!("开始下载 {name}…"));
        let mut builder = std::process::Command::new("curl");
        // --progress-bar：进度写入 stderr（\r 分隔，含 xx%）
        builder
            .args(["-L", "-f", "--progress-bar", "-o"])
            .arg(&dest)
            .arg(&url)
            .stderr(Stdio::piped());
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            builder.creation_flags(0x0800_0000);
        }
        let mut child = match builder.spawn() {
            Ok(c) => c,
            Err(e) => {
                emit("error", None, &format!("启动下载失败：{e}"));
                return;
            }
        };
        let mut last_pct: u64 = 0;
        if let Some(err) = child.stderr.take() {
            let reader = BufReader::new(err);
            for line in reader.lines() {
                let Ok(line) = line else { break };
                if let Some(idx) = line.rfind('%') {
                    if let Ok(pct) = line[..idx].trim().parse::<u64>() {
                        if pct >= last_pct + 5 || pct == 100 {
                            last_pct = pct;
                            emit("progress", Some(pct), &format!("下载中 {pct}%"));
                        }
                    }
                }
            }
        }
        match child.wait() {
            Ok(st) if st.success() => emit("done", Some(100), "下载完成"),
            Ok(_) => emit("error", None, "下载失败"),
            Err(e) => emit("error", None, &format!("下载失败：{e}")),
        }
    });
    Ok(dest_str)
}
