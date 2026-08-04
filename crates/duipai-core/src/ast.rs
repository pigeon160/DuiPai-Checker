//! DSL 中间表示（IR）——图形化界面与 DSL 文本之间的唯一数据模型。
//!
//! 所有数值字段保存**表达式字符串**（如 `2*n`、`int(1,100)`），与 legacy Python
//! 实现同构：复杂表达式天然无损往返，GUI 无法精细编辑时按只读表达式展示。
//! 求值在 [`crate::expr`] 中进行（引用环境为前面已生成变量的值）。

use serde::{Deserialize, Serialize};

/// 多测模式：顶部注释 `# 多测模式：重复 N 次`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RepeatMode {
    pub enabled: bool,
    /// 重复次数表达式文本（通常为数字字符串）。
    pub count: String,
}

/// 一份完整配置。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Config {
    pub repeat: Option<RepeatMode>,
    /// 按定义顺序排列的语句列表（顺序即生成顺序）。
    pub items: Vec<Item>,
}

/// 顶层语句（行块或数组/树/图等命令）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub name: String,
    pub kind: VarKind,
    /// 该语句起始行号（1 起；供错误定位与前端高亮）。
    pub line: usize,
}

/// 元素类型（数组 / 权值）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ElemType {
    /// 整数
    Int,
    /// 浮点数
    Float,
}

impl ElemType {
    pub fn is_float(self) -> bool {
        matches!(self, ElemType::Float)
    }
}

/// 树结构类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TreeType {
    /// 随机树（默认）
    Random,
    /// 菊花图（star）：中心连所有点
    Star,
    /// 链（chain）：随机排列顶点连成链
    Chain,
}

/// 图结构类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GraphType {
    /// 一般图（graph 命令，有向/无向 + 可选连通）
    General,
    /// 有向无环图
    Dag,
    /// 二分图
    Bipartite,
    /// 环（ring 命令，n 条边首尾相连）
    Ring,
    /// 基环树（base_ring 命令）
    BaseRing,
}

/// 边权 / 节点权值描述。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Weight {
    pub kind: ElemType,
    pub min: String,
    pub max: String,
    pub prec: String,
}

/// 行块内的单个输出项（行内项，等级低于行）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LineItem {
    pub name: String,
    pub kind: LineItemKind,
}

/// 行内项类型（只能输出单个值）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LineItemKind {
    /// 整数：`整数 n: 1, 100`
    Int { min: String, max: String },
    /// 浮点：`浮点 x: 0, 1, 4`
    Float { min: String, max: String, prec: String },
    /// 表达式：`表达式 e: 2 * n`（任意标量表达式）
    Scalar { expr: String },
    /// 文本：`文本 s: "---"`（固定字面量，不可引用）
    Text { text: String },
    /// 字符串：`字符串 c: 10, "ab"`（长度可为表达式，不可引用）
    Str { len: String, charset: String },
}

/// 顶层变量类型（行块或命令）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum VarKind {
    /// 行块：`行 (N):` + 缩进子项；重复时数值项数组化（引用须 n[k]）
    Line { rows: String, items: Vec<LineItem> },
    /// 数组/矩阵：`ints/floats(cols, el_min, el_max[, prec])` 或
    /// `matrix/matf(rows, cols, el_min, el_max[, prec])`
    Array {
        elem_type: ElemType,
        el_min: String,
        el_max: String,
        prec: String,
        rows: String,
        cols: String,
    },
    /// 排列：`p = perm(n)`
    Perm { n: String },
    /// 0/1 序列：`b = binseq(n, k)`，一行 n 位，其中 k 个 1
    Binseq { n: String, k: String },
    /// 区间：`iv = intervals(n, lo, hi)`，n 行 `l r`
    Intervals { n: String, lo: String, hi: String },
    /// 点集：`ps = points(n, xlo, xhi, ylo, yhi)`，n 行 `x y`
    Points {
        n: String,
        xlo: String,
        xhi: String,
        ylo: String,
        yhi: String,
    },
    /// 树：`t = tree(n[, type=...][, w=...])`（type: star 菊花图 / chain 链）
    Tree {
        n: String,
        ttype: TreeType,
        w: Option<Weight>,
    },
    /// 图：`g = graph(n, m, directed, connected[, multi=1][, loop=1][, type=...][, w=...])`
    Graph {
        gtype: GraphType,
        n: String,
        m: String,
        directed: bool,
        connected: bool,
        /// 允许重边（multi=1，m 无上限）
        multi: bool,
        /// 允许自环（loop=1，u 可等于 v）
        loop_: bool,
        /// base_ring 的环大小 k
        k: Option<String>,
        w: Option<Weight>,
    },
}

impl Config {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}
