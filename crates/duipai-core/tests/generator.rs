//! 生成引擎测试：种子可复现、各类型输出形态、多测、动态错误。

use duipai_core::{format_float, generate, parse};

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
n = int(1, 100)
x = float(0, 1, 4)
a = ints(n, 1, 100)
p = perm(n)
t = tree(n, w=int(1, 10))
g = graph(n, 50, 1, 0, w=int(1, 9))
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
    let cfg = parse("# 多测模式：重复 3 次\nn = int(5, 9)\na = ints(n, 1, 9)\n").unwrap();
    let lines = generate(&cfg, Some(1)).unwrap();
    assert_eq!(lines[0], "3", "首行输出组数");
    // 每组：1 行 n + 1 行数组
    assert_eq!(lines.len(), 1 + 3 * 2);
}

#[test]
fn int_var_output() {
    let cfg = parse("n = int(5, 5)\nm = int(n, n)\n").unwrap();
    let lines = generate(&cfg, Some(0)).unwrap();
    assert_eq!(lines, vec!["5", "5"]);
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
    let cfg = parse("t = tree(6, w=int(1, 5))\n").unwrap();
    let lines = generate(&cfg, Some(3)).unwrap();
    assert_eq!(lines[0], "6");
    assert_eq!(lines.len(), 6, "1 行 n + 5 条边");
    for l in &lines[1..] {
        let parts: Vec<&str> = l.split_whitespace().collect();
        assert_eq!(parts.len(), 3, "边权树边应有 3 个字段：{l}");
        assert!(parts[0] != parts[1]);
    }
}

#[test]
fn tree_val_shape() {
    let cfg = parse("t = tree(4, val=int(1, 9))\n").unwrap();
    let lines = generate(&cfg, Some(3)).unwrap();
    assert_eq!(lines[0], "4");
    assert_eq!(lines[1].split_whitespace().count(), 4, "节点权值行");
    assert_eq!(lines.len(), 1 + 1 + 3);
}

#[test]
fn graph_general_shape() {
    let cfg = parse("g = graph(5, 6, 0, 1, w=int(1, 3))\n").unwrap();
    let lines = generate(&cfg, Some(5)).unwrap();
    assert_eq!(lines[0], "5 6");
    assert_eq!(lines.len(), 7);
    // 连通无向图，每条边无自环
    for l in &lines[1..] {
        let parts: Vec<&str> = l.split_whitespace().collect();
        assert_eq!(parts.len(), 3);
        assert_ne!(parts[0], parts[1]);
    }
}

#[test]
fn graph_ring_base_ring() {
    let cfg = parse("r = ring(5)\nbr = base_ring(6, 3)\n").unwrap();
    let lines = generate(&cfg, Some(1)).unwrap();
    assert_eq!(lines[0], "5");
    assert_eq!(lines.len(), 1 + 5 + 1 + 6, "环 5 边 + 基环树 6 边");
}

#[test]
fn dag_bipartite() {
    let cfg = parse("g = graph(6, 5, 1, 0, type=\"dag\")\nb = graph(6, 5, 0, 0, type=\"bipartite\")\n").unwrap();
    let lines = generate(&cfg, Some(2)).unwrap();
    assert_eq!(lines.len(), 1 + 5 + 1 + 5);
    // DAG：u < v
    for l in &lines[1..6] {
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
    let cfg = parse("s = str(5, \"01\")\nf = floats(3, 0, 1, 2)\n").unwrap();
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
    let cfg = parse("n = int(1, 2)\na = ints(1, n+1, n)\n").unwrap();
    let e = generate(&cfg, Some(0)).expect_err("should fail");
    assert!(e.message.contains("数组元素范围"), "{e}");
    assert_eq!(e.line, Some(2));
}

#[test]
fn empty_config() {
    let cfg = parse("").unwrap();
    let lines = generate(&cfg, Some(0)).unwrap();
    assert!(lines.is_empty());
}

#[test]
fn repeat_bad_count() {
    let cfg = parse("# 多测模式：重复 0 次\nn = int(1, 2)\n").unwrap();
    let e = generate(&cfg, Some(0)).expect_err("should fail");
    assert!(e.message.contains(">= 1"), "{e}");
}
