//! 解析 / 序列化往返一致性 + 错误用例（层级 DSL：行块 + 顶层命令）。

use std::collections::HashMap;

use duipai_core::{eval_expr, generate, parse, serialize, validate, EnvValue};
use rand::rngs::StdRng;
use rand::SeedableRng;

fn seeded() -> StdRng {
    StdRng::seed_from_u64(42)
}

#[test]
fn parse_basic_kinds() {
    let text = "\
# 多测模式：重复 3 次
line:
    int n: 1, 100
    float x: 0, 1
    float y: 0, 1, 4
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
    assert_eq!(cfg.items.len(), 7);
    assert!(matches!(cfg.items[0].kind, duipai_core::VarKind::Line { .. }));
    assert_eq!(cfg.items[1].name, "a");
    assert_eq!(cfg.items[5].name, "M");
}

#[test]
fn roundtrip_stable() {
    let text = "\
# 多测模式：重复 3 次
line:
    int n: 1, 100
    float x: 0, 1, 4
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
line:
    int n: 1, 100
    float x: 0, 1
a = ints(2*n, 1, 9)
b = floats(3, 0, 1)
";
    let cfg = parse(text).expect("parse");
    assert!(cfg.repeat.is_none());
    let out = serialize(&cfg).expect("serialize");
    // legacy 行为：`*` 运算符序列化时带空格（"2 * n"）
    assert_eq!(
        out,
        "line:\n    int n: 1, 100\n    float x: 0, 1\na = ints(2 * n, 1, 9)\nb = floats(3, 0, 1)"
    );
}

#[test]
fn repeat_comment_variants() {
    // 无次数 -> 1
    let cfg = parse("# 多测模式\nline:\n    int n: 1, 2").expect("parse");
    assert_eq!(cfg.repeat.unwrap().count, "1");
    // 中文冒号 + 次
    let cfg = parse("# 多测模式：重复 5 次\nline:\n    int n: 1, 2").expect("parse");
    assert_eq!(cfg.repeat.unwrap().count, "5");
    // 英文冒号 + 无“次”
    let cfg = parse("# 多测模式: 重复 12\nline:\n    int n: 1, 2").expect("parse");
    assert_eq!(cfg.repeat.unwrap().count, "12");
    // 只在前 8 行内识别
    let cfg = parse("\n\n\n\n\n\n\n\n# 多测模式：重复 5 次\nline:\n    int n: 1, 2\n").expect("parse");
    assert!(cfg.repeat.is_none());
}

#[test]
fn comments_and_blank_lines_ignored() {
    let text = "\
# 注释行
line:
    int n: 1, 100   # 行尾注释不被解析
";
    // 行尾注释进入行内项参数 -> 解析失败（legacy 行为一致）
    assert!(parse(text).is_err());
    let text2 = "\
# 注释行

line:
    int n: 1, 100
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
fn top_level_scalar_rejected_with_guide() {
    // 整数/浮点/字符串强制行下，顶层写报错引导
    let e = parse("n = int(1, 100)").expect_err("int must be in line block");
    assert!(e.message.contains("行块"), "{e}");
    let e = parse("x = float(0, 1)").expect_err("float must be in line block");
    assert!(e.message.contains("行块"), "{e}");
    let e = parse("s = str(10)").expect_err("str must be in line block");
    assert!(e.message.contains("行块"), "{e}");
    let e = parse("s = strs(3, 5)").expect_err("strs removed");
    assert!(e.message.contains("行块"), "{e}");
    // 顶层标量表达式同样拒绝
    let e = parse("n = 2 * m + 1").expect_err("scalar expr top-level");
    assert!(e.message.contains("行块"), "{e}");
}

#[test]
fn full_commands_roundtrip() {
    let text = "\
line:
    int n: 1, 100
    str s: 10
    str s2: 5, \"01\"
p = perm(n)
b = binseq(n, 3)
iv = intervals(n, 1, 10)
ps = points(n, 0, 9, 0, 9)
t = tree(n)
tw = tree(n, int(1, 100))
tv = tree(n, float(0, 1, 4))
g = graph(n, int(n, n*n), 1, 0)
gd = graph(n, 10, 1, 0, type=\"dag\")
gb = graph(n, 10, 0, 0, type=\"bipartite\")
gw = graph(n, 20, 1, 1, int(1, 10))
r = ring(5)
rw = ring(5, int(1, 10))
br = base_ring(n, 3)
brw = base_ring(n, 4, float(0, 1, 4))
";
    let cfg = parse(text).expect("parse all commands");
    assert_eq!(cfg.items.len(), 16);
    let out = serialize(&cfg).expect("serialize");
    let cfg2 = parse(&out).expect("re-parse");
    assert_eq!(cfg, cfg2, "全命令往返 IR 一致");
    assert!(out.contains("ring(5, int(1, 10))"), "{out}");
    assert!(out.contains("base_ring(n, 4, float(0, 1, 4))"), "{out}");
    assert!(out.contains("tree(n, float(0, 1, 4))"), "{out}");
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
fn err_reserved_word_name() {
    let e = parse("line:\n    int int: 1, 2").expect_err("reserved");
    assert!(e.message.contains("保留字"), "{e}");
    // line 也是保留字
    let e = parse("line:\n    int line: 1, 2").expect_err("line reserved");
    assert!(e.message.contains("保留字"), "{e}");
}

#[test]
fn err_duplicate_name() {
    let e = parse("line:\n    int n: 1, 2\n    int n: 3, 4").expect_err("should fail");
    assert_eq!(e.line, Some(3));
    assert!(e.message.contains("变量名重复"), "{e}");
}

#[test]
fn err_bad_indent() {
    // 行块内的缩进行必须是行内项（此处是命令形式 -> 行内项类型错误）
    let e = parse("line:\n    int n: 1, 2\n    m = ints(3, 1, 9)").expect_err("should fail");
    assert_eq!(e.line, Some(3));
    assert!(e.message.contains("行内项"), "{e}");
    // 顶层缩进
    let e = parse("  a = ints(3, 1, 9)").expect_err("top indent");
    assert!(e.message.contains("缩进"), "{e}");
}

#[test]
fn err_missing_equals() {
    let e = parse("a ints(3, 1, 9)").expect_err("should fail");
    assert!(e.message.contains("缺少 '='"), "{e}");
}

#[test]
fn err_missing_paren() {
    let e = parse("a = ints(3, 1, 9").expect_err("should fail");
    assert!(e.message.contains("缺少右括号"), "{e}");
}

#[test]
fn err_extra_close_paren_no_panic() {
    // 多余右括号（历史上导致 usize 下溢 panic，进而整个应用闪退）。
    // 修复后：parse 不 panic；表达式字段含非法 `)`，由静态校验兜底报错。
    for text in [
        "a = ints(3, 1, 9))",
        "a = ints((3, 1, 9))",
        "a = ints(3), 9)",
        "t = tree(5, int(1, 10)))",
        "a = ints(3, ))",
        "line:\n    int n: 1, 2))",
    ] {
        if let Err(e) = parse(text) {
            assert!(!e.message.is_empty(), "{text}");
            continue;
        }
        let cfg = parse(text).expect("parse ok");
        let errs = validate(&cfg);
        assert!(!errs.is_empty(), "{text} 应被静态校验捕获：{cfg:?}");
    }
}

#[test]
fn err_bad_char() {
    let e = parse("a = ints(3, 1, 9)!\nb = ints(3, 1, 9)").expect_err("should fail");
    assert_eq!(e.line, Some(1));
    assert!(e.message.contains("无法识别的字符"), "{e}");
}

#[test]
fn err_wrong_arity() {
    let e = parse("a = ints(3)").expect_err("should fail");
    assert!(e.message.contains("需要 3~4 个位置参数"), "{e}");
}

#[test]
fn err_kw_dup() {
    let e = parse("t = tree(5, type=\"star\", type=\"chain\")").expect_err("should fail");
    assert!(e.message.contains("关键字参数重复"), "{e}");
}

#[test]
fn line_block_errors() {
    // 行块嵌套
    let e = parse("line:\n    line:\n        int n: 1, 2").expect_err("nested");
    assert!(e.message.contains("嵌套"), "{e}");
    // 行块缺冒号
    let e = parse("line (3)").expect_err("no colon");
    assert!(e.message.contains("line (N):"), "{e}");
    // 行块空
    let e = parse("line:").expect_err("empty block");
    assert!(e.message.contains("至少需要一个子项"), "{e}");
    // 行内项缺类型
    let e = parse("line:\n    n: 1, 2").expect_err("no kind");
    assert!(e.message.contains("int / float / text / expr / str"), "{e}");
    // 行内项缺冒号
    let e = parse("line:\n    int n 1, 2").expect_err("no colon item");
    assert!(e.message.contains("缺少冒号"), "{e}");
}

// --------------------------------------------------------------------------- //
// 表达式
// --------------------------------------------------------------------------- //

#[test]
fn expr_arithmetic() {
    let mut env = HashMap::new();
    env.insert("n".to_string(), EnvValue::Scalar(100.0));
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
    let cfg = parse("a = ints(5, 1, 9)\nline:\n    int n: a, 100").expect("parse");
    let errs = validate(&cfg);
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert!(errs[0].message.contains("不可作为引用源"), "{errs:?}");
    assert_eq!(errs[0].line, Some(2));
}

#[test]
fn validate_undefined_ref() {
    let cfg = parse("line:\n    int n: m, 100").expect("parse");
    let errs = validate(&cfg);
    assert!(errs.iter().any(|e| e.message.contains("未定义的变量")), "{errs:?}");
    // 前向引用同样拒绝
    let cfg = parse("line:\n    int n: m, 100\n    int m: 1, 5").expect("parse");
    let errs = validate(&cfg);
    assert!(errs.iter().any(|e| e.message.contains("未定义的变量")), "{errs:?}");
}

#[test]
fn validate_ref_scale_of_structure_ok() {
    // perm/tree/graph 引用取其规模值，合法
    let cfg = parse(
        "t = tree(10)\na = ints(t, 1, 5)\np = perm(6)\ng = graph(8, 5, 0, 0)\nline:\n    int m: p + g, t",
    )
    .expect("parse");
    let errs = validate(&cfg);
    assert!(errs.is_empty(), "{errs:?}");
}

#[test]
fn validate_const_range() {
    let cfg = parse("line:\n    int n: 5, 4").expect("parse");
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
line:
    int n: 1, 100
    float x: 0, 1, 4
a = ints(n, 1, 100)
p = perm(n)
t = tree(n, int(1, 10))
g = graph(n, 50, 1, 1, int(1, 9))
r = ring(n)
br = base_ring(n, 3)
";
    let cfg = parse(text).expect("parse");
    let errs = validate(&cfg);
    assert!(errs.is_empty(), "{errs:?}");
}

#[test]
fn validate_bad_weight_range() {
    let cfg = parse("t = tree(5, float(9, 1))").expect("parse");
    let errs = validate(&cfg);
    assert!(errs.iter().any(|e| e.message.contains("最小值不能大于最大值")), "{errs:?}");
}

// --------------------------------------------------------------------------- //
// 行块（一行多个数，行内项可不同类型）
// --------------------------------------------------------------------------- //

#[test]
fn line_block_roundtrip() {
    let text = "\
line:
    int n: 1, 100
    float x: 0, 1
    text s: \"---\"
    expr e: 2 * n
    str c: 10, \"ab\"
";
    let cfg = parse(text).expect("parse");
    let out = serialize(&cfg).expect("serialize");
    assert_eq!(out, text.trim_end());
    let cfg2 = parse(&out).expect("re-parse");
    assert_eq!(cfg, cfg2);
}

#[test]
fn line_block_generate_mixed() {
    let cfg = parse(
        "line:\n    int n: 5, 5\n    float x: 1, 1\n    text s: \"---\"\n    expr e: 2 * n\n",
    )
    .expect("parse");
    let errs = validate(&cfg);
    assert!(errs.is_empty(), "{errs:?}");
    let lines = generate(&cfg, Some(0)).unwrap();
    assert_eq!(lines[0], "5 1 --- 10", "混合行输出：{lines:?}");
}

#[test]
fn line_block_text_validation() {
    // 文本为空报错
    let cfg = parse("line:\n    text s: \"\"").expect("parse");
    let errs = validate(&cfg);
    assert!(errs.iter().any(|e| e.message.contains("不能为空")), "{errs:?}");
    // 文本含双引号 -> tokenizer 报字符串未闭合
    let e = parse("line:\n    text s: \"a\\\"b\"").expect_err("quote in text");
    assert!(e.message.contains("字符串缺少结束引号"), "{e}");
}

#[test]
fn line_block_str_random_len() {
    // 字符串长度可区间随机
    let cfg = parse("line:\n    str c: int(3, 5), \"01\"").expect("parse");
    let lines = generate(&cfg, Some(0)).unwrap();
    let len = lines[0].len();
    assert!((3..=5).contains(&len), "{lines:?}");
}

#[test]
fn line_refs_within_line() {
    // 同一行内后者可引用前者
    let cfg = parse("line:\n    int n: 1, 10\n    expr m: 2 * n").expect("parse");
    let errs = validate(&cfg);
    assert!(errs.is_empty(), "{errs:?}");
    let lines = generate(&cfg, Some(1)).unwrap();
    let v: Vec<i64> = lines[0].split_whitespace().map(|x| x.parse().unwrap()).collect();
    assert_eq!(v[1], 2 * v[0], "m = 2*n：{lines:?}");
}

#[test]
fn line_refs_from_following_statements() {
    let cfg = parse(
        "line:\n    int n: 5, 5\n    int m: 2, 2\na = ints(m, 5, 5)",
    )
    .expect("parse");
    let errs = validate(&cfg);
    assert!(errs.is_empty(), "{errs:?}");
    let lines = generate(&cfg, Some(0)).unwrap();
    assert_eq!(lines[0], "5 2");
    assert_eq!(lines[1], "5 5", "数组引用 n、m：{lines:?}");
}

#[test]
fn line_scalar_float_output() {
    let cfg = parse("line:\n    expr x: float(1, 2) / 4").expect("parse");
    let lines = generate(&cfg, Some(0)).unwrap();
    assert!(lines[0].contains('.'), "浮点结果带小数：{lines:?}");
}

// --------------------------------------------------------------------------- //
// 行重复（重复行变量数组化 n[k]）
// --------------------------------------------------------------------------- //

#[test]
fn line_repeat_roundtrip() {
    let text = "line (3):\n    int n: 1, 5\n    expr m: 2 * n\n";
    let cfg = parse(text).expect("parse");
    let out = serialize(&cfg).expect("serialize");
    assert_eq!(out, "line (3):\n    int n: 1, 5\n    expr m: 2 * n");
    let cfg2 = parse(&out).expect("re-parse");
    assert_eq!(cfg, cfg2);
}

#[test]
fn line_repeat_generate() {
    let cfg = parse("line (3):\n    int n: 1, 10\n    expr m: 2 * n").expect("parse");
    let errs = validate(&cfg);
    assert!(errs.is_empty(), "{errs:?}");
    let lines = generate(&cfg, Some(2)).unwrap();
    assert_eq!(lines.len(), 3, "输出 3 行：{lines:?}");
    for l in &lines {
        let v: Vec<i64> = l.split_whitespace().map(|x| x.parse().unwrap()).collect();
        assert_eq!(v.len(), 2, "{l}");
        assert_eq!(v[1], 2 * v[0], "每行 m = 2*n：{l}");
    }
}

#[test]
fn line_repeat_rows_expr() {
    // 行数可以是表达式（引用前面变量）
    let cfg = parse(
        "line:\n    int k: 2, 2\nline (k):\n    int n: 1, 5\n    expr m: 2 * n",
    )
    .expect("parse");
    let errs = validate(&cfg);
    assert!(errs.is_empty(), "{errs:?}");
    let lines = generate(&cfg, Some(0)).unwrap();
    assert_eq!(lines.len(), 3, "k 一行 + 重复 2 行：{lines:?}");
}

#[test]
fn line_repeat_zero_rejected() {
    let cfg = parse("line (0):\n    int n: 1, 5").expect("parse");
    let errs = validate(&cfg);
    assert!(errs.iter().any(|e| e.message.contains("不能小于 1")), "{errs:?}");
}

#[test]
fn line_repeat_name_array_ref() {
    // repeat(3) 后 n、m 数组化，x = n[2] 取第 2 行 n
    let cfg = parse(
        "line (3):\n    int n: 1, 5\n    int m: 1, 5\nline:\n    expr x: n[2]\n    expr y: m[1]",
    )
    .expect("parse");
    let errs = validate(&cfg);
    assert!(errs.is_empty(), "{errs:?}");
    let lines = generate(&cfg, Some(7)).unwrap();
    assert_eq!(lines.len(), 4, "3 行重复 + 1 行引用：{lines:?}");
    let row2_n = lines[1].split_whitespace().next().unwrap();
    let row1_m = lines[0].split_whitespace().nth(1).unwrap();
    let vals: Vec<&str> = lines[3].split_whitespace().collect();
    assert_eq!(vals[0], row2_n, "x = n[2] 应等于第 2 行 n：{lines:?}");
    assert_eq!(vals[1], row1_m, "y = m[1] 应等于第 1 行 m：{lines:?}");
}

#[test]
fn line_repeat_scalar_ref_rejected() {
    let cfg = parse(
        "line (3):\n    int n: 1, 5\n    int m: 1, 5\nline:\n    expr x: n + 1",
    )
    .expect("parse");
    let errs = validate(&cfg);
    assert!(errs.iter().any(|e| e.message.contains("已数组化")), "{errs:?}");
}

#[test]
fn line_repeat_index_oob() {
    let cfg = parse("line (3):\n    int n: 1, 5\nline:\n    expr x: n[5]").expect("parse");
    let errs = validate(&cfg);
    assert!(errs.iter().any(|e| e.message.contains("越界")), "{errs:?}");
}

#[test]
fn single_row_line_not_indexable() {
    // 不重复的行变量是标量，不能索引
    let cfg = parse("line:\n    int n: 1, 5\nline:\n    expr x: n[1]").expect("parse");
    let errs = validate(&cfg);
    assert!(errs.iter().any(|e| e.message.contains("不是数组")), "{errs:?}");
}

// --------------------------------------------------------------------------- //
// 数组索引引用 a[i] / a[i][j]（1 起）
// --------------------------------------------------------------------------- //

#[test]
fn index_single_row_array() {
    let cfg = parse(
        "a = ints(3, 10, 10)\nline:\n    int first: a[1], a[1]\n    int last: a[3], a[3]",
    )
    .expect("parse");
    let errs = validate(&cfg);
    assert!(errs.is_empty(), "{errs:?}");
    let lines = generate(&cfg, Some(0)).unwrap();
    assert_eq!(lines[0], "10 10 10");
    assert_eq!(lines[1], "10 10", "a[1] 与 a[3] 同行输出：{lines:?}");
}

#[test]
fn index_matrix() {
    let cfg = parse(
        "M = matrix(2, 3, 5, 5)\nline:\n    int x: M[1][2], M[1][2]\n    int y: M[2][3], M[2][3]",
    )
    .expect("parse");
    let errs = validate(&cfg);
    assert!(errs.is_empty(), "{errs:?}");
    let lines = generate(&cfg, Some(0)).unwrap();
    assert_eq!(lines[0], "5 5 5");
    assert_eq!(lines[1], "5 5 5");
    assert_eq!(lines[2], "5 5", "M[1][2] 与 M[2][3] 同行输出：{lines:?}");
}

#[test]
fn index_validate_layer_mismatch() {
    // 矩阵单层索引
    let cfg = parse("M = matrix(2, 3, 1, 9)\nline:\n    int x: M[1], 9").expect("parse");
    let errs = validate(&cfg);
    assert!(errs.iter().any(|e| e.message.contains("需要 2 个索引")), "{errs:?}");
    // 单行数组双层索引
    let cfg = parse("a = ints(3, 1, 9)\nline:\n    int x: a[1][2], 9").expect("parse");
    let errs = validate(&cfg);
    assert!(errs.iter().any(|e| e.message.contains("需要 1 个索引")), "{errs:?}");
}

#[test]
fn index_validate_oob() {
    let cfg = parse("a = ints(3, 1, 9)\nline:\n    int x: a[5], 9").expect("parse");
    let errs = validate(&cfg);
    assert!(errs.iter().any(|e| e.message.contains("越界")), "{errs:?}");
    let cfg = parse("M = matrix(2, 3, 1, 9)\nline:\n    int x: M[3][1], 9").expect("parse");
    let errs = validate(&cfg);
    assert!(errs.iter().any(|e| e.message.contains("越界")), "{errs:?}");
}

#[test]
fn index_non_array_rejected() {
    // 单行行变量（标量）索引
    let cfg = parse("line:\n    int n: 1, 9\nline:\n    expr x: n[1]").expect("parse");
    let errs = validate(&cfg);
    assert!(errs.iter().any(|e| e.message.contains("不是数组")), "{errs:?}");
}

#[test]
fn index_runtime_oob() {
    // 行数/索引含变量，静态无法判定 -> 生成期报错（错误行号为行块起始行）
    let cfg = parse(
        "a = ints(2, 1, 9)\nline:\n    int k: 3, 3\nline:\n    expr x: a[k]",
    )
    .expect("parse");
    let e = generate(&cfg, Some(0)).expect_err("runtime oob");
    assert!(e.message.contains("越界"), "{e}");
    assert_eq!(e.line, Some(4));
}

#[test]
fn index_into_string_item_rejected() {
    let cfg = parse("line:\n    str s: 3, \"ab\"\nline:\n    expr x: s[1]").expect("parse");
    let errs = validate(&cfg);
    assert!(errs.iter().any(|e| e.message.contains("不是数组")), "{errs:?}");
}

// --------------------------------------------------------------------------- //
// 新功能：重边/自环/树类型/val 移除/点集容量
// --------------------------------------------------------------------------- //

#[test]
fn graph_multi_loop_generate() {
    // multi=1 允许重边（m 超过无重边上限），loop=1 允许自环
    let cfg = parse("g = graph(3, 20, 0, 0, multi=1, loop=1)\n").expect("parse");
    let errs = validate(&cfg);
    assert!(errs.is_empty(), "{errs:?}");
    let lines = generate(&cfg, Some(1)).unwrap();
    assert_eq!(lines.len(), 20, "multi 模式输出 20 条边：{lines:?}");
    // 应能出现自环
    let has_loop = lines.iter().any(|l| {
        let p: Vec<&str> = l.split_whitespace().collect();
        p[0] == p[1]
    });
    assert!(has_loop, "loop=1 应产生自环：{lines:?}");
}

#[test]
fn graph_multi_loop_roundtrip() {
    let text = "g = graph(5, 20, 1, 1, multi=1, loop=1, int(1, 9))\n";
    let cfg = parse(text).expect("parse");
    let out = serialize(&cfg).expect("serialize");
    assert_eq!(out, "g = graph(5, 20, 1, 1, int(1, 9), multi=1, loop=1)");
    let cfg2 = parse(&out).expect("re-parse");
    assert_eq!(cfg, cfg2);
}

#[test]
fn graph_no_multi_limit_enforced() {
    // 无重边时 m 超上限报错（n=3 无向无自环上限 3）
    let cfg = parse("g = graph(3, 4, 0, 0)\n").expect("parse");
    let errs = validate(&cfg);
    assert!(errs.iter().any(|e| e.message.contains("超过上限")), "{errs:?}");
    // 允许自环后上限 6
    let cfg = parse("g = graph(3, 6, 0, 0, loop=1)\n").expect("parse");
    let errs = validate(&cfg);
    assert!(errs.is_empty(), "{errs:?}");
    // 7 条超上限
    let cfg = parse("g = graph(3, 7, 0, 0, loop=1)\n").expect("parse");
    let errs = validate(&cfg);
    assert!(errs.iter().any(|e| e.message.contains("超过上限")), "{errs:?}");
}

#[test]
fn val_removed_rejected() {
    let e = parse("t = tree(5, val=int(1, 9))\n").expect_err("val removed");
    assert!(e.message.contains("已移除"), "{e}");
    let e = parse("g = graph(5, 5, 1, 0, val=int(1, 9))\n").expect_err("val removed");
    assert!(e.message.contains("已移除"), "{e}");
}

#[test]
fn kw_weight_retired() {
    for text in [
        "t = tree(5, w=int(1, 9))\n",
        "r = ring(5, w=int(1, 9))\n",
        "br = base_ring(5, 3, w=int(1, 9))\n",
        "g = graph(5, 5, 1, 0, w=int(1, 9))\n",
    ] {
        let e = parse(text).expect_err("w= retired");
        assert!(e.message.contains("已废弃"), "{text} -> {e}");
    }
    // 位置参数仍可用
    parse("t = tree(5, int(1, 9))\n").expect("positional ok");
}

#[test]
fn kw_prec_retired() {
    let e = parse("a = ints(3, 1, 9, prec=2)\n").expect_err("prec= retired");
    assert!(e.message.contains("已废弃"), "{e}");
    let e = parse("M = matf(3, 3, 0, 1, prec=4)\n").expect_err("prec= retired");
    assert!(e.message.contains("已废弃"), "{e}");
    // 位置参数仍可用
    parse("a = ints(3, 1, 9, 2)\n").expect("positional ok");
    parse("M = matf(3, 3, 0, 1, 4)\n").expect("positional ok");
}

#[test]
fn tree_type_roundtrip() {
    let text = "t = tree(5, type=\"star\", int(1, 9))\nc = tree(6, type=\"chain\")\n";
    let cfg = parse(text).expect("parse");
    let out = serialize(&cfg).expect("serialize");
    assert_eq!(out, "t = tree(5, type=\"star\", int(1, 9))\nc = tree(6, type=\"chain\")");
    let cfg2 = parse(&out).expect("re-parse");
    assert_eq!(cfg, cfg2);
}

#[test]
fn star_min_n() {
    let cfg = parse("t = tree(1, type=\"star\")\n").expect("parse");
    let errs = validate(&cfg);
    assert!(errs.iter().any(|e| e.message.contains(">= 2")), "{errs:?}");
}

#[test]
fn points_capacity() {
    // 点个数超过坐标组合数
    let cfg = parse("ps = points(26, 0, 4, 0, 4)\n").expect("parse");
    let errs = validate(&cfg);
    assert!(errs.iter().any(|e| e.message.contains("超过可用坐标组合数")), "{errs:?}");
    // 25 = 5*5 恰好够
    let cfg = parse("ps = points(25, 0, 4, 0, 4)\n").expect("parse");
    let errs = validate(&cfg);
    assert!(errs.is_empty(), "{errs:?}");
}

#[test]
fn charset_dedup_generate() {
    // 字符集含重复字符：生成时去重
    let cfg = parse("line:\n    str s: 10, \"aaaaab\"\n").expect("parse");
    let errs = validate(&cfg);
    assert!(errs.is_empty(), "{errs:?}");
    let lines = generate(&cfg, Some(0)).unwrap();
    assert!(lines[0].chars().all(|c| c == 'a' || c == 'b'), "{lines:?}");
}