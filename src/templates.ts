/** 常用题型预设模板（载入 DSL 编辑器）。 */

export interface DslTemplate {
  name: string;
  dsl: string;
}

export const DSL_TEMPLATES: DslTemplate[] = [
  {
    name: "经典多测（T 组）",
    dsl: `line:
    int t: 1, 10
repeat (t):
    line:
        int n: 1, 100
        int m: 1, 100
    line (n):
        int a: 1, 1000000000
        int b: 1, 1000000000`,
  },
  {
    name: "树 + 边权",
    dsl: `line:
    int n: 1, 100
t = tree(n, int(1, 100))`,
  },
  {
    name: "菊花图树",
    dsl: `line:
    int n: 2, 100
t = tree(n, type="star", int(1, 100))`,
  },
  {
    name: "图（无向连通 + 边权）",
    dsl: `line:
    int n: 1, 100
    int m: 1, 500
g = graph(n, m, 0, 1, int(1, 100))`,
  },
  {
    name: "DAG 图",
    dsl: `line:
    int n: 1, 100
    int m: 1, 500
g = graph(n, m, 1, 0, type="dag")`,
  },
  {
    name: "整数矩阵",
    dsl: `line:
    int n: 1, 100
    int m: 1, 100
M = matrix(n, m, 0, 1)`,
  },
  {
    name: "排列 + 数组",
    dsl: `line:
    int n: 1, 100
p = perm(n)
a = ints(n, 1, 100)`,
  },
  {
    name: "0/1 序列",
    dsl: `line:
    int n: 1, 100
z = binseq(n, 10)`,
  },
  {
    name: "字符串序列",
    dsl: `line:
    int n: 1, 100
line (n):
    str s: int(1, 10), "abcXYZ012"`,
  },
  {
    name: "区间",
    dsl: `line:
    int n: 1, 100
iv = intervals(n, 1, 1000000000)`,
  },
  {
    name: "点集",
    dsl: `line:
    int n: 1, 100
pt = points(n, 0, 1000000000, 0, 1000000000)`,
  },
  {
    name: "多测 + 矩阵",
    dsl: `line:
    int t: 1, 10
repeat (t):
    line:
        int n: 1, 50
        int m: 1, 50
    M = matrix(n, m, 0, 1)`,
  },
];
