//! 自然语言 → DSL 的本地大模型推理框架。
//!
//! 当前阶段实现「管理框架」：模型通道状态、配置持久化、few-shot 提示模板、
//! 推理接口占位。`nl-model` feature 开启时（需要 llama-cpp-2 依赖）才真正
//! 接入 GGUF 加载与推理；默认关闭，规则引擎（[`crate::nlg`]）承担转换。
//!
//! 推理管道（完整版）：规则未命中 → 模型推理 → parse + validate 校验 →
//! 失败重试 ≤2 次 → 输出 DSL + 置信度。

use serde::{Deserialize, Serialize};

/// 模型通道是否编译启用（`nl-model` feature）。
pub const MODEL_AVAILABLE: bool = cfg!(feature = "nl-model");

/// 模型通道状态（前端模型状态条）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelStatus {
    /// 模型通道是否编译启用（nl-model feature）。
    pub available: bool,
    /// 模型文件路径（未加载时为空）。
    pub path: Option<String>,
    /// 是否已加载。
    pub loaded: bool,
    /// 是否正在推理。
    pub busy: bool,
}

impl Default for ModelStatus {
    fn default() -> Self {
        Self {
            available: MODEL_AVAILABLE,
            path: None,
            loaded: false,
            busy: false,
        }
    }
}

impl ModelStatus {
    pub fn with_path(mut self, path: Option<String>) -> Self {
        self.path = path;
        self
    }

    pub fn is_ready(&self) -> bool {
        self.available && self.loaded && !self.busy
    }
}

/// 模型通道配置（持久化到应用配置目录 `models/config.json`）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelConfig {
    /// 模型文件路径。
    pub path: Option<String>,
    /// 下载地址（GitHub Releases 等；当前为占位配置）。
    pub download_url: Option<String>,
}

/// 模型推理请求。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInferRequest {
    /// 自然语言描述。
    pub text: String,
    /// 上次解析错误信息（重试提示用）。
    pub last_error: Option<String>,
}

/// 模型推理结果。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelInferResult {
    pub dsl: String,
    /// 模型自评置信度 0~1（依据输出格式与重试结果）。
    pub confidence: f64,
}

/// few-shot 提示模板：DSL 语法说明 + 示例 + 输出指令。
///
/// 模型推理时按 `示例（中文描述 + DSL）… + 用户描述 + 只输出 DSL` 拼接。
pub fn build_prompt(text: &str, last_error: Option<&str>) -> String {
    let mut s = String::with_capacity(2048);
    s.push_str(
        "你是对拍数据生成器 DSL 翻译器。把输入格式的中文/英文描述翻译成 DSL。\n\
         DSL 规则：\n\
         - 行块 `line:` + 缩进子项 `int n: 1, 100` / `float x: 0, 1` / `text s: \"---\"` / `expr e: 2*n` / `str c: 10, \"ab\"`\n\
         - 重复行：`line (n):`（n 可省 = 1 行）\n\
         - repeat 块：`repeat (T):` + 缩进所有语句，整体重复 T 次，变量每轮覆盖\n\
         - 顶层命令：`a = ints(n, 1, 100)`、`M = matrix(n, m, 0, 1)`、`p = perm(n)`、\n\
           `iv = intervals(n, 1, 100)`、`pt = points(n, 1, 10, 1, 10)`、`t = tree(n, int(1, 9))`、\n\
           `t = tree(n, type=\"parent\")`（1 为根，输出 n-1 行父节点）\n\
           `g = graph(n, m, 0, 0, int(1, 9))`（0/1 有向/连通，multi=1/loop=1/type=\"dag\"/\"bipartite\"）\n\
         - 树/图只输出边，无规模行；只能引用前面定义的名字\n\n\
         示例：\n\
         描述：第一行两个整数 n m，接下来 n 行每行两个整数 a b\n\
         DSL：\n\
         line:\n    int n: 1, 100\n    int m: 1, 100\n\
         line (n):\n    int a: 1, 100\n    int b: 1, 100\n\n\
         描述：n 个点的树，边权 1 到 10^9\n\
         DSL：\n\
         t = tree(n, int(1, 1000000000))\n\n\
         描述：多测，第一行 T，接下来 T 组，每组一个 n 行 m 列的矩阵\n\
         DSL：\n\
         repeat (t):\n\
         \x20   line:\n\x20   \x20   int t: 1, 100\n\
         \x20   M = matrix(n, m, 0, 1)\n\n",
    );
    if let Some(e) = last_error {
        s.push_str(&format!("上次输出解析失败：{e}。请修正后重新输出。\n"));
    }
    s.push_str("描述：");
    s.push_str(text.trim());
    s.push_str("\nDSL：\n");
    s
}

/// 模型推理入口。
///
/// `nl-model` 未启用时返回 Err（由调用方降级到规则结果）。
/// 启用时：few-shot prompt → 生成 → 提取 DSL → parse + validate 校验 →
/// 失败用错误信息重试 ≤2 次 → 返回 DSL + 置信度（首次 0.85，每次重试 -0.1）。
pub fn infer(req: &ModelInferRequest) -> Result<ModelInferResult, String> {
    #[cfg(feature = "nl-model")]
    {
        llm::infer(req)
    }
    #[cfg(not(feature = "nl-model"))]
    {
        let _ = req;
        Err("模型通道未编译启用：需要 nl-model 特性（llama-cpp-2），且需放置 GGUF 模型文件".to_string())
    }
}

/// 模型是否已加载（供管道判断是否走模型通道）。
pub fn model_loaded() -> bool {
    #[cfg(feature = "nl-model")]
    {
        llm::is_loaded()
    }
    #[cfg(not(feature = "nl-model"))]
    {
        false
    }
}

/// 加载模型（feature 未启用时报错）。
pub fn model_load(path: &str) -> Result<(), String> {
    #[cfg(feature = "nl-model")]
    {
        llm::load(path)
    }
    #[cfg(not(feature = "nl-model"))]
    {
        let _ = path;
        Err("模型通道未编译启用（nl-model 特性）：需要启用该特性重新编译".to_string())
    }
}

/// 卸载模型（feature 未启用时无操作）。
pub fn model_unload() {
    #[cfg(feature = "nl-model")]
    llm::unload();
    #[cfg(not(feature = "nl-model"))]
    {}
}

/// 校验模型路径是否存在（供加载前检查）。
pub fn check_model_path(path: &str) -> bool {
    std::path::Path::new(path).is_file()
}

// --------------------------------------------------------------------------- //
// llama.cpp 实现（nl-model feature 门控）
// --------------------------------------------------------------------------- //

#[cfg(feature = "nl-model")]
mod llm {
    use std::sync::Mutex;

    use llama_cpp_2::context::params::LlamaContextParams;
    use llama_cpp_2::llama_backend::LlamaBackend;
    use llama_cpp_2::llama_batch::LlamaBatch;
    use llama_cpp_2::model::{AddBos, LlamaModel};
    use llama_cpp_2::sampling::LlamaSampler;

    use crate::model::{build_prompt, ModelInferRequest, ModelInferResult};

    struct Inner {
        backend: LlamaBackend,
        model: LlamaModel,
        path: String,
        busy: bool,
    }

    static MANAGER: Mutex<Option<Inner>> = Mutex::new(None);

    pub fn is_loaded() -> bool {
        MANAGER.lock().map(|g| g.is_some()).unwrap_or(false)
    }

    pub fn load(path: &str) -> Result<(), String> {
        {
            let mut g = MANAGER.lock().map_err(|_| "模型管理器锁损坏".to_string())?;
            if let Some(inner) = g.as_ref() {
                if inner.path == path {
                    return Ok(());
                }
            }
            *g = None;
        }
        let backend = LlamaBackend::init().map_err(|e| format!("初始化推理后端失败：{e}"))?;
        let model = LlamaModel::load_from_file(&backend, std::path::Path::new(path), &Default::default())
            .map_err(|e| format!("加载模型失败：{e}"))?;
        let mut g = MANAGER.lock().map_err(|_| "模型管理器锁损坏".to_string())?;
        *g = Some(Inner {
            backend,
            model,
            path: path.to_string(),
            busy: false,
        });
        Ok(())
    }

    pub fn unload() {
        if let Ok(mut g) = MANAGER.lock() {
            *g = None;
        }
    }

    /// 生成一段文本（prompt → max_tokens 采样 → 文本）。
    fn generate(inner: &mut Inner, prompt: &str, max_tokens: usize) -> Result<String, String> {
        let tokens = inner
            .model
            .str_to_token(prompt, AddBos::Always)
            .map_err(|e| format!("tokenize 失败：{e}"))?;
        let mut context = inner
            .model
            .new_context(&inner.backend, LlamaContextParams::default())
            .map_err(|e| format!("创建推理上下文失败：{e}"))?;
        let eos = inner.model.token_eos();

        let mut batch = LlamaBatch::new(tokens.len() + 512, 1);
        for (i, t) in tokens.iter().enumerate() {
            batch
                .add(*t, i as i32, &[0], false)
                .map_err(|e| format!("batch 添加失败：{e}"))?;
        }
        let mut generated: Vec<_> = Vec::new();
        let mut pos = tokens.len() as i32;
        for _ in 0..max_tokens {
            context
                .decode(&mut batch)
                .map_err(|e| format!("推理失败：{e}"))?;
            let next = LlamaSampler::greedy().sample(&context, 0);
            if next == eos {
                break;
            }
            generated.push(next);
            batch.clear();
            batch
                .add(next, pos, &[0], true)
                .map_err(|e| format!("batch 添加失败：{e}"))?;
            pos += 1;
        }
        let mut decoder = encoding_rs::UTF_8.new_decoder();
        let mut text = String::new();
        for t in &generated {
            let piece = inner
                .model
                .token_to_piece(*t, &mut decoder, false, None)
                .map_err(|e| format!("解码失败：{e}"))?;
            text.push_str(&piece);
        }
        Ok(text)
    }

    /// 从生成文本提取 DSL：去掉 code fence 与「DSL：」前缀。
    fn extract_dsl(raw: &str) -> String {
        let mut s = raw.trim().to_string();
        // 模型有时会回显「DSL：」行
        if let Some(idx) = s.find("DSL：") {
            s = s[idx + 4..].to_string();
        } else if let Some(idx) = s.find("DSL:") {
            s = s[idx + 4..].to_string();
        }
        let s = s.trim().to_string();
        if s.starts_with("```") {
            let body = s.trim_start_matches("```").trim();
            let body = body
                .strip_suffix("```")
                .map(|b| b.to_string())
                .unwrap_or_else(|| body.to_string());
            return body.trim().to_string();
        }
        s
    }

    pub fn infer(req: &ModelInferRequest) -> Result<ModelInferResult, String> {
        let mut guard = MANAGER.lock().map_err(|_| "模型管理器锁损坏".to_string())?;
        let inner = guard.as_mut().ok_or("模型未加载")?;
        if inner.busy {
            return Err("模型正在推理中".to_string());
        }
        inner.busy = true;
        let result = (|| {
            let mut last_err = req.last_error.clone();
            for attempt in 0..3usize {
                let prompt = build_prompt(&req.text, last_err.as_deref());
                let raw = generate(inner, &prompt, 512)?;
                let dsl = extract_dsl(&raw);
                if dsl.is_empty() {
                    last_err = Some("输出为空".to_string());
                    continue;
                }
                match crate::parser::parse(&dsl) {
                    Ok(cfg) => {
                        let errs = crate::validate::validate(&cfg);
                        if errs.is_empty() {
                            return Ok(ModelInferResult {
                                dsl,
                                confidence: 0.85 - attempt as f64 * 0.1,
                            });
                        }
                        last_err = Some(format!("DSL 语义错误：{}", errs[0].message));
                    }
                    Err(e) => last_err = Some(format!("DSL 解析错误：{}", e.message)),
                }
            }
            Err(format!("模型推理重试后仍无法生成合法 DSL：{}", last_err.unwrap_or_default()))
        })();
        inner.busy = false;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_default_not_available_without_feature() {
        let s = ModelStatus::default();
        #[cfg(feature = "nl-model")]
        assert!(s.available);
        #[cfg(not(feature = "nl-model"))]
        assert!(!s.available);
        assert!(!s.loaded);
        assert!(!s.busy);
    }

    #[test]
    fn prompt_contains_examples() {
        let p = build_prompt("第一行一个整数 n", None);
        assert!(p.contains("line (n):"), "{p}");
        assert!(p.contains("t = tree(n, int(1, 1000000000))"), "{p}");
        assert!(p.contains("描述：第一行一个整数 n"), "{p}");
        assert!(p.ends_with("DSL：\n"), "{p}");
    }

    #[test]
    fn prompt_appends_last_error() {
        let p = build_prompt("x", Some("第 2 行解析失败"));
        assert!(p.contains("上次输出解析失败：第 2 行解析失败"), "{p}");
    }

    #[test]
    fn infer_disabled_without_feature() {
        let r = infer(&ModelInferRequest { text: "x".into(), last_error: None });
        #[cfg(not(feature = "nl-model"))]
        assert!(r.is_err());
        #[cfg(feature = "nl-model")]
        let _ = r; // TODO 实现后断言
    }

    #[test]
    fn config_roundtrip() {
        let c = ModelConfig {
            path: Some("models/ggml.gguf".into()),
            download_url: None,
        };
        let j = serde_json::to_string(&c).expect("to json");
        let back: ModelConfig = serde_json::from_str(&j).expect("from json");
        assert_eq!(c, back);
    }

    #[cfg(feature = "nl-model")]
    #[test]
    #[ignore = "需要 GGUF 模型文件：设置 DUIPAI_TEST_MODEL 环境变量指向 .gguf 后运行"]
    fn infer_end_to_end_with_model() {
        let path = std::env::var("DUIPAI_TEST_MODEL").expect("设置 DUIPAI_TEST_MODEL 指向 .gguf 后运行");
        model_load(&path).expect("加载模型");
        assert!(model_loaded());
        let r = infer(&ModelInferRequest {
            text: "第一行一个整数 n，接下来 n 行每行一个整数 a".into(),
            last_error: None,
        });
        let r = r.expect("推理成功");
        assert!(!r.dsl.is_empty(), "输出不应为空");
        crate::parser::parse(&r.dsl).expect("生成的 DSL 应可解析");
        model_unload();
        assert!(!model_loaded());
    }
}
