//! 验证 src/templates.ts 中所有内置模板的 DSL 合法性（parse + validate + generate）。

use duipai_core::{generate, parse, validate};

const TEMPLATES: &[(&str, &str)] = &[
    (
        "经典多测（T 组）",
        "line:\n    int t: 1, 10\nrepeat (t):\n    line:\n        int n: 1, 100\n        int m: 1, 100\n    line (n):\n        int a: 1, 1000000000\n        int b: 1, 1000000000",
    ),
    ("树 + 边权", "line:\n    int n: 1, 100\nt = tree(n, int(1, 100))"),
    ("菊花图树", "line:\n    int n: 2, 100\nt = tree(n, type=\"star\", int(1, 100))"),
    (
        "图（无向连通 + 边权）",
        "line:\n    int n: 1, 100\n    int m: 1, 500\ng = graph(n, m, 0, 1, int(1, 100))",
    ),
    ("DAG 图", "line:\n    int n: 1, 100\n    int m: 1, 500\ng = graph(n, m, 1, 0, type=\"dag\")"),
    ("整数矩阵", "line:\n    int n: 1, 100\n    int m: 1, 100\nM = matrix(n, m, 0, 1)"),
    ("排列 + 数组", "line:\n    int n: 1, 100\np = perm(n)\na = ints(n, 1, 100)"),
    ("0/1 序列", "line:\n    int n: 1, 100\nz = binseq(n, 10)"),
    (
        "字符串序列",
        "line:\n    int n: 1, 100\nline (n):\n    str s: int(1, 10), \"abcXYZ012\"",
    ),
    ("区间", "line:\n    int n: 1, 100\niv = intervals(n, 1, 1000000000)"),
    (
        "点集",
        "line:\n    int n: 1, 100\npt = points(n, 0, 1000000000, 0, 1000000000)",
    ),
    (
        "多测 + 矩阵",
        "line:\n    int t: 1, 10\nrepeat (t):\n    line:\n        int n: 1, 50\n        int m: 1, 50\n    M = matrix(n, m, 0, 1)",
    ),
];

#[test]
fn all_builtin_templates_valid() {
    for (name, dsl) in TEMPLATES {
        let cfg = match parse(dsl) {
            Ok(cfg) => cfg,
            Err(e) => panic!("模板「{name}」解析失败: {} ({})", e.message, dsl),
        };
        let errs = validate(&cfg);
        assert!(
            errs.is_empty(),
            "模板「{name}」校验错误: {:?}",
            errs.iter().map(|e| e.message.clone()).collect::<Vec<_>>()
        );
        // 生成一次（固定种子），确保运行期也合法
        match generate(&cfg, Some(1)) {
            Ok(lines) => assert!(!lines.is_empty(), "模板「{name}」生成结果为空"),
            Err(e) => panic!("模板「{name}」生成失败: {} ({})", e.message, dsl),
        }
    }
}
