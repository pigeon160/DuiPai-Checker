//! 解析 / 序列化往返一致性 + 错误用例（Phase 1 子集）。

use std::collections::HashMap;

use duipai_core::{eval_expr, generate, parse, serialize, validate};
use rand::rngs::StdRng;
use rand::SeedableRng;

fn seeded() -> StdRng {
    StdRng::seed_from_u64(42)
}

#[test]
fn parse_basic_kinds() {
    let text = "\
# 多测模式：重复 3 次
n = int(1, 100)
x = float(0, 1)
y = float(0, 1, 4)
a = ints(n, 1, 100)
b = floats(3, 0, 1, 5)
c = ints(int(1, 5), 1, 9)
d = ints(2*n, 0, 1)
M = matrix(3, n, 0, 1)
F = matf(int(1, 5), n, 0, 1, 4)
";
    let cfg = parse(text).expect("parse should succeed");
    assert!(cfg.repeat.is_some());
    assert_eq!(cfg.repeat.as_ref().unwrap().count, "3");
    assert_eq!(cfg.items.len(), 9);
    assert_eq!(cfg.items[0].name, "n");
    assert_eq!(cfg.items[3].name, "a");
    assert_eq!(cfg.items[7].name, "M");
}

#[test]
fn roundtrip_stable() {
    let text = "\
# 多测模式：重复 3 次
n = int(1, 100)
x = float(0, 1, 4)
a = ints(n, 1, 100)
M = matrix(3, n, 0, 1)
F = matf(int(1, 5), n, 0, 1, 4)
";
    let cfg = parse(text).expect("parse");
    let out = serialize(&cfg).expect("serialize");
    assert_eq!(out, text.trim_end(), "serialize 应为规范化文本");
    let cfg2 = parse(&out).expect("re-parse");
    assert_eq!(cfg, cfg2, "二次解析 IR 应完全一致");
}

#[test]
fn roundtrip_no_repeat_no_prec() {
    let text = "\
n = int(1, 100)
x = float(0, 1)
a = ints(2*n, 1, 9)
b = floats(3, 0, 1)
";
    let cfg = parse(text).expect("parse");
    assert!(cfg.repeat.is_none());
    let out = serialize(&cfg).expect("serialize");
    // legacy 行为：`*` 运算符序列化时带空格（"2 * n"）
    assert_eq!(
        out,
        "n = int(1, 100)\nx = float(0, 1)\na = ints(2 * n, 1, 9)\nb = floats(3, 0, 1)"
    );
}

#[test]
fn repeat_comment_variants() {
    // 无次数 -> 1
    let cfg = parse("# 多测模式\nn = int(1, 2)").expect("parse");
    assert_eq!(cfg.repeat.unwrap().count, "1");
    // 中文冒号 + 次
    let cfg = parse("# 多测模式：重复 5 次\nn = int(1, 2)").expect("parse");
    assert_eq!(cfg.repeat.unwrap().count, "5");
    // 英文冒号 + 无“次”
    let cfg = parse("# 多测模式: 重复 12\nn = int(1, 2)").expect("parse");
    assert_eq!(cfg.repeat.unwrap().count, "12");
    // 只在前 8 行内识别
    let cfg = parse("\n\n\n\n\n\n\n\n# 多测模式：重复 5 次\nn = int(1, 2)\n").expect("parse");
    assert!(cfg.repeat.is_none());
}

#[test]
fn comments_and_blank_lines_ignored() {
    let text = "\
# 注释行
n = int(1, 100)   # 行尾注释不被解析（按语句右括号截断？）
";
    // 行尾注释会进入 rhs 导致解析失败 —— legacy 行为一致
    assert!(parse(text).is_err());
    let text2 = "\
# 注释行

n = int(1, 100)
";
    let cfg = parse(text2).expect("parse");
    assert_eq!(cfg.items.len(), 1);
}

#[test]
fn err_unknown_command() {
    let e = parse("a = foo(1, 2)").expect_err("should fail");
    assert_eq!(e.line, Some(1));
    assert!(e.message.contains("未知命令"), "{e}");
}

#[test]
fn full_commands_roundtrip() {
    let text = "\
n = int(1, 100)
p = perm(n)
s = str(10)
s2 = strs(3, 5, \"01\")
b = binseq(n, 3)
iv = intervals(n, 1, 10)
ps = points(n, 0, 9, 0, 9)
t = tree(n)
tw = tree(n, int(1, 100))
tv = tree(n, w=float(0, 1, 4), val=int(1, 9))
g = graph(n, int(n, n*n), 1, 0)
gd = graph(n, 10, 1, 0, type=\"dag\")
gb = graph(n, 10, 0, 0, type=\"bipartite\")
gw = graph(n, 20, 1, 1, w=int(1, 10), val=float(0, 1))
r = ring(5)
rw = ring(5, w=int(1, 10), val=float(0, 1))
br = base_ring(n, 3)
brw = base_ring(n, 4, w=float(0, 1, 4))
";
    let cfg = parse(text).expect("parse all commands");
    assert_eq!(cfg.items.len(), 18);
    let out = serialize(&cfg).expect("serialize");
    let cfg2 = parse(&out).expect("re-parse");
    assert_eq!(cfg, cfg2, "全命令往返 IR 一致");
    assert!(out.contains("ring(5, w=int(1, 10), val=float(0, 1))"), "{out}");
    assert!(out.contains("base_ring(n, 4, w=float(0, 1, 4))"), "{out}");
}

#[test]
fn err_unknown_gtype() {
    let e = parse("g = graph(5, 5, 1, 0, type=\"foo\")").expect_err("should fail");
    assert!(e.message.contains("未知图类型"), "{e}");
}

#[test]
fn err_bad_weight() {
    let e = parse("t = tree(5, 2*n)").expect_err("should fail");
    assert!(e.message.contains("边权参数必须是"), "{e}");
}

#[test]
fn err_bad_charset_position() {
    // charset 必须是字符串字面量位置参数
    let e = parse("s = str(10, abc)").expect_err("should fail");
    assert!(e.message.contains("需要 1~1 个位置参数"), "{e}");
}

#[test]
fn err_reserved_word_name() {
    let e = parse("int = int(1, 2)").expect_err("should fail");
    assert!(e.message.contains("保留字"), "{e}");
}

#[test]
fn err_duplicate_name() {
    let e = parse("n = int(1, 2)\nn2 = int(3, 4)\nn = int(5, 6)").expect_err("should fail");
    assert_eq!(e.line, Some(3));
    assert!(e.message.contains("变量名重复"), "{e}");
}

#[test]
fn err_bad_indent() {
    let e = parse("n = int(1, 2)\n  m = int(3, 4)").expect_err("should fail");
    assert_eq!(e.line, Some(2));
    assert!(e.message.contains("缩进"), "{e}");
}

#[test]
fn err_missing_equals() {
    let e = parse("n int(1, 2)").expect_err("should fail");
    assert!(e.message.contains("缺少 '='"), "{e}");
}

#[test]
fn err_missing_paren() {
    let e = parse("n = int(1, 2").expect_err("should fail");
    assert!(e.message.contains("缺少右括号"), "{e}");
}

#[test]
fn err_extra_close_paren_no_panic() {
    // 多余右括号（历史上导致 usize 下溢 panic，进而整个应用闪退）。
    // 修复后：parse 不 panic；表达式字段含非法 `)`，由静态校验兜底报错。
    for text in [
        "n = int(1, 2))",
        "n = int((1, 2))",
        "n = int(1), 2)",
        "n = tree(5, w=int(1, 10)))",
        "n = int(1, ))",
    ] {
        // 优雅失败路径一：parse 直接报错（不带 panic）
        if let Err(e) = parse(text) {
            assert!(!e.message.is_empty(), "{text}");
            continue;
        }
        // 优雅失败路径二：parse 通过，静态校验兜底
        let cfg = parse(text).expect("parse ok");
        let errs = validate(&cfg);
        assert!(!errs.is_empty(), "{text} 应被静态校验捕获：{cfg:?}");
    }
}

#[test]
fn err_bad_char() {
    let e = parse("n = int(1, 2)!\nm = int(1, 2)").expect_err("should fail");
    assert_eq!(e.line, Some(1));
    assert!(e.message.contains("无法识别的字符"), "{e}");
}

#[test]
fn err_wrong_arity() {
    let e = parse("n = int(1)").expect_err("should fail");
    assert!(e.message.contains("需要 2~2 个位置参数"), "{e}");
}

#[test]
fn err_kw_dup() {
    let e = parse("x = float(0, 1, prec=3, prec=4)").expect_err("should fail");
    assert!(e.message.contains("关键字参数重复"), "{e}");
}

// --------------------------------------------------------------------------- //
// 表达式
// --------------------------------------------------------------------------- //

#[test]
fn expr_arithmetic() {
    let mut env = HashMap::new();
    env.insert("n".to_string(), 100.0);
    let mut rng = seeded();
    assert_eq!(eval_expr("2*n", &env, &mut rng).unwrap(), 200.0);
    assert_eq!(eval_expr("n+1", &env, &mut rng).unwrap(), 101.0);
    assert_eq!(eval_expr("n//3", &env, &mut rng).unwrap(), 33.0);
    assert_eq!(eval_expr("7 % 3", &env, &mut rng).unwrap(), 1.0);
    assert_eq!(eval_expr("5.5 % 2", &env, &mut rng).unwrap(), 1.5);
    assert_eq!(eval_expr("-(n - 1)", &env, &mut rng).unwrap(), -99.0);
    assert_eq!(eval_expr("2 ** 10", &env, &mut rng).unwrap(), 1024.0);
    assert_eq!(eval_expr("(2+3)*4", &env, &mut rng).unwrap(), 20.0);
    assert_eq!(eval_expr("7 // 2", &env, &mut rng).unwrap(), 3.0);
    assert_eq!(eval_expr("-7 // 2", &env, &mut rng).unwrap(), -4.0);
}

#[test]
fn expr_int_range() {
    let env = HashMap::new();
    let mut rng = seeded();
    for _ in 0..200 {
        let v = eval_expr("int(1, 100)", &env, &mut rng).unwrap();
        assert!((1.0..=100.0).contains(&v), "{v}");
    }
    // 边界
    assert_eq!(eval_expr("int(5, 5)", &env, &mut rng).unwrap(), 5.0);
    let e = eval_expr("int(5, 4)", &env, &mut rng).expect_err("lo > hi");
    assert!(e.message.contains("int 范围 5 > 4"), "{e}");
}

#[test]
fn expr_float_range() {
    let env = HashMap::new();
    let mut rng = seeded();
    for _ in 0..200 {
        let v = eval_expr("float(0, 1)", &env, &mut rng).unwrap();
        assert!((0.0..1.0).contains(&v), "{v}");
    }
    let e = eval_expr("float(2, 1)", &env, &mut rng).expect_err("lo > hi");
    assert!(e.message.contains("float 范围"), "{e}");
    // prec 参数合法（3 参数），结果正常
    let v = eval_expr("float(0, 10, 4)", &env, &mut rng).unwrap();
    assert!((0.0..10.0).contains(&v));
}

#[test]
fn expr_errors() {
    let env = HashMap::new();
    let mut rng = seeded();
    let e = eval_expr("n + 1", &env, &mut rng).expect_err("undefined");
    assert!(e.message.contains("引用了未定义的变量"), "{e}");
    let e = eval_expr("foo(1)", &env, &mut rng).expect_err("unknown fn");
    assert!(e.message.contains("未知函数调用"), "{e}");
    let e = eval_expr("int(1,2,3)", &env, &mut rng).expect_err("arity");
    assert!(e.message.contains("需要两个参数"), "{e}");
    let e = eval_expr("1 +", &env, &mut rng).expect_err("syntax");
    assert!(e.message.contains("语法错误"), "{e}");
    let e = eval_expr("(1", &env, &mut rng).expect_err("syntax");
    assert!(e.message.contains("语法错误"), "{e}");
}

#[test]
fn expr_seeded_deterministic() {
    let env = HashMap::new();
    let mut rng1 = seeded();
    let mut rng2 = seeded();
    let a: Vec<f64> = (0..10).map(|_| eval_expr("int(1, 1000)", &env, &mut rng1).unwrap()).collect();
    let b: Vec<f64> = (0..10).map(|_| eval_expr("int(1, 1000)", &env, &mut rng2).unwrap()).collect();
    assert_eq!(a, b, "同种子应产生相同序列");
}

#[test]
fn empty_and_whitespace_parse() {
    let cfg = parse("").expect("empty ok");
    assert!(cfg.items.is_empty() && cfg.repeat.is_none());
    let cfg = parse("   \n\n  # 只有注释\n").expect("blank ok");
    assert!(cfg.items.is_empty());
}

#[test]
fn unknown_ast_node_string_in_expr() {
    // 字符串字面量在求值位置应报错（字符集参数不进求值）
    let env = HashMap::new();
    let mut rng = seeded();
    let e = eval_expr("\"abc\"", &env, &mut rng).expect_err("str node");
    assert!(e.message.contains("未知 AST 节点"), "{e}");
}

// --------------------------------------------------------------------------- //
// 静态校验
// --------------------------------------------------------------------------- //

#[test]
fn validate_ref_rule() {
    // 数组不可作为引用源
    let cfg = parse("a = ints(5, 1, 9)\nn = int(a, 100)").expect("parse");
    let errs = validate(&cfg);
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert!(errs[0].message.contains("不可作为引用源"), "{errs:?}");
    assert_eq!(errs[0].line, Some(2));
}

#[test]
fn validate_undefined_ref() {
    let cfg = parse("n = int(m, 100)").expect("parse");
    let errs = validate(&cfg);
    assert!(errs.iter().any(|e| e.message.contains("未定义的变量")), "{errs:?}");
    // 前向引用同样拒绝
    let cfg = parse("n = int(m, 100)\nm = int(1, 5)").expect("parse");
    let errs = validate(&cfg);
    assert!(errs.iter().any(|e| e.message.contains("未定义的变量")), "{errs:?}");
}

#[test]
fn validate_ref_scale_of_structure_ok() {
    // perm/tree/graph 引用取其规模值，合法
    let cfg = parse("t = tree(10)\na = ints(t, 1, 5)\np = perm(6)\ng = graph(8, 5, 0, 0)\nm = int(p + g, t)").expect("parse");
    let errs = validate(&cfg);
    assert!(errs.is_empty(), "{errs:?}");
}

#[test]
fn validate_const_range() {
    let cfg = parse("n = int(5, 4)").expect("parse");
    let errs = validate(&cfg);
    assert!(errs.iter().any(|e| e.message.contains("最小值不能大于最大值")), "{errs:?}");
}

#[test]
fn validate_perm_size() {
    let cfg = parse("p = perm(0)").expect("parse");
    let errs = validate(&cfg);
    assert!(errs.iter().any(|e| e.message.contains("n 应 >= 1")), "{errs:?}");
}

#[test]
fn validate_graph_m_limit() {
    let cfg = parse("g = graph(3, 10, 0, 0)").expect("parse");
    let errs = validate(&cfg);
    assert!(errs.iter().any(|e| e.message.contains("超过上限")), "{errs:?}");
    // 无向 n=3 上限 3，m=4 应报错
    let cfg = parse("g = graph(3, 4, 0, 0)").expect("parse");
    let errs = validate(&cfg);
    assert!(errs.iter().any(|e| e.message.contains("超过上限")), "{errs:?}");
}

#[test]
fn validate_connected_min_edges() {
    let cfg = parse("g = graph(5, 3, 0, 1)").expect("parse");
    let errs = validate(&cfg);
    assert!(errs.iter().any(|e| e.message.contains("连通图要求")), "{errs:?}");
}

#[test]
fn validate_binseq_k() {
    let cfg = parse("b = binseq(5, 6)").expect("parse");
    let errs = validate(&cfg);
    assert!(errs.iter().any(|e| e.message.contains("0~n 之间")), "{errs:?}");
}

#[test]
fn validate_clean_config() {
    let text = "\
n = int(1, 100)
x = float(0, 1, 4)
a = ints(n, 1, 100)
p = perm(n)
t = tree(n, int(1, 10))
g = graph(n, 50, 1, 1, w=int(1, 9))
r = ring(n)
br = base_ring(n, 3)
";
    let cfg = parse(text).expect("parse");
    let errs = validate(&cfg);
    assert!(errs.is_empty(), "{errs:?}");
}

#[test]
fn validate_bad_weight_range() {
    let cfg = parse("t = tree(5, w=float(9, 1))").expect("parse");
    let errs = validate(&cfg);
    assert!(errs.iter().any(|e| e.message.contains("最小值不能大于最大值")), "{errs:?}");
}

// --------------------------------------------------------------------------- //
// 多值行（一行多个数，可命名）
// --------------------------------------------------------------------------- //

#[test]
fn multi_roundtrip() {
    let text = "a, b = int(1, 100), float(0, 1)\nn, m = int(1, 5), int(1, 5)\n";
    let cfg = parse(text).expect("parse");
    assert_eq!(cfg.items.len(), 2);
    let out = serialize(&cfg).expect("serialize");
    assert_eq!(out, "a, b = int(1, 100), float(0, 1)\nn, m = int(1, 5), int(1, 5)");
    let cfg2 = parse(&out).expect("re-parse");
    assert_eq!(cfg, cfg2);
}

#[test]
fn multi_count_mismatch() {
    let e = parse("a, b = int(1, 2)\n").expect_err("2 names 1 cmd");
    assert!(e.message.contains("数量不一致"), "{e}");
    let e = parse("a = int(1, 2), int(3, 4)\n").expect_err("1 name 2 cmds");
    assert!(e.message.contains("数量不一致"), "{e}");
}

#[test]
fn multi_rejects_compound_kind() {
    let e = parse("a, b = int(1, 2), tree(5)\n").expect_err("tree not allowed");
    assert!(e.message.contains("不支持一行多值"), "{e}");
}

#[test]
fn multi_duplicate_part_name() {
    let e = parse("a, a = int(1, 2), int(3, 4)\n").expect_err("dup");
    assert!(e.message.contains("变量名重复"), "{e}");
}

#[test]
fn multi_generate_one_line() {
    let cfg = parse("a, b = int(5, 5), float(1, 1)\nc = int(b, a)\n").expect("parse");
    let errs = validate(&cfg);
    assert!(errs.is_empty(), "{errs:?}");
    let lines = generate(&cfg, Some(0)).unwrap();
    assert_eq!(lines[0], "5 1", "一行输出 a b：{lines:?}");
    let c: i64 = lines[1].parse().unwrap();
    assert!((1..=5).contains(&c), "c 引用 a、b 成功（b 取整 1，a=5）：{lines:?}");
}

#[test]
fn multi_refs_allowed() {
    // 多值行 part 名可被后续引用
    let cfg = parse("n, m = int(1, 10), int(1, 10)\nx = ints(n, 1, m)\n").expect("parse");
    let errs = validate(&cfg);
    assert!(errs.is_empty(), "{errs:?}");
}
