//! 行级 DSL 解析：文本 -> IR（移植 legacy/dsl.py 的 parse）。
//!
//! 支持全部命令：`int` `float` `ints` `floats` `matrix` `matf` `perm` `tree`
//! `graph` `str` `strs` `binseq` `intervals` `points` `ring` `base_ring`。

use std::collections::{HashMap, HashSet};

use crate::ast::{Config, ElemType, GraphType, Item, MultiPart, RepeatMode, VarKind, Weight};
use crate::error::{DslError, DslResult};
use crate::expr::{tokenize, Tok};

/// 全部命令（保留字，不可用作变量名）。
pub const KNOWN_COMMANDS: &[&str] = &[
    "int", "float", "ints", "floats", "matrix", "matf", "perm", "tree", "graph",
    "str", "strs", "binseq", "intervals", "points", "ring", "base_ring", "repeat",
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
/// 括号不平衡（多余 `)`）时不panic：`depth` 不为 0 才递减，多余括号进入当前参数，
/// 由后续 arity / 表达式语法检查给出友好错误。
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
                if depth > 0 {
                    depth -= 1;
                }
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
                Tok::Op(s) if s == ")" => {
                    if depth > 0 {
                        depth -= 1;
                    }
                }
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

/// 解析单条命令的参数为统一类型（不处理变量名）。
fn parse_cmd(cmd: &str, args: &[Tok]) -> DslResult<VarKind> {
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
                w: kw_expr("w").map(|v| weight_from_kw(&v)).transpose()?.flatten(),
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
                w: kw_expr("w").map(|v| weight_from_kw(&v)).transpose()?.flatten(),
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
    Ok(item)
}

/// 校验并登记一个变量名（重复/保留字检查）。
fn check_name(name: &str, lineno: usize, seen: &mut HashSet<String>) -> DslResult<()> {
    if !is_name(name) {
        return Err(DslError::at(lineno, format!("非法变量名：{name}")));
    }
    if seen.contains(name) {
        return Err(DslError::at(lineno, format!("变量名重复：{name}")));
    }
    if is_known(name) {
        return Err(DslError::at(lineno, format!("变量名不能是保留字：{name}")));
    }
    Ok(())
}

/// 检查表达式中的函数调用均为已知函数（int/float）。
fn check_known_calls(node: &crate::expr::ExprNode) -> DslResult<()> {
    match node {
        crate::expr::ExprNode::Call { name, args } => {
            if name != "int" && name != "float" {
                return Err(DslError::bare(format!("未知函数调用：{name}")));
            }
            for a in args {
                check_known_calls(a)?;
            }
        }
        crate::expr::ExprNode::Neg(x) => check_known_calls(x)?,
        crate::expr::ExprNode::Bin { l, r, .. } => {
            check_known_calls(l)?;
            check_known_calls(r)?;
        }
        _ => {}
    }
    Ok(())
}

/// 解析单个 `name = expr` 赋值组，返回 (name, 类型)。
///
/// RHS 判定优先级：
/// 1. 命令调用（ints/perm/tree/…）→ 命令语义（原行为）
/// 2. `int(a,b[,p])` / `float(a,b[,p])` 简单形式 → Int/Float（GUI 快捷编辑态）
/// 3. 其他任意标量表达式 → Scalar（`n = 2*m+1`）
fn parse_assign(rhs_toks: &[Tok], lineno: usize) -> DslResult<VarKind> {
    // 命令调用？
    if let Some(Tok::Name(cmd)) = rhs_toks.first() {
        if is_known(cmd) && matches!(rhs_toks.get(1), Some(Tok::Op(s)) if s == "(") {
            if let Some(Tok::Op(s)) = rhs_toks.last() {
                if s == ")" {
                    if cmd == "repeat" {
                        return Err(DslError::at(
                            lineno,
                            "repeat(N) 只能用于一行多赋值（如 n = int(1, 5), m = 2*n, repeat(3)）",
                        ));
                    }
                    return parse_cmd(cmd, &rhs_toks[2..rhs_toks.len() - 1])
                        .map_err(|e| e.with_line(lineno));
                }
            }
        }
    }
    // int(a,b[,p]) / float(a,b[,p]) 简单形式 -> Int/Float
    if let Some(r) = is_range_call(rhs_toks, "int") {
        if r.len() == 2 {
            return Ok(VarKind::Int {
                min: r[0].clone(),
                max: r[1].clone(),
            });
        }
    }
    if let Some(r) = is_range_call(rhs_toks, "float") {
        let prec = if r.len() == 3 { r[2].clone() } else { "6".to_string() };
        return Ok(VarKind::Float {
            min: r[0].clone(),
            max: r[1].clone(),
            prec,
        });
    }
    // 其他标量表达式
    let expr = expr_text(rhs_toks);
    if expr.is_empty() {
        return Err(DslError::at(lineno, "缺少表达式"));
    }
    let node = crate::expr::parse_expr(&expr).map_err(|e| e.with_line(lineno))?;
    check_known_calls(&node).map_err(|e| e.with_line(lineno))?;
    Ok(VarKind::Scalar { expr })
}

/// 解析单条语句行。`seen` 累积已定义变量名。
///
/// 一行 = 顶层逗号分隔的多个 `name = expr` 赋值：
///   - `n = int(1, 100)`                     单赋值
///   - `n = 2*m + 1`                          标量表达式
///   - `n = int(1, 100), m = 2*n`             多赋值（一行输出多个数，每项可命名）
fn parse_statement(line: &str, lineno: usize, seen: &mut HashSet<String>) -> DslResult<Item> {
    let line = line.trim();
    let toks = tokenize(line).map_err(|e| e.with_line(lineno))?;

    // 顶层逗号切分成若干个 `name = expr` 赋值组（最后可为 `repeat(N)` 行数标记）
    let groups = split_args(&toks);
    let mut assigns: Vec<(String, &[Tok])> = Vec::with_capacity(groups.len());
    let mut repeat: Option<String> = None;
    for group in &groups {
        if group.is_empty() {
            return Err(DslError::at(lineno, "语句缺少 '='"));
        }
        // 组内找顶层 '='
        let mut eq_idx: Option<usize> = None;
        let mut depth = 0usize;
        for (i, tok) in group.iter().enumerate() {
            match tok {
                Tok::Op(s) if s == "(" => depth += 1,
                Tok::Op(s) if s == ")" => {
                    if depth > 0 {
                        depth -= 1;
                    }
                }
                Tok::Op(s) if s == "=" && depth == 0 => {
                    eq_idx = Some(i);
                    break;
                }
                _ => {}
            }
        }
        let eq = match eq_idx {
            Some(i) => i,
            None => {
                // 无 '=' 的组只允许 `repeat(N)`（多赋值行数标记）
                if matches!(group.as_slice(), [Tok::Name(n), Tok::Op(o), .., Tok::Op(c)]
                    if n.as_str() == "repeat" && o == "(" && c == ")")
                {
                    if repeat.is_some() {
                        return Err(DslError::at(lineno, "repeat(N) 只能出现一次"));
                    }
                    if group.len() < 4 {
                        return Err(DslError::at(lineno, "repeat(N) 缺少行数表达式"));
                    }
                    let inner = expr_text(&group[2..group.len() - 1]);
                    if inner.is_empty() {
                        return Err(DslError::at(lineno, "repeat(N) 缺少行数表达式"));
                    }
                    crate::expr::parse_expr(&inner).map_err(|e| e.with_line(lineno))?;
                    repeat = Some(inner);
                    continue;
                }
                return Err(DslError::at(lineno, "语句缺少 '='"));
            }
        };
        let name = expr_text(&group[..eq]);
        if name.is_empty() {
            return Err(DslError::at(lineno, "缺少变量名"));
        }
        check_name(&name, lineno, seen)?;
        assigns.push((name, &group[eq + 1..]));
    }

    // 语句内部重名检测
    let mut local: HashSet<String> = HashSet::new();
    for (name, _) in &assigns {
        if !local.insert(name.clone()) {
            return Err(DslError::at(lineno, format!("变量名重复：{name}")));
        }
    }

    // 单赋值（repeat 标记只在多赋值有效）
    if assigns.len() == 1 {
        if repeat.is_some() {
            return Err(DslError::at(lineno, "repeat(N) 只能用于一行多赋值"));
        }
        let (name, rhs) = assigns.pop().unwrap();
        let kind = parse_assign(rhs, lineno)?;
        seen.insert(name.clone());
        return Ok(Item {
            name,
            kind,
            line: lineno,
        });
    }

    // 多赋值：每项必须是单行标量（命令限 int/float，其余按表达式）
    let mut parts: Vec<MultiPart> = Vec::with_capacity(assigns.len());
    for (name, rhs) in &assigns {
        let kind = parse_assign(rhs, lineno)?;
        let expr = match &kind {
            VarKind::Int { min, max } => format!("int({min}, {max})"),
            VarKind::Float { min, max, prec } => {
                if prec == "6" {
                    format!("float({min}, {max})")
                } else {
                    format!("float({min}, {max}, {prec})")
                }
            }
            VarKind::Scalar { expr } => expr.clone(),
            other => {
                return Err(DslError::at(
                    lineno,
                    format!(
                        "一行多个数时每项必须输出单个数值（{} 输出多行）",
                        kind_name(other)
                    ),
                ));
            }
        };
        parts.push(MultiPart {
            name: name.clone(),
            expr,
        });
    }
    for (name, _) in &assigns {
        seen.insert(name.clone());
    }
    Ok(Item {
        name: assigns
            .iter()
            .map(|(n, _)| n.as_str())
            .collect::<Vec<_>>()
            .join(","),
        kind: VarKind::Multi {
            rows: repeat.unwrap_or_else(|| "1".to_string()),
            parts,
        },
        line: lineno,
    })
}

/// 类型中文名（错误消息用）。
fn kind_name(kind: &VarKind) -> &'static str {
    match kind {
        VarKind::Int { .. } => "整数",
        VarKind::Float { .. } => "浮点",
        VarKind::Multi { .. } => "多值",
        VarKind::Scalar { .. } => "表达式",
        VarKind::Array { .. } => "数组",
        VarKind::Perm { .. } => "排列",
        VarKind::String { .. } => "字符串",
        VarKind::Binseq { .. } => "0/1 序列",
        VarKind::Intervals { .. } => "区间",
        VarKind::Points { .. } => "点集",
        VarKind::Tree { .. } => "树",
        VarKind::Graph { .. } => "图",
    }
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
