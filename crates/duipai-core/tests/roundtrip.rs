//! 解析 / 序列化往返一致性 + 错误用例（Phase 1 子集）。

use std::collections::HashMap;

use duipai_core::{eval_expr, parse, serialize, DslError};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

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
fn err_unsupported_command() {
    let e = parse("p = perm(5)").expect_err("should fail");
    assert_eq!(e.line, Some(1));
    assert!(e.message.contains("暂不支持"), "{e}");
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
    let mut env = HashMap::new();
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
    let mut env = HashMap::new();
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
    let mut env = HashMap::new();
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
    let mut env = HashMap::new();
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
    let mut env = HashMap::new();
    let mut rng = seeded();
    let e = eval_expr("\"abc\"", &env, &mut rng).expect_err("str node");
    assert!(e.message.contains("未知 AST 节点"), "{e}");
}
