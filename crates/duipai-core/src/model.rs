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
    /// 推理线程数配置（None=自动留 2 核；Some(0)=全部核；Some(n)=指定）。
    pub threads: Option<u32>,
}

impl Default for ModelStatus {
    fn default() -> Self {
        Self {
            available: MODEL_AVAILABLE,
            path: None,
            loaded: false,
            busy: false,
            threads: None,
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
    /// 推理线程数：None = 自动（留 2 核给界面）；Some(0) = 全部核；Some(n) = 指定 n。
    pub threads: Option<u32>,
}

/// 按机器核数与配置计算推理线程数。
/// - None（自动）：`min(max(2, 核数 - 2), AUTO_MAX_THREADS)`，留 2 核给界面/系统
/// - Some(0)：全部核
/// - Some(n)：指定 n（下限 1）
///
/// `AUTO_MAX_THREADS` 为自动档安全上限：推理受内存带宽限制，线程数对速度影响小，
/// 降线程可减少带宽争抢与发热（3B q4 每 token 读约 1.9GB，2-4 线程已接近带宽上限）。
pub const AUTO_MAX_THREADS: u32 = 4;

pub fn compute_threads(avail: usize, cfg: Option<u32>) -> u32 {
    match cfg {
        None => (avail.saturating_sub(2) as u32).max(2).min(AUTO_MAX_THREADS),
        Some(0) => avail.max(1) as u32,
        Some(n) => n.max(1),
    }
}

/// 自动线程数（按本机核数）。
pub fn auto_threads() -> u32 {
    let avail = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    compute_threads(avail, None)
}

/// 推理线程数的全局生效值（由 IPC 在推理前按配置设置）。
static INFER_THREADS: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0);

/// 设置推理线程数（None 用自动策略；由前端配置驱动，推理前调用）。
pub fn set_infer_threads(cfg: Option<u32>) {
    let avail = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    INFER_THREADS.store(compute_threads(avail, cfg), std::sync::atomic::Ordering::Relaxed);
}

/// 当前生效的推理线程数。
pub fn infer_threads() -> u32 {
    let v = INFER_THREADS.load(std::sync::atomic::Ordering::Relaxed);
    if v == 0 {
        auto_threads()
    } else {
        v
    }
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
    /// 模型思维链（输出中「分析：」到「DSL：」之间的内容）。
    pub thought: String,
}

/// 预处理用户输入：剥离 LaTeX/HTML 排版符号，保留结构、变量与数值范围。
/// 仅影响喂给模型的文本，界面原文不变。
pub fn normalize_input(text: &str) -> String {
    let mut s = text
        .replace("&#95;", "_")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("&#39;", "'")
        .replace("&quot;", "\"");
    // 通用 HTML 数字实体（如 &#160;）
    let html_ent = regex::Regex::new(r"&#(\d+);").unwrap();
    s = html_ent
        .replace_all(&s, |caps: &regex::Captures| {
            let n: u32 = caps[1].parse().unwrap_or(0);
            char::from_u32(n).map(String::from).unwrap_or_default()
        })
        .into_owned();
    // 删除数学模式标记 $
    s = s.replace('$', "");
    // LaTeX 命令 → 可读符号
    for (cmd, rep) in [
        (r"\le", "<="),
        (r"\leq", "<="),
        (r"\ge", ">="),
        (r"\geq", ">="),
        (r"\cdot", "*"),
        (r"\times", "*"),
        (r"\dots", "..."),
        (r"\ldots", "..."),
        (r"\left", ""),
        (r"\right", ""),
    ] {
        s = s.replace(cmd, rep);
    }
    // 残留 LaTeX 命令（\operatorname 等）直接删除
    let latex_cmd = regex::Regex::new(r"\\[a-zA-Z]+\b").unwrap();
    s = latex_cmd.replace_all(&s, "").into_owned();
    // 下标空格合并：a _ 1 → a_1
    let sub = regex::Regex::new(r"\s*_\s*").unwrap();
    s = sub.replace_all(&s, "_").into_owned();
    // 折叠连续空白（保留换行）
    let ws = regex::Regex::new(r"[^\S\n]+").unwrap();
    s = ws.replace_all(&s, " ").into_owned();
    s.trim().to_string()
}

/// 把一次推理的原始输出追加写入模型同目录 `infer_log.txt`（诊断用）。
/// 失败静默忽略，不影响推理流程。
pub fn log_infer(model_path: &str, attempt: usize, raw: &str) {
    if model_path.is_empty() {
        return;
    }
    let dir = std::path::Path::new(model_path)
        .parent()
        .unwrap_or(std::path::Path::new("."));
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let entry = format!("\n=== [{ts}] attempt={attempt} ===\n--- raw ---\n{raw}\n--- end ---\n");
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("infer_log.txt"))
    {
        let _ = f.write_all(entry.as_bytes());
    }
}

/// 按行过滤 DSL 文本：DSL 语句行首必为纯 ASCII 字母/下划线
/// （`line:`/`repeat (n):`/`int x: ...`/`t = tree(...)`），而模型复读的题面
/// 文本行首是中文或符号（如「第一行包含…」「**重要**」）。遇非法行即截断，
/// 这样 echo 即使没有「描述：」标记也能切干净；中文 str/text 内容在行内不受影响。
fn keep_dsl_lines(s: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for line in s.lines() {
        let l = line.trim_start();
        if l.is_empty() {
            out.push(line);
            continue;
        }
        let first = l.chars().next().unwrap();
        if first.is_ascii_alphabetic() || first == '_' {
            out.push(line);
        } else {
            break;
        }
    }
    out.join("\n").trim().to_string()
}

/// 从生成文本提取 (思考链, DSL)：模型按「分析：…\nDSL：…」输出。
/// DSL 取第一个「DSL：/DSL:」标记后的内容，再截断到后续「描述/分析」续写前，
/// 并按行过滤 echo 污染；思考链为「DSL：」前的内容（去掉「分析：」前缀与 code fence）。
/// 无「DSL：」标记时整段视为 DSL（兼容旧格式），思考链为空。
pub fn extract_thought_and_dsl(raw: &str) -> (String, String) {
    let s = raw.trim();
    let mut head = s.to_string();
    let mut tail = String::new();
    let mut found = false;
    for marker in ["DSL：", "DSL:", "DSL :"] {
        if let Some(idx) = head.find(marker) {
            tail = head[idx + marker.len()..].to_string();
            head.truncate(idx);
            found = true;
            break;
        }
    }
    if !found {
        // 无「DSL：」标记 → 整段视为 DSL（兼容旧格式），思考链为空
        let clean = |x: &str| x.trim().trim_start_matches("```").trim().trim_end_matches("```").trim().to_string();
        return (String::new(), keep_dsl_lines(&clean(s)));
    }
    let clean = |x: &str| {
        let mut t = x.trim().to_string();
        if t.starts_with("```") {
            let body = t.trim_start_matches("```").trim();
            t = body
                .strip_suffix("```")
                .map(|b| b.to_string())
                .unwrap_or_else(|| body.to_string());
        }
        t.trim().to_string()
    };
    let mut tail = clean(&tail);
    // 防续写：截到「描述：/描述:/分析：」前
    if let Some(end) = tail
        .find("描述：")
        .or_else(|| tail.find("描述:"))
        .or_else(|| tail.find("分析："))
    {
        tail = tail[..end].trim().to_string();
    }
    // 按行过滤（根治 echo 污染），详见 keep_dsl_lines
    tail = keep_dsl_lines(&tail);
    let mut thought = clean(&head);
    // 模型复读示例「描述：…」而非思考链 → 视为无思考链
    if thought.starts_with("描述：") || thought.starts_with("描述:") {
        thought.clear();
    } else if let Some(stripped) = thought
        .strip_prefix("分析：")
        .or_else(|| thought.strip_prefix("分析:"))
    {
        thought = stripped.trim().to_string();
    }
    (thought, tail)
}

/// 示例库条目：关键词（中/英） + 自包含可解析的「描述 + DSL」段。
struct Example {
    /// 任一关键词命中输入（normalize 后小写文本 contains）即加分。
    kw: &'static [&'static str],
    /// 拼入 prompt 的示例体。
    body: &'static str,
}

/// 基础示例：始终拼入，不参与匹配。
const BASE_EXAMPLES: &[Example] = &[
    Example {
        kw: &[],
        body: r#"描述：第一行两个整数 n m，接下来 n 行每行两个整数 a b
分析：单测；首行两个规模变量 n m；随后 n 行每行两个整数。
DSL：
line:
    int n: 1, 100
    int m: 1, 100
line (n):
    int a: 1, 100
    int b: 1, 100"#,
    },
    Example {
        kw: &[],
        body: r#"描述：第二行包含 n 个整数 a_1, a_2, ..., a_n（1 <= a_i <= 10^9）
分析：单测；一行 n 个整数，用数组命令 a = ints(n, lo, hi)。
DSL：
line:
    int n: 1, 100
a = ints(n, 1, 1000000000)"#,
    },
    Example {
        kw: &[],
        body: r#"描述：多测。第一行一个整数 t，表示测试用例数。每个测试用例：第一行两个整数 n m，接下来 n 行每行两个整数 a b
分析：多测 t 组；每组首行规模 n m，随后 n 行每行两个整数，用 line (n) 放 repeat 块内。
DSL：
line:
    int t: 1, 1000
repeat (t):
    line:
        int n: 1, 100
        int m: 1, 100
    line (n):
        int a: 1, 100
        int b: 1, 100"#,
    },
];

/// 专题示例：按输入关键词动态挑选（最多 [MAX_PICK] 条）。
const EXAMPLES: &[Example] = &[
    Example {
        kw: &["父节点", "parent", "p_2"],
        body: r#"描述：多测。第一行 t - 测试用例数。每个测试用例：第一行一个整数 n - 顶点个数；第二行 n 个整数 a_1..a_n - 顶点的值；第三行 n-1 个整数 p_2..p_n，p_i 是 i 的父节点
分析：多测 t 组；每组是树，输入方式是父节点数组（一行 n-1 个数），用 type="parent"。
DSL：
line:
    int t: 1, 10000
repeat (t):
    line:
        int n: 3, 200000
    a = ints(n, 1, 1000000000)
    tr = tree(n, type="parent")"#,
    },
    Example {
        kw: &["树", "tree", "无向边", "顶点", "edge", "vertex"],
        body: r#"描述：第一行一个整数 n - 顶点个数，接下来 n-1 行每行两个整数 u v，表示一条无向边
分析：单测；树，输入方式是边列表（n-1 行每行两个整数）。
DSL：
line:
    int n: 2, 200000
t = tree(n)"#,
    },
    Example {
        kw: &["边权", "weight", "weighted"],
        body: r#"描述：n 个点的树，边权 1 到 9
DSL：
line:
    int n: 2, 1000
t = tree(n, int(1, 9))"#,
    },
    Example {
        kw: &["菊花", "星形", "链", "路径", "star", "chain"],
        body: r#"描述：n 个点的菊花树（星形），1 号点为中心
DSL：
line:
    int n: 2, 100000
t = tree(n, type="star")"#,
    },
    Example {
        kw: &["图", "graph", "无向"],
        body: r#"描述：第一行两个整数 n m - 点数和边数，接下来 m 行每行两个整数 u v，表示无向连通图的边
分析：单测；无向连通图，输入方式是边列表（m 行每行两个整数）。
DSL：
line:
    int n: 1, 100000
    int m: 1, 200000
g = graph(n, m, 0, 0)"#,
    },
    Example {
        kw: &["有向", "directed"],
        body: r#"描述：第一行两个整数 n m，有向图，接下来 m 行每行两个整数 u v
DSL：
line:
    int n: 1, 100000
    int m: 1, 200000
g = graph(n, m, 1, 0)"#,
    },
    Example {
        kw: &["边权", "weight", "每条边有权"],
        body: r#"描述：第一行两个整数 n m，接下来 m 行每行三个整数 u v w，w 是边权 1 到 9
DSL：
line:
    int n: 1, 100000
    int m: 1, 200000
g = graph(n, m, 0, 0, int(1, 9))"#,
    },
    Example {
        kw: &["dag", "拓扑", "有向无环"],
        body: r#"描述：第一行两个整数 n m，DAG（有向无环图），边从小编号指向大编号
DSL：
line:
    int n: 1, 100000
    int m: 1, 200000
g = graph(n, m, 0, 0, type="dag")"#,
    },
    Example {
        kw: &["二分", "bipartite"],
        body: r#"描述：第一行两个整数 n m，二分图，接下来 m 行每行两个整数 u v
DSL：
line:
    int n: 2, 100000
    int m: 1, 200000
g = graph(n, m, 0, 0, type="bipartite")"#,
    },
    Example {
        kw: &["环", "圈", "ring"],
        body: r#"描述：n 个点的环，1 号到 n 号点依次相连
DSL：
line:
    int n: 3, 100000
r = ring(n)"#,
    },
    Example {
        kw: &["基环", "base"],
        body: r#"描述：n 个点的基环树，环上有 k 个点
DSL：
line:
    int n: 3, 100000
    int k: 3, 100000
b = base_ring(n, k)"#,
    },
    Example {
        kw: &["0/1", "01 串", "二进制", "binary"],
        body: r#"描述：长度为 n 的 0/1 串，其中恰好 k 个 1
DSL：
line:
    int n: 1, 100000
    int k: 0, 100000
b = binseq(n, k)"#,
    },
    Example {
        kw: &["个点", "坐标", "points", "平面"],
        body: r#"描述：平面上的 n 个点，x 和 y 坐标都在 1 到 10^9
DSL：
line:
    int n: 1, 100000
pt = points(n, 1, 1000000000, 1, 1000000000)"#,
    },
    Example {
        kw: &["排列", "permutation"],
        body: r#"描述：第一行一个整数 n，第二行是 1 到 n 的一个排列
DSL：
line:
    int n: 1, 100000
p = perm(n)"#,
    },
    Example {
        kw: &["矩阵", "matrix", "grid"],
        body: r#"描述：第一行两个整数 n m，接下来 n 行每行 m 个整数，范围 0 到 1
DSL：
line:
    int n: 1, 1000
    int m: 1, 1000
M = matrix(n, m, 0, 1)"#,
    },
    Example {
        kw: &["浮点", "小数", "实数", "float", "real"],
        body: r#"描述：第一行两个浮点数 x y，范围 0 到 1
DSL：
line:
    float x: 0, 1
    float y: 0, 1"#,
    },
    Example {
        kw: &["字符串", "字符", "string", "text", "字母"],
        body: r#"描述：第一行一个整数 n，第二行一个长度为 n 的字符串 s
DSL：
line:
    int n: 1, 100
    str s: n, "ab""#,
    },
    Example {
        kw: &["区间", "interval"],
        body: r#"描述：n 个区间，端点范围 1 到 100
DSL：
line:
    int n: 1, 100
iv = intervals(n, 1, 100)"#,
    },
    Example {
        kw: &["test cases", "test case", "each test"],
        body: r#"Description: The first line contains an integer t - the number of test cases. Each test case: the first line contains an integer n; the second line contains n integers a_1, ..., a_n; the third line contains n-1 integers p_2, ..., p_n, where p_i is the parent of vertex i
Analysis: multiple test cases t; each case is a tree given as parent array (one line of n-1 numbers), use type="parent".
DSL:
line:
    int t: 1, 10000
repeat (t):
    line:
        int n: 3, 200000
    a = ints(n, 1, 1000000000)
    tr = tree(n, type="parent")"#,
    },
];

/// 匹配出的专题示例最大条数。
const MAX_PICK: usize = 5;

/// 计算一条示例命中输入的关键词个数。
fn score_example(ex: &Example, low: &str) -> usize {
    ex.kw.iter().filter(|k| low.contains(**k)).count()
}

/// 按关键词命中数排序挑选专题示例（无命中返回空）。
fn pick_examples(low: &str) -> Vec<&'static Example> {
    let mut v: Vec<(&'static Example, usize)> = EXAMPLES
        .iter()
        .map(|e| (e, score_example(e, low)))
        .filter(|(_, s)| *s > 0)
        .collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    v.into_iter().take(MAX_PICK).map(|(e, _)| e).collect()
}

/// few-shot 提示模板：DSL 语法说明 + 基础示例 + 按关键词挑选的专题示例 + 输出指令。
///
/// 模型推理时按 `示例… + 用户描述 + 只输出 DSL` 拼接。
pub fn build_prompt(text: &str, last_error: Option<&str>) -> String {
    let mut s = String::with_capacity(4096);
    s.push_str(
        r#"你是对拍数据生成器 DSL 翻译器。把输入格式的中文/英文描述翻译成 DSL。
DSL 规则：
- 行块 `line:` + 缩进子项 `int n: 1, 100` / `float x: 0, 1` / `text s: "---"` / `expr e: 2*n` / `str c: 10, "ab"`
- 重复行：`line (n):`（n 可省 = 1 行）
- 数组：`a = ints(n, 1, 100)`（一行 n 个整数，范围 1~100）或 `M = matrix(n, m, 0, 1)`（n 行 m 列）
- 一行 n 个整数 → 数组命令 `a = ints(n, lo, hi)`，不要用 `line (n): int a`
- repeat 块：`repeat (T):` + 缩进所有语句，整体重复 T 次；T 必须在块外的 line 里先定义；禁止嵌套 repeat（多测内每组多行用 line (n)）；块内不能重新定义块外已有的变量（只能引用）
- 顶层命令：`M = matrix(n, m, 0, 1)`、`p = perm(n)`、`b = binseq(n, k)`、`iv = intervals(n, 1, 100)`、`pt = points(n, 1, 10, 1, 10)`、`r = ring(n)`、`b = base_ring(n, k)`
- 树/图先判断输入方式：
  - 边列表（n-1 行每行两个整数 u v）→ `t = tree(n)`；带权 → `t = tree(n, int(1, 9))`
  - 父节点数组（一行 n-1 个整数 p_2..p_n，p_i 是 i 的父节点）→ `t = tree(n, type="parent")`（1 为根，一行输出 n-1 个父节点，第 i 个是节点 i+1 的父节点）
  - 无向图（m 行每行两个整数）→ `g = graph(n, m, 0, 0)`；带权 → `g = graph(n, m, 0, 0, int(1, 9))`；有向 → `g = graph(n, m, 1, 0)`
- 树/图只输出边或父节点，无规模行；只能引用前面定义的名字

描述里可能含 LaTeX 排版符号（$$$、\le、\cdot、下划线下标、^ 幂），它们只是排版不是数据：$$$n$$$ 就是 n，1 \le x \le 10^5 表示范围 [1, 100000]，2 \cdot 10^5 = 200000，10^9 = 1000000000。忽略这些符号，只提取结构：规模变量、每行格式、取值范围。

示例：
"#,
    );
    for ex in BASE_EXAMPLES {
        s.push_str(ex.body);
        s.push_str("\n\n");
    }
    let low = text.to_lowercase();
    for ex in pick_examples(&low) {
        s.push_str(ex.body);
        s.push_str("\n\n");
    }
    if let Some(e) = last_error {
        let e: String = e.chars().take(60).collect();
        s.push_str(&format!("上次输出解析失败：{e}。请修正后重新输出。\n"));
    }
    s.push_str(
        "注意：先写分析，再写 DSL。输出格式：\n\
         分析：<用中文简述输入结构：是否多测、每行几个数、是否数组、树/图类型与输入方式>\n\
         DSL：\n\
         <DSL 代码>\n\
         分析不超过 2 句；只输出 DSL 代码本身，不要复读示例或错误；变量名不能重复；多测内每组多行用 line (n)，禁止嵌套 repeat。\n",
    );
    s.push_str("描述：");
    s.push_str(text.trim());
    s.push_str("\nDSL：\n");
    s
}

/// 模型推理入口。
///
/// `nl-model` 未启用时返回 Err（由调用方降级到规则结果）。
/// 启用时：few-shot prompt → 生成 → 提取 DSL → parse + validate 校验 →
/// 失败用错误信息重试 1 次 → 返回 DSL + 置信度（首次 0.85，重试 0.75）。
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
    fn generate(inner: &mut Inner, prompt: &str, max_tokens: usize, threads: u32) -> Result<String, String> {
        let tokens = inner
            .model
            .str_to_token(prompt, AddBos::Always)
            .map_err(|e| format!("tokenize 失败：{e}"))?;

        let ctx_params = LlamaContextParams::default()
            .with_n_batch(4096)
            .with_n_threads(threads as i32)
            .with_n_threads_batch(threads as i32)
            // n_ctx 3072：prompt（规则+示例+输入）~2500 token + 生成 384，够用；
            // 3B 的 KV cache 从 ~1.2GB 降到 ~0.9GB，内存带宽争抢随之下降
            .with_n_ctx(std::num::NonZeroU32::new(3072));

        let mut context = inner
            .model
            .new_context(&inner.backend, ctx_params)
            .map_err(|e| format!("创建推理上下文失败：{e}"))?;

        let eos = inner.model.token_eos();

        let mut batch = LlamaBatch::new(tokens.len() + 512, 1);
        for (i, t) in tokens.iter().enumerate() {
            // 最后一个 prompt token 需要 logits（供首次采样）
            batch
                .add(*t, i as i32, &[0], i == tokens.len() - 1)
                .map_err(|e| format!("batch 添加失败：{e}"))?;
        }
        let mut sampler = LlamaSampler::greedy();
        let mut generated: Vec<_> = Vec::new();
        let mut pos = tokens.len() as i32;
        let mut last_idx: i32 = tokens.len() as i32 - 1;
        for _ in 0..max_tokens {
            context
                .decode(&mut batch)
                .map_err(|e| format!("推理失败：{e}"))?;
            let next = sampler.sample(&context, last_idx);
            if next == eos {
                break;
            }
            generated.push(next);
            batch.clear();
            batch
                .add(next, pos, &[0], true)
                .map_err(|e| format!("batch 添加失败：{e}"))?;
            last_idx = 0;
            pos += 1;
        }
        // 采样器状态清理（重复惩罚等）
        sampler.accept_many(&generated);
        let mut decoder = encoding_rs::UTF_8.new_decoder();
        let mut text = String::new();
        for t in &generated {
            // special=true：特殊 token 也转文本；未知类型跳过（不中断）
            if let Ok(piece) = inner.model.token_to_piece(*t, &mut decoder, true, None) {
                text.push_str(&piece);
            }
        }
        Ok(text)
    }

    /// 轻量修复模型输出：第 `line` 行的行内项名与前面重复时自动改名（a → a2）。
    fn fix_dup_name(dsl: &str, line: usize) -> Option<String> {
        let lines: Vec<&str> = dsl.lines().collect();
        let idx = line.checked_sub(1)?;
        let l = *lines.get(idx)?;
        let re = regex::Regex::new(r"^(\s*)(int|float|str|text|expr)\s+([A-Za-z_]\w*):").ok()?;
        let cap = re.captures(l)?;
        let name = cap.get(3)?.as_str();
        let mut used: Vec<String> = lines
            .iter()
            .filter_map(|x| re.captures(x).map(|c| c.get(3).unwrap().as_str().to_string()))
            .collect();
        let mut i = 2;
        let mut cand = format!("{name}{i}");
        while used.contains(&cand) {
            i += 1;
            cand = format!("{name}{i}");
        }
        let fixed = l.replacen(&format!("{name}:"), &format!("{cand}:"), 1);
        let mut out = lines[..idx].join("\n");
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&fixed);
        if idx + 1 < lines.len() {
            out.push('\n');
            out.push_str(&lines[idx + 1..].join("\n"));
        }
        Some(out)
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
            let text = crate::model::normalize_input(&req.text);
            for attempt in 0..2usize {
                let prompt = build_prompt(&text, last_err.as_deref());
                let raw = generate(inner, &prompt, 384, crate::model::infer_threads())?;
                crate::model::log_infer(&inner.path, attempt, &raw);

                let (thought, dsl0) = crate::model::extract_thought_and_dsl(&raw);
                let mut dsl = dsl0;
                if dsl.is_empty() {
                    last_err = Some("输出为空".to_string());
                    continue;
                }
                // 重名轻量修复：模型常见「变量名重复」错误 → 逐行改名后重新校验
                for _ in 0..4 {
                    match crate::parser::parse(&dsl) {
                        Ok(cfg) => {
                            let errs = crate::validate::validate(&cfg);
                            if errs.is_empty() {
                                return Ok(ModelInferResult {
                                    dsl,
                                    confidence: 0.85 - attempt as f64 * 0.1,
                                    thought,
                                });
                            }
                            last_err = Some(format!("DSL 语义错误：{}", errs[0].message));
                            break;
                        }
                        Err(e) if e.message.contains("变量名重复") && e.line.is_some() => {
                            if let Some(fixed) = fix_dup_name(&dsl, e.line.unwrap()) {
                                dsl = fixed;
                                continue;
                            }
                            last_err = Some(format!("DSL 解析错误：{}", e.message));
                            break;
                        }
                        Err(e) => {
                            last_err = Some(format!("DSL 解析错误：{}", e.message));
                            break;
                        }
                    }
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
    fn prompt_base_examples_always_present() {
        let p = build_prompt("无关输入内容", None);
        assert!(p.contains("line (n):"), "{p}");
        assert!(p.contains("a = ints(n, 1, 1000000000)"), "{p}");
        assert!(p.contains("repeat (t):"), "{p}");
        assert!(p.contains("$$$n$$$"), "{p}");
        assert!(p.contains("禁止嵌套 repeat"), "{p}");
        assert!(p.contains("不能重新定义块外已有的变量"), "{p}");
        assert!(p.contains("分析："), "{p}");
        assert!(p.ends_with("DSL：\n"), "{p}");
        assert!(!p.contains("第二行是 1 到 n 的一个排列"), "无关输入不应带排列示例");
    }

    #[test]
    fn extract_thought_and_dsl_basic() {
        let (t, d) =
            extract_thought_and_dsl("分析：多测 t 组，每组是父节点数组。\nDSL：\nline:\n    int t: 1, 100");
        assert_eq!(t, "多测 t 组，每组是父节点数组。");
        assert!(d.contains("line:"), "{d}");
    }

    #[test]
    fn extract_thought_with_desc_word_not_mis_truncated() {
        let (t, d) = extract_thought_and_dsl(
            "分析：该描述包含 n 个整数数组。\nDSL：\na = ints(n, 1, 100)\n\n描述：续写示例",
        );
        assert_eq!(t, "该描述包含 n 个整数数组。");
        assert_eq!(d, "a = ints(n, 1, 100)");
    }

    #[test]
    fn extract_no_dsl_marker_falls_back() {
        let (t, d) = extract_thought_and_dsl("line:\n    int n: 1, 100");
        assert!(t.is_empty());
        assert!(d.contains("line:"), "{d}");
    }

    #[test]
    fn extract_plain_dsl_after_marker() {
        let (t, d) = extract_thought_and_dsl("分析：单测。\nDSL：\n```\nt = tree(n)\n```");
        assert_eq!(t, "单测。");
        assert_eq!(d, "t = tree(n)");
    }

    #[test]
    fn extract_trims_echo_without_desc_marker() {
        let raw = "line:\n    int t: 1, 10000\nrepeat (t):\n    line:\n        int n: 3, 200000\n    a = ints(n, 1, 1000000000)\n    tr = tree(n, type=\"parent\")\n\n保证所有测试用例的 n 之和不超过 2 * 10^5\n第一行包含一个整数 t";
        let (t, d) = extract_thought_and_dsl(raw);
        assert!(t.is_empty(), "无分析标记时思考链为空：{t}");
        assert!(d.contains("repeat (t):"), "{d}");
        assert!(d.contains("tr = tree(n, type=\"parent\")"), "{d}");
        assert!(!d.contains('上'), "不应混入 echo 中文：{d}");
    }

    #[test]
    fn extract_trims_symbol_echo() {
        let raw = "t = tree(n)\n\n**重要**\n第一行包含一个整数 n";
        let (_t, d) = extract_thought_and_dsl(raw);
        assert_eq!(d, "t = tree(n)");
    }

    #[test]
    fn extract_keeps_chinese_str_content() {
        let raw = "DSL：\nline:\n    int n: 1, 100\n    str s: n, \"abc中文\"";
        let (_t, d) = extract_thought_and_dsl(raw);
        assert!(d.contains("str s: n, \"abc中文\""), "{d}");
    }

    #[test]
    fn compute_threads_strategy() {
        assert_eq!(compute_threads(16, None), 4, "16 核自动档封顶 4");
        assert_eq!(compute_threads(8, None), 4, "8 核自动档封顶 4");
        assert_eq!(compute_threads(4, None), 2, "4 核自动留 2");
        assert_eq!(compute_threads(2, None), 2, "2 核下限 2");
        assert_eq!(compute_threads(1, None), 2, "1 核下限 2");
        assert_eq!(compute_threads(16, Some(0)), 16, "全部核");
        assert_eq!(compute_threads(8, Some(3)), 3, "指定 3");
        assert_eq!(compute_threads(8, Some(0)), 8, "Some(0) 表示全部核");
        assert_eq!(compute_threads(8, Some(14)), 14, "自定义不受上限限制");
    }

    #[test]
    fn config_roundtrip_with_threads() {
        let cfg = ModelConfig {
            path: Some("x.gguf".into()),
            download_url: None,
            threads: Some(4),
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let back: ModelConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.threads, Some(4));
        let def = ModelConfig::default();
        assert_eq!(def.threads, None);
    }

    #[test]
    fn infer_threads_global_defaults_to_auto() {
        let v = infer_threads();
        assert!(v >= 2, "默认至少 2 线程，实际 {v}");
        let avail = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
        assert!(v as usize <= avail, "线程数不应超过核数");
    }

    #[test]
    fn prompt_picks_tree_edge_example() {
        let p = build_prompt("n 个点的树，接下来 n-1 行每行两个整数 u v", None);
        assert!(p.contains("t = tree(n)"), "{p}");
    }

    #[test]
    fn prompt_picks_tree_weight_example() {
        let p = build_prompt("n 个点的树，边权 1 到 9", None);
        assert!(p.contains("t = tree(n, int(1, 9))"), "{p}");
    }

    #[test]
    fn prompt_picks_parent_example() {
        let p = build_prompt("第三行包含 n-1 个整数 p_2..p_n，p_i 是 i 的父节点", None);
        assert!(p.contains("tr = tree(n, type=\"parent\")"), "{p}");
        assert!(p.contains("一行输出 n-1 个父节点"), "{p}");
    }

    #[test]
    fn prompt_picks_directed_graph() {
        let p = build_prompt("第一行两个整数 n m，有向图，接下来 m 行每行两个整数 u v", None);
        assert!(p.contains("g = graph(n, m, 1, 0)"), "{p}");
    }

    #[test]
    fn prompt_picks_binseq_and_matrix() {
        let p = build_prompt("长度为 n 的 0/1 串，恰好 k 个 1；n 行 m 列的矩阵", None);
        assert!(p.contains("b = binseq(n, k)"), "{p}");
        assert!(p.contains("M = matrix(n, m, 0, 1)"), "{p}");
    }

    #[test]
    fn prompt_picks_english_example() {
        let p = build_prompt("The first line contains an integer t - the number of test cases", None);
        assert!(p.contains("Description:"), "{p}");
        assert!(p.contains("tr = tree(n, type=\"parent\")"), "{p}");
    }

    #[test]
    fn prompt_limited_to_max_pick() {
        let low = "树 图 有向 边权 父节点 二分 0/1 排列 矩阵 区间 坐标".to_lowercase();
        let picked = pick_examples(&low);
        assert!(picked.len() <= MAX_PICK, "挑选示例数 {} 超过上限", picked.len());
    }

    #[test]
    fn normalize_strips_latex() {
        let s = normalize_input(
            "第一行 $$$t$$$ ($$$1 \\le t \\le 10^4$$$) - 测试用例数，\
             接下来 $$$n$$$ 个整数 $$$a &#95; 1, a &#95; 2, \\dots, a &#95; n$$$ \
             ($$$1 \\le a _ i \\le 2 \\cdot 10^9$$$)",
        );
        assert!(!s.contains('$'), "{s}");
        assert!(s.contains("1 <= t <= 10^4"), "{s}");
        assert!(s.contains("a_1, a_2, ..., a_n"), "{s}");
        assert!(s.contains("1 <= a_i <= 2 * 10^9"), "{s}");
        assert!(!s.contains("\\le"), "{s}");
    }

    #[test]
    fn prompt_multi_example_has_t_outside_repeat() {
        let p = build_prompt("x", None);
        let pos = p.find("repeat (t):").expect("多测示例存在");
        let head = &p[..pos];
        assert!(head.contains("line:\n    int t: 1, 1000"), "t 必须在 repeat 块外定义");
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
            threads: None,
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
