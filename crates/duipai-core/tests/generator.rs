//! 生成引擎测试：种子可复现、各类型输出形态、多测、动态错误。

use duipai_core::{format_float, generate, parse, validate};

#[test]
fn format_float_matches_legacy() {
    assert_eq!(format_float(1.5, 6), "1.5");
    assert_eq!(format_float(1.0, 6), "1");
    assert_eq!(format_float(0.0, 6), "0");
    assert_eq!(format_float(-0.00001, 4), "0");
    assert_eq!(format_float(3.14159, 2), "3.14");
    assert_eq!(format_float(100.0, 2), "100");
}

#[test]
fn seed_reproducible() {
    let text = "\
line:
    int n: 1, 100
    float x: 0, 1, 4
a = ints(n, 1, 100)
p = perm(n)
t = tree(n, int(1, 10))
g = graph(n, 50, 1, 0, int(1, 9))
r = ring(5)
br = base_ring(n, 3)
";
    let cfg = parse(text).unwrap();
    let a = generate(&cfg, Some(42)).unwrap();
    let b = generate(&cfg, Some(42)).unwrap();
    assert_eq!(a, b, "同种子输出应逐字节一致");
    // 不同种子大概率不同
    let c = generate(&cfg, Some(43)).unwrap();
    assert_ne!(a, c);
}

#[test]
fn multi_test_shape() {
    let cfg = parse("repeat (3):\n    line:\n        int n: 5, 9\n    a = ints(n, 1, 9)\n").unwrap();
    let lines = generate(&cfg, Some(1)).unwrap();
    // 不输出组数行；每组：1 行 n + 1 行数组
    assert_eq!(lines.len(), 3 * 2, "repeat 3 轮，无组数行：{lines:?}");
    // 变量每轮覆盖：每组 n 在 5..=9
    for l in lines.iter().step_by(2) {
        let n: i64 = l.parse().unwrap();
        assert!((5..=9).contains(&n), "n={n}");
    }
}

#[test]
fn repeat_var_overwrites() {
    // 第二轮的 n 应覆盖第一轮（每轮独立随机），且数组 a 引用当轮 n
    let cfg = parse("repeat (3):\n    line:\n        int n: 5, 5\n    a = ints(n, 1, 9)\n").unwrap();
    let lines = generate(&cfg, Some(1)).unwrap();
    assert_eq!(lines.len(), 6);
    // n 恒 5，数组首行 = 5 个元素
    assert_eq!(lines[0], "5");
    assert_eq!(lines[1].split_whitespace().count(), 5);
    assert_eq!(lines[2], "5");
    assert_eq!(lines[3].split_whitespace().count(), 5);
}

#[test]
fn repeat_count_expr() {
    let cfg = parse("repeat (2*2):\n    line:\n        int n: 1, 2\n").unwrap();
    let errs = validate(&cfg);
    assert!(errs.is_empty(), "{errs:?}");
    let lines = generate(&cfg, Some(1)).unwrap();
    assert_eq!(lines.len(), 4, "count 表达式 2*2=4 轮");
}

#[test]
fn int_var_output() {
    let cfg = parse("line:\n    int n: 5, 5\n    int m: n, n\n").unwrap();
    let lines = generate(&cfg, Some(0)).unwrap();
    assert_eq!(lines, vec!["5 5"]);
}

#[test]
fn perm_output() {
    let cfg = parse("p = perm(5)\n").unwrap();
    let lines = generate(&cfg, Some(7)).unwrap();
    let mut v: Vec<i32> = lines[0].split_whitespace().map(|x| x.parse().unwrap()).collect();
    v.sort_unstable();
    assert_eq!(v, vec![1, 2, 3, 4, 5], "排列包含 1..5");
}

#[test]
fn tree_shape() {
    let cfg = parse("t = tree(6, int(1, 5))\n").unwrap();
    let lines = generate(&cfg, Some(3)).unwrap();
    assert_eq!(lines.len(), 5, "无规模行，只输出 5 条边：{lines:?}");
    for l in &lines {
        let parts: Vec<&str> = l.split_whitespace().collect();
        assert_eq!(parts.len(), 3, "边权树边应有 3 个字段：{l}");
        assert!(parts[0] != parts[1]);
    }
}

#[test]
fn tree_star_chain() {
    let cfg = parse("t = tree(5, type=\"star\")\nc = tree(5, type=\"chain\")\n").unwrap();
    let errs = validate(&cfg);
    assert!(errs.is_empty(), "{errs:?}");
    let lines = generate(&cfg, Some(3)).unwrap();
    assert_eq!(lines.len(), 4 + 4, "菊花图 4 边 + 链 4 边：{lines:?}");
    // 菊花图：中心 1 出现在所有边上
    for l in &lines[..4] {
        assert!(l.split_whitespace().any(|p| p == "1"), "菊花图中心 1：{l}");
    }
    // 链：每条边端点合并后能连成通路（顶点数 5）
    let mut used = std::collections::HashSet::new();
    for l in &lines[4..] {
        for p in l.split_whitespace().take(2) {
            used.insert(p.to_string());
        }
    }
    assert_eq!(used.len(), 5, "链覆盖全部顶点：{lines:?}");
}

#[test]
fn tree_parent_shape() {
    // 以 1 为根：输出 n-1 行，第 i 行 = 节点 i+1 的父节点（1..=i）
    let cfg = parse("t = tree(6, type=\"parent\")\n").unwrap();
    let errs = validate(&cfg);
    assert!(errs.is_empty(), "{errs:?}");
    let lines = generate(&cfg, Some(3)).unwrap();
    assert_eq!(lines.len(), 5, "parent 树输出 n-1=5 行：{lines:?}");
    for (i, l) in lines.iter().enumerate() {
        let p: i64 = l.parse().unwrap();
        let node = (i + 2) as i64;
        assert!((1..node).contains(&p), "节点 {node} 的父节点 {p} 应 < {node}");
    }
    // 带权：每行 父节点 + 权值
    let cfg = parse("t = tree(5, type=\"parent\", int(1, 9))\n").unwrap();
    let lines = generate(&cfg, Some(3)).unwrap();
    assert_eq!(lines.len(), 4);
    for l in &lines {
        assert_eq!(l.split_whitespace().count(), 2, "带权父节点行：{l}");
    }
}

#[test]
fn graph_general_shape() {
    let cfg = parse("g = graph(5, 6, 0, 1, int(1, 3))\n").unwrap();
    let lines = generate(&cfg, Some(5)).unwrap();
    assert_eq!(lines.len(), 6, "无规模行，只输出 6 条边：{lines:?}");
    // 连通无向图，每条边无自环
    for l in &lines {
        let parts: Vec<&str> = l.split_whitespace().collect();
        assert_eq!(parts.len(), 3);
        assert_ne!(parts[0], parts[1]);
    }
}

#[test]
fn graph_ring_base_ring() {
    let cfg = parse("r = ring(5)\nbr = base_ring(6, 3)\n").unwrap();
    let lines = generate(&cfg, Some(1)).unwrap();
    assert_eq!(lines.len(), 5 + 6, "环 5 边 + 基环树 6 边（无规模行）：{lines:?}");
}

#[test]
fn dag_bipartite() {
    let cfg = parse("g = graph(6, 5, 1, 0, type=\"dag\")\nb = graph(6, 5, 0, 0, type=\"bipartite\")\n").unwrap();
    let lines = generate(&cfg, Some(2)).unwrap();
    assert_eq!(lines.len(), 5 + 5, "DAG 5 边 + 二分 5 边（无规模行）：{lines:?}");
    // DAG：u < v
    for l in &lines[..5] {
        let p: Vec<i64> = l.split_whitespace().map(|x| x.parse().unwrap()).collect();
        assert!(p[0] < p[1], "DAG 边应 u<v：{l}");
    }
}

#[test]
fn binseq_intervals_points() {
    let cfg = parse("b = binseq(10, 3)\niv = intervals(4, 1, 10)\nps = points(3, 0, 5, 0, 5)\n").unwrap();
    let lines = generate(&cfg, Some(9)).unwrap();
    let ones = lines[0].split_whitespace().filter(|x| *x == "1").count();
    assert_eq!(ones, 3);
    assert_eq!(lines.len(), 1 + 4 + 3);
    for l in &lines[1..5] {
        let p: Vec<i64> = l.split_whitespace().map(|x| x.parse().unwrap()).collect();
        assert!(p[0] <= p[1]);
    }
}

#[test]
fn string_and_float_array() {
    let cfg = parse("line:\n    str s: 5, \"01\"\nf = floats(3, 0, 1, 2)\n").unwrap();
    let lines = generate(&cfg, Some(0)).unwrap();
    assert_eq!(lines[0].len(), 5);
    assert!(lines[0].chars().all(|c| c == '0' || c == '1'));
    assert_eq!(lines[1].split_whitespace().count(), 3);
    for x in lines[1].split_whitespace() {
        assert!(x.contains('.'), "精度 2 的浮点应带小数：{x}");
    }
}

#[test]
fn dynamic_range_error() {
    // 引用导致的范围错误（静态无法判定，生成期报错），带变量行号
    let cfg = parse("line:\n    int n: 1, 2\na = ints(1, n+1, n)\n").unwrap();
    let e = generate(&cfg, Some(0)).expect_err("should fail");
    assert!(e.message.contains("数组元素范围"), "{e}");
    assert_eq!(e.line, Some(3));
}

#[test]
fn empty_config() {
    let cfg = parse("").unwrap();
    let lines = generate(&cfg, Some(0)).unwrap();
    assert!(lines.is_empty());
}

#[test]
fn repeat_bad_count() {
    let cfg = parse("repeat (0):\n    line:\n        int n: 1, 2\n").unwrap();
    let e = generate(&cfg, Some(0)).expect_err("should fail");
    assert!(e.message.contains(">= 1"), "{e}");
}
