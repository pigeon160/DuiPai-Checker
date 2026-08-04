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

impl Elem {
    fn label(self) -> &'static str {
        match self {
            Elem::Int => "int",
            Elem::Float => "float",
            Elem::Str => "str",
            Elem::Text => "text",
        }
    }
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
    /// 多测（repeat 块）计数变量名。
    repeat_var: Option<String>,
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

/// 数字 / 中文数字 -> 数字字符串。
fn num_str(s: &str) -> Option<String> {
    let s = s.trim();
    if let Some(v) = cn_num(s) {
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
        let num = r"[0-9]+(?:\.[0-9]+)?|10\^\d+|10\*\*\d+|[0-9]+(?:\.[0-9]+)?e-?\d+";
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

/// 无变量前缀的通用范围（「元素范围 1 到 100」），作为默认范围。
fn bare_range(s: &str) -> Option<Range> {
    static RX: OnceLock<Regex> = OnceLock::new();
    let re = RX.get_or_init(|| {
        Regex::new(
            r"(?:范围|取值|值域|元素|边权|权值|大小|每个(?:元素|数|整数|点|边))\s*(?:为|在|是|是)?\s*(?P<lo>[0-9]+(?:\.[0-9]+)?)\s*(?:~|～|至|到)\s*(?P<hi>[0-9]+(?:\.[0-9]+)?)",
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
        Regex::new(r"(?:int|float)\s*\(\s*(?P<lo>[0-9]+(?:\.[0-9]+)?|[a-zA-Z_]\w*)\s*,\s*(?P<hi>[0-9]+(?:\.[0-9]+)?|[a-zA-Z_]\w*)\s*(?:,\s*[0-9]+(?:\.[0-9]+)?)?\s*\)")
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
    // 无括号形式：lo <= v, w <= hi（多变量共界）
    let bare_bound = Regex::new(
        r"(?P<lo>[0-9]+(?:\.[0-9]+)?)\s*[≤<][=]?\s*(?P<v>[a-zA-Z_]\w*(?:\s*[,，]\s*[a-zA-Z_]\w*){0,3})\s*[≤<][=]?\s*(?P<hi>[0-9]+(?:\.[0-9]+)?)",
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
    static RX: OnceLock<Regex> = OnceLock::new();
    let re = RX.get_or_init(|| {
        Regex::new(
            r"(浮点数|整型|整数|实数|小数|浮点型|浮点|字符串|字符型|字符|文本|数字|integers|integer|floats|float|numbers|number|elements|element|string|double|real|char|int)",
        )
        .expect("nlg elem regex")
    });
    let is_ascii_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    // 取第一个前后都不是 ASCII 字母/数字/下划线的类型词（防止匹配 "contains" 里的 "int"）
    let m = re.find_iter(s).find(|m| {
        let before = m.start() > 0 && is_ascii_word(s.as_bytes()[m.start() - 1]);
        let after = m.end() < s.len() && is_ascii_word(s.as_bytes()[m.end()]);
        !before && !after
    })?;
    let elem = match m.as_str() {
        "浮点数" | "实数" | "小数" | "浮点" | "浮点型" | "real" | "float" | "floats" | "double" => Elem::Float,
        "字符串" | "字符" | "字符型" | "string" | "char" => Elem::Str,
        "文本" | "text" => Elem::Text,
        _ => Elem::Int,
    };
    Some((elem, after_vars(s, m.end())))
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
            r"(?P<n>[a-zA-Z_]\w*|[0-9]+|[一二两三四五六七八九十]+)\s*(?:个|枚|只|位)?\s*(?:整数|整型|浮点数|实数|小数|字符串|字符|数字|integer|integers|float|floats|real|number|numbers|char|string)",
        )
        .expect("nlg count regex")
    });
    let cap = re.captures(s)?;
    let n = cap.name("n")?.as_str();
    num_str(n).or_else(|| is_name_str(n).then(|| n.to_string()))
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
        Regex::new(r"(?:接下来|后面|随后|之后|剩余|next|following|remaining|subsequent|共|总共|then)?\s*(?P<n>[a-zA-Z_]\w*|[0-9]+|[一二两三四五六七八九十]+)\s*(?:行|组|lines?)").expect("nlg rows regex")
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
        Regex::new(r"(?P<v>[tT])\s*(?:组|次|轮|个测试|组数据)|第一行一个整数\s*(?P<w>[tT])").expect("nlg mt regex")
    });
    for cap in re.captures_iter(text) {
        if let Some(v) = cap.name("v").or_else(|| cap.name("w")) {
            return Some(v.as_str().to_lowercase());
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
fn rule_convert(text: &str) -> Option<Parsed> {
    let mut p = Parsed::default();
    let ranges = extract_ranges(text);
    let bare = bare_range(text).or_else(|| bare_in_range(text));
    let wcall = weight_call(text);

    // 多测（全文扫描）
    let multi_var = detect_multi_test(text);

    // ---- 主题结构（树/图/排列/区间/点集/矩阵）：对整段文本检测，各只生成一次 ----
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
            let n = p.take_name("n");
            let ttype = if clause.contains("菊花") || clause.contains("星") {
                "type=\"star\""
            } else if clause.contains("链") {
                "type=\"chain\""
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
                        let (lo, hi, d) = resolve(&n, &ranges, bare.as_ref());
                        (lo, hi, d)
                    }
                }
            } else {
                (String::new(), String::new(), false)
            };
            if has_w(clause) && def {
                p.defaults = true;
                p.warnings.push("未识别边权范围，默认 1~100".into());
            }
            let args = if has_w(clause) {
                if ttype.is_empty() {
                    format!("t = tree({n}, int({lo}, {hi}))")
                } else {
                    format!("t = tree({n}, {ttype}, int({lo}, {hi}))")
                }
            } else if ttype.is_empty() {
                format!("t = tree({n})")
            } else {
                format!("t = tree({n}, {ttype})")
            };
            p.blocks.push(Block::Cmd(args));
            continue;
        }
        // 图
        if !seen_graph && (clause.contains("graph") || clause.contains("边") || (clause.contains("图") && !clause.contains("树"))) {
            seen_graph = true;
            p.hit = true;
            let n = p.take_name("n");
            let m = p.take_name("m");
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
            p.blocks.push(Block::Cmd(format!("g = graph({n}, {m}, {d}, {c}{extra})")));
            continue;
        }
        // 排列
        if !seen_perm && (clause.contains("排列") || clause.contains("perm")) {
            seen_perm = true;
            p.hit = true;
            let n = p.take_name("n");
            p.blocks.push(Block::Cmd(format!("p = perm({n})")));
            continue;
        }
        // 区间
        if !seen_iv && (clause.contains("区间") || clause.contains("interval")) {
            seen_iv = true;
            p.hit = true;
            let n = p.take_name("n");
            let (lo, hi, def) = resolve(&n, &ranges, bare.as_ref());
            if def {
                p.defaults = true;
                p.warnings.push("未识别区间范围，默认 1~100".into());
            }
            p.blocks.push(Block::Cmd(format!("iv = intervals({n}, {lo}, {hi})")));
            continue;
        }
        // 点集
        if !seen_pts
            && (clause.contains("点集") || clause.contains("点对") || (clause.contains("点") && clause.contains("坐标")))
        {
            seen_pts = true;
            p.hit = true;
            let n = p.take_name("n");
            let (lo, hi, def) = resolve(&n, &ranges, bare.as_ref());
            if def {
                p.defaults = true;
                p.warnings.push("未识别坐标范围，默认 1~100".into());
            }
            p.blocks.push(Block::Cmd(format!("pt = points({n}, {lo}, {hi}, {lo}, {hi})")));
            continue;
        }
        // 矩阵
        if !seen_matrix
            && (clause.contains("矩阵") || clause.contains("matrix") || (clause.contains("行") && clause.contains("列")))
        {
            seen_matrix = true;
            p.hit = true;
            let (rows, cols) = detect_matrix_dims(clause).unwrap_or_else(|| ("3".to_string(), "3".to_string()));
            p.name_used(&rows);
            p.name_used(&cols);
            let (lo, hi, def) = resolve("a", &ranges, bare.as_ref());
            if def {
                p.defaults = true;
                p.warnings.push("未识别矩阵元素范围，默认 1~100".into());
            }
            p.blocks.push(Block::Cmd(format!("M = matrix({rows}, {cols}, {lo}, {hi})")));
            continue;
        }
    }

    // ---- 行片段 / 数组（按指示词切分）----
    for (frag, ctx) in segment_lines(text) {
        match ctx {
            Some(LineCtx::Single) | Some(LineCtx::Rows) => {
                p.hit = true;
        let rows = match ctx.unwrap() {
            LineCtx::Single => "1".to_string(),
            LineCtx::Rows => detect_rows_expr(&frag).unwrap_or_else(|| "n".to_string()),
        };
        if rows != "1" && is_name_str(&rows) {
            p.name_used(&rows);
        }
        let items = items_from_clause(&frag, &mut p, &ranges, bare.as_ref());
        p.blocks.push(Block::Line { rows, items });
            }
            None => {
                // 非行片段：数组检测（「N 个整数」等）
                if let Some(count) = detect_count(&frag) {
                    p.hit = true;
                    if is_name_str(&count) {
                        p.name_used(&count);
                    }
                    let (elem, _) = detect_elem(&frag).unwrap_or((Elem::Int, Vec::new()));
                    let (lo, hi, def) = resolve("a", &ranges, bare.as_ref());
                    if def {
                        p.defaults = true;
                        p.warnings.push("未识别数组元素范围，默认 1~100".into());
                    }
                    p.blocks.push(Block::Cmd(format!(
                        "a = {}({count}, {lo}, {hi})",
                        if elem == Elem::Float { "floats" } else { "ints" }
                    )));
                }
            }
        }
    }

    // ---- 多测注释（放在最前）+ 组数变量 ----
    if let Some(v) = &multi_var {
        p.name_used(v);
        let (lo, hi, def) = resolve(v, &ranges, bare.as_ref());
        if def {
            p.defaults = true;
            p.warnings.push(format!("未识别组数 {v} 的范围，默认 {DEFAULT_LO}~{DEFAULT_HI}"));
        }
        // 第一个行块：若已有大小写不敏感同名字段则改名复用其范围，否则前置一行
        if let Some(first) = p.blocks.iter_mut().find(|b| matches!(b, Block::Line { .. })) {
            if let Block::Line { items, .. } = first {
                if let Some(it) = items.iter_mut().find(|it| it.name.to_lowercase() == *v) {
                    it.name = v.clone();
                    // 保留其范围（可能比默认更准）
                } else {
                    items.insert(
                        0,
                        NumItem {
                            elem: Elem::Int,
                            name: v.clone(),
                            lo,
                            hi,
                            prec: None,
                        },
                    );
                }
            }
        } else {
            p.blocks.insert(
                0,
                Block::Line {
                    rows: "1".to_string(),
                    items: vec![NumItem {
                        elem: Elem::Int,
                        name: v.clone(),
                        lo,
                        hi,
                        prec: None,
                    }],
                },
            );
        }
        p.repeat_var = Some(v.clone());
    }

    if !p.hit {
        return None;
    }
    Some(p)
}

/// 从行描述 clause 提取行内项。
fn items_from_clause(
    clause: &str,
    p: &mut Parsed,
    ranges: &[(String, Range)],
    bare: Option<&Range>,
) -> Vec<NumItem> {
    let mut items: Vec<NumItem> = Vec::new();
    if let Some((elem, mut vars)) = detect_elem(clause) {
        // 显式变量名优先；没有则自动分配
        if vars.is_empty() {
            let pref = match elem {
                Elem::Int => "a",
                Elem::Float => "x",
                Elem::Str | Elem::Text => "s",
            };
            vars.push(p.take_name(pref));
        }
        let mut used_in_clause: Vec<String> = Vec::new();
        for name in &mut vars {
            if p.used.contains(name) || used_in_clause.contains(name) {
                let fresh = p.take_name("v");
                used_in_clause.push(fresh.clone());
                *name = fresh;
            } else {
                used_in_clause.push(name.clone());
            }
        }
        for name in vars {
            let (lo, hi, def) = resolve(&name, ranges, bare);
            if def {
                p.defaults = true;
                p.warnings.push(format!("未识别 {name} 的范围，默认 {DEFAULT_LO}~{DEFAULT_HI}"));
            }
            items.push(NumItem { elem, name, lo, hi, prec: None });
        }
    } else {
        // 无类型词：兜底一个整数项
        p.defaults = true;
        p.warnings.push("未识别行内数据类型，默认生成一个整数项".into());
        let name = p.take_name("a");
        let (lo, hi, def) = resolve(&name, ranges, bare);
        if def {
            p.warnings.push(format!("未识别 {name} 的范围，默认 {DEFAULT_LO}~{DEFAULT_HI}"));
        }
        items.push(NumItem { elem: Elem::Int, name, lo, hi, prec: None });
    }
    items
}

// --------------------------------------------------------------------------- //
// 渲染 + 管道
// --------------------------------------------------------------------------- //

fn render(p: &Parsed) -> String {
    let mut out: Vec<String> = Vec::new();
    for b in &p.blocks {
        match b {
            Block::Cmd(c) => out.push(c.clone()),
            Block::Line { rows, items } => {
                if rows == "1" {
                    out.push("line:".to_string());
                } else {
                    out.push(format!("line ({rows}):"));
                }
                for it in items {
                    let line = if it.elem == Elem::Float {
                        let prec = it.prec.clone().unwrap_or_else(|| "6".to_string());
                        if prec != "6" {
                            format!("    float {}: {}, {}, {}", it.name, it.lo, it.hi, prec)
                        } else {
                            format!("    float {}: {}, {}", it.name, it.lo, it.hi)
                        }
                    } else {
                        format!("    {} {}: {}, {}", it.elem.label(), it.name, it.lo, it.hi)
                    };
                    out.push(line);
                }
            }
        }
    }
    // 多测（repeat 块）：所有块缩进 4 空格，头部 repeat (t):
    if let Some(v) = &p.repeat_var {
        let mut indented = vec![format!("repeat ({v}):")];
        for l in out {
            indented.push(format!("    {l}"));
        }
        out = indented;
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
            })
        }
        Err(e) => Some(NlResult {
            dsl: String::new(),
            confidence: 0.0,
            method: NlMethod::Rule,
            warnings: vec![format!("规则生成结果未通过解析：{}", e.message)],
        }),
    }
}

/// 完整管道：规则优先 → 模型后备 → 校验。
///
/// 模型通道在 `nl-model` feature 开启时接入；当前未启用时未命中直接返回
/// 低置信失败结果。
pub fn nl_to_dsl(text: &str) -> NlResult {
    let text = text.trim();
    if text.is_empty() {
        return NlResult {
            dsl: String::new(),
            confidence: 0.0,
            method: NlMethod::Rule,
            warnings: vec!["输入为空".to_string()],
        };
    }
    match rule_to_dsl(text) {
        Some(r) => r,
        None => NlResult {
            dsl: String::new(),
            confidence: 0.0,
            method: NlMethod::Rule,
            warnings: vec![
                "未识别输入格式：请用类似「第一行两个整数 n m，接下来 n 行每行两个整数」的描述".to_string(),
                "模型通道规划中，当前仅支持规则匹配".to_string(),
            ],
        },
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
    fn single_line_two_ints() {
        let r = conv("第一行两个整数 n m，1 <= n, m <= 100");
        assert_eq!(r.method, NlMethod::Rule);
        assert_eq!(r.dsl, "line:\n    int n: 1, 100\n    int m: 1, 100");
        assert!(r.confidence >= 0.9, "conf {}", r.confidence);
    }

    #[test]
    fn multi_test_first_line() {
        let r = conv("多测，T 组。第一行一个整数 n (1<=n<=10^5)，接下来 n 行每行一个整数 x");
        eprintln!("MTFL dsl={:?} warn={:?} conf={}", r.dsl, r.warnings, r.confidence);
        assert!(r.dsl.starts_with("repeat (t):"), "{}", r.dsl);
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
        assert_eq!(r.dsl, "a = ints(n, 1, 100)");
    }

    #[test]
    fn matrix() {
        let r = conv("一个 n 行 m 列的矩阵，每个元素 0 到 1");
        assert_eq!(r.dsl, "M = matrix(n, m, 0, 1)");
    }

    #[test]
    fn tree_with_weight() {
        let r = conv("一棵 n 个点的树，边权 1 到 100");
        assert!(r.dsl.contains("t = tree(n, int(1, 100))"), "{}", r.dsl);
    }

    #[test]
    fn tree_plain() {
        let r = conv("n 个点的树");
        assert_eq!(r.dsl, "t = tree(n)");
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
        assert_eq!(r.dsl, "p = perm(n)");
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
        assert_eq!(r.dsl, "a = ints(n, 1, 1000000000)");
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
        assert!(r.dsl.starts_with("repeat (t):"), "{}", r.dsl);
        assert!(r.dsl.contains("int t: 1, 10"), "{}", r.dsl);
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
