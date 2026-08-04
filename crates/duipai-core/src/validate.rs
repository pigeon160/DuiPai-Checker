//! 静态校验（不依赖随机生成）：变量定义顺序、引用规则、类型匹配、常量值域非空。
//!
//! 与 legacy 生成期检查对齐：只做**常量可判定**的部分；含变量引用的范围
//! 在生成期（Phase 3）动态检查。错误消息沿用 legacy 文案。

use std::collections::HashMap;

use crate::ast::{Config, ElemType, GraphType, VarKind, Weight};
use crate::error::DslError;
use crate::expr::{collect_names, eval_node, parse_expr};
use rand::rngs::StdRng;
use rand::SeedableRng;

/// 可被引用的变量类型（引用其“规模值”；legacy 生成期仅这些类型写入环境）。
fn is_refable(kind: &VarKind) -> bool {
    matches!(
        kind,
        VarKind::Int { .. }
            | VarKind::Float { .. }
            | VarKind::Perm { .. }
            | VarKind::Tree { .. }
            | VarKind::Graph { .. }
    )
}

/// 常量可判定求值：表达式不含变量引用时返回 Some(值)；含引用返回 Ok(None)；
/// 求值错误（如 int 范围倒置）返回 Err。
fn try_const(expr: &str) -> Result<Option<f64>, DslError> {
    let node = parse_expr(expr)?;
    let mut names = Vec::new();
    collect_names(&node, &mut names);
    if !names.is_empty() {
        return Ok(None);
    }
    let mut rng = StdRng::seed_from_u64(0);
    let v = eval_node(&node, &HashMap::new(), &mut rng)?;
    Ok(Some(v))
}

/// 检查单个数值字段：语法 + 引用规则 + 类型匹配。
fn check_field(expr: &str, label: &str, types: &HashMap<String, VarKind>, errors: &mut Vec<DslError>, line: usize) {
    let node = match parse_expr(expr) {
        Ok(n) => n,
        Err(e) => {
            errors.push(DslError::at(line, format!("{label}表达式错误：{}", e.message)));
            return;
        }
    };
    let mut names = Vec::new();
    collect_names(&node, &mut names);
    for name in names {
        match types.get(&name) {
            None => errors.push(DslError::at(
                line,
                format!("{label}表达式错误：引用了未定义的变量：{name}"),
            )),
            Some(kind) if !is_refable(kind) => errors.push(DslError::at(
                line,
                format!("{label}表达式错误：变量 {name} 类型不可作为引用源"),
            )),
            Some(_) => {}
        }
    }
}

/// 对两个表达式做常量值域检查（lo <= hi）。
fn check_lo_hi(lo: &str, hi: &str, label: &str, err_msg: &str, errors: &mut Vec<DslError>, line: usize) {
    let lo = try_const(lo);
    let hi = try_const(hi);
    match (lo, hi) {
        (Ok(Some(a)), Ok(Some(b))) if a > b => {
            errors.push(DslError::at(line, err_msg.to_string()));
        }
        (Err(e), _) => errors.push(DslError::at(line, format!("{label}表达式错误：{}", e.message))),
        (_, Err(e)) => errors.push(DslError::at(line, format!("{label}表达式错误：{}", e.message))),
        _ => {}
    }
}

/// 单个数值约束：常量时要求满足 predicate，否则报 err_msg。
fn check_const(expr: &str, label: &str, err_msg: &str, ok: impl Fn(f64) -> bool, errors: &mut Vec<DslError>, line: usize) {
    match try_const(expr) {
        Ok(Some(v)) if !ok(v) => errors.push(DslError::at(line, err_msg.to_string())),
        Ok(Some(_)) => {}
        Ok(None) => {}
        Err(e) => errors.push(DslError::at(line, format!("{label}表达式错误：{}", e.message))),
    }
}

fn check_weight(
    w: Option<&Weight>,
    range_label: &str,
    prec_label: &str,
    types: &HashMap<String, VarKind>,
    errors: &mut Vec<DslError>,
    line: usize,
) {
    let Some(w) = w else { return };
    check_field(&w.min, range_label, types, errors, line);
    check_field(&w.max, range_label, types, errors, line);
    check_field(&w.prec, prec_label, types, errors, line);
    check_lo_hi(&w.min, &w.max, range_label, format!("{range_label}最小值不能大于最大值").as_str(), errors, line);
    check_const(&w.prec, prec_label, "精度应在 0~15 之间", |v| (0.0..=15.0).contains(&v), errors, line);
}

/// 校验一份配置，返回错误列表（每个错误带行号）。
pub fn validate(config: &Config) -> Vec<DslError> {
    let mut errors: Vec<DslError> = Vec::new();
    let mut types: HashMap<String, VarKind> = HashMap::new();
    for item in &config.items {
        let line = item.line;
        let kind = &item.kind;
        match kind {
            VarKind::Int { min, max } => {
                check_field(min, "整数变量范围", &types, &mut errors, line);
                check_field(max, "整数变量范围", &types, &mut errors, line);
                check_lo_hi(min, max, "整数变量范围", "整数变量范围最小值不能大于最大值", &mut errors, line);
            }
            VarKind::Float { min, max, prec } => {
                check_field(min, "浮点数变量范围", &types, &mut errors, line);
                check_field(max, "浮点数变量范围", &types, &mut errors, line);
                check_field(prec, "浮点精度", &types, &mut errors, line);
                check_lo_hi(min, max, "浮点数变量范围", "浮点数变量范围最小值不能大于最大值", &mut errors, line);
                check_const(prec, "浮点精度", "浮点数变量精度应在 0~15 之间", |v| (0.0..=15.0).contains(&v), &mut errors, line);
            }
            VarKind::Array { elem_type, el_min, el_max, prec, rows, cols } => {
                let is_float = *elem_type == ElemType::Float;
                let range_label = "数组元素范围";
                let prec_label = "数组元素精度";
                check_field(el_min, range_label, &types, &mut errors, line);
                check_field(el_max, range_label, &types, &mut errors, line);
                check_field(prec, prec_label, &types, &mut errors, line);
                check_field(rows, "数组行数", &types, &mut errors, line);
                check_field(cols, "数组每行长度", &types, &mut errors, line);
                check_lo_hi(el_min, el_max, range_label, "数组元素范围最小值不能大于最大值", &mut errors, line);
                if is_float {
                    check_const(prec, prec_label, "数组元素精度应在 0~15 之间", |v| (0.0..=15.0).contains(&v), &mut errors, line);
                }
                check_const(rows, "数组行数", "数组行数不能小于 1", |v| v >= 1.0, &mut errors, line);
                check_const(cols, "数组每行长度", "数组每行长度不能为负", |v| v >= 0.0, &mut errors, line);
            }
            VarKind::Perm { n } => {
                check_field(n, "排列长度", &types, &mut errors, line);
                check_const(n, "排列长度", "排列长度 n 应 >= 1", |v| v >= 1.0, &mut errors, line);
            }
            VarKind::String { rows, cols, charset } => {
                check_field(rows, "字符串行数", &types, &mut errors, line);
                check_field(cols, "字符串长度", &types, &mut errors, line);
                if charset.is_empty() {
                    errors.push(DslError::at(line, "字符串字符集不能为空"));
                }
                check_const(rows, "字符串行数", "字符串行数不能小于 1", |v| v >= 1.0, &mut errors, line);
                check_const(cols, "字符串长度", "字符串长度不能为负", |v| v >= 0.0, &mut errors, line);
            }
            VarKind::Binseq { n, k } => {
                check_field(n, "0/1序列长度", &types, &mut errors, line);
                check_field(k, "0/1序列中1的个数", &types, &mut errors, line);
                check_const(n, "0/1序列长度", "0/1序列长度不能为负", |v| v >= 0.0, &mut errors, line);
                match (try_const(n), try_const(k)) {
                    (Ok(Some(nv)), Ok(Some(kv))) if !(0.0..=nv).contains(&kv) => {
                        errors.push(DslError::at(line, "1 的个数 k 应在 0~n 之间"));
                    }
                    (Err(e), _) => errors.push(DslError::at(line, format!("0/1序列长度表达式错误：{}", e.message))),
                    (_, Err(e)) => errors.push(DslError::at(line, format!("0/1序列中1的个数表达式错误：{}", e.message))),
                    _ => {}
                }
            }
            VarKind::Intervals { n, lo, hi } => {
                check_field(n, "区间个数", &types, &mut errors, line);
                check_field(lo, "区间下界", &types, &mut errors, line);
                check_field(hi, "区间上界", &types, &mut errors, line);
                check_const(n, "区间个数", "区间个数不能为负", |v| v >= 0.0, &mut errors, line);
                check_lo_hi(lo, hi, "区间范围", "区间下界不能大于上界", &mut errors, line);
            }
            VarKind::Points { n, xlo, xhi, ylo, yhi } => {
                check_field(n, "点个数", &types, &mut errors, line);
                check_field(xlo, "点 x 下界", &types, &mut errors, line);
                check_field(xhi, "点 x 上界", &types, &mut errors, line);
                check_field(ylo, "点 y 下界", &types, &mut errors, line);
                check_field(yhi, "点 y 上界", &types, &mut errors, line);
                check_const(n, "点个数", "点个数不能为负", |v| v >= 0.0, &mut errors, line);
                let xok = match (try_const(xlo), try_const(xhi)) {
                    (Ok(Some(a)), Ok(Some(b))) => a <= b,
                    (Err(e), _) => { errors.push(DslError::at(line, format!("点 x 下界表达式错误：{}", e.message))); true }
                    (_, Err(e)) => { errors.push(DslError::at(line, format!("点 x 上界表达式错误：{}", e.message))); true }
                    _ => true,
                };
                let yok = match (try_const(ylo), try_const(yhi)) {
                    (Ok(Some(a)), Ok(Some(b))) => a <= b,
                    (Err(e), _) => { errors.push(DslError::at(line, format!("点 y 下界表达式错误：{}", e.message))); true }
                    (_, Err(e)) => { errors.push(DslError::at(line, format!("点 y 上界表达式错误：{}", e.message))); true }
                    _ => true,
                };
                if !(xok && yok) {
                    errors.push(DslError::at(line, "点坐标范围无效"));
                }
            }
            VarKind::Tree { n, w, val } => {
                check_field(n, "树顶点数", &types, &mut errors, line);
                check_const(n, "树顶点数", "树顶点数 n 应 >= 1", |v| v >= 1.0, &mut errors, line);
                check_weight(w.as_ref(), "边权范围", "边权精度", &types, &mut errors, line);
                check_weight(val.as_ref(), "节点权值范围", "节点权值精度", &types, &mut errors, line);
            }
            VarKind::Graph { gtype, n, m, directed, connected, k, w, val } => {
                match gtype {
                    GraphType::Ring => {
                        check_field(n, "环顶点数", &types, &mut errors, line);
                        check_const(n, "环顶点数", "环顶点数 n 应 >= 3", |v| v >= 3.0, &mut errors, line);
                        check_weight(val.as_ref(), "节点权值范围", "节点权值精度", &types, &mut errors, line);
                    }
                    GraphType::BaseRing => {
                        check_field(n, "基环树顶点数", &types, &mut errors, line);
                        check_field(k.as_deref().unwrap_or("3"), "环大小", &types, &mut errors, line);
                        check_const(n, "基环树顶点数", "基环树顶点数 n 应 >= 3", |v| v >= 3.0, &mut errors, line);
                        match (try_const(n), try_const(k.as_deref().unwrap_or("3"))) {
                            (Ok(Some(nv)), Ok(Some(kv))) if !(3.0..=nv).contains(&kv) => {
                                errors.push(DslError::at(line, "环大小 k 应在 3~n 之间"));
                            }
                            (Err(e), _) => errors.push(DslError::at(line, format!("基环树顶点数表达式错误：{}", e.message))),
                            (_, Err(e)) => errors.push(DslError::at(line, format!("环大小表达式错误：{}", e.message))),
                            _ => {}
                        }
                        check_weight(val.as_ref(), "节点权值范围", "节点权值精度", &types, &mut errors, line);
                    }
                    _ => {
                        let n_label = "图顶点数";
                        let m_label = "图边数";
                        check_field(n, n_label, &types, &mut errors, line);
                        check_field(m, m_label, &types, &mut errors, line);
                        check_const(n, n_label, "图顶点数 n 应 >= 1", |v| v >= 1.0, &mut errors, line);
                        check_const(m, m_label, "图边数 m 不能为负", |v| v >= 0.0, &mut errors, line);
                        check_weight(w.as_ref(), "边权范围", "边权精度", &types, &mut errors, line);
                        check_weight(val.as_ref(), "节点权值范围", "节点权值精度", &types, &mut errors, line);
                        // 边数上限（常量可判定时）
                        if let (Ok(Some(nv)), Ok(Some(mv))) = (try_const(n), try_const(m)) {
                            let nv = nv.floor() as i64;
                            let mv = mv.floor() as i64;
                            match gtype {
                                GraphType::Dag => {
                                    let possible = nv * (nv - 1) / 2;
                                    if mv > possible {
                                        errors.push(DslError::at(line, format!("图边数 m={mv} 超过上限 {possible}（DAG，n={nv}）")));
                                    }
                                }
                                GraphType::Bipartite => {
                                    let left = nv / 2;
                                    let right = nv - left;
                                    if left < 1 || right < 1 {
                                        errors.push(DslError::at(line, "二分图 n 过小，无法分两部"));
                                    } else {
                                        let possible = left * right;
                                        if mv > possible {
                                            errors.push(DslError::at(line, format!("图边数 m={mv} 超过上限 {possible}（二分图，n={nv}）")));
                                        }
                                    }
                                }
                                _ => {
                                    let possible = if *directed { nv * (nv - 1) } else { nv * (nv - 1) / 2 };
                                    if mv > possible {
                                        errors.push(DslError::at(line, format!("图边数 m={mv} 超过上限 {possible}（{}，n={nv}）", if *directed { "有向" } else { "无向" })));
                                    }
                                    if *connected && mv < nv - 1 {
                                        errors.push(DslError::at(line, "连通图要求 m >= n-1"));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        types.insert(item.name.clone(), kind.clone());
    }
    errors
}
