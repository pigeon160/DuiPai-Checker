//! DSL 解析：文本 -> IR（层级语法）。
//!
//! 顶层语句（缩进 0）：
//!   - 行块：`行 (N):` + 缩进子项（整数/浮点/文本/表达式/字符串）
//!   - 命令：`name = ints(...)` / `perm` / `tree` / `graph` / `ring` / `base_ring` /
//!     `binseq` / `intervals` / `points` / `matrix` / `matf`
//!
//! 整数/浮点/字符串等标量类型**必须**放在行块内（缩进子项），顶层写会报错引导。

use std::collections::{HashMap, HashSet};

use crate::ast::{
    Config, ElemType, GraphType, Item, LineItem, LineItemKind, VarKind, Weight,
};
use crate::error::{DslError, DslResult};
use crate::expr::{tokenize, Tok};

/// 顶层命令。
pub const TOP_COMMANDS: &[&str] = &[
    "ints", "floats", "matrix", "matf", "perm", "binseq", "intervals", "points",
    "tree", "graph", "ring", "base_ring",
];

/// 行块关键字。
pub const LINE_KEYWORD: &str = "line";

/// 行内项类型关键字。
pub const LINE_ITEM_KINDS: &[&str] = &["int", "float", "text", "expr", "str"];

/// 已废弃但保留字（顶层写这些会报错引导）。
pub const RETIRED_COMMANDS: &[&str] = &["int", "float", "str"];

/// 全部保留字（不可用作变量名）。
pub const KNOWN_COMMANDS: &[&str] = &[
    "ints", "floats", "matrix", "matf", "perm", "binseq", "intervals", "points",
    "tree", "graph", "ring", "base_ring", "repeat",
    "line", "int", "float", "text", "expr", "str",
];

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

/// 边权参数 token -> 权值描述。`none` 或空表示无。
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

/// 解析顶层命令的参数为统一类型（不处理变量名）。
fn parse_cmd(cmd: &str, args: &[Tok]) -> DslResult<VarKind> {
    let (pos, kw) = split_kw_args(args)?;

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
        "ints" | "floats" => {
            arity(&pos, 3, 4)?;
            if kw.contains_key("prec") {
                return Err(DslError::bare("精度 prec= 已废弃：请用位置参数（ints(n, 1, 9, 6)）"));
            }
            VarKind::Array {
                elem_type: if cmd == "ints" { ElemType::Int } else { ElemType::Float },
                el_min: expr_text(&pos[1]),
                el_max: expr_text(&pos[2]),
                prec: if pos.len() == 4 {
                    expr_text(&pos[3])
                } else {
                    "6".to_string()
                },
                rows: "1".to_string(),
                cols: expr_text(&pos[0]),
            }
        }
        "matrix" | "matf" => {
            arity(&pos, 4, 5)?;
            if kw.contains_key("prec") {
                return Err(DslError::bare("精度 prec= 已废弃：请用位置参数（matrix(rows, cols, 0, 1, 6)）"));
            }
            VarKind::Array {
                elem_type: if cmd == "matrix" { ElemType::Int } else { ElemType::Float },
                el_min: expr_text(&pos[2]),
                el_max: expr_text(&pos[3]),
                prec: if pos.len() == 5 {
                    expr_text(&pos[4])
                } else {
                    "6".to_string()
                },
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
            let ttype = match kw_expr("type").as_deref() {
                Some("star") => crate::ast::TreeType::Star,
                Some("chain") => crate::ast::TreeType::Chain,
                Some(t) => return Err(DslError::bare(format!("未知树类型：{t}"))),
                _ => crate::ast::TreeType::Random,
            };
            let mut w = None;
            if pos.len() == 2 {
                w = weight_to_item(&pos[1])?;
            }
            if kw.contains_key("w") {
                return Err(DslError::bare("边权 w= 已废弃：请用位置参数（tree(n, int(1,10))）"));
            }
            if kw.contains_key("val") {
                return Err(DslError::bare("节点权值 val= 已移除（树/图只输出边）"));
            }
            VarKind::Tree { n: expr_text(&pos[0]), ttype, w }
        }
        "ring" => {
            arity(&pos, 1, 2)?;
            if kw.contains_key("val") {
                return Err(DslError::bare("节点权值 val= 已移除（树/图只输出边）"));
            }
            let mut w = None;
            if pos.len() == 2 {
                w = weight_to_item(&pos[1])?;
            }
            if kw.contains_key("w") {
                return Err(DslError::bare("边权 w= 已废弃：请用位置参数（ring(n, int(1,10))）"));
            }
            if kw.contains_key("val") {
                return Err(DslError::bare("节点权值 val= 已移除（树/图只输出边）"));
            }
            VarKind::Graph {
                gtype: GraphType::Ring,
                n: expr_text(&pos[0]),
                m: expr_text(&pos[0]),
                directed: false,
                connected: true,
                multi: false,
                loop_: false,
                k: None,
                w,
            }
        }
        "base_ring" => {
            arity(&pos, 2, 3)?;
            if kw.contains_key("val") {
                return Err(DslError::bare("节点权值 val= 已移除（树/图只输出边）"));
            }
            let mut w = None;
            if pos.len() == 3 {
                w = weight_to_item(&pos[2])?;
            }
            if kw.contains_key("w") {
                return Err(DslError::bare("边权 w= 已废弃：请用位置参数（base_ring(n, k, int(1,10))）"));
            }
            if kw.contains_key("val") {
                return Err(DslError::bare("节点权值 val= 已移除（树/图只输出边）"));
            }
            VarKind::Graph {
                gtype: GraphType::BaseRing,
                n: expr_text(&pos[0]),
                m: expr_text(&pos[0]),
                directed: false,
                connected: true,
                multi: false,
                loop_: false,
                k: Some(expr_text(&pos[1])),
                w,
            }
        }
        "graph" => {
            arity(&pos, 3, 5)?;
            if kw.contains_key("val") {
                return Err(DslError::bare("节点权值 val= 已移除（树/图只输出边）"));
            }
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
            let kw_bool = |k: &str| {
                kw_expr(k).map(|v| v == "1" || v.eq_ignore_ascii_case("true")).unwrap_or(false)
            };
            let multi = kw_bool("multi");
            let loop_ = kw_bool("loop");
            let mut w = None;
            if pos.len() == 5 {
                w = weight_to_item(&pos[4])?;
            }
            if kw.contains_key("w") {
                return Err(DslError::bare("边权 w= 已废弃：请用位置参数（graph(n, m, d, c, int(1,10))）"));
            }
            VarKind::Graph {
                gtype,
                n: expr_text(&pos[0]),
                m: expr_text(&pos[1]),
                directed,
                connected,
                multi,
                loop_,
                k: None,
                w,
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

/// 解析行内项：`类型 名字: 参数`。
fn parse_line_item(line: &str, lineno: usize, seen: &mut HashSet<String>) -> DslResult<LineItem> {
    let toks = tokenize(line).map_err(|e| e.with_line(lineno))?;
    let kind_kw = match toks.first() {
        Some(Tok::Name(k)) if LINE_ITEM_KINDS.contains(&k.as_str()) => k.as_str(),
        Some(Tok::Name(k)) if k == LINE_KEYWORD => {
            return Err(DslError::at(lineno, "行块不能嵌套在行内"));
        }
        Some(Tok::Name(_)) => {
            return Err(DslError::at(lineno, "行内项类型必须是 int / float / text / expr / str"));
        }
        _ => return Err(DslError::at(lineno, "行内项必须是「类型 名字: 参数」形式")),
    };
    let name = match toks.get(1) {
        Some(Tok::Name(n)) => n.clone(),
        _ => return Err(DslError::at(lineno, "行内项缺少名字（类型 名字: 参数）")),
    };
    check_name(&name, lineno, seen)?;
    if !matches!(toks.get(2), Some(Tok::Op(s)) if s == ":") {
        return Err(DslError::at(lineno, "行内项缺少冒号（类型 名字: 参数）"));
    }    let args = split_args(&toks[3..]);
    let arity = |lo: usize, hi: usize| -> DslResult<()> {
        if !(lo..=hi).contains(&args.len()) {
            return Err(DslError::at(
                lineno,
                format!("{kind_kw} 项需要 {lo}~{hi} 个参数，实际 {} 个", args.len()),
            ));
        }
        Ok(())
    };

    let kind = match kind_kw {
        "int" => {
            arity(2, 2)?;
            LineItemKind::Int {
                min: expr_text(&args[0]),
                max: expr_text(&args[1]),
            }
        }
        "float" => {
            arity(2, 3)?;
            LineItemKind::Float {
                min: expr_text(&args[0]),
                max: expr_text(&args[1]),
                prec: if args.len() == 3 { expr_text(&args[2]) } else { "6".to_string() },
            }
        }
        "text" => {
            arity(1, 1)?;
            let text = match args[0].as_slice() {
                [Tok::Str(s)] => s.clone(),
                _ => return Err(DslError::at(lineno, "text 项参数必须是字符串字面量（\"内容\"）")),
            };
            if text.contains('"') || text.contains('\n') {
                return Err(DslError::at(lineno, "文本内容不能包含双引号或换行"));
            }
            LineItemKind::Text { text }
        }
        "expr" => {
            arity(1, 1)?;
            let expr = expr_text(&args[0]);
            if expr.is_empty() {
                return Err(DslError::at(lineno, "expr 项缺少表达式"));
            }
            let node = crate::expr::parse_expr(&expr).map_err(|e| e.with_line(lineno))?;
            check_known_calls(&node).map_err(|e| e.with_line(lineno))?;
            LineItemKind::Scalar { expr }
        }
        "str" => {
            arity(1, 2)?;
            let len = expr_text(&args[0]);
            let mut charset: Option<String> = None;
            if args.len() == 2 {
                match args[1].as_slice() {
                    [Tok::Str(s)] => charset = Some(s.clone()),
                    _ => {
                        return Err(DslError::at(
                            lineno,
                            "str 项字符集必须是字符串字面量（\"ab\"）",
                        ));
                    }
                }
            }
            if len.is_empty() {
                return Err(DslError::at(lineno, "str 项缺少长度"));
            }
            let charset = charset.unwrap_or_else(|| DEFAULT_CHARSET.to_string());
            LineItemKind::Str { len, charset }
        }
        _ => unreachable!("LINE_ITEM_KINDS 已过滤"),
    };
    seen.insert(name.clone());
    Ok(LineItem { name, kind })
}

/// 解析行块：`行 (N):` + 缩进子项。返回 (Item, 消费的行数)。
///
/// `base_indent` 为声明行的缩进（顶层 0；repeat 块内为其子语句缩进），
/// 子项必须缩进更深；缩进等于或小于 base 的行属于外层（结束本块）。
fn parse_line_block(
    lines: &[&str],
    start: usize,
    base_indent: usize,
    seen: &mut HashSet<String>,
) -> DslResult<(Item, usize)> {
    let lineno = start + 1;
    let toks = tokenize(lines[start]).map_err(|e| e.with_line(lineno))?;
    // 行 [ ( N ) ] :
    let mut p = 1usize;
    let mut rows = "1".to_string();
    if matches!(toks.get(p), Some(Tok::Op(s)) if s == "(") {
        p += 1;
        let mut depth = 1usize;
        let mut inner: Vec<Tok> = Vec::new();
        while p < toks.len() {
            match &toks[p] {
                Tok::Op(s) if s == "(" => {
                    depth += 1;
                    inner.push(toks[p].clone());
                }
                Tok::Op(s) if s == ")" => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    inner.push(toks[p].clone());
                }
                t => inner.push(t.clone()),
            }
            p += 1;
        }
        if depth != 0 {
            return Err(DslError::at(lineno, "行块行数表达式缺少右括号"));
        }
        let rows_expr = expr_text(&inner);
        if rows_expr.is_empty() {
            return Err(DslError::at(lineno, "行块行数表达式为空（line (N):）"));
        }
        let node = crate::expr::parse_expr(&rows_expr).map_err(|e| e.with_line(lineno))?;
        check_known_calls(&node).map_err(|e| e.with_line(lineno))?;
        rows = rows_expr;
        p += 1; // 跳过 ')'
    }
    if !matches!(toks.get(p), Some(Tok::Op(s)) if s == ":") {
        return Err(DslError::at(lineno, "行块必须以「line (N):」开头"));
    }
    if p + 1 != toks.len() {
        return Err(DslError::at(lineno, "行块声明行有多余内容"));
    }

    // 缩进子项（必须比声明行缩进更深）
    let mut items: Vec<LineItem> = Vec::new();
    let mut i = start + 1;
    while i < lines.len() {
        let raw = lines[i];
        if raw.trim().is_empty() || raw.trim_start().starts_with('#') {
            i += 1;
            continue;
        }
        let indent = raw.len() - raw.trim_start_matches(' ').len();
        if indent <= base_indent {
            break;
        }
        items.push(parse_line_item(raw.trim(), i + 1, seen)?);
        i += 1;
    }
    if items.is_empty() {
        return Err(DslError::at(lineno, "行块内至少需要一个子项（整数/浮点/文本/表达式/字符串）"));
    }
    Ok((
        Item {
            name: String::new(),
            kind: VarKind::Line { rows, items },
            line: lineno,
        },
        i - start,
    ))
}

/// 解析顶层命令语句：`name = cmd(...)`。
fn parse_statement(line: &str, lineno: usize, seen: &mut HashSet<String>) -> DslResult<Item> {
    let toks = tokenize(line).map_err(|e| e.with_line(lineno))?;
    let groups = split_args(&toks);
    if groups.len() > 1 {
        return Err(DslError::at(
            lineno,
            "顶层一行只允许一条语句；多个数请放入行块（行: 块 + 缩进子项）",
        ));
    }
    let group = &groups[0];
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
        None => return Err(DslError::at(lineno, "语句缺少 '='")),
    };
    let name = expr_text(&group[..eq]);
    if name.is_empty() {
        return Err(DslError::at(lineno, "缺少变量名"));
    }
    check_name(&name, lineno, seen)?;
    let rhs = &group[eq + 1..];
    let cmd = match rhs.first() {
        Some(Tok::Name(c)) => c.as_str(),
        _ => {
            return Err(DslError::at(
                lineno,
                "顶层语句必须是数组/树/图等命令（name = ints(...)）；整数/浮点/表达式请放入行块",
            ));
        }
    };
    if TOP_COMMANDS.contains(&cmd) {
        if !matches!(rhs.get(1), Some(Tok::Op(s)) if s == "(") {
            return Err(DslError::at(lineno, format!("{cmd} 命令缺少左括号")));
        }
        if !matches!(rhs.last(), Some(Tok::Op(s)) if s == ")") {
            return Err(DslError::at(lineno, format!("{cmd} 命令缺少右括号")));
        }
        let kind = parse_cmd(cmd, &rhs[2..rhs.len() - 1]).map_err(|e| e.with_line(lineno))?;
        seen.insert(name.clone());
        return Ok(Item {
            name,
            kind,
            line: lineno,
        });
    }
    if RETIRED_COMMANDS.contains(&cmd) {
        return Err(DslError::at(
            lineno,
            format!("{cmd} 已改为行内项：请放入行块（line: 块 + 缩进子项「类型 名字: 参数」）"),
        ));
    }
    if LINE_ITEM_KINDS.contains(&cmd) || cmd == LINE_KEYWORD {
        return Err(DslError::at(
            lineno,
            format!("{cmd} 是行内项类型，必须缩进放在行块内"),
        ));
    }
    if is_known(cmd) {
        return Err(DslError::at(lineno, format!("命令 {cmd} 只能用于行块内部")));
    }
    Err(DslError::at(
        lineno,
        format!(
            "未知命令：{cmd}（顶层语句必须是数组/树/图等命令；整数/浮点/表达式请放入行块）"
        ),
    ))
}

/// 解析 DSL 文本，返回 IR 配置；语法/语义错误带行号。
///
/// 顶层结构：
/// - `repeat (N):` 块：普通顶层语句（可多个、可与其他语句混排），不能嵌套
/// - 行块 / 顶层命令
pub fn parse(text: &str) -> DslResult<Config> {
    let lines: Vec<&str> = text.lines().collect();
    let mut items: Vec<Item> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut i = 0usize;
    while i < lines.len() {
        let raw = lines[i];
        if raw.trim().is_empty() || raw.trim_start().starts_with('#') {
            i += 1;
            continue;
        }
        let indent = raw.len() - raw.trim_start_matches(' ').len();
        if indent > 0 {
            return Err(DslError::at(
                i + 1,
                "缩进不正确：顶层语句不能缩进（repeat 块或行块子项除外）",
            ));
        }
        let toks = tokenize(raw.trim()).map_err(|e| e.with_line(i + 1))?;
        if matches!(toks.first(), Some(Tok::Name(k)) if k == "repeat") {
            let (item, consumed) = parse_repeat_block(&lines, i, &mut seen)?;
            items.push(item);
            i += consumed;
            continue;
        }
        if matches!(toks.first(), Some(Tok::Name(k)) if k == LINE_KEYWORD) {
            let (item, consumed) = parse_line_block(&lines, i, 0, &mut seen)?;
            items.push(item);
            i += consumed;
        } else {
            items.push(parse_statement(raw.trim(), i + 1, &mut seen)?);
            i += 1;
        }
    }
    Ok(Config { items })
}

/// 解析 repeat 块：`repeat (N):` + 缩进子语句，返回 (Item, 消费的行数)。
fn parse_repeat_block(
    lines: &[&str],
    start: usize,
    seen: &mut HashSet<String>,
) -> DslResult<(Item, usize)> {
    let lineno = start + 1;
    let toks = tokenize(lines[start]).map_err(|e| e.with_line(lineno))?;
    // repeat [ ( N ) ] :
    let mut p = 1usize;
    let mut count = "1".to_string();
    if matches!(toks.get(p), Some(Tok::Op(s)) if s == "(") {
        p += 1;
        let mut depth = 1usize;
        let mut inner: Vec<Tok> = Vec::new();
        while p < toks.len() {
            match &toks[p] {
                Tok::Op(s) if s == "(" => {
                    depth += 1;
                    inner.push(toks[p].clone());
                }
                Tok::Op(s) if s == ")" => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    inner.push(toks[p].clone());
                }
                t => inner.push(t.clone()),
            }
            p += 1;
        }
        if depth != 0 {
            return Err(DslError::at(lineno, "repeat 重复次数表达式缺少右括号"));
        }
        let count_expr = expr_text(&inner);
        if count_expr.is_empty() {
            return Err(DslError::at(lineno, "repeat 重复次数表达式为空（repeat (N):）"));
        }
        let node = crate::expr::parse_expr(&count_expr).map_err(|e| e.with_line(lineno))?;
        check_known_calls(&node).map_err(|e| e.with_line(lineno))?;
        count = count_expr;
        p += 1; // 跳过 ')'
    }
    if !matches!(toks.get(p), Some(Tok::Op(s)) if s == ":") {
        return Err(DslError::at(lineno, "repeat 块必须以「repeat (N):」开头"));
    }
    if p + 1 != toks.len() {
        return Err(DslError::at(lineno, "repeat 块声明行有多余内容"));
    }

    // 缩进子语句（行块 / 顶层命令）；块内变量不泄漏到块外（作用域隔离）
    let mut items: Vec<Item> = Vec::new();
    let mut inner_seen = seen.clone();
    let mut i = start + 1;
    while i < lines.len() {
        let raw = lines[i];
        if raw.trim().is_empty() || raw.trim_start().starts_with('#') {
            i += 1;
            continue;
        }
        let indent = raw.len() - raw.trim_start_matches(' ').len();
        if indent == 0 {
            break;
        }
        let sub = raw.trim();
        let sub_toks = tokenize(sub).map_err(|e| e.with_line(i + 1))?;
        if matches!(sub_toks.first(), Some(Tok::Name(k)) if k == "repeat") {
            return Err(DslError::at(i + 1, "repeat 块不能嵌套"));
        }
        if matches!(sub_toks.first(), Some(Tok::Name(k)) if k == LINE_KEYWORD) {
            let (item, consumed) = parse_line_block(&lines, i, indent, &mut inner_seen)?;
            items.push(item);
            i += consumed;
        } else {
            items.push(parse_statement(sub, i + 1, &mut inner_seen)?);
            i += 1;
        }
    }
    if items.is_empty() {
        return Err(DslError::at(
            lineno,
            "repeat 块内至少需要一个语句（行块或命令）",
        ));
    }
    Ok((
        Item {
            name: String::new(),
            kind: VarKind::Repeat { count, items },
            line: lineno,
        },
        i - start,
    ))
}
