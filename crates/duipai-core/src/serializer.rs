//! IR -> DSL 文本（规范化）。
//!
//! 输出为规范化文本：`repeat (N):` 块（缩进子语句）、行块 `行 (N):` + 缩进子项；
//! 命令每行一条。

use crate::ast::{Config, ElemType, GraphType, Item, LineItemKind, VarKind, Weight};
use crate::error::DslResult;
use crate::expr::{parse_expr, ExprNode};

/// 把 min/max 字段格式化为 int(a,b) / float(a,b,prec) 形式。
fn fmt_range(item: &Weight) -> String {
    if item.kind == ElemType::Int {
        format!("int({}, {})", item.min, item.max)
    } else if item.prec == "6" {
        format!("float({}, {})", item.min, item.max)
    } else {
        format!("float({}, {}, {})", item.min, item.max, item.prec)
    }
}

fn fmt_weight(w: &Weight) -> String {
    fmt_range(w)
}

/// 判断 array 的 rows 是否恒为 1（单行数组 -> ints/floats，否则 matrix/matf）。
/// 结构化判断：常量 1 或 `int(1, 1)`；不做随机求值（避免 int(1,5) 碰巧得 1）。
pub fn is_single_row(rows: &str) -> bool {
    let Ok(node) = parse_expr(rows) else {
        return false;
    };
    match &node {
        ExprNode::Num(v) => *v == 1.0,
        ExprNode::Call { name, args } if name == "int" && args.len() == 2 => {
            matches!(&args[0], ExprNode::Num(v) if *v == 1.0)
                && matches!(&args[1], ExprNode::Num(v) if *v == 1.0)
        }
        _ => false,
    }
}

/// 行内项序列化为一行。
fn line_item_line(item: &crate::ast::LineItem) -> String {
    let name = &item.name;
    match &item.kind {
        LineItemKind::Int { min, max } => format!("int {name}: {min}, {max}"),
        LineItemKind::Float { min, max, prec } => {
            if prec == "6" {
                format!("float {name}: {min}, {max}")
            } else {
                format!("float {name}: {min}, {max}, {prec}")
            }
        }
        LineItemKind::Scalar { expr } => format!("expr {name}: {expr}"),
        LineItemKind::Text { text } => format!("text {name}: \"{text}\""),
        LineItemKind::Str { len, charset } => {
            if charset == "abcdefghijklmnopqrstuvwxyz" {
                format!("str {name}: {len}")
            } else {
                format!("str {name}: {len}, \"{charset}\"")
            }
        }
    }
}

/// 把一个配置项序列化为 DSL 行列表（行块为多行）。
pub fn lines_for(item: &Item) -> DslResult<Vec<String>> {
    let name = &item.name;
    let lines: Vec<String> = match &item.kind {
        VarKind::Repeat { count, items } => {
            let mut out = vec![format!("repeat ({count}):")];
            for sub in items {
                for l in lines_for(sub)? {
                    out.push(format!("    {l}"));
                }
            }
            out
        }
        VarKind::Line { rows, items } => {
            let mut out = Vec::new();
            if rows == "1" {
                out.push("line:".to_string());
            } else {
                out.push(format!("line ({rows}):"));
            }
            for it in items {
                out.push(format!("    {}", line_item_line(it)));
            }
            out
        }
        VarKind::Array {
            elem_type,
            el_min,
            el_max,
            prec,
            rows,
            cols,
        } => {
            let cmd_single = if *elem_type == ElemType::Int { "ints" } else { "floats" };
            let cmd_multi = if *elem_type == ElemType::Int { "matrix" } else { "matf" };
            let base = if is_single_row(rows) {
                format!("{name} = {cmd_single}({cols}, {el_min}, {el_max}")
            } else {
                format!("{name} = {cmd_multi}({rows}, {cols}, {el_min}, {el_max}")
            };
            if elem_type.is_float() && prec != "6" {
                vec![format!("{base}, {prec})")]
            } else {
                vec![format!("{base})")]
            }
        }
        VarKind::Perm { n } => vec![format!("{name} = perm({n})")],
        VarKind::Binseq { n, k } => vec![format!("{name} = binseq({n}, {k})")],
        VarKind::Intervals { n, lo, hi } => vec![format!("{name} = intervals({n}, {lo}, {hi})")],
        VarKind::Points {
            n,
            xlo,
            xhi,
            ylo,
            yhi,
        } => vec![format!("{name} = points({n}, {xlo}, {xhi}, {ylo}, {yhi})")],
        VarKind::Tree { n, ttype, w } => {
            let mut parts = vec![format!("{name} = tree({n}")];
            if *ttype != crate::ast::TreeType::Random {
                let t = match ttype {
                    crate::ast::TreeType::Star => "star",
                    crate::ast::TreeType::Chain => "chain",
                    crate::ast::TreeType::Parent => "parent",
                    crate::ast::TreeType::Random => unreachable!(),
                };
                parts.push(format!("type=\"{t}\""));
            }
            if let Some(w) = w {
                parts.push(fmt_weight(w));
            }
            vec![format!("{})", parts.join(", "))]
        }
        VarKind::Graph {
            gtype,
            n,
            m,
            directed,
            connected,
            multi,
            loop_,
            k,
            w,
        } => match gtype {
            GraphType::Ring => {
                let mut parts = vec![format!("{name} = ring({n}")];
                if let Some(w) = w {
                    parts.push(fmt_weight(w));
                }
                vec![format!("{})", parts.join(", "))]
            }
            GraphType::BaseRing => {
                let k = k.as_deref().unwrap_or("3");
                let mut parts = vec![format!("{name} = base_ring({n}, {k}")];
                if let Some(w) = w {
                    parts.push(fmt_weight(w));
                }
                vec![format!("{})", parts.join(", "))]
            }
            _ => {
                let d = if *directed { 1 } else { 0 };
                let c = if *connected { 1 } else { 0 };
                let mut parts = vec![format!("{name} = graph({n}, {m}, {d}, {c}")];
                if let Some(w) = w {
                    parts.push(fmt_weight(w));
                }
                if *multi {
                    parts.push("multi=1".to_string());
                }
                if *loop_ {
                    parts.push("loop=1".to_string());
                }
                if *gtype != GraphType::General {
                    let t = match gtype {
                        GraphType::Dag => "dag",
                        GraphType::Bipartite => "bipartite",
                        _ => unreachable!(),
                    };
                    parts.push(format!("type=\"{t}\""));
                }
                vec![format!("{})", parts.join(", "))]
            }
        },
    };
    Ok(lines)
}

/// 把配置序列化为 DSL 文本。
pub fn serialize(config: &Config) -> DslResult<String> {
    let mut out: Vec<String> = Vec::new();
    for item in &config.items {
        out.extend(lines_for(item)?);
    }
    Ok(out.join("\n"))
}
