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
         - 顶层命令：`a = ints(n, 1, 100)`、`M = matrix(n, m, 0, 1)`、`p = perm(n)`、\n\
           `iv = intervals(n, 1, 100)`、`pt = points(n, 1, 10, 1, 10)`、`t = tree(n, int(1, 9))`、\n\
           `g = graph(n, m, 0, 0, int(1, 9))`（0/1 有向/连通，multi=1/loop=1/type=\"dag\"/\"bipartite\"）\n\
         - 多测：第一行注释 `# 多测模式：重复 T 次`\n\
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
         # 多测模式：重复 T 次\n\
         line:\n    int T: 1, 100\n\
         M = matrix(n, m, 0, 1)\n\n",
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
pub fn infer(req: &ModelInferRequest) -> Result<ModelInferResult, String> {
    #[cfg(feature = "nl-model")]
    {
        // TODO(nl-model)：加载 GGUF（llama-cpp-2），执行 build_prompt 推理，
        // 解析输出 -> parse + validate，失败重试 <=2 次。
        let _ = req;
        Err("模型通道尚未接入推理实现（nl-model 编译特性已启用，但推理代码待补）".to_string())
    }
    #[cfg(not(feature = "nl-model"))]
    {
        let _ = req;
        Err("模型通道未编译启用：需要 nl-model 特性（llama-cpp-2），且需放置 GGUF 模型文件".to_string())
    }
}

/// 校验模型路径是否存在（供加载前检查）。
pub fn check_model_path(path: &str) -> bool {
    std::path::Path::new(path).is_file()
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
}
