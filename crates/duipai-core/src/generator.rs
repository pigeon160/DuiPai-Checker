//! 随机数据生成引擎（移植 legacy/duipai.py 的 _gen_items 及图/权值辅助）。
//!
//! 基于 IR 直接生成：全类型 + 多测模式 + 种子可复现（同种子输出逐字节一致）。

use std::collections::HashMap;

use crate::ast::{Config, ElemType, GraphType, VarKind, Weight};
use crate::error::{DslError, DslResult};
use crate::expr::eval_expr;
use rand::rngs::StdRng;
use rand::seq::{IndexedRandom, SliceRandom};
use rand::{Rng, SeedableRng};

/// 浮点格式化，与 legacy format_float 逐字符对齐：
/// `f"{v:.{prec}f}".rstrip("0").rstrip(".")`，空串 / "-0" -> "0"。
pub fn format_float(v: f64, prec: i64) -> String {
    let s = format!("{v:.prec$}", v = v, prec = prec as usize);
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    if s.is_empty() || s == "-0" {
        "0".to_string()
    } else {
        s
    }
}

struct GenCtx<'a> {
    env: HashMap<String, f64>,
    rng: &'a mut StdRng,
}

impl GenCtx<'_> {
    /// 求值一个表达式，错误带上字段标签（legacy 文案：`{label}表达式错误：...`）。
    fn ev(&mut self, expr: &str, label: &str) -> DslResult<f64> {
        eval_expr(expr, &self.env, self.rng).map_err(|e| {
            DslError::bare(format!("{label}表达式错误：{}", e.message))
        })
    }

    fn int(&mut self, lo: i64, hi: i64) -> i64 {
        self.rng.random_range(lo..=hi)
    }

    fn uniform(&mut self, lo: f64, hi: f64) -> f64 {
        self.rng.random_range(lo..hi)
    }

    fn shuffle<T>(&mut self, v: &mut [T]) {
        v.shuffle(self.rng);
    }
}

fn node_val_line(ctx: &mut GenCtx, n: i64, val: &Weight, lines: &mut Vec<String>) -> DslResult<()> {
    let line = if val.kind == ElemType::Int {
        let lo = ctx.ev(&val.min, "节点权值范围")? as i64;
        let hi = ctx.ev(&val.max, "节点权值范围")? as i64;
        if lo > hi {
            return Err(DslError::bare("节点权值范围最小值不能大于最大值"));
        }
        (0..n).map(|_| ctx.int(lo, hi).to_string()).collect::<Vec<_>>().join(" ")
    } else {
        let lo = ctx.ev(&val.min, "节点权值范围")?;
        let hi = ctx.ev(&val.max, "节点权值范围")?;
        if lo > hi {
            return Err(DslError::bare("节点权值范围最小值不能大于最大值"));
        }
        let prec = ctx.ev(&val.prec, "节点权值精度")? as i64;
        if !(0..=15).contains(&prec) {
            return Err(DslError::bare("节点权值精度应在 0~15 之间"));
        }
        (0..n)
            .map(|_| format_float(ctx.uniform(lo, hi), prec))
            .collect::<Vec<_>>()
            .join(" ")
    };
    lines.push(line);
    Ok(())
}

fn edge_line(ctx: &mut GenCtx, u: i64, v: i64, w: Option<&Weight>) -> DslResult<String> {
    match w {
        None => Ok(format!("{u} {v}")),
        Some(w) if w.kind == ElemType::Int => {
            let lo = ctx.ev(&w.min, "边权范围")? as i64;
            let hi = ctx.ev(&w.max, "边权范围")? as i64;
            if lo > hi {
                return Err(DslError::bare("边权范围最小值不能大于最大值"));
            }
            Ok(format!("{u} {v} {}", ctx.int(lo, hi)))
        }
        Some(w) => {
            let lo = ctx.ev(&w.min, "边权范围")?;
            let hi = ctx.ev(&w.max, "边权范围")?;
            if lo > hi {
                return Err(DslError::bare("边权范围最小值不能大于最大值"));
            }
            let prec = ctx.ev(&w.prec, "边权精度")? as i64;
            if !(0..=15).contains(&prec) {
                return Err(DslError::bare("边权精度应在 0~15 之间"));
            }
            Ok(format!("{u} {v} {}", format_float(ctx.uniform(lo, hi), prec)))
        }
    }
}

fn graph_ring(ctx: &mut GenCtx, n: i64, w: Option<&Weight>, val: Option<&Weight>, lines: &mut Vec<String>) -> DslResult<()> {
    if let Some(val) = val {
        lines.push(n.to_string());
        node_val_line(ctx, n, val, lines)?;
    } else {
        lines.push(n.to_string());
    }
    let mut edges = Vec::new();
    for i in 1..=n {
        let u = i;
        let v = if i % n == 0 { 1 } else { i + 1 };
        let (u, v) = if ctx.uniform(0.0, 1.0) < 0.5 { (v, u) } else { (u, v) };
        edges.push(edge_line(ctx, u, v, w)?);
    }
    lines.extend(edges);
    Ok(())
}

fn graph_base_ring(ctx: &mut GenCtx, n: i64, k: i64, w: Option<&Weight>, val: Option<&Weight>, lines: &mut Vec<String>) -> DslResult<()> {
    if let Some(val) = val {
        lines.push(n.to_string());
        node_val_line(ctx, n, val, lines)?;
    } else {
        lines.push(n.to_string());
    }
    let mut edge_set: Vec<(i64, i64)> = Vec::new();
    for i in 1..=k {
        let (u, v) = if i % k == 0 { (i, 1) } else { (i, i + 1) };
        let (u, v) = if u > v { (v, u) } else { (u, v) };
        edge_set.push((u, v));
    }
    for i in (k + 1)..=n {
        let p = ctx.int(1, i - 1);
        let (u, v) = if i > p { (p, i) } else { (i, p) };
        edge_set.push((u, v));
    }
    ctx.shuffle(&mut edge_set);
    let mut out = Vec::new();
    for (u, v) in edge_set {
        out.push(edge_line(ctx, u, v, w)?);
    }
    lines.extend(out);
    Ok(())
}

fn graph_dag(ctx: &mut GenCtx, n: i64, m: i64, w: Option<&Weight>, val: Option<&Weight>, lines: &mut Vec<String>) -> DslResult<()> {
    if m < 0 {
        return Err(DslError::bare("图边数 m 不能为负"));
    }
    let possible = n * (n - 1) / 2;
    if m > possible {
        return Err(DslError::bare(format!("图边数 m={m} 超过上限 {possible}（DAG，n={n}）")));
    }
    let mut set: Vec<(i64, i64)> = Vec::new();
    let mut attempts = 0i64;
    while (set.len() as i64) < m && attempts < m * 50 + 2000 {
        let u = ctx.int(1, n - 1);
        let v = ctx.int(u + 1, n);
        if !set.contains(&(u, v)) {
            set.push((u, v));
        }
        attempts += 1;
    }
    if (set.len() as i64) < m {
        return Err(DslError::bare("随机补边失败，请检查参数"));
    }
    ctx.shuffle(&mut set);
    let mut out = vec![format!("{n} {m}")];
    if let Some(val) = val {
        node_val_line(ctx, n, val, &mut out)?;
    }
    for (u, v) in set {
        out.push(edge_line(ctx, u, v, w)?);
    }
    lines.extend(out);
    Ok(())
}

fn graph_bipartite(ctx: &mut GenCtx, n: i64, m: i64, w: Option<&Weight>, val: Option<&Weight>, lines: &mut Vec<String>) -> DslResult<()> {
    if m < 0 {
        return Err(DslError::bare("图边数 m 不能为负"));
    }
    let left = n / 2;
    let right = n - left;
    if left < 1 || right < 1 {
        return Err(DslError::bare("二分图 n 过小，无法分两部"));
    }
    let possible = left * right;
    if m > possible {
        return Err(DslError::bare(format!("图边数 m={m} 超过上限 {possible}（二分图，n={n}）")));
    }
    let mut pairs: Vec<(i64, i64)> = Vec::with_capacity(possible as usize);
    for u in 1..=left {
        for v in 1..=right {
            pairs.push((u, left + v));
        }
    }
    pairs.shuffle(ctx.rng);
    let set = &pairs[..m as usize];
    let mut out = vec![format!("{n} {m}")];
    if let Some(val) = val {
        node_val_line(ctx, n, val, &mut out)?;
    }
    for (u, v) in set {
        out.push(edge_line(ctx, *u, *v, w)?);
    }
    lines.extend(out);
    Ok(())
}

fn gen_items(ctx: &mut GenCtx, items: &[crate::ast::Item], lines: &mut Vec<String>) -> DslResult<()> {
    for item in items {
        let line = item.line;
        if let Err(e) = gen_one(ctx, item, lines) {
            return Err(e.with_line(line));
        }
    }
    Ok(())
}

fn gen_one(ctx: &mut GenCtx, item: &crate::ast::Item, lines: &mut Vec<String>) -> DslResult<()> {
    let name = &item.name;
    match &item.kind {
        VarKind::Int { min, max } => {
            let lo = ctx.ev(min, "整数变量范围")? as i64;
            let hi = ctx.ev(max, "整数变量范围")? as i64;
            if lo > hi {
                return Err(DslError::bare("整数变量范围最小值不能大于最大值"));
            }
            let value = ctx.int(lo, hi);
            lines.push(value.to_string());
            ctx.env.insert(name.clone(), value as f64);
        }
        VarKind::Float { min, max, prec } => {
            let lo = ctx.ev(min, "浮点数变量范围")?;
            let hi = ctx.ev(max, "浮点数变量范围")?;
            if lo > hi {
                return Err(DslError::bare("浮点数变量范围最小值不能大于最大值"));
            }
            let prec = ctx.ev(prec, "浮点精度")? as i64;
            if !(0..=15).contains(&prec) {
                return Err(DslError::bare("浮点数变量精度应在 0~15 之间"));
            }
            let value = ctx.uniform(lo, hi);
            lines.push(format_float(value, prec));
            ctx.env.insert(name.clone(), value);
        }
        VarKind::Array { elem_type, el_min, el_max, prec, rows, cols } => {
            let rows_n = ctx.ev(rows, "数组行数")? as i64;
            let cols_n = ctx.ev(cols, "数组每行长度")? as i64;
            if rows_n < 1 {
                return Err(DslError::bare("数组行数不能小于 1"));
            }
            if cols_n < 0 {
                return Err(DslError::bare("数组每行长度不能为负"));
            }
            if *elem_type == ElemType::Float {
                let lo = ctx.ev(el_min, "数组元素范围")?;
                let hi = ctx.ev(el_max, "数组元素范围")?;
                if lo > hi {
                    return Err(DslError::bare("数组元素范围最小值不能大于最大值"));
                }
                let prec = ctx.ev(prec, "数组元素精度")? as i64;
                if !(0..=15).contains(&prec) {
                    return Err(DslError::bare("数组元素精度应在 0~15 之间"));
                }
                for _ in 0..rows_n {
                    let row: Vec<String> =
                        (0..cols_n).map(|_| format_float(ctx.uniform(lo, hi), prec)).collect();
                    lines.push(row.join(" "));
                }
            } else {
                let elo = ctx.ev(el_min, "数组元素范围")? as i64;
                let ehi = ctx.ev(el_max, "数组元素范围")? as i64;
                if elo > ehi {
                    return Err(DslError::bare("数组元素范围最小值不能大于最大值"));
                }
                for _ in 0..rows_n {
                    let row: Vec<String> =
                        (0..cols_n).map(|_| ctx.int(elo, ehi).to_string()).collect();
                    lines.push(row.join(" "));
                }
            }
        }
        VarKind::String { rows, cols, charset } => {
            if charset.is_empty() {
                return Err(DslError::bare("字符串字符集不能为空"));
            }
            let rows_n = ctx.ev(rows, "字符串行数")? as i64;
            let len = ctx.ev(cols, "字符串长度")? as i64;
            if rows_n < 1 {
                return Err(DslError::bare("字符串行数不能小于 1"));
            }
            if len < 0 {
                return Err(DslError::bare("字符串长度不能为负"));
            }
            let chars: Vec<char> = charset.chars().collect();
            for _ in 0..rows_n {
                let s: String = (0..len).map(|_| *chars.choose(ctx.rng).unwrap()).collect();
                lines.push(s);
            }
        }
        VarKind::Binseq { n, k } => {
            let n = ctx.ev(n, "0/1序列长度")? as i64;
            let k = ctx.ev(k, "0/1序列中1的个数")? as i64;
            if n < 0 {
                return Err(DslError::bare("0/1序列长度不能为负"));
            }
            if !(0..=n).contains(&k) {
                return Err(DslError::bare("1 的个数 k 应在 0~n 之间"));
            }
            let mut seq: Vec<i64> = vec![1; k as usize];
            seq.extend(vec![0; (n - k) as usize]);
            ctx.shuffle(&mut seq);
            lines.push(seq.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(" "));
        }
        VarKind::Intervals { n, lo, hi } => {
            let n = ctx.ev(n, "区间个数")? as i64;
            let lo = ctx.ev(lo, "区间下界")? as i64;
            let hi = ctx.ev(hi, "区间上界")? as i64;
            if n < 0 {
                return Err(DslError::bare("区间个数不能为负"));
            }
            if lo > hi {
                return Err(DslError::bare("区间下界不能大于上界"));
            }
            for _ in 0..n {
                let l = ctx.int(lo, hi);
                let r = ctx.int(l, hi);
                lines.push(format!("{l} {r}"));
            }
        }
        VarKind::Points { n, xlo, xhi, ylo, yhi } => {
            let n = ctx.ev(n, "点个数")? as i64;
            let xlo = ctx.ev(xlo, "点 x 下界")? as i64;
            let xhi = ctx.ev(xhi, "点 x 上界")? as i64;
            let ylo = ctx.ev(ylo, "点 y 下界")? as i64;
            let yhi = ctx.ev(yhi, "点 y 上界")? as i64;
            if n < 0 {
                return Err(DslError::bare("点个数不能为负"));
            }
            if xlo > xhi || ylo > yhi {
                return Err(DslError::bare("点坐标范围无效"));
            }
            for _ in 0..n {
                lines.push(format!("{} {}", ctx.int(xlo, xhi), ctx.int(ylo, yhi)));
            }
        }
        VarKind::Perm { n } => {
            let n = ctx.ev(n, "排列长度")? as i64;
            if n < 1 {
                return Err(DslError::bare("排列长度 n 应 >= 1"));
            }
            let mut perm: Vec<i64> = (1..=n).collect();
            ctx.shuffle(&mut perm);
            lines.push(perm.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(" "));
            ctx.env.insert(name.clone(), n as f64);
        }
        VarKind::Tree { n, w, val } => {
            let n = ctx.ev(n, "树顶点数")? as i64;
            if n < 1 {
                return Err(DslError::bare("树顶点数 n 应 >= 1"));
            }
            let mut out = vec![n.to_string()];
            if let Some(val) = val {
                node_val_line(ctx, n, val, &mut out)?;
            }
            let mut edges = Vec::new();
            for i in 2..=n {
                let p = ctx.int(1, i - 1);
                let (u, v) = if ctx.uniform(0.0, 1.0) < 0.5 { (i, p) } else { (p, i) };
                edges.push(edge_line(ctx, u, v, w.as_ref())?);
            }
            ctx.shuffle(&mut edges);
            out.extend(edges);
            lines.extend(out);
            ctx.env.insert(name.clone(), n as f64);
        }
        VarKind::Graph { gtype, n, m, directed, connected, k, w, val } => {
            match gtype {
                GraphType::Ring => {
                    let n = ctx.ev(n, "环顶点数")? as i64;
                    if n < 3 {
                        return Err(DslError::bare("环顶点数 n 应 >= 3"));
                    }
                    graph_ring(ctx, n, w.as_ref(), val.as_ref(), lines)?;
                    ctx.env.insert(name.clone(), n as f64);
                    return Ok(());
                }
                GraphType::BaseRing => {
                    let n = ctx.ev(n, "基环树顶点数")? as i64;
                    let k = ctx.ev(k.as_deref().unwrap_or("3"), "环大小")? as i64;
                    if n < 3 {
                        return Err(DslError::bare("基环树顶点数 n 应 >= 3"));
                    }
                    if !(3..=n).contains(&k) {
                        return Err(DslError::bare("环大小 k 应在 3~n 之间"));
                    }
                    graph_base_ring(ctx, n, k, w.as_ref(), val.as_ref(), lines)?;
                    ctx.env.insert(name.clone(), n as f64);
                    return Ok(());
                }
                GraphType::Dag => {
                    let n = ctx.ev(n, "图顶点数")? as i64;
                    let m = ctx.ev(m, "图边数")? as i64;
                    if n < 1 {
                        return Err(DslError::bare("图顶点数 n 应 >= 1"));
                    }
                    graph_dag(ctx, n, m, w.as_ref(), val.as_ref(), lines)?;
                    ctx.env.insert(name.clone(), n as f64);
                    return Ok(());
                }
                GraphType::Bipartite => {
                    let n = ctx.ev(n, "图顶点数")? as i64;
                    let m = ctx.ev(m, "图边数")? as i64;
                    if n < 1 {
                        return Err(DslError::bare("图顶点数 n 应 >= 1"));
                    }
                    graph_bipartite(ctx, n, m, w.as_ref(), val.as_ref(), lines)?;
                    ctx.env.insert(name.clone(), n as f64);
                    return Ok(());
                }
                GraphType::General => {}
            }
            let n = ctx.ev(n, "图顶点数")? as i64;
            let m = ctx.ev(m, "图边数")? as i64;
            if n < 1 {
                return Err(DslError::bare("图顶点数 n 应 >= 1"));
            }
            if m < 0 {
                return Err(DslError::bare("图边数 m 不能为负"));
            }
            let possible = if *directed { n * (n - 1) } else { n * (n - 1) / 2 };
            if m > possible {
                return Err(DslError::bare(format!(
                    "图边数 m={m} 超过上限 {possible}（{}，n={n}）",
                    if *directed { "有向" } else { "无向" }
                )));
            }
            if *connected && m < n - 1 {
                return Err(DslError::bare("连通图要求 m >= n-1"));
            }
            let mut edge_set: Vec<(i64, i64)> = Vec::new();
            if *connected {
                for i in 2..=n {
                    let p = ctx.int(1, i - 1);
                    let (mut u, mut v) = if ctx.uniform(0.0, 1.0) < 0.5 { (i, p) } else { (p, i) };
                    if !*directed && u > v {
                        std::mem::swap(&mut u, &mut v);
                    }
                    if !edge_set.contains(&(u, v)) {
                        edge_set.push((u, v));
                    }
                }
            }
            let need = (m - edge_set.len() as i64) as usize;
            if (edge_set.len() as i64) < m {
                if possible <= 100_000 {
                    let mut all: Vec<(i64, i64)> = Vec::with_capacity(possible as usize);
                    for u in 1..=n {
                        for v in 1..=n {
                            if u == v {
                                continue;
                            }
                            if !*directed && u > v {
                                continue;
                            }
                            all.push((u, v));
                        }
                    }
                    let candidates: Vec<(i64, i64)> =
                        all.into_iter().filter(|e| !edge_set.contains(e)).collect();
                    if need > candidates.len() {
                        return Err(DslError::bare("随机补边失败，请检查参数"));
                    }
                    edge_set.extend(candidates.choose_multiple(ctx.rng, need).cloned());
                } else {
                    let mut attempts = 0i64;
                    while (edge_set.len() as i64) < m && attempts < m * 50 + 2000 {
                        let mut u = ctx.int(1, n);
                        let mut v = ctx.int(1, n);
                        if u == v {
                            attempts += 1;
                            continue;
                        }
                        if !*directed && u > v {
                            std::mem::swap(&mut u, &mut v);
                        }
                        if !edge_set.contains(&(u, v)) {
                            edge_set.push((u, v));
                        }
                        attempts += 1;
                    }
                    if (edge_set.len() as i64) < m {
                        return Err(DslError::bare("随机补边失败，请检查参数"));
                    }
                }
            }
            ctx.shuffle(&mut edge_set);
            let mut out = vec![format!("{n} {m}")];
            if let Some(val) = val {
                node_val_line(ctx, n, val, &mut out)?;
            }
            for (u, v) in edge_set {
                out.push(edge_line(ctx, u, v, w.as_ref())?);
            }
            lines.extend(out);
            ctx.env.insert(name.clone(), n as f64);
        }
    }
    Ok(())
}

/// 按配置生成一组数据，返回行列表。`seed` 为 None 时随机种子（不可复现）。
pub fn generate(config: &Config, seed: Option<u64>) -> DslResult<Vec<String>> {
    let mut rng = match seed {
        Some(s) => StdRng::seed_from_u64(s),
        None => StdRng::from_os_rng(),
    };
    generate_with(&config, &mut rng)
}

/// 指定 RNG 生成（供对拍循环复用同一种子流）。
pub fn generate_with(config: &Config, rng: &mut StdRng) -> DslResult<Vec<String>> {
    let mut lines: Vec<String> = Vec::new();
    let items = &config.items;
    match &config.repeat {
        Some(rep) if rep.enabled => {
            let count: i64 = rep
                .count
                .trim()
                .parse()
                .map_err(|_| DslError::bare(format!("多测模式重复次数必须是整数：{:?}", rep.count)))?;
            if count < 1 {
                return Err(DslError::bare("多测模式重复次数应 >= 1"));
            }
            lines.push(count.to_string());
            for _ in 0..count {
                let mut ctx = GenCtx { env: HashMap::new(), rng };
                let mut sub = Vec::new();
                gen_items(&mut ctx, items, &mut sub)?;
                lines.extend(sub);
            }
        }
        _ => {
            let mut ctx = GenCtx { env: HashMap::new(), rng };
            gen_items(&mut ctx, items, &mut lines)?;
        }
    }
    Ok(lines)
}
