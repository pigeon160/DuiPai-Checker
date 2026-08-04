//! 行级 DSL 解析：文本 -> IR（移植 legacy/dsl.py 的 parse）。
//!
//! Phase 1 支持命令：`int` `float` `ints` `floats` `matrix` `matf`；
//! 其余命令（perm/tree/graph/str/strs/binseq/intervals/points/ring/base_ring）
//! 已列入已知命令表，报“暂不支持”，Phase 2 补齐。

use std::collections::{HashMap, HashSet};

use crate::ast::{Config, ElemType, Item, RepeatMode, VarKind};
use crate::error::{DslError, DslResult};
use crate::expr::{tokenize, Tok};

/// 全部命令（保留字，不可用作变量名）。
pub const KNOWN_COMMANDS: &[&str] = &[
    "int", "float", "ints", "floats", "matrix", "matf", "perm", "tree", "graph",
    "str", "strs", "binseq", "intervals", "points", "ring", "base_ring",
];

/// Phase 1 已实现的命令。
const SUPPORTED_COMMANDS: &[&str] = &["int", "float", "ints", "floats", "matrix", "matf"];

const REPEAT_COMMENT: &str = "多测模式";

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
                let v = expr_text(&arg[i + 1..]);
                kw.insert(kname, v);
            }
            _ => pos.push(arg),
        }
    }
    Ok((pos, kw))
}

/// 解析单条命令的参数为统一配置项。
fn parse_cmd(name: &str, cmd: &str, args: &[Tok]) -> DslResult<Item> {
    let (pos, kw) = split_kw_args(args)?;

    // 已知但未实现的命令给出清晰提示
    if !SUPPORTED_COMMANDS.contains(&cmd) {
        return Err(DslError::bare(format!(
            "命令 {cmd} 暂不支持（当前版本仅支持 {}）",
            SUPPORTED_COMMANDS.join(" / ")
        )));
    }

    let arity = |lo: usize, hi: usize| -> DslResult<()> {
        if !(lo..=hi).contains(&pos.len()) {
            return Err(DslError::bare(format!(
                "{cmd} 需要 {lo}~{hi} 个位置参数，实际 {} 个",
                pos.len()
            )));
        }
        Ok(())
    };

    match cmd {
        "int" => {
            arity(2, 2)?;
            Ok(Item {
                name: name.to_string(),
                kind: VarKind::Int {
                    min: expr_text(&pos[0]),
                    max: expr_text(&pos[1]),
                },
            })
        }
        "float" => {
            arity(2, 3)?;
            let prec = kw
                .get("prec")
                .cloned()
                .unwrap_or_else(|| {
                    if pos.len() == 3 {
                        expr_text(&pos[2])
                    } else {
                        "6".to_string()
                    }
                });
            Ok(Item {
                name: name.to_string(),
                kind: VarKind::Float {
                    min: expr_text(&pos[0]),
                    max: expr_text(&pos[1]),
                    prec,
                },
            })
        }
        "ints" | "floats" => {
            arity(3, 4)?;
            let prec = kw
                .get("prec")
                .cloned()
                .unwrap_or_else(|| {
                    if pos.len() == 4 {
                        expr_text(&pos[3])
                    } else {
                        "6".to_string()
                    }
                });
            Ok(Item {
                name: name.to_string(),
                kind: VarKind::Array {
                    elem_type: if cmd == "ints" { ElemType::Int } else { ElemType::Float },
                    el_min: expr_text(&pos[1]),
                    el_max: expr_text(&pos[2]),
                    prec,
                    rows: "1".to_string(),
                    cols: expr_text(&pos[0]),
                },
            })
        }
        "matrix" | "matf" => {
            arity(4, 5)?;
            let prec = kw
                .get("prec")
                .cloned()
                .unwrap_or_else(|| {
                    if pos.len() == 5 {
                        expr_text(&pos[4])
                    } else {
                        "6".to_string()
                    }
                });
            Ok(Item {
                name: name.to_string(),
                kind: VarKind::Array {
                    elem_type: if cmd == "matrix" { ElemType::Int } else { ElemType::Float },
                    el_min: expr_text(&pos[2]),
                    el_max: expr_text(&pos[3]),
                    prec,
                    rows: expr_text(&pos[0]),
                    cols: expr_text(&pos[1]),
                },
            })
        }
        _ => unreachable!("SUPPORTED_COMMANDS 已过滤"),
    }
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
    Ok(item)
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
