//! IR -> DSL 文本（移植 legacy/dsl.py 的 serialize）。
//!
//! 输出为规范化文本：每行一条语句，多测模式首行注释。

use crate::ast::{Config, ElemType, GraphType, Item, VarKind, Weight};
use crate::error::DslResult;
use crate::expr::{parse_expr, ExprNode};

const REPEAT_COMMENT: &str = "多测模式";

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
fn is_single_row(rows: &str) -> bool {
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

/// 把一个配置项序列化为一行 DSL 语句。
pub fn line_for(item: &Item) -> DslResult<String> {
    let name = &item.name;
    let line = match &item.kind {
        VarKind::Int { min, max } => format!("{name} = int({min}, {max})"),
        VarKind::Multi { parts } => {
            let assigns: Vec<String> = parts
                .iter()
                .map(|p| format!("{} = {}", p.name, p.expr))
                .collect();
            assigns.join(", ")
        }
        VarKind::Scalar { expr } => format!("{name} = {expr}"),
        VarKind::Float { min, max, prec } => {
            if prec == "6" {
                format!("{name} = float({min}, {max})")
            } else {
                format!("{name} = float({min}, {max}, {prec})")
            }
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
                format!("{base}, {prec})")
            } else {
                format!("{base})")
            }
        }
        VarKind::Perm { n } => format!("{name} = perm({n})"),
        VarKind::String {
            rows,
            cols,
            charset,
        } => {
            if rows == "1" {
                if charset.is_empty() || charset == "abcdefghijklmnopqrstuvwxyz" {
                    format!("{name} = str({cols})")
                } else {
                    format!("{name} = str({cols}, \"{charset}\")")
                }
            } else if charset.is_empty() || charset == "abcdefghijklmnopqrstuvwxyz" {
                format!("{name} = strs({rows}, {cols})")
            } else {
                format!("{name} = strs({rows}, {cols}, \"{charset}\")")
            }
        }
        VarKind::Binseq { n, k } => format!("{name} = binseq({n}, {k})"),
        VarKind::Intervals { n, lo, hi } => format!("{name} = intervals({n}, {lo}, {hi})"),
        VarKind::Points {
            n,
            xlo,
            xhi,
            ylo,
            yhi,
        } => format!("{name} = points({n}, {xlo}, {xhi}, {ylo}, {yhi})"),
        VarKind::Tree { n, w, val } => {
            let mut base = format!("{name} = tree({n}");
            if let Some(w) = w {
                base.push_str(&format!(", w={}", fmt_weight(w)));
            }
            if let Some(val) = val {
                base.push_str(&format!(", val={}", fmt_weight(val)));
            }
            format!("{base})")
        }
        VarKind::Graph {
            gtype,
            n,
            m,
            directed,
            connected,
            k,
            w,
            val,
        } => match gtype {
            GraphType::Ring => {
                let mut base = format!("{name} = ring({n}");
                if let Some(w) = w {
                    base.push_str(&format!(", w={}", fmt_weight(w)));
                }
                if let Some(val) = val {
                    base.push_str(&format!(", val={}", fmt_weight(val)));
                }
                format!("{base})")
            }
            GraphType::BaseRing => {
                let k = k.as_deref().unwrap_or("3");
                let mut base = format!("{name} = base_ring({n}, {k}");
                if let Some(w) = w {
                    base.push_str(&format!(", w={}", fmt_weight(w)));
                }
                if let Some(val) = val {
                    base.push_str(&format!(", val={}", fmt_weight(val)));
                }
                format!("{base})")
            }
            _ => {
                let d = if *directed { 1 } else { 0 };
                let c = if *connected { 1 } else { 0 };
                let mut base = format!("{name} = graph({n}, {m}, {d}, {c}");
                if *gtype != GraphType::General {
                    let t = match gtype {
                        GraphType::Dag => "dag",
                        GraphType::Bipartite => "bipartite",
                        _ => unreachable!(),
                    };
                    base.push_str(&format!(", type=\"{t}\""));
                }
                if let Some(w) = w {
                    base.push_str(&format!(", w={}", fmt_weight(w)));
                }
                if let Some(val) = val {
                    base.push_str(&format!(", val={}", fmt_weight(val)));
                }
                format!("{base})")
            }
        },
    };
    Ok(line)
}

/// 把配置序列化为 DSL 文本（每行一条语句）。
pub fn serialize(config: &Config) -> DslResult<String> {
    let mut out: Vec<String> = Vec::new();
    if let Some(repeat) = &config.repeat {
        if repeat.enabled {
            out.push(format!("# {REPEAT_COMMENT}：重复 {} 次", repeat.count));
        }
    }
    for item in &config.items {
        out.push(line_for(item)?);
    }
    Ok(out.join("\n"))
}
