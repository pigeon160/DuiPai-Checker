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
    /// 按定义顺序排列的变量列表（顺序即生成顺序）。
    pub items: Vec<Item>,
}

/// 单条变量定义。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub name: String,
    pub kind: VarKind,
    /// 该语句在 DSL 文本中的行号（1 起；供错误定位与前端高亮）。
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

/// 变量类型与参数（字段均为表达式字符串，bool 除外）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum VarKind {
    /// 整数变量：`n = int(min, max)`
    Int { min: String, max: String },
    /// 浮点变量：`x = float(min, max[, prec])`
    Float { min: String, max: String, prec: String },
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
    /// 字符串（单行用 `str(len[, "charset"])`，多行用 `strs(rows, len[, "charset"])`）
    String {
        rows: String,
        cols: String,
        charset: String,
    },
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
    /// 树：`t = tree(n[, w=...][, val=...])`
    Tree {
        n: String,
        w: Option<Weight>,
        val: Option<Weight>,
    },
    /// 图：`g = graph(n, m, directed, connected[, type=...][, w=...][, val=...])`
    Graph {
        gtype: GraphType,
        n: String,
        m: String,
        directed: bool,
        connected: bool,
        /// base_ring 的环大小 k
        k: Option<String>,
        w: Option<Weight>,
        val: Option<Weight>,
    },
}

impl Config {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}
