//! 静态校验（不依赖随机生成）：变量定义顺序、引用规则、类型匹配、常量值域非空。
//!
//! 与 legacy 生成期检查对齐：只做**常量可判定**的部分；含变量引用的范围
//! 在生成期（Phase 3）动态检查。错误消息沿用 legacy 文案。

use std::collections::HashMap;

use crate::ast::{Config, ElemType, GraphType, VarKind, Weight};
use crate::error::DslError;
use crate::expr::{collect_names, eval_node, parse_expr, ExprNode};
use crate::serializer::is_single_row;
use rand::rngs::StdRng;
use rand::SeedableRng;

/// 可被引用的变量类型（引用其“规模值”；legacy 生成期仅这些类型写入环境）。
fn is_refable(kind: &VarKind) -> bool {
    matches!(
        kind,
        VarKind::Int { .. }
            | VarKind::Float { .. }
            | VarKind::Multi { .. }
            | VarKind::Scalar { .. }
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

/// 收集普通名字引用（跳过数组索引名，索引有专门检查）。
fn collect_plain_names(node: &ExprNode, out: &mut Vec<String>) {
    match node {
        ExprNode::Name(n) => out.push(n.clone()),
        ExprNode::Index { indices, .. } => {
            for i in indices {
                collect_plain_names(i, out);
            }
        }
        ExprNode::Neg(x) => collect_plain_names(x, out),
        ExprNode::Bin { l, r, .. } => {
            collect_plain_names(l, out);
            collect_plain_names(r, out);
        }
        ExprNode::Call { args, .. } => {
            for a in args {
                collect_plain_names(a, out);
            }
        }
        _ => {}
    }
}

/// 检查数组索引引用：层数匹配（单行 1 层 / 矩阵 2 层）、常量越界提前报错。
fn check_indexes(
    node: &ExprNode,
    types: &HashMap<String, VarKind>,
    errors: &mut Vec<DslError>,
    line: usize,
) {
    match node {
        ExprNode::Index { name, indices } => {
            match types.get(name) {
                Some(VarKind::Array { rows, cols, .. }) => {
                    let single = is_single_row(rows);
                    let want = if single { 1 } else { 2 };
                    if indices.len() != want {
                        errors.push(DslError::at(
                            line,
                            format!(
                                "索引层数错误：{name} 是{}数组，需要 {want} 个索引（{}）",
                                if single { "单行" } else { "矩阵" },
                                if want == 1 { "{name}[i]" } else { "{name}[i][j]" }
                            ),
                        ));
                    } else {
                        let dims = if single { vec![cols.as_str()] } else { vec![rows.as_str(), cols.as_str()] };
                        for (k, (idx_node, dim_expr)) in indices.iter().zip(dims).enumerate() {
                            let idx_text = expr_text_of(idx_node);
                            if idx_text.is_empty() {
                                continue;
                            }
                            if let (Ok(Some(dim)), Ok(Some(iv))) = (try_const(dim_expr), try_const(&idx_text)) {
                                if iv < 1.0 || iv > dim {
                                    errors.push(DslError::at(
                                        line,
                                        format!("索引 {name} 第 {} 维越界：{iv}（长度 {dim}）", k + 1),
                                    ));
                                }
                            }
                        }
                    }
                }
                // 重复行变量数组化：1 层索引，维度 = 行数
                Some(VarKind::Multi { rows, .. }) if !is_single_row(rows) => {
                    if indices.len() != 1 {
                        errors.push(DslError::at(
                            line,
                            format!("索引层数错误：重复行变量 {name} 需要 1 个索引（{name}[k]）"),
                        ));
                    } else {
                        let idx_text = expr_text_of(&indices[0]);
                        if !idx_text.is_empty() {
                            if let (Ok(Some(dim)), Ok(Some(iv))) = (try_const(rows), try_const(&idx_text)) {
                                if iv < 1.0 || iv > dim {
                                    errors.push(DslError::at(
                                        line,
                                        format!("索引 {name}[{iv}] 越界（重复行数 {dim}）"),
                                    ));
                                }
                            }
                        }
                    }
                }
                Some(_) => {
                    errors.push(DslError::at(line, format!("变量 {name} 不是数组，不能索引引用")));
                }
                None => {
                    errors.push(DslError::at(
                        line,
                        format!("引用了未定义的变量：{name}"),
                    ));
                }
            }
            for i in indices {
                check_indexes(i, types, errors, line);
            }
        }
        ExprNode::Neg(x) => check_indexes(x, types, errors, line),
        ExprNode::Bin { l, r, .. } => {
            check_indexes(l, types, errors, line);
            check_indexes(r, types, errors, line);
        }
        ExprNode::Call { args, .. } => {
            for a in args {
                check_indexes(a, types, errors, line);
            }
        }
        _ => {}
    }
}

/// AST 节点转表达式文本（常量求值用）。
fn expr_text_of(node: &ExprNode) -> String {
    match node {
        ExprNode::Num(v) => v.to_string(),
        ExprNode::Name(n) => n.clone(),
        ExprNode::Neg(x) => format!("-{}", expr_text_of(x)),
        ExprNode::Bin { op, l, r } => format!(
            "({} {} {})",
            expr_text_of(l),
            match op {
                crate::expr::BinOp::Add => "+",
                crate::expr::BinOp::Sub => "-",
                crate::expr::BinOp::Mul => "*",
                crate::expr::BinOp::Div => "/",
                crate::expr::BinOp::FloorDiv => "//",
                crate::expr::BinOp::Mod => "%",
                crate::expr::BinOp::Pow => "**",
            },
            expr_text_of(r)
        ),
        _ => String::new(),
    }
}

/// 检查单个数值字段：语法 + 引用规则 + 类型匹配 + 数组索引。
fn check_field(expr: &str, label: &str, types: &HashMap<String, VarKind>, errors: &mut Vec<DslError>, line: usize) {
    let node = match parse_expr(expr) {
        Ok(n) => n,
        Err(e) => {
            errors.push(DslError::at(line, format!("{label}表达式错误：{}", e.message)));
            return;
        }
    };
    let mut names = Vec::new();
    collect_plain_names(&node, &mut names);
    for name in names {
        match types.get(&name) {
            None => errors.push(DslError::at(
                line,
                format!("{label}表达式错误：引用了未定义的变量：{name}"),
            )),
            Some(kind) => {
                // 重复行（rows 非 1）变量已数组化：普通引用须改用 n[k]
                if let VarKind::Multi { rows, .. } = kind {
                    if !is_single_row(rows) {
                        errors.push(DslError::at(
                            line,
                            format!(
                                "{label}表达式错误：重复行变量 {name} 已数组化，请用 {name}[k] 索引引用"
                            ),
                        ));
                        continue;
                    }
                }
                if !is_refable(kind) {
                    errors.push(DslError::at(
                        line,
                        format!("{label}表达式错误：变量 {name} 类型不可作为引用源"),
                    ));
                }
            }
        }
    }
    check_indexes(&node, types, errors, line);
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
            VarKind::Multi { rows, parts } => {
                // 逐 part 检查并渐进登记名字：同一行内后者可引用前者（按当前行标量语义）
                for p in parts {
                    check_field(&p.expr, "", &types, &mut errors, line);
                    types.insert(
                        p.name.clone(),
                        VarKind::Scalar { expr: String::new() },
                    );
                }
                // 行结束后按真实语义登记（rows 非 1 时数组化，供后续语句引用检查）
                for p in parts {
                    types.insert(p.name.clone(), kind.clone());
                }
                check_field(rows, "重复行数", &types, &mut errors, line);
                check_const(rows, "重复行数", "重复行数不能小于 1", |v| v >= 1.0, &mut errors, line);
            }
            VarKind::Scalar { expr } => {
                check_field(expr, "", &types, &mut errors, line);
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
                        check_weight(w.as_ref(), "边权范围", "边权精度", &types, &mut errors, line);
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
                        check_weight(w.as_ref(), "边权范围", "边权精度", &types, &mut errors, line);
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
        match kind {
            VarKind::Multi { parts, .. } => {
                for p in parts {
                    types.insert(p.name.clone(), kind.clone());
                }
            }
            _ => {
                types.insert(item.name.clone(), kind.clone());
            }
        }
    }
    errors
}
