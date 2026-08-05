//! 自然语言 → DSL 示例集：占位符例子 + 典型题型描述。
//! 每个示例断言输出包含关键 DSL 片段（parse+validate+generate 全链路合法）。

use duipai_core::{generate, nl_to_dsl, parse, validate};

struct Ex {
    text: &'static str,
    /** 必须包含的关键片段。 */
    must: &'static [&'static str],
}

const EXS: &[Ex] = &[
    Ex {
        text: "多测，T 组。第一行两个整数 n m，接下来 n 行每行两个整数 a b，然后一棵带边权的树，边权 1 到 10^9",
        must: &["repeat (T):", "int n: 1, 100", "int m: 1, 100", "line (n):\n        int a: 1, 100\n        int b: 1, 100", "t = tree(n, int(1, 1000000000))"],
    },
    Ex {
        text: "第一行一个整数 n (1≤n≤10^5)，接下来 n 行每行一个整数 a",
        must: &["int n: 1, 100000", "line (n):\n    int a: 1, 100"],
    },
    Ex {
        text: "n 个点 m 条边的无向连通图，边权 int(1, 9)",
        must: &["g = graph(n, m, 0, 1, int(1, 9))"],
    },
    Ex {
        text: "一个 n 行 m 列的 01 矩阵",
        must: &["M = matrix(n, m, 0, 1)"],
    },
    Ex {
        text: "第一行一个整数 n，接下来 n 行每行一个字符串，长度不超过 100",
        must: &["str s: int(1, 100)"],
    },
    Ex {
        text: "n 个区间 [l, r]，1<=l<=r<=10^9",
        must: &["iv = intervals(n, 1, 1000000000)"],
    },
    Ex {
        text: "n 个点的树，以 1 为根，输入每个节点的父节点",
        must: &["t = tree(n, type=\"parent\")"],
    },
    Ex {
        text: "第一行一个整数 T，接下来 T 组，每组第一行一个整数 n，然后 n 行每行一个整数",
        must: &["repeat (T):", "int T: 1, 100", "line (n):"],
    },
    Ex {
        text: "第一行两个整数 n m，接下来 n 行每行 m 个整数",
        must: &["M = matrix(n, m, 1, 100)"],
    },
    Ex {
        text: "一个 n 个整数的数组，元素范围 1 到 10^9",
        must: &["a = ints(n, 1, 1000000000)"],
    },
    Ex {
        text: "第一行一个浮点数 x，保留 3 位小数，接下来 n 行每行一个浮点数 y",
        must: &["float x: 1, 100, 3", "float y: 1, 100"],
    },
    Ex {
        text: "n 的排列，后面 m 个查询，每个查询两个整数 l r",
        must: &["p = perm(n)", "line (m):\n    int l: 1, 100\n    int r: 1, 100"],
    },
    Ex {
        text: "多测，T 组。每组一个 n 行 m 列的矩阵，元素 0 到 1",
        must: &["repeat (T):", "M = matrix(n, m, 0, 1)"],
    },
    Ex {
        text: "一棵菊花图（star 树），n 个点，边权 1 到 100",
        must: &["t = tree(n, type=\"star\", int(1, 100))"],
    },
    Ex {
        text: "第一行 n，第二行 n 个数",
        must: &["line:", "line:"],
    },
    Ex {
        text: "first line contains n, then n lines each with one integer a",
        must: &["int n: 1, 100", "line (n):\n    int a: 1, 100"],
    },
    Ex {
        text: "an array of n integers in [1, 10^9]",
        must: &["a = ints(n, 1, 1000000000)"],
    },
    Ex {
        text: "T test cases. each test case: first line contains n and m, then n lines with m integers",
        must: &["repeat (T):", "M = matrix(n, m, 1, 100)"],
    },
    Ex {
        text: "a tree with n nodes and weighted edges in [1, 100]",
        must: &["t = tree(n, int(1, 100))"],
    },
    Ex {
        text: "n intervals [l, r] with 1 <= l <= r <= 10^9",
        must: &["iv = intervals(n, 1, 1000000000)"],
    },
];

#[test]
fn nl_examples_all_valid() {
    for (i, ex) in EXS.iter().enumerate() {
        let r = nl_to_dsl(ex.text);
        assert!(
            !r.dsl.is_empty(),
            "示例 {i} 生成失败: {:?}（text: {}）",
            r.warnings,
            ex.text
        );
        let cfg = match parse(&r.dsl) {
            Ok(c) => c,
            Err(e) => panic!("示例 {i} 解析失败: {}（{}）", e.message, r.dsl),
        };
        let errs = validate(&cfg);
        assert!(
            errs.is_empty(),
            "示例 {i} 校验错误: {:?}（{}）",
            errs.iter().map(|e| e.message.clone()).collect::<Vec<_>>(),
            r.dsl
        );
        // 生成（固定种子，失败换种子重试——连通图等随机约束可能恰好非法）
        let mut gen_ok = false;
        let mut gen_err = String::new();
        for seed in 0..8u64 {
            match generate(&cfg, Some(seed)) {
                Ok(lines) if !lines.is_empty() => {
                    gen_ok = true;
                    break;
                }
                Ok(_) => {}
                Err(e) => gen_err = e.message.clone(),
            }
        }
        assert!(
            gen_ok,
            "示例 {i} 生成失败: {gen_err}（{}）",
            r.dsl
        );
        for must in ex.must {
            assert!(
                r.dsl.contains(must),
                "示例 {i} 缺少片段「{must}」\n  输入: {}\n  输出:\n{}",
                ex.text,
                r.dsl
            );
        }
    }
}
