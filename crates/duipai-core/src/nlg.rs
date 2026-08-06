//! 自然语言 → DSL 规则引擎。
//!
//! 中英文模板匹配，把「第一行两个整数 n m，接下来 n 行每行一个整数」这类
//! 输入格式描述转成 DSL。规则命中返回高置信度结果（零延迟）；未命中返回
//! `None`，由上层管道决定走本地模型推理。
//!
//! 覆盖常见题型描述：多测 / 单行 / 重复行 / 数组 / 矩阵 / 树 / 图 / 排列 /
//! 区间 / 点集。范围写法支持 `(1<=n<=100)`、`n∈[1,100]`、`1 到 100`、
//! `不超过 10^9`、`int(1, 9)` 等。识别不完整的地方用默认值并在 `warnings` 提示。

use std::sync::OnceLock;

use regex::Regex;

use crate::error::DslResult;
use crate::parser::parse;
use crate::validate::validate;

/// 转换方法。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum NlMethod {
    /// 规则引擎（零延迟）
    Rule,
    /// 本地大模型推理
    Model,
}

impl NlMethod {
    pub fn label(self) -> &'static str {
        match self {
            NlMethod::Rule => "规则",
            NlMethod::Model => "模型",
        }
    }
}

/// 自然语言转换结果。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct NlResult {
    pub dsl: String,
    /// 置信度 0~1。
    pub confidence: f64,
    pub method: NlMethod,
    /// 非致命提示（默认值推断、无法识别的句子等）。
    pub warnings: Vec<String>,
    /// 模型思维链（模型通道转换时的「分析：」内容；规则引擎为空）。
    pub thought: String,
}

// --------------------------------------------------------------------------- //
// 中间结构
// --------------------------------------------------------------------------- //

#[derive(Debug, Clone, Copy, PartialEq)]
enum Elem {
    Int,
    Float,
    Str,
    Text,
}

/// 单个数项：`类型 名字: 范围`。
#[derive(Debug, Clone)]
struct NumItem {
    elem: Elem,
    name: String,
    lo: String,
    hi: String,
    prec: Option<String>,
}

/// 一个输出块。
#[derive(Debug, Clone)]
enum Block {
    /// 行块：rows = "1" 单行，否则重复行数表达式。
    Line { rows: String, items: Vec<NumItem> },
    /// 顶层命令（含多测注释）。
    Cmd(String),
}

/// 规则解析中间结果。
#[derive(Debug, Default)]
struct Parsed {
    blocks: Vec<Block>,
    /// 已用变量名（避免重名）。
    used: Vec<String>,
    warnings: Vec<String>,
    /// 是否有任何结构命中。
    hit: bool,
    /// 是否有默认推断（拉低置信度）。
    defaults: bool,
    /// 多测（repeat 块）计数变量名（定义在 repeat 块外）。
    repeat_var: Option<String>,
    /// 多测计数变量的定义行（repeat 块外的 line: int t）。
    repeat_count: Option<NumItem>,
    /// 隐式数量变量定义（树/图/数组等的 n/m，未显式定义时自动补行）。
    implicit: Vec<NumItem>,
}

impl Parsed {
    fn take_name(&mut self, pref: &str) -> String {
        for i in 1..1000 {
            let cand = if i == 1 { pref.to_string() } else { format!("{pref}{i}") };
            if !self.used.contains(&cand) {
                self.used.push(cand.clone());
                return cand;
            }
        }
        "v".to_string()
    }

    /// 命令变量名分配（t=tree、g=graph、M=matrix…）：固定名被占用时加序号（t2）。
    fn take_fixed(&mut self, pref: &str) -> String {
        self.take_name(pref)
    }

    fn name_used(&mut self, name: &str) {
        if !self.used.contains(&name.to_string()) {
            self.used.push(name.to_string());
        }
    }
}

// --------------------------------------------------------------------------- //
// 工具：数字 / 中文数字 / 范围提取
// --------------------------------------------------------------------------- //

/// `10^9` / `1e9` / `10**9` -> `1000000000`。
fn norm_num(s: &str) -> String {
    let s = s.trim();
    if let Some(p) = s.strip_prefix("10^").or_else(|| s.strip_prefix("10**")) {
        let p: usize = p.parse().unwrap_or(1);
        return format!("1{}", "0".repeat(p));
    }
    if let Some((mant, exp)) = s.split_once('e') {
        if let (Ok(m), Ok(e)) = (mant.parse::<f64>(), exp.parse::<i32>()) {
            let v = m * 10f64.powi(e);
            if v.fract() == 0.0 {
                return format!("{}", v as i64);
            }
            return v.to_string();
        }
    }
    s.to_string()
}

/// 中文数字（一~十、两）转数字字符串。
fn cn_num(s: &str) -> Option<&'static str> {
    Some(match s {
        "一" => "1",
        "两" | "二" => "2",
        "三" => "3",
        "四" => "4",
        "五" => "5",
        "六" => "6",
        "七" => "7",
        "八" => "8",
        "九" => "9",
        "十" => "10",
        _ => return None,
    })
}

/// 英文数字词（one~ten）→ 数字字符串。
fn en_num(s: &str) -> Option<&'static str> {
    Some(match s {
        "one" => "1",
        "two" => "2",
        "three" => "3",
        "four" => "4",
        "five" => "5",
        "six" => "6",
        "seven" => "7",
        "eight" => "8",
        "nine" => "9",
        "ten" => "10",
        _ => return None,
    })
}

/// 数字 / 中文数字 / 英文数字词 -> 数字字符串。
fn num_str(s: &str) -> Option<String> {
    let s = s.trim();
    if let Some(v) = cn_num(s) {
        return Some(v.to_string());
    }
    if let Some(v) = en_num(s) {
        return Some(v.to_string());
    }
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_digit() || c == '.') && s.chars().next().map_or(false, |c| c.is_ascii_digit()) {
        return Some(s.to_string());
    }
    None
}

fn is_name_str(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[derive(Debug, Clone)]
struct Range {
    lo: String,
    hi: String,
}

fn named_range_regexes() -> Vec<(String, String)> {
    static RX: OnceLock<Vec<(String, String)>> = OnceLock::new();
    RX.get_or_init(|| {
        let num = r"10\^\d+|10\*\*\d+|[0-9]+(?:\.[0-9]+)?e-?\d+|[0-9]+(?:\.[0-9]+)?";
        vec![
            // (lo <= v <= hi) / (1≤n≤100)
            (
                "bound".to_string(),
                format!(r"[（(]\s*(?P<lo>{num})\s*[≤<][=]?\s*(?P<v>[a-zA-Z_]\w*)\s*[≤<][=]?\s*(?P<hi>{num})\s*[）)]"),
            ),
            // v ∈ [lo, hi] / v in [lo, hi] / v 属于 [lo, hi]
            (
                "in".to_string(),
                r"(?P<v>[a-zA-Z_]\w*)\s*(?:∈|in|属于)\s*[\[(]\s*(?P<lo>[0-9]+(?:\.[0-9]+)?|10\^\d+|10\*\*\d+)\s*[,，]\s*(?P<hi>[0-9]+(?:\.[0-9]+)?|10\^\d+|10\*\*\d+)\s*[\])]".to_string(),
            ),
            // v 范围 lo 到 hi / lo <= v <= hi（无括号）
            (
                "range".to_string(),
                format!(r"(?P<v>[a-zA-Z_]\w*)\s*(?:取值范围|取值区间|范围|区间)?\s*(?:为|在|是)?\s*(?P<lo>{num})\s*(?:~|～|至|到)\s*(?P<hi>{num})"),
            ),
            // v 不超过 hi
            (
                "le".to_string(),
                format!(r"(?P<v>[a-zA-Z_]\w*)\s*(?:不超过|最多|至多|≤|<=|小于等于)\s*(?P<hi>{num})"),
            ),
        ]
    })
    .clone()
}

/// 无类型词行提取变量名（「first line contains n」「有一个 n」→ n）。
fn extract_var_name(s: &str) -> Option<String> {
    static RX: OnceLock<Regex> = OnceLock::new();
    let re = RX.get_or_init(|| Regex::new(r"[a-zA-Z_]\w*").expect("nlg var name regex"));
    for m in re.find_iter(s) {
        let w = m.as_str();
        if is_name_str(w)
            && !STOP_WORDS.contains(&w)
            && !matches!(
                w,
                "first" | "second" | "line" | "lines" | "contains" | "has" | "have" | "case"
                    | "cases" | "then" | "each" | "input" | "output" | "cnt" | "data"
            )
        {
            return Some(w.to_string());
        }
    }
    None
}

/// 无变量前缀的通用范围（「元素范围 1 到 100」），作为默认范围。
fn bare_range(s: &str) -> Option<Range> {
    static RX: OnceLock<Regex> = OnceLock::new();
    let re = RX.get_or_init(|| {
        let num = r"10\^\d+|10\*\*\d+|[0-9]+(?:\.[0-9]+)?e-?\d+|[0-9]+(?:\.[0-9]+)?";
        Regex::new(
            &format!(
                r"(?:范围|取值|值域|元素|边权|权值|大小|每个(?:元素|数|整数|点|边))\s*(?:为|在|是|是)?\s*(?P<lo>{num})\s*(?:~|～|至|到)\s*(?P<hi>{num})"
            ),
        )
        .expect("nlg bare range regex")
    });
    let cap = re.captures(s)?;
    Some(Range {
        lo: norm_num(cap.name("lo")?.as_str()),
        hi: norm_num(cap.name("hi")?.as_str()),
    })
}

/// `int(1, 9)` / `float(0, 1, 4)` 调用形式（边权等）。
fn weight_call(s: &str) -> Option<Range> {
    static RX: OnceLock<Regex> = OnceLock::new();
    let re = RX.get_or_init(|| {
        let num = r"10\^\d+|10\*\*\d+|[0-9]+(?:\.[0-9]+)?e-?\d+|[0-9]+(?:\.[0-9]+)?|[a-zA-Z_]\w*";
        Regex::new(
            &format!(
                r"(?:int|float)\s*\(\s*(?P<lo>{num})\s*,\s*(?P<hi>{num})\s*(?:,\s*[0-9]+(?:\.[0-9]+)?)?\s*\)"
            ),
        )
        .expect("nlg weight call regex")
    });
    let cap = re.captures(s)?;
    Some(Range {
        lo: norm_num(cap.name("lo")?.as_str()),
        hi: norm_num(cap.name("hi")?.as_str()),
    })
}

/// 无变量前缀的 `in [lo, hi]`（「N integers in [1, 10^9]」），作为默认范围。
fn bare_in_range(s: &str) -> Option<Range> {
    static RX: OnceLock<Regex> = OnceLock::new();
    let re = RX.get_or_init(|| {
        Regex::new(r"(?:∈|in|属于)\s*[\[(]\s*(?P<lo>[0-9]+(?:\.[0-9]+)?|10\^\d+|10\*\*\d+)\s*[,，]\s*(?P<hi>[0-9]+(?:\.[0-9]+)?|10\^\d+|10\*\*\d+)\s*[\])]").expect("nlg bare in regex")
    });
    let cap = re.captures(s)?;
    Some(Range {
        lo: norm_num(cap.name("lo")?.as_str()),
        hi: norm_num(cap.name("hi")?.as_str()),
    })
}

/// 从整段文本提取命名范围约束：变量 -> Range。
fn extract_ranges(s: &str) -> Vec<(String, Range)> {
    let mut out: Vec<(String, Range)> = Vec::new();
    for (_, pat) in named_range_regexes() {
        let re = Regex::new(&pat).expect("nlg range regex");
        for cap in re.captures_iter(s) {
            let v = cap.name("v").map(|m| m.as_str()).unwrap_or("");
            if !is_name_str(v) || is_type_word(v) {
                continue;
            }
            if out.iter().any(|(n, _)| n == v) {
                continue;
            }
            let lo = cap.name("lo").map(|m| norm_num(m.as_str())).unwrap_or_else(|| "1".into());
            let hi = cap.name("hi").map(|m| norm_num(m.as_str())).unwrap_or_else(|| "100".into());
            out.push((v.to_string(), Range { lo, hi }));
        }
    }
    // 无括号形式：lo <= v, w <= hi（多变量共界）；支持 10^ 形式与 3 段链 lo<=a<=b<=hi
    let num_any = r"10\^\d+|10\*\*\d+|[0-9]+(?:\.[0-9]+)?e-?\d+|[0-9]+(?:\.[0-9]+)?";
    let bare_bound = Regex::new(
        &format!(
            r"(?P<lo>{num_any})\s*[≤<][=]?\s*(?P<v>[a-zA-Z_]\w*(?:\s*[,，]\s*[a-zA-Z_]\w*)*)\s*[≤<][=]?\s*(?P<hi>{num_any})"
        ),
    )
    .expect("nlg bare bound regex");
    for cap in bare_bound.captures_iter(s) {
        let vars: Vec<&str> = cap
            .name("v")
            .map(|m| m.as_str().split([',', '，']).map(str::trim).collect())
            .unwrap_or_default();
        let lo = cap.name("lo").map(|m| norm_num(m.as_str())).unwrap_or_else(|| "1".into());
        let hi = cap.name("hi").map(|m| norm_num(m.as_str())).unwrap_or_else(|| "100".into());
        for v in vars {
            if is_name_str(v) && !is_type_word(v) && !out.iter().any(|(n, _)| n == v) {
                out.push((v.to_string(), Range { lo: lo.clone(), hi: hi.clone() }));
            }
        }
    }
    // 3 段链：lo <= a <= b <= hi（如 1<=l<=r<=10^9）→ a、b 同界
    let chain = Regex::new(
        &format!(
            r"(?P<lo>{num_any})\s*[≤<][=]?\s*(?P<a>[a-zA-Z_]\w*)\s*[≤<][=]?\s*(?P<b>[a-zA-Z_]\w*)\s*[≤<][=]?\s*(?P<hi>{num_any})"
        ),
    )
    .expect("nlg chain regex");
    for cap in chain.captures_iter(s) {
        let lo = cap.name("lo").map(|m| norm_num(m.as_str())).unwrap_or_else(|| "1".into());
        let hi = cap.name("hi").map(|m| norm_num(m.as_str())).unwrap_or_else(|| "100".into());
        for v in [cap.name("a"), cap.name("b")].into_iter().flatten() {
            let v = v.as_str();
            if is_name_str(v) && !is_type_word(v) && !out.iter().any(|(n, _)| n == v) {
                out.push((v.to_string(), Range { lo: lo.clone(), hi: hi.clone() }));
            }
        }
    }
    out
}

/// 是否常见类型词（不能作为范围约束的变量名）。
fn is_type_word(s: &str) -> bool {
    matches!(
        s,
        "int" | "integer" | "integers" | "float" | "floats" | "double" | "real"
            | "number" | "numbers" | "char" | "string" | "element" | "elements"
            | "long" | "longlong" | "data" | "case" | "cases" | "line" | "lines"
    )
}

// --------------------------------------------------------------------------- //
// 结构识别
// --------------------------------------------------------------------------- //

const DEFAULT_LO: &str = "1";
const DEFAULT_HI: &str = "100";

/// 识别类型词，返回 (类型, 类型词后方的变量名列表)。
fn detect_elem(s: &str) -> Option<(Elem, Vec<String>)> {
    let (start, len) = detect_elem_pos(s)?;
    let kind = &s[start..start + len];
    let elem = match kind {
        "浮点数" | "实数" | "小数" | "浮点" | "浮点型" | "real" | "float" | "floats" | "double" => {
            Elem::Float
        }
        "字符串" | "字符" | "字符型" | "string" | "char" => Elem::Str,
        "文本" | "text" => Elem::Text,
        _ => Elem::Int,
    };
    Some((elem, after_vars(s, start + len)))
}

/// 取类型词之后的变量名列表（最多 3 个）。
fn after_vars(s: &str, start: usize) -> Vec<String> {
    let tail = &s[start..];
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut i = 0usize;
    while i < tail.len() && out.len() < 3 {
        let c = tail[i..].chars().next().unwrap();
        if c.is_ascii_alphanumeric() || c == '_' {
            cur.push(c);
            i += c.len_utf8();
            continue;
        }
        if !cur.is_empty() {
            let name = std::mem::take(&mut cur);
            if is_name_str(&name) && !STOP_WORDS.contains(&name.as_str()) {
                out.push(name);
            }
        }
        if matches!(c, ',' | '，' | '、' | ' ' | '　') || c == '和' || c == '与' {
            i += c.len_utf8();
            continue;
        }
        break;
    }
    if !cur.is_empty() && is_name_str(&cur) && !STOP_WORDS.contains(&cur.as_str()) && out.len() < 3 {
        out.push(cur);
    }
    out
}

const STOP_WORDS: &[&str] = &[
    "and", "or", "the", "of", "with", "in", "to", "from", "each", "are", "is", "be", "then",
];

/// 识别「N 个 类型」或「N 个整数」的个数。
fn detect_count(s: &str) -> Option<String> {
    static RX: OnceLock<Regex> = OnceLock::new();
    let re = RX.get_or_init(|| {
        Regex::new(
            r"(?P<n>[a-zA-Z_]\w*|[0-9]+|[一二两三四五六七八九十]+)\s*(?P<unit>个|枚|只|位)?\s*(?P<type>整数|整型|浮点数|实数|小数|字符串|字符|数字|integer|integers|float|floats|real|number|numbers|char|string)",
        )
        .expect("nlg count regex")
    });
    for cap in re.captures_iter(s) {
        let n = cap.name("n")?.as_str();
        let unit = cap.name("unit").map(|m| m.as_str()).unwrap_or("");
        let ty = cap.name("type")?.as_str();
        // 精度语境（「3 位小数」「保留 3 位小数」）不算数量
        if ty == "小数" && unit == "位" {
            continue;
        }
        if let Some(v) = num_str(n) {
            return Some(v);
        }
        if is_name_str(n) {
            return Some(n.to_string());
        }
    }
    None
}

/// 浮点精度提取：「保留 3 位小数」「精度 3」「3 位小数」。
fn detect_prec(s: &str) -> Option<String> {
    static RX: OnceLock<Regex> = OnceLock::new();
    let re = RX.get_or_init(|| {
        Regex::new(r"(?:保留|精度|精确到|取|四舍五入到)?\s*(?P<p>[0-9]+)\s*(?:位|位小数|位有效数字|位精度)?(?:小数|精度)?").expect("nlg prec regex")
    });
    for cap in re.captures_iter(s) {
        let p = cap.name("p")?.as_str();
        if let Some(v) = num_str(p) {
            return Some(v);
        }
    }
    None
}

/// 行上下文。
#[derive(Debug, Clone, Copy, PartialEq)]
enum LineCtx {
    Single,
    Rows,
}

/// 从「接下来 N 行」提取行数表达式（支持 行/组/lines 单位）。
fn detect_rows_expr(s: &str) -> Option<String> {
    static RX: OnceLock<Regex> = OnceLock::new();
    let re = RX.get_or_init(|| {
        Regex::new(r"(?:接下来|后面|随后|之后|剩余|next|following|remaining|subsequent|共|总共|then)?\s*(?P<n>[a-zA-Z_]\w*|[0-9]+|[一二两三四五六七八九十]+)\s*(?:个|的)?\s*(?:行|组|lines?|查询)").expect("nlg rows regex")
    });
    if let Some(cap) = re.captures(s) {
        let n = cap.name("n")?.as_str();
        if let Some(v) = num_str(n) {
            return Some(v);
        }
        if is_name_str(n) {
            return Some(n.to_string());
        }
    }
    if s.contains("每行") || s.contains("每一行") || s.contains("each line") || s.contains("per line") {
        return Some("n".to_string());
    }
    None
}

/// 按「行指示词」把文本切成片段。
///
/// 每个片段以指示词开头（如「第一行」「接下来」「每行」），直到下一个指示词。
/// 返回 (片段文本, 行上下文)；无指示词的片段 ctx 为 None（用于主题/数组检测）。
fn segment_lines(text: &str) -> Vec<(String, Option<LineCtx>)> {
    static RX: OnceLock<Regex> = OnceLock::new();
    let re = RX.get_or_init(|| {
        Regex::new(r"(第一行|第\s*1\s*行|第1行|first line|first row|接下来|后面|随后|之后|剩余|每行|每一行|next|following|remaining|subsequent|then|each line|per line)").expect("nlg segment regex")
    });
    let marks: Vec<(usize, LineCtx)> = re
        .find_iter(text)
        .filter_map(|m| {
            let s = m.as_str();
            let ctx = if matches!(s, "第一行" | "第 1 行" | "第1行" | "first line" | "first row") {
                LineCtx::Single
            } else {
                LineCtx::Rows
            };
            Some((m.start(), ctx))
        })
        .collect();
    if marks.is_empty() {
        return vec![(text.to_string(), None)];
    }
    // 相邻两个 Rows 标记间距过近（字节 < 24，如「接下来 n 行每行」）视为同一片段
    let mut marks: Vec<(usize, LineCtx)> = marks;
    let mut i = 1;
    while i < marks.len() {
        if marks[i].1 == LineCtx::Rows && marks[i - 1].1 == LineCtx::Rows && marks[i].0 - marks[i - 1].0 < 24 {
            marks.remove(i);
        } else {
            i += 1;
        }
    }
    let mut out: Vec<(String, Option<LineCtx>)> = Vec::new();
    if marks[0].0 > 0 {
        out.push((text[..marks[0].0].to_string(), None));
    }
    for (idx, (pos, ctx)) in marks.iter().enumerate() {
        let end = marks.get(idx + 1).map(|(p, _)| *p).unwrap_or(text.len());
        out.push((text[*pos..end].to_string(), Some(*ctx)));
    }
    out
}

/// 多测检测：返回计数变量名（默认 "t"）。全文扫描以支持「第一行一个整数 T」。
fn detect_multi_test(text: &str) -> Option<String> {
    if !(text.contains("多测") || text.contains("多组") || text.contains("多次")
        || text.contains("多组数据") || text.contains("multiple test") || text.contains("test cases")
        || text.contains("T 组") || text.contains("t 组") || text.contains("每组"))
    {
        return None;
    }
    static RX: OnceLock<Regex> = OnceLock::new();
    let re = RX.get_or_init(|| {
        Regex::new(r"(?P<v>[tT])\s*(?:组|次|轮|个测试|组数据|test cases?|tests?)|第一行一个整数\s*(?P<w>[tT])").expect("nlg mt regex")
    });
    for cap in re.captures_iter(text) {
        if let Some(v) = cap.name("v").or_else(|| cap.name("w")) {
            // 保留原始大小写：「T 组」→ T（与命令变量 t=tree 不冲突）
            return Some(v.as_str().to_string());
        }
    }
    Some("t".to_string())
}

fn has_w(s: &str) -> bool {
    s.contains("边权") || s.contains("带权") || s.contains("权值") || s.contains("有权")
        || s.contains("weight") || s.contains("edge weight") || s.contains("每个边") || s.contains("每条边")
}

fn has_directed(s: &str) -> bool {
    s.contains("有向") || s.contains("directed") || s.contains("DAG") || s.contains("无环")
}

fn has_connected(s: &str) -> bool {
    s.contains("连通") || s.contains("connected")
}

fn detect_matrix_dims(s: &str) -> Option<(String, String)> {
    static RX: OnceLock<Regex> = OnceLock::new();
    let re = RX.get_or_init(|| {
        Regex::new(r"(?P<r>[a-zA-Z_]\w*|[0-9]+|[一二两三四五六七八九十]+)\s*行\s*(?P<c>[a-zA-Z_]\w*|[0-9]+|[一二两三四五六七八九十]+)\s*列").expect("nlg matrix regex")
    });
    let cap = re.captures(s)?;
    let r = cap.name("r")?;
    let c = cap.name("c")?;
    let conv = |v: &str| num_str(v).unwrap_or_else(|| v.to_string());
    Some((conv(r.as_str()), conv(c.as_str())))
}

// --------------------------------------------------------------------------- //
// 主解析
// --------------------------------------------------------------------------- //

/// 取某个变量的范围：(显式 > 通用默认 > 1~100)。
/// 返回 (lo, hi, used_default)。
fn resolve(
    name: &str,
    ranges: &[(String, Range)],
    bare: Option<&Range>,
) -> (String, String, bool) {
    if let Some((_, r)) = ranges.iter().find(|(n, _)| n == name) {
        return (r.lo.clone(), r.hi.clone(), false);
    }
    if let Some(r) = bare {
        return (r.lo.clone(), r.hi.clone(), true);
    }
    (DEFAULT_LO.to_string(), DEFAULT_HI.to_string(), true)
}

/// 把一段描述解析成 Parsed；无任何结构命中返回 None。
/// 主题数量：提取「N 个点/条边/区间…」的数量表达式。
/// 已定义的变量名直接引用；未定义的变量自动补定义行（implicit）；数字直接用。
fn theme_count(
    clause: &str,
    keys: &[&str],
    p: &mut Parsed,
    ranges: &[(String, Range)],
) -> String {
    static RX: OnceLock<Regex> = OnceLock::new();
    let re = RX.get_or_init(|| {
        Regex::new(
            r"(?P<n>[a-zA-Z_]\w*|[0-9]+|[一二两三四五六七八九十]+)\s*(?:(?:[一二两三四五六七八九十]+个?|个|的|个的)\s*)*(?P<k>点|顶点|条边|条|区间|点对|行|列|排列|perm)",
        )
        .expect("nlg theme count regex")
    });
    let mut cand: Option<String> = None;
    for cap in re.captures_iter(clause) {
        let k = cap.name("k").map(|m| m.as_str()).unwrap_or("");
        if !keys.iter().any(|x| *x == k) {
            continue;
        }
        let n = cap.name("n").map(|m| m.as_str()).unwrap_or("");
        // 冠词「一个排列」「一个区间」的「一」不算数量
        let seg = &clause[cap.get(0).unwrap().start()..cap.get(0).unwrap().end()];
        if cn_num(n).is_some() && seg.starts_with("一个") {
            continue;
        }
        if let Some(v) = num_str(n) {
            cand = Some(v);
            break;
        }
        if is_name_str(n) {
            cand = Some(n.to_string());
            break;
        }
    }
    let name = match cand {
        Some(n) if is_name_str(&n) && p.used.contains(&n) => n,
        Some(n) => {
            if !is_name_str(&n) {
                return n;
            }
            p.name_used(&n);
            let (lo, hi, def) = resolve(&n, ranges, None);
            if def {
                p.defaults = true;
                p.warnings.push(format!("未识别 {n} 的范围，默认 {DEFAULT_LO}~{DEFAULT_HI}"));
            }
            p.implicit.push(NumItem { elem: Elem::Int, name: n.clone(), lo, hi, prec: None });
            n
        }
        None => {
            // 无显式数量（「一棵树」）：复用已定义的 n（如行块的 n），否则新建
            if p.used.contains(&"n".to_string()) {
                return "n".to_string();
            }
            let n = p.take_name("n");
            let (lo, hi, def) = resolve(&n, ranges, None);
            if def {
                p.defaults = true;
                p.warnings.push(format!("未识别 {n} 的范围，默认 {DEFAULT_LO}~{DEFAULT_HI}"));
            }
            p.implicit.push(NumItem { elem: Elem::Int, name: n.clone(), lo, hi, prec: None });
            n
        }
    };
    name
}

/// 生成数组/矩阵命令：rows=None → `v = ints/floats(count, lo, hi)`；
/// rows=Some → `v = matrix(rows, count, lo, hi)`（「每行 N 个类型」场景）。
/// count 为未定义变量时自动补定义行（implicit）。
fn push_array_cmd(
    count: &str,
    rows: Option<&str>,
    frag: &str,
    p: &mut Parsed,
    ranges: &[(String, Range)],
    bare: Option<&Range>,
    var: &str,
) -> bool {
    let count_expr = if is_name_str(count) && !p.used.iter().any(|u| u == count) {
        p.name_used(count);
        let (lo, hi, def) = resolve(count, ranges, None);
        if def {
            p.defaults = true;
            p.warnings.push(format!("未识别 {count} 的范围，默认 {DEFAULT_LO}~{DEFAULT_HI}"));
        }
        p.implicit.push(NumItem { elem: Elem::Int, name: count.to_string(), lo, hi, prec: None });
        count.to_string()
    } else {
        count.to_string()
    };
    let (elem, _) = detect_elem(frag).unwrap_or((Elem::Int, Vec::new()));
    let (lo, hi, def) = resolve("a", ranges, bare);
    if def {
        p.defaults = true;
        p.warnings.push("未识别数组元素范围，默认 1~100".into());
    }
    let v = p.take_fixed(var);
    let cmd = match rows {
        Some(r) => format!("{v} = matrix({r}, {count_expr}, {lo}, {hi})"),
        None => format!(
            "{v} = {}({count_expr}, {lo}, {hi})",
            if elem == Elem::Float { "floats" } else { "ints" }
        ),
    };
    p.blocks.push(Block::Cmd(cmd));
    true
}

/// 把一段描述解析成 Parsed；无任何结构命中返回 None。
fn rule_convert(text: &str) -> Option<Parsed> {
    let mut p = Parsed::default();
    let ranges = extract_ranges(text);
    let bare = bare_range(text).or_else(|| bare_in_range(text));
    let wcall = weight_call(text);

    // 多测（全文扫描）
    let multi_var = detect_multi_test(text);

    // ---- 1) 行片段 / 数组（先处理：显式变量（如第一行的 n）先定义，供主题引用）----
    for (frag, ctx) in segment_lines(text) {
        match ctx {
            Some(LineCtx::Single) => {
                p.hit = true;
                // 无类型词：先试数量（「第二行 n 个数」→ 数组），再试变量名（「first line contains n」→ int n）
                if detect_elem(&frag).is_none() {
                    if let Some(count) = detect_count(&frag) {
                        if push_array_cmd(&count, None, &frag, &mut p, &ranges, bare.as_ref(), "a") {
                            continue;
                        }
                    }
                    if let Some(name) = extract_var_name(&frag) {
                        p.name_used(&name);
                        let (lo, hi, def) = resolve(&name, &ranges, None);
                        if def {
                            p.defaults = true;
                            p.warnings.push(format!("未识别 {name} 的范围，默认 {DEFAULT_LO}~{DEFAULT_HI}"));
                        }
                        let items = vec![NumItem { elem: Elem::Int, name, lo, hi, prec: None }];
                        p.blocks.push(Block::Line { rows: "1".to_string(), items });
                        continue;
                    }
                }
                let items = items_from_clause(&frag, &mut p, &ranges);
                p.blocks.push(Block::Line { rows: "1".to_string(), items });
            }
            Some(LineCtx::Rows) => {
                p.hit = true;
                let rows = detect_rows_expr(&frag).unwrap_or_else(|| "n".to_string());
                if rows != "1" && is_name_str(&rows) && !p.used.contains(&rows) {
                    p.name_used(&rows);
                    let (lo, hi, def) = resolve(&rows, &ranges, None);
                    if def {
                        p.defaults = true;
                        p.warnings.push(format!("未识别 {rows} 的范围，默认 {DEFAULT_LO}~{DEFAULT_HI}"));
                    }
                    p.implicit.push(NumItem { elem: Elem::Int, name: rows.clone(), lo, hi, prec: None });
                }
                // 「每行 N 个类型」（N ≠ 1、单类型词、类型词后无变量名）→ 矩阵：
                // 「接下来 n 行每行 m 个整数」= n 行 m 列 → M = matrix(n, m, …)
                let per_row = detect_count(&frag);
                let single_type = count_type_words(&frag) <= 1;
                let has_vars = detect_elem(&frag)
                    .map(|(_, v)| !v.is_empty())
                    .unwrap_or(false);
                if let Some(count) = per_row {
                    if count != "1" && single_type && !has_vars {
                        if push_array_cmd(&count, Some(&rows), &frag, &mut p, &ranges, bare.as_ref(), "M") {
                            continue;
                        }
                    }
                }
                let items = items_from_clause(&frag, &mut p, &ranges);
                p.blocks.push(Block::Line { rows, items });
            }
            None => {
                // 非行片段：数组检测（「N 个整数」等）
                if let Some(count) = detect_count(&frag) {
                    p.hit = true;
                    push_array_cmd(&count, None, &frag, &mut p, &ranges, bare.as_ref(), "a");
                }
            }
        }
    }

    // ---- 2) 主题结构（树/图/排列/区间/点集/矩阵）：对整段文本检测，各只生成一次 ----
    let mut seen_tree = false;
    let mut seen_graph = false;
    let mut seen_perm = false;
    let mut seen_iv = false;
    let mut seen_pts = false;
    let mut seen_matrix = false;
    for clause in text.split(['。', '；', ';', '\n']) {
        let clause = clause.trim();
        if clause.is_empty() {
            continue;
        }
        // 树
        if !seen_tree && (clause.contains("树") || clause.contains("tree")) {
            seen_tree = true;
            p.hit = true;
            let n = theme_count(clause, &["点", "顶点"], &mut p, &ranges);
            let ttype = if clause.contains("菊花") || clause.contains("星") {
                "type=\"star\""
            } else if clause.contains("链") {
                "type=\"chain\""
            } else if clause.contains("父节点") || clause.contains("父亲") || clause.contains("parent") {
                "type=\"parent\""
            } else {
                ""
            };
            let (lo, hi, def) = if has_w(clause) {
                let r = wcall
                    .clone()
                    .or_else(|| ranges.iter().find(|(nn, _)| nn.as_str() == n.as_str()).map(|(_, r)| r.clone()));
                match r {
                    Some(r) => (r.lo, r.hi, false),
                    None => {
                        let (l, h, d) = resolve(&n, &ranges, bare.as_ref());
                        (l, h, d)
                    }
                }
            } else {
                (String::new(), String::new(), false)
            };
            if has_w(clause) && def {
                p.defaults = true;
                p.warnings.push("未识别边权范围，默认 1~100".into());
            }
            let tv = p.take_fixed("t");
            let args = if has_w(clause) {
                if ttype.is_empty() {
                    format!("{tv} = tree({n}, int({lo}, {hi}))")
                } else {
                    format!("{tv} = tree({n}, {ttype}, int({lo}, {hi}))")
                }
            } else if ttype.is_empty() {
                format!("{tv} = tree({n})")
            } else {
                format!("{tv} = tree({n}, {ttype})")
            };
            p.blocks.push(Block::Cmd(args));
            continue;
        }
        // 图
        if !seen_graph && (clause.contains("graph") || clause.contains("边") || (clause.contains("图") && !clause.contains("树"))) {
            seen_graph = true;
            p.hit = true;
            let n = theme_count(clause, &["点", "顶点"], &mut p, &ranges);
            let m = theme_count(clause, &["条边", "条"], &mut p, &ranges);
            let d = if has_directed(clause) { "1" } else { "0" };
            let c = if has_connected(clause) { "1" } else { "0" };
            let (mut lo, mut hi, mut def) = (String::new(), String::new(), false);
            if has_w(clause) {
                let r = wcall
                    .clone()
                    .or_else(|| ranges.iter().find(|(nn, _)| nn.as_str() == n.as_str()).map(|(_, r)| r.clone()));
                match r {
                    Some(r) => {
                        lo = r.lo;
                        hi = r.hi;
                    }
                    None => {
                        let (l, h, d) = resolve(&n, &ranges, bare.as_ref());
                        lo = l;
                        hi = h;
                        def = d;
                    }
                }
            }
            if has_w(clause) && def {
                p.defaults = true;
                p.warnings.push("未识别边权范围，默认 1~100".into());
            }
            let extra = if has_w(clause) {
                format!(", int({lo}, {hi})")
            } else {
                String::new()
            };
            let gv = p.take_fixed("g");
            p.blocks.push(Block::Cmd(format!("{gv} = graph({n}, {m}, {d}, {c}{extra})")));
            continue;
        }
        // 排列
        if !seen_perm && (clause.contains("排列") || clause.contains("perm")) {
            seen_perm = true;
            p.hit = true;
            let n = theme_count(clause, &["排列", "perm"], &mut p, &ranges);
            let pv = p.take_fixed("p");
            p.blocks.push(Block::Cmd(format!("{pv} = perm({n})")));
            continue;
        }
        // 区间
        if !seen_iv && (clause.contains("区间") || clause.contains("interval")) {
            seen_iv = true;
            p.hit = true;
            let n = theme_count(clause, &["区间"], &mut p, &ranges);
            let (lo, _, _) = resolve("l", &ranges, bare.as_ref());
            let (_, hi, _) = resolve("r", &ranges, bare.as_ref());
            let ivv = p.take_fixed("iv");
            p.blocks.push(Block::Cmd(format!("{ivv} = intervals({n}, {lo}, {hi})")));
            continue;
        }
        // 点集
        if !seen_pts
            && (clause.contains("点集") || clause.contains("点对") || (clause.contains("点") && clause.contains("坐标")))
        {
            seen_pts = true;
            p.hit = true;
            let n = theme_count(clause, &["点", "点对"], &mut p, &ranges);
            let (xlo, _, _) = resolve("x", &ranges, bare.as_ref());
            let (_, xhi, _) = resolve("x", &ranges, bare.as_ref());
            let (ylo, _, _) = resolve("y", &ranges, bare.as_ref());
            let (_, yhi, _) = resolve("y", &ranges, bare.as_ref());
            let ptv = p.take_fixed("pt");
            p.blocks.push(Block::Cmd(format!("{ptv} = points({n}, {xlo}, {xhi}, {ylo}, {yhi})")));
            continue;
        }
        // 矩阵
        if !seen_matrix
            && (clause.contains("矩阵") || clause.contains("matrix") || (clause.contains("行") && clause.contains("列")))
        {
            seen_matrix = true;
            p.hit = true;
            let (rows, cols) = detect_matrix_dims(clause).unwrap_or_else(|| ("3".to_string(), "3".to_string()));
            let dim = |d: String, p: &mut Parsed| -> String {
                if is_name_str(&d) && !p.used.contains(&d) {
                    p.name_used(&d);
                    let (lo, hi, def) = resolve(&d, &ranges, None);
                    if def {
                        p.defaults = true;
                        p.warnings.push(format!("未识别 {d} 的范围，默认 {DEFAULT_LO}~{DEFAULT_HI}"));
                    }
                    p.implicit.push(NumItem { elem: Elem::Int, name: d.clone(), lo, hi, prec: None });
                    d
                } else {
                    d
                }
            };
            let rows = dim(rows, &mut p);
            let cols = dim(cols, &mut p);
            // 01/0-1 矩阵 → 元素范围 0,1
            let (lo, hi) = if clause.contains("01") || clause.contains("0/1") || clause.contains("0-1") || clause.contains("二进制") {
                ("0".to_string(), "1".to_string())
            } else {
                let (l, h, def) = resolve("a", &ranges, bare.as_ref());
                if def {
                    p.defaults = true;
                    p.warnings.push("未识别矩阵元素范围，默认 1~100".into());
                }
                (l, h)
            };
            p.blocks.push(Block::Cmd(format!("M = matrix({rows}, {cols}, {lo}, {hi})")));
            continue;
        }
    }

    // ---- 3) 多测计数变量（repeat 外定义）+ repeat_var ----
    if let Some(v) = &multi_var {
        p.name_used(v);
        // 用户已在行块定义同名项（大小写不敏感）→ 取出复用其范围（t 移到 repeat 外定义）
        let mut reused: Option<(String, String)> = None;
        'outer: for b in &mut p.blocks {
            if let Block::Line { items, .. } = b {
                for (i, it) in items.iter().enumerate() {
                    if it.name.to_lowercase() == v.to_lowercase() {
                        reused = Some((it.lo.clone(), it.hi.clone()));
                        items.remove(i);
                        break 'outer;
                    }
                }
            }
        }
        // 移除计数变量后若行块为空则整个移除
        p.blocks.retain(|b| match b {
            Block::Line { items, .. } => !items.is_empty(),
            _ => true,
        });
        // 行块 rows 若引用计数变量（「接下来 T 组」→ line (t)）统一改名
        for b in &mut p.blocks {
            if let Block::Line { rows, .. } = b {
                if rows.to_lowercase() == v.to_lowercase() {
                    *rows = v.clone();
                }
            }
        }
        let (lo, hi, def) = match reused {
            Some((l, h)) => (l, h, false),
            None => {
                let (l, h, d) = resolve(v, &ranges, None);
                (l, h, d)
            }
        };
        if def {
            p.defaults = true;
            p.warnings.push(format!("未识别组数 {v} 的范围，默认 {DEFAULT_LO}~{DEFAULT_HI}"));
        }
        p.repeat_count = Some(NumItem { elem: Elem::Int, name: v.clone(), lo, hi, prec: None });
        p.repeat_var = Some(v.clone());
    }

    if !p.hit {
        return None;
    }
    Some(p)
}

/// 从行描述 clause 提取行内项（支持一行多个类型词，如「两个整数 u v 和一个浮点数 w」）。
fn items_from_clause(
    clause: &str,
    p: &mut Parsed,
    ranges: &[(String, Range)],
) -> Vec<NumItem> {
    let mut items: Vec<NumItem> = Vec::new();
    let prec = detect_prec(clause);
    let mut rest = clause;
    let mut found = false;
    while let Some((elem, mut vars)) = detect_elem(rest) {
        found = true;
        // 从类型词后继续扫描（同一行可能还有别的类型词）
        let m = detect_elem_pos(rest);
        rest = match m {
            Some((start, len)) => {
                let next = start + len;
                if next < rest.len() { &rest[next..] } else { "" }
            }
            None => "",
        };
        let mut used_in_clause: Vec<String> = Vec::new();
        // 显式变量名优先；没有则自动分配
        if vars.is_empty() {
            let pref = match elem {
                Elem::Int => "a",
                Elem::Float => "x",
                Elem::Str | Elem::Text => "s",
            };
            let fresh = p.take_name(pref);
            used_in_clause.push(fresh.clone());
            vars.push(fresh);
        }
        for name in &mut vars {
            if used_in_clause.contains(name) {
                continue;
            }
            if p.used.contains(name) || used_in_clause.contains(name) {
                let fresh = p.take_name("v");
                used_in_clause.push(fresh.clone());
                *name = fresh;
            } else {
                used_in_clause.push(name.clone());
            }
            // 行内项全局可见：显式/自动名都登记（供主题数量检测与后续引用）
            p.name_used(name);
        }
        for name in vars {
            // 行内项范围只用显式 ranges + 默认（bare 范围（如边权）不污染行内项）
            let (lo, hi, def) = resolve(&name, ranges, None);
            if def {
                p.defaults = true;
                p.warnings.push(format!("未识别 {name} 的范围，默认 {DEFAULT_LO}~{DEFAULT_HI}"));
            }
            items.push(NumItem {
                elem,
                name,
                lo,
                hi,
                prec: prec.clone(),
            });
        }
    }
    if !found {
        // 无类型词：兜底一个整数项
        p.defaults = true;
        p.warnings.push("未识别行内数据类型，默认生成一个整数项".into());
        let name = p.take_name("a");
        let (lo, hi, def) = resolve(&name, ranges, None);
        if def {
            p.warnings.push(format!("未识别 {name} 的范围，默认 {DEFAULT_LO}~{DEFAULT_HI}"));
        }
        items.push(NumItem { elem: Elem::Int, name, lo, hi, prec: None });
    }
    items
}

/// 统计片段中的类型词个数（辅助「每行 N 个类型 → 矩阵」判定）。
fn count_type_words(s: &str) -> usize {
    let mut n = 0;
    let mut rest = s;
    while let Some((start, len)) = detect_elem_pos(rest) {
        n += 1;
        let next = start + len;
        rest = if next < rest.len() { &rest[next..] } else { "" };
        if n > 1 {
            break;
        }
    }
    n
}

/// 找到第一个类型词的位置与长度（辅助多类型词扫描）。
fn detect_elem_pos(s: &str) -> Option<(usize, usize)> {
    static RX: OnceLock<Regex> = OnceLock::new();
    let re = RX.get_or_init(|| {
        Regex::new(
            r"(浮点数|整型|整数|实数|小数|浮点型|浮点|字符串|字符型|字符|文本|数字|integers|integer|floats|float|numbers|number|elements|element|string|double|real|char|int)",
        )
        .expect("nlg elem pos regex")
    });
    let is_ascii_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let bit_small = "位".as_bytes();
    for m in re.find_iter(s) {
        let before = m.start() > 0 && is_ascii_word(s.as_bytes()[m.start() - 1]);
        let after = m.end() < s.len() && is_ascii_word(s.as_bytes()[m.end()]);
        // 「3 位小数」的「小数」是精度语境，不是类型词
        let kw = &s[m.start()..m.end()];
        let prec_ctx = kw == "小数"
            && m.start() >= 3
            && &s.as_bytes()[m.start() - 3..m.start()] == bit_small;
        if !before && !after && !prec_ctx {
            return Some((m.start(), m.end() - m.start()));
        }
    }
    None
}

// --------------------------------------------------------------------------- //
// 渲染 + 管道
// --------------------------------------------------------------------------- //

/// 行内项渲染为一行（int/float/str）。
fn item_line(it: &NumItem) -> String {
    match it.elem {
        Elem::Int => format!("    int {}: {}, {}", it.name, it.lo, it.hi),
        Elem::Float => {
            let prec = it.prec.clone().unwrap_or_else(|| "6".to_string());
            if prec != "6" {
                format!("    float {}: {}, {}, {}", it.name, it.lo, it.hi, prec)
            } else {
                format!("    float {}: {}, {}", it.name, it.lo, it.hi)
            }
        }
        Elem::Str => format!("    str {}: int({}, {})", it.name, it.lo, it.hi),
        Elem::Text => format!("    text {}: \"{}\"", it.name, it.lo),
    }
}

fn render(p: &Parsed) -> String {
    let mut body: Vec<String> = Vec::new();
    // 隐式数量变量定义行（在最前；多测时位于 repeat 块内）
    if !p.implicit.is_empty() {
        body.push("line:".to_string());
        for it in &p.implicit {
            body.push(item_line(it));
        }
    }
    for b in &p.blocks {
        match b {
            Block::Cmd(c) => body.push(c.clone()),
            Block::Line { rows, items } => {
                if rows == "1" {
                    body.push("line:".to_string());
                } else {
                    body.push(format!("line ({rows}):"));
                }
                for it in items {
                    body.push(item_line(it));
                }
            }
        }
    }
    let mut out: Vec<String> = Vec::new();
    // 多测：计数行（repeat 外）+ repeat 块（body 缩进）
    if let Some(v) = &p.repeat_var {
        if let Some(rc) = &p.repeat_count {
            out.push("line:".to_string());
            out.push(item_line(rc));
        }
        out.push(format!("repeat ({v}):"));
        for l in body {
            out.push(format!("    {l}"));
        }
    } else {
        out.extend(body);
    }
    out.join("\n")
}

fn confidence_of(p: &Parsed) -> f64 {
    if p.defaults {
        0.7
    } else if p.warnings.is_empty() {
        0.95
    } else {
        0.8
    }
}

/// 规则转换：命中返回 Some，未命中返回 None（由管道决定走模型）。
pub fn rule_to_dsl(text: &str) -> Option<NlResult> {
    let p = rule_convert(text)?;
    let dsl = render(&p);
    // 安全网：生成的 DSL 必须可解析。
    match parse(&dsl) {
        Ok(cfg) => {
            let mut warnings = p.warnings.clone();
            for e in validate(&cfg) {
                warnings.push(format!("生成结果存在警告：{}", e.message));
            }
            Some(NlResult {
                dsl,
                confidence: confidence_of(&p),
                method: NlMethod::Rule,
                warnings,
                thought: String::new(),
            })
        }
        Err(e) => {
            let _ = std::fs::write(
                "C:\\Users\\pigeon\\AppData\\Local\\Temp\\opencode\\raw.txt",
                &dsl,
            );
            Some(NlResult {
                dsl: String::new(),
                confidence: 0.0,
                method: NlMethod::Rule,
                warnings: vec![format!("规则生成结果未通过解析：{}", e.message)],
                thought: String::new(),
            })
        }
    }
}

/// 完整管道：规则优先 → 模型后备 → 校验。
///
/// 规则命中（置信度 > 0）直接返回；未命中且模型通道就绪时走本地大模型
/// 推理（parse + validate 校验重试 ≤2 次）；模型不可用时返回低置信失败结果。
pub fn nl_to_dsl(text: &str) -> NlResult {
    nl_to_dsl_opt(text, false)
}

/// 同 [`nl_to_dsl`]，但 `model_only=true` 时跳过规则引擎，直接走模型
/// （模型未就绪或失败时返回失败提示，不回退规则）。
pub fn nl_to_dsl_opt(text: &str, model_only: bool) -> NlResult {
    let text = text.trim();
    if text.is_empty() {
        return NlResult {
            dsl: String::new(),
            confidence: 0.0,
            method: NlMethod::Rule,
            warnings: vec!["输入为空".to_string()],
            thought: String::new(),
        };
    }
    if !model_only {
        if let Some(r) = rule_to_dsl(text) {
            if r.confidence > 0.0 {
                return r;
            }
        }
    }
    #[cfg(feature = "nl-model")]
    if crate::model::model_loaded() {
        let req = crate::model::ModelInferRequest {
            text: text.to_string(),
            last_error: None,
        };
        match crate::model::infer(&req) {
            Ok(r) if !r.dsl.is_empty() => {
                return NlResult {
                    dsl: r.dsl,
                    confidence: r.confidence,
                    method: NlMethod::Model,
                    warnings: Vec::new(),
                    thought: r.thought,
                };
            }
            Ok(_) => {
                return NlResult {
                    dsl: String::new(),
                    confidence: 0.0,
                    method: NlMethod::Model,
                    warnings: vec!["模型推理输出为空".to_string()],
                    thought: String::new(),
                };
            }
            Err(e) => {
                return NlResult {
                    dsl: String::new(),
                    confidence: 0.0,
                    method: NlMethod::Model,
                    warnings: vec![format!("模型推理失败：{e}")],
                    thought: String::new(),
                };
            }
        }
    }
    if model_only {
        return NlResult {
            dsl: String::new(),
            confidence: 0.0,
            method: NlMethod::Model,
            warnings: vec![
                "模型通道未启用或模型未加载：请先在模型状态区设置路径并加载".to_string(),
            ],
            thought: String::new(),
        };
    }
    NlResult {
        dsl: String::new(),
        confidence: 0.0,
        method: NlMethod::Rule,
        warnings: vec![
            "未识别输入格式：请用类似「第一行两个整数 n m，接下来 n 行每行两个整数」的描述".to_string(),
            "模型通道未启用或未加载：当前仅规则引擎可用".to_string(),
        ],
        thought: String::new(),
    }
}

/// 供测试使用：规则直接转换。
#[allow(dead_code)]
pub fn _rule_only(text: &str) -> Option<NlResult> {
    rule_to_dsl(text)
}

/// 供测试使用：解析（与 parser::parse 等价）。
#[allow(dead_code)]
pub fn _parse_for_test(text: &str) -> DslResult<crate::ast::Config> {
    parse(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conv(text: &str) -> NlResult {
        nl_to_dsl(text)
    }

    #[test]
    fn mixed_types_in_row() {
        let r = conv("第一行一个整数 n，接下来 n 行每行两个整数 u v 和一个浮点数 w (0<=w<=1)");
        assert!(r.dsl.contains("line (n):"), "{}", r.dsl);
        assert!(r.dsl.contains("int u: 1, 100"), "{}", r.dsl);
        assert!(r.dsl.contains("int v: 1, 100"), "{}", r.dsl);
        assert!(r.dsl.contains("float w: 0, 1"), "{}", r.dsl);
    }

    #[test]
    fn float_precision() {
        let r = conv("第一行一个浮点数 x，保留 3 位小数");
        let _ = std::fs::write(
            "C:\\Users\\pigeon\\AppData\\Local\\Temp\\opencode\\fp.txt",
            format!("warn={:?}\n{}", r.warnings, r.dsl),
        );
        assert!(r.dsl.contains("float x: 1, 100, 3"), "{}", r.dsl);
    }

    #[test]
    fn intervals_with_lr_chain() {
        let r = conv("n 个区间 [l, r]，1<=l<=r<=10^9");
        let _ = std::fs::write(
            "C:\\Users\\pigeon\\AppData\\Local\\Temp\\opencode\\iv.txt",
            format!("warn={:?}\n{}", r.warnings, r.dsl),
        );
        assert!(r.dsl.contains("iv = intervals(n, 1, 1000000000)"), "{}", r.dsl);
        assert!(_parse_for_test(&r.dsl).is_ok(), "{}", r.dsl);
    }

    #[test]
    fn tree_weight_default_ok() {
        let r = conv("一棵树的边带权，权值 1 到 10");
        assert!(r.dsl.contains("t = tree(n, int(1, 10))"), "{}", r.dsl);
    }

    #[test]
    fn powers_of_ten() {
        let r = conv("第一行两个整数 n m (1<=n,m<=10^5)");
        let _ = std::fs::write(
            "C:\\Users\\pigeon\\AppData\\Local\\Temp\\opencode\\pot.txt",
            format!("warn={:?}\n{}", r.warnings, r.dsl),
        );
        assert!(r.dsl.contains("int n: 1, 100000"), "{}", r.dsl);
        assert!(r.dsl.contains("int m: 1, 100000"), "{}", r.dsl);
    }

    #[test]
    fn zero_one_matrix() {
        let r = conv("n 行 m 列的 01 矩阵");
        assert!(r.dsl.contains("M = matrix(n, m, 0, 1)"), "{}", r.dsl);
    }

    #[test]
    fn str_row_random_len() {
        let r = conv("n 行每行一个字符串，长度不超过 100");
        let _ = std::fs::write(
            "C:\\Users\\pigeon\\AppData\\Local\\Temp\\opencode\\str.txt",
            format!("warn={:?}\n{}", r.warnings, r.dsl),
        );
        assert!(r.dsl.contains("str s: int(1, 100)"), "{}", r.dsl);
        assert!(_parse_for_test(&r.dsl).is_ok(), "{}", r.dsl);
    }

    #[test]
    fn nested_group_matrix() {
        // 每组内：第一行 n m + 接下来 n 行每行 m 个整数 → 块内矩阵
        let r = conv("T 组数据，每组：第一行 n m，接下来 n 行每行 m 个整数");
        assert!(r.dsl.contains("repeat (T):"), "{}", r.dsl);
        assert!(r.dsl.contains("M = matrix(n, m, 1, 100)"), "{}", r.dsl);
        assert!(_parse_for_test(&r.dsl).is_ok(), "{}", r.dsl);
    }

    #[test]
    fn per_row_two_ints_matrix() {
        // 非多测：接下来 n 行每行两个整数 → matrix(n, 2)
        let r = conv("第一行一个整数 n (1<=n<=100)，接下来 n 行每行两个整数");
        assert!(r.dsl.contains("M = matrix(n, 2, 1, 100)"), "{}", r.dsl);
        assert!(_parse_for_test(&r.dsl).is_ok(), "{}", r.dsl);
    }

    #[test]
    fn multi_test_mixed_theme() {
        let r = conv("T 组数据，每组：第一行 n m，接下来 n 行每行 m 个整数");
        assert!(r.dsl.contains("repeat (T):"), "{}", r.dsl);
        assert!(_parse_for_test(&r.dsl).is_ok(), "{}", r.dsl);
    }

    #[test]
    fn english_multi_test() {
        let r = conv("T test cases. each test case: first line contains n, then n lines with m integers");
        assert!(r.dsl.contains("repeat (T):"), "{}", r.dsl);
        assert!(_parse_for_test(&r.dsl).is_ok(), "{}", r.dsl);
    }

    #[test]
    fn single_line_two_ints() {
        let r = conv("第一行两个整数 n m，1 <= n, m <= 100");
        assert_eq!(r.method, NlMethod::Rule);
        assert_eq!(r.dsl, "line:\n    int n: 1, 100\n    int m: 1, 100");
        assert!(r.confidence >= 0.9, "conf {}", r.confidence);
    }

    #[test]
    fn multi_test_first_line() {
        let r = conv("多测，T 组。第一行一个整数 n (1<=n<=10^5)，接下来 n 行每行一个整数 x");
        assert!(r.dsl.contains("repeat (T):"), "{}", r.dsl);
        assert!(r.dsl.contains("int n: 1, 100000"), "{}", r.dsl);
        assert!(r.dsl.contains("    line (n):\n        int x: 1, 100"), "{}", r.dsl);
    }

    #[test]
    fn next_lines_each_int() {
        let r = conv("第一行一个整数 n (1≤n≤100)。接下来 n 行，每行一个整数 a (1≤a≤10^9)");
        assert!(r.dsl.contains("line:\n    int n: 1, 100"), "{}", r.dsl);
        assert!(r.dsl.contains("line (n):\n    int a: 1, 1000000000"), "{}", r.dsl);
    }

    #[test]
    fn array_of_n_ints() {
        let r = conv("一个 n 个整数的数组，元素范围 1 到 100");
        assert_eq!(r.dsl, "line:\n    int n: 1, 100\na = ints(n, 1, 100)");
    }

    #[test]
    fn matrix() {
        let r = conv("一个 n 行 m 列的矩阵，每个元素 0 到 1");
        assert_eq!(r.dsl, "line:\n    int n: 1, 100\n    int m: 1, 100\nM = matrix(n, m, 0, 1)");
    }

    #[test]
    fn tree_with_weight() {
        let r = conv("一棵 n 个点的树，边权 1 到 100");
        assert!(r.dsl.contains("t = tree(n, int(1, 100))"), "{}", r.dsl);
    }

    #[test]
    fn tree_parent() {
        let r = conv("以 1 为根的树，n 个节点，输入每个节点的父节点");
        assert!(r.dsl.contains("t = tree(n, type=\"parent\")"), "{}", r.dsl);
    }

    #[test]
    fn tree_plain() {
        let r = conv("n 个点的树");
        assert_eq!(r.dsl, "line:\n    int n: 1, 100\nt = tree(n)");
    }

    #[test]
    fn tree_star() {
        let r = conv("一棵 n 个点的菊花图（star 树）");
        assert!(r.dsl.contains("t = tree(n, type=\"star\")"), "{}", r.dsl);
    }

    #[test]
    fn graph_undirected() {
        let r = conv("n 个点 m 条边的无向连通图");
        assert!(r.dsl.contains("g = graph(n, m, 0, 1)"), "{}", r.dsl);
    }

    #[test]
    fn graph_weighted() {
        let r = conv("n 个点 m 条边的图，边权 int(1, 9)");
        assert!(r.dsl.contains("g = graph(n, m, 0, 0, int(1, 9))"), "{}", r.dsl);
    }

    #[test]
    fn graph_dag() {
        let r = conv("n 个点 m 条边的 DAG");
        assert!(r.dsl.contains("g = graph(n, m, 1, 0)"), "{}", r.dsl);
    }

    #[test]
    fn permutation() {
        let r = conv("n 的一个排列");
        assert_eq!(r.dsl, "line:\n    int n: 1, 100\np = perm(n)");
    }

    #[test]
    fn english_basic() {
        let r = conv("first line contains two integers n and m, then n lines each with one integer a");
        assert!(r.dsl.contains("line:\n    int n: 1, 100\n    int m: 1, 100"), "{}", r.dsl);
        assert!(r.dsl.contains("line (n):\n    int a: 1, 100"), "{}", r.dsl);
    }

    #[test]
    fn english_array() {
        let r = conv("an array of n integers in [1, 10^9]");
        assert_eq!(r.dsl, "line:\n    int n: 1, 100\na = ints(n, 1, 1000000000)");
    }

    #[test]
    fn empty_input() {
        let r = conv("   ");
        assert_eq!(r.confidence, 0.0);
        assert!(r.warnings.iter().any(|w| w.contains("为空")));
    }

    #[test]
    fn unknown_input() {
        let r = conv("请生成一组符合要求的随机数据");
        assert_eq!(r.confidence, 0.0);
        assert!(r.warnings.iter().any(|w| w.contains("未识别")));
    }

    #[test]
    fn multi_test_with_explicit_t() {
        let r = conv("第一行一个整数 T (1<=T<=10)，接下来 T 组，每组第一行一个整数 n，然后 n 行每行一个整数");
        assert!(r.dsl.contains("repeat (T):"), "warn={:?} dsl={}", r.warnings, r.dsl);
        assert!(r.dsl.contains("int T: 1, 10"), "{}", r.dsl);
        assert!(r.dsl.contains("line (n):"), "{}", r.dsl);
    }

    #[test]
    fn float_item() {
        let r = conv("第一行一个整数 n，接下来 n 行每行一个浮点数 x (0 <= x <= 1)");
        assert!(r.dsl.contains("line (n):\n    float x: 0, 1"), "{}", r.dsl);
    }

    #[test]
    fn generated_dsl_always_valid() {
        for text in [
            "第一行两个整数 n m，接下来 n 行每行两个整数 a b",
            "多测。第一行一个整数 T，接下来 T 组，每组第一行一个整数 n，然后 n 行每行一个整数",
            "一个 n 行 m 列的 0/1 矩阵",
            "n 个点 m 条边的有向图，无自环",
            "第一行一个整数 n (1<=n<=100)。接下来 n 行每行一个字符串 s",
            "多测，T 组。第一行一个整数 n (1<=n<=10^5)，接下来 n 行每行一个整数 x",
        ] {
            let r = conv(text);
            if r.confidence > 0.0 {
                assert!(
                    _parse_for_test(&r.dsl).is_ok(),
                    "{text} -> parse failed: {}",
                    r.dsl
                );
            }
        }
    }
}
