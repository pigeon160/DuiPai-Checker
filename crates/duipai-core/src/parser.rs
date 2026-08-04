//! 行级 DSL 解析：文本 -> IR（移植 legacy/dsl.py 的 parse）。
//!
//! 支持全部命令：`int` `float` `ints` `floats` `matrix` `matf` `perm` `tree`
//! `graph` `str` `strs` `binseq` `intervals` `points` `ring` `base_ring`。

use std::collections::{HashMap, HashSet};

use crate::ast::{Config, ElemType, GraphType, Item, RepeatMode, VarKind, Weight};
use crate::error::{DslError, DslResult};
use crate::expr::{tokenize, Tok};

/// 全部命令（保留字，不可用作变量名）。
pub const KNOWN_COMMANDS: &[&str] = &[
    "int", "float", "ints", "floats", "matrix", "matf", "perm", "tree", "graph",
    "str", "strs", "binseq", "intervals", "points", "ring", "base_ring",
];

const REPEAT_COMMENT: &str = "多测模式";

const DEFAULT_CHARSET: &str = "abcdefghijklmnopqrstuvwxyz";

fn is_name(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn is_known(cmd: &str) -> bool {
    KNOWN_COMMANDS.contains(&cmd)
}

/// token -> 文本（与 legacy `_tok_text` 一致，保证序列化格式化）。
fn tok_text(tok: &Tok) -> String {
    match tok {
        Tok::Num(v) => v.to_string(),
        Tok::Name(s) => s.clone(),
        Tok::Str(s) => format!("\"{s}\""),
        Tok::Op(s) if s == "*" || s == "//" || s == "=" => format!(" {s} "),
        Tok::Op(s) => s.clone(),
        Tok::Comma => ", ".to_string(),
    }
}

fn expr_text(toks: &[Tok]) -> String {
    toks.iter().map(tok_text).collect::<String>().trim().to_string()
}

/// 把括号内参数按顶层逗号切成若干子 token 列表。
fn split_args(toks: &[Tok]) -> Vec<Vec<Tok>> {
    let mut args: Vec<Vec<Tok>> = Vec::new();
    let mut depth = 0usize;
    let mut cur: Vec<Tok> = Vec::new();
    for tok in toks {
        match tok {
            Tok::Op(s) if s == "(" => {
                depth += 1;
                cur.push(tok.clone());
            }
            Tok::Op(s) if s == ")" => {
                depth -= 1;
                cur.push(tok.clone());
            }
            Tok::Comma if depth == 0 => {
                args.push(std::mem::take(&mut cur));
            }
            _ => cur.push(tok.clone()),
        }
    }
    args.push(cur);
    args
}

/// 拆分为（位置参数列表, 关键字参数字典）。含顶层 `=` 的参数视为关键字参数。
fn split_kw_args(toks: &[Tok]) -> DslResult<(Vec<Vec<Tok>>, HashMap<String, String>)> {
    let mut raw = split_args(toks);
    if let Some(last) = raw.last() {
        if last.is_empty() {
            raw.pop();
        }
    }
    let mut pos: Vec<Vec<Tok>> = Vec::new();
    let mut kw: HashMap<String, String> = HashMap::new();
    for arg in raw {
        // 找顶层 '='
        let mut eq_idx: Option<usize> = None;
        let mut depth = 0usize;
        for (i, tok) in arg.iter().enumerate() {
            match tok {
                Tok::Op(s) if s == "(" => depth += 1,
                Tok::Op(s) if s == ")" => depth -= 1,
                Tok::Op(s) if s == "=" && depth == 0 => {
                    eq_idx = Some(i);
                    break;
                }
                _ => {}
            }
        }
        match eq_idx {
            Some(i) if i > 0 => {
                let kname = expr_text(&arg[..i]);
                if !is_name(&kname) {
                    return Err(DslError::bare(format!("非法关键字参数名：{kname}")));
                }
                if kw.contains_key(&kname) {
                    return Err(DslError::bare(format!("关键字参数重复：{kname}")));
                }
                let mut v = expr_text(&arg[i + 1..]);
                if kname == "type" {
                    v = v.trim().trim_matches('"').trim().to_string();
                }
                kw.insert(kname, v);
            }
            _ => pos.push(arg),
        }
    }
    Ok((pos, kw))
}

/// 判断表达式 token 是否为 `fname( ... )` 调用，返回参数文本列表（失败返回 None）。
fn is_range_call(toks: &[Tok], fname: &str) -> Option<Vec<String>> {
    if toks.len() < 3 {
        return None;
    }
    if !matches!(toks.first(), Some(Tok::Name(n)) if n == fname) {
        return None;
    }
    if !matches!(toks.get(1), Some(Tok::Op(s)) if s == "(") {
        return None;
    }
    if !matches!(toks.last(), Some(Tok::Op(s)) if s == ")") {
        return None;
    }
    let inner = split_args(&toks[2..toks.len() - 1]);
    if inner.is_empty() || inner.last().map_or(true, |x| x.is_empty()) {
        return None;
    }
    let n = inner.len();
    let ok = match fname {
        "int" => n == 2,
        "float" => (2..=3).contains(&n),
        _ => return None,
    };
    if !ok {
        return None;
    }
    Some(inner.iter().map(|x| expr_text(x)).collect())
}

/// 边权 / 节点权值参数 token -> 权值描述。`none` 或空表示无。
fn weight_to_item(toks: &[Tok]) -> DslResult<Option<Weight>> {
    if toks.is_empty() {
        return Ok(None);
    }
    if matches!(toks, [Tok::Name(n)] if n.as_str() == "none") {
        return Ok(None);
    }
    if let Some(r) = is_range_call(toks, "int") {
        return Ok(Some(Weight {
            kind: ElemType::Int,
            min: r[0].clone(),
            max: r[1].clone(),
            prec: "6".to_string(),
        }));
    }
    if let Some(r) = is_range_call(toks, "float") {
        return Ok(Some(Weight {
            kind: ElemType::Float,
            min: r[0].clone(),
            max: r[1].clone(),
            prec: if r.len() == 3 { r[2].clone() } else { "6".to_string() },
        }));
    }
    Err(DslError::bare(
        "边权参数必须是 int(a,b) 或 float(a,b[,prec])",
    ))
}

/// 解析 `w=` / `val=` 关键字参数（值为表达式文本）。
fn weight_from_kw(v: &str) -> DslResult<Option<Weight>> {
    let toks = tokenize(v)?;
    weight_to_item(&toks)
}

/// 解析单条命令的参数为统一配置项。
fn parse_cmd(name: &str, cmd: &str, args: &[Tok]) -> DslResult<Item> {
    let (mut pos, kw) = split_kw_args(args)?;

    let arity = |pos: &[Vec<Tok>], lo: usize, hi: usize| -> DslResult<()> {
        if !(lo..=hi).contains(&pos.len()) {
            return Err(DslError::bare(format!(
                "{cmd} 需要 {lo}~{hi} 个位置参数，实际 {} 个",
                pos.len()
            )));
        }
        Ok(())
    };

    let kw_expr = |k: &str| kw.get(k).cloned();

    let item = match cmd {
        "int" => {
            arity(&pos, 2, 2)?;
            VarKind::Int {
                min: expr_text(&pos[0]),
                max: expr_text(&pos[1]),
            }
        }
        "float" => {
            arity(&pos, 2, 3)?;
            VarKind::Float {
                min: expr_text(&pos[0]),
                max: expr_text(&pos[1]),
                prec: kw_expr("prec").unwrap_or_else(|| {
                    if pos.len() == 3 {
                        expr_text(&pos[2])
                    } else {
                        "6".to_string()
                    }
                }),
            }
        }
        "ints" | "floats" => {
            arity(&pos, 3, 4)?;
            VarKind::Array {
                elem_type: if cmd == "ints" { ElemType::Int } else { ElemType::Float },
                el_min: expr_text(&pos[1]),
                el_max: expr_text(&pos[2]),
                prec: kw_expr("prec").unwrap_or_else(|| {
                    if pos.len() == 4 {
                        expr_text(&pos[3])
                    } else {
                        "6".to_string()
                    }
                }),
                rows: "1".to_string(),
                cols: expr_text(&pos[0]),
            }
        }
        "matrix" | "matf" => {
            arity(&pos, 4, 5)?;
            VarKind::Array {
                elem_type: if cmd == "matrix" { ElemType::Int } else { ElemType::Float },
                el_min: expr_text(&pos[2]),
                el_max: expr_text(&pos[3]),
                prec: kw_expr("prec").unwrap_or_else(|| {
                    if pos.len() == 5 {
                        expr_text(&pos[4])
                    } else {
                        "6".to_string()
                    }
                }),
                rows: expr_text(&pos[0]),
                cols: expr_text(&pos[1]),
            }
        }
        "perm" => {
            arity(&pos, 1, 1)?;
            VarKind::Perm {
                n: expr_text(&pos[0]),
            }
        }
        "str" | "strs" => {
            arity(&pos, 1, 3)?;
            let mut charset = kw_expr("charset").map(|s| s.trim().trim_matches('"').to_string());
            if charset.is_none() {
                for (i, p) in pos.iter().enumerate() {
                    if matches!(p.as_slice(), [Tok::Str(_)]) {
                        charset = Some(expr_text(p).trim_matches('"').to_string());
                        pos.remove(i);
                        break;
                    }
                }
            }
            let charset = charset.unwrap_or_else(|| DEFAULT_CHARSET.to_string());
            if cmd == "str" {
                arity(&pos, 1, 1)?;
                VarKind::String {
                    rows: "1".to_string(),
                    cols: expr_text(&pos[0]),
                    charset,
                }
            } else {
                arity(&pos, 2, 2)?;
                VarKind::String {
                    rows: expr_text(&pos[0]),
                    cols: expr_text(&pos[1]),
                    charset,
                }
            }
        }
        "binseq" => {
            arity(&pos, 2, 2)?;
            VarKind::Binseq {
                n: expr_text(&pos[0]),
                k: expr_text(&pos[1]),
            }
        }
        "intervals" => {
            arity(&pos, 3, 3)?;
            VarKind::Intervals {
                n: expr_text(&pos[0]),
                lo: expr_text(&pos[1]),
                hi: expr_text(&pos[2]),
            }
        }
        "points" => {
            arity(&pos, 5, 5)?;
            VarKind::Points {
                n: expr_text(&pos[0]),
                xlo: expr_text(&pos[1]),
                xhi: expr_text(&pos[2]),
                ylo: expr_text(&pos[3]),
                yhi: expr_text(&pos[4]),
            }
        }
        "tree" => {
            arity(&pos, 1, 2)?;
            let mut w = None;
            let mut val = None;
            if pos.len() == 2 {
                w = weight_to_item(&pos[1])?;
            }
            if let Some(v) = kw_expr("w") {
                w = weight_from_kw(&v)?;
            }
            if let Some(v) = kw_expr("val") {
                val = weight_from_kw(&v)?;
            }
            VarKind::Tree {
                n: expr_text(&pos[0]),
                w,
                val,
            }
        }
        "ring" => {
            arity(&pos, 1, 1)?;
            VarKind::Graph {
                gtype: GraphType::Ring,
                n: expr_text(&pos[0]),
                m: expr_text(&pos[0]),
                directed: false,
                connected: true,
                k: None,
                w: None,
                val: kw_expr("val").map(|v| weight_from_kw(&v)).transpose()?.flatten(),
            }
        }
        "base_ring" => {
            arity(&pos, 2, 2)?;
            VarKind::Graph {
                gtype: GraphType::BaseRing,
                n: expr_text(&pos[0]),
                m: expr_text(&pos[0]),
                directed: false,
                connected: true,
                k: Some(expr_text(&pos[1])),
                w: None,
                val: kw_expr("val").map(|v| weight_from_kw(&v)).transpose()?.flatten(),
            }
        }
        "graph" => {
            arity(&pos, 3, 5)?;
            let gtype = match kw_expr("type").as_deref() {
                Some("dag") => GraphType::Dag,
                Some("bipartite") => GraphType::Bipartite,
                Some(t) => {
                    return Err(DslError::bare(format!("未知图类型：{t}")));
                }
                _ => GraphType::General,
            };
            let directed = match gtype {
                GraphType::Dag => true,
                GraphType::Bipartite => false,
                _ => {
                    let d = expr_text(&pos[2]);
                    d == "1" || d.eq_ignore_ascii_case("true")
                }
            };
            let connected = {
                let c = expr_text(&pos[3]);
                c == "1" || c.eq_ignore_ascii_case("true")
            };
            let mut w = None;
            let mut val = None;
            if pos.len() == 5 {
                w = weight_to_item(&pos[4])?;
            }
            if let Some(v) = kw_expr("w") {
                w = weight_from_kw(&v)?;
            }
            if let Some(v) = kw_expr("val") {
                val = weight_from_kw(&v)?;
            }
            VarKind::Graph {
                gtype,
                n: expr_text(&pos[0]),
                m: expr_text(&pos[1]),
                directed,
                connected,
                k: None,
                w,
                val,
            }
        }
        _ => return Err(DslError::bare(format!("未知命令：{cmd}"))),
    };
    Ok(Item {
        name: name.to_string(),
        kind: item,
        line: 0, // 行号由 parse 回填
    })
}

/// 解析单条语句行。`seen` 累积已定义变量名。
fn parse_statement(line: &str, lineno: usize, seen: &mut HashSet<String>) -> DslResult<Item> {
    let line = line.trim();
    let eq = match line.find('=') {
        Some(i) => i,
        None => return Err(DslError::at(lineno, "语句缺少 '='")),
    };
    let name = line[..eq].trim();
    let rhs = line[eq + 1..].trim();
    if name.is_empty() {
        return Err(DslError::at(lineno, "缺少变量名"));
    }
    if !is_name(name) {
        return Err(DslError::at(lineno, format!("非法变量名：{name}")));
    }
    if seen.contains(name) {
        return Err(DslError::at(lineno, format!("变量名重复：{name}")));
    }
    if is_known(name) {
        return Err(DslError::at(lineno, format!("变量名不能是保留字：{name}")));
    }
    let toks = tokenize(rhs).map_err(|e| e.with_line(lineno))?;
    let cmd = match toks.first() {
        Some(Tok::Name(c)) => c.clone(),
        _ => return Err(DslError::at(lineno, "语句右侧必须是命令")),
    };
    if !is_known(&cmd) {
        return Err(DslError::at(lineno, format!("未知命令：{cmd}")));
    }
    if !matches!(toks.get(1), Some(Tok::Op(s)) if s == "(") {
        return Err(DslError::at(lineno, format!("{cmd} 命令缺少左括号")));
    }
    if !matches!(toks.last(), Some(Tok::Op(s)) if s == ")") {
        return Err(DslError::at(lineno, format!("{cmd} 命令缺少右括号")));
    }
    let item = parse_cmd(name, &cmd, &toks[2..toks.len() - 1]).map_err(|e| e.with_line(lineno))?;
    seen.insert(name.to_string());
    Ok(Item { line: lineno, ..item })
}

/// 从 DSL 文本顶部读取多测模式注释。
/// 识别 `# 多测模式`、`# 多测模式：重复 3 次`（冒号中英文均可）。
fn parse_repeat(lines: &[&str]) -> Option<RepeatMode> {
    for raw in lines.iter().take(8) {
        let s = raw.trim();
        if !s.starts_with('#') {
            continue;
        }
        let body = s.trim_start_matches('#').trim();
        if body == REPEAT_COMMENT {
            return Some(RepeatMode {
                enabled: true,
                count: "1".to_string(),
            });
        }
        let rest = body
            .strip_prefix(REPEAT_COMMENT)
            .and_then(|r| {
                let r = r.strip_prefix('：').or_else(|| r.strip_prefix(':'));
                r.map(|x| x.trim())
            });
        if let Some(rest) = rest {
            if let Some(count) = parse_repeat_count(rest) {
                return Some(RepeatMode {
                    enabled: true,
                    count,
                });
            }
        }
    }
    None
}

/// 解析 `重复 N 次?`（对应 legacy 正则 `重复\s*(\d+)\s*次?`）。
fn parse_repeat_count(rest: &str) -> Option<String> {
    let rest = rest.strip_prefix("重复")?.trim_start();
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    let after = rest[digits.len()..].trim_start();
    if after == "次" || after.is_empty() {
        Some(digits)
    } else {
        None
    }
}

/// 解析 DSL 文本，返回 IR 配置；语法/语义错误带行号。
pub fn parse(text: &str) -> DslResult<Config> {
    let lines: Vec<&str> = text.lines().collect();
    let repeat = parse_repeat(&lines);
    let mut items: Vec<Item> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for (idx, raw) in lines.iter().enumerate() {
        let lineno = idx + 1;
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let indent = raw.len() - raw.trim_start_matches(' ').len();
        if indent > 0 {
            return Err(DslError::at(lineno, "缩进不正确（顶层语句不能缩进）"));
        }
        items.push(parse_statement(raw, lineno, &mut seen)?);
    }
    Ok(Config { repeat, items })
}
