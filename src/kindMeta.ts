import type { Item, VarKind, Weight, ElemType } from "./api";

/** DSL 保留字（命令 + 关键字），不可用作变量名。 */
export const RESERVED_COMMANDS = new Set([
  "ints", "floats", "matrix", "matf", "perm", "binseq", "intervals", "points",
  "tree", "graph", "ring", "base_ring", "repeat",
  "line", "int", "float", "text", "expr", "str",
]);

/** 变量名格式：字母/下划线开头，后跟字母数字下划线。 */
export const NAME_RE = /^[A-Za-z_][A-Za-z0-9_]*$/;

/** 校验变量名，返回错误信息或 null。 */
export function nameError(name: string, taken?: (n: string) => boolean): string | null {
  if (name === "") return null;
  if (!NAME_RE.test(name)) return "变量名须以字母或 _ 开头，仅含字母/数字/_";
  if (RESERVED_COMMANDS.has(name)) return `“${name}”是保留字`;
  if (taken?.(name)) return `变量名重复：${name}`;
  return null;
}

/** 顶层类型（顺序即“添加变量”下拉顺序）。 */
export const KIND_ORDER: { kind: VarKind; label: string }[] = [
  {
    kind: {
      Repeat: {
        count: "3",
        items: [
          {
            name: "",
            kind: { Line: { rows: "1", items: [{ name: "n", kind: { Int: { min: "1", max: "100" } } }] } },
            line: 0,
          },
        ],
      },
    },
    label: "repeat 块（整体重复）",
  },
  {
    kind: {
      Line: {
        rows: "1",
        items: [
          { name: "n", kind: { Int: { min: "1", max: "100" } } },
          { name: "m", kind: { Int: { min: "1", max: "100" } } },
        ],
      },
    },
    label: "行（一行多个数）",
  },
  {
    kind: {
      Array: {
        elem_type: "Int",
        el_min: "1",
        el_max: "100",
        prec: "6",
        rows: "1",
        cols: "10",
      },
    },
    label: "数组 / 矩阵",
  },
  { kind: { Perm: { n: "10" } }, label: "排列" },
  { kind: { Binseq: { n: "10", k: "5" } }, label: "0/1 序列" },
  { kind: { Intervals: { n: "5", lo: "1", hi: "10" } }, label: "区间" },
  { kind: { Points: { n: "5", xlo: "0", xhi: "9", ylo: "0", yhi: "9" } }, label: "点集" },
  { kind: { Tree: { n: "10", ttype: "Random", w: null } }, label: "树" },
  { kind: { Tree: { n: "10", ttype: "Star", w: null } }, label: "树（菊花图）" },
  { kind: { Tree: { n: "10", ttype: "Chain", w: null } }, label: "树（链）" },
  { kind: { Tree: { n: "10", ttype: "Parent", w: null } }, label: "树（父节点序列）" },
  { kind: { Graph: { gtype: "General", n: "10", m: "15", directed: true, connected: false, multi: false, loop_: false, k: null, w: null } }, label: "图" },
  { kind: { Graph: { gtype: "Dag", n: "10", m: "15", directed: true, connected: false, multi: false, loop_: false, k: null, w: null } }, label: "图（DAG）" },
  { kind: { Graph: { gtype: "Bipartite", n: "10", m: "15", directed: false, connected: false, multi: false, loop_: false, k: null, w: null } }, label: "图（二分）" },
  { kind: { Graph: { gtype: "Ring", n: "5", m: "5", directed: false, connected: true, multi: false, loop_: false, k: null, w: null } }, label: "环" },
  { kind: { Graph: { gtype: "BaseRing", n: "8", m: "8", directed: false, connected: true, multi: false, loop_: false, k: "3", w: null } }, label: "基环树" },
];

/** 类型徽标配色（淡底深字 pill）。 */
const KIND_COLORS: Record<string, { bg: string; fg: string }> = {
  Repeat: { bg: "#FEF3C7", fg: "#B45309" },
  Line: { bg: "#EDE9FE", fg: "#7C3AED" },
  Array: { bg: "#DBEAFE", fg: "#2563EB" },
  Perm: { bg: "#CFFAFE", fg: "#0891B2" },
  Binseq: { bg: "#CFFAFE", fg: "#0891B2" },
  Intervals: { bg: "#FCE7F3", fg: "#DB2777" },
  Points: { bg: "#FCE7F3", fg: "#DB2777" },
  Tree: { bg: "#D1FAE5", fg: "#059669" },
  Graph: { bg: "#FFEDD5", fg: "#EA580C" },
};

/** 返回类型徽标配色（未知类型回退灰）。 */
export function kindColor(kind: VarKind): { bg: string; fg: string } {
  return KIND_COLORS[Object.keys(kind)[0]] ?? { bg: "#F3F4F6", fg: "#6B7280" };
}

export function kindLabel(kind: VarKind): string {
  const key = Object.keys(kind)[0];
  const k = kind as Record<string, unknown>;
  switch (key) {
    case "Repeat": return "repeat";
    case "Line": return "行";
    case "Array": return (k.Array as { elem_type: ElemType }).elem_type === "Int" ? "整数数组" : "浮点数组";
    case "Perm": return "排列";
    case "Binseq": return "0/1 序列";
    case "Intervals": return "区间";
    case "Points": return "点集";
    case "Tree": {
      const t = k.Tree as { ttype: string };
      switch (t.ttype) {
        case "Star": return "树（菊花图）";
        case "Chain": return "树（链）";
        case "Parent": return "树（父节点序列）";
        default: return "树";
      }
    }
    case "Graph": {
      const g = k.Graph as { gtype: string };
      switch (g.gtype) {
        case "Dag": return "图（DAG）";
        case "Bipartite": return "图（二分）";
        case "Ring": return "环";
        case "BaseRing": return "基环树";
        default: return "图";
      }
    }
    default: return key;
  }
}

/** 简单文本字段定义。 */
interface F { key: string; label: string; ph?: string }

export function kindFields(kind: VarKind): F[] {
  const k = kind as Record<string, unknown>;
  switch (Object.keys(kind)[0]) {
    case "Array": {
      const v = k.Array as { rows: string; cols: string; el_min: string; el_max: string; prec: string };
      return [
        { key: "rows", label: "行数", ph: v.rows },
        { key: "cols", label: "每行个数", ph: v.cols },
        { key: "el_min", label: "元素最小", ph: v.el_min },
        { key: "el_max", label: "元素最大", ph: v.el_max },
        { key: "prec", label: "精度", ph: v.prec },
      ];
    }
    case "Perm": {
      const v = k.Perm as { n: string };
      return [{ key: "n", label: "长度", ph: v.n }];
    }
    case "Binseq": {
      const v = k.Binseq as { n: string; k: string };
      return [{ key: "n", label: "长度", ph: v.n }, { key: "k", label: "1 的个数", ph: v.k }];
    }
    case "Intervals": {
      const v = k.Intervals as { n: string; lo: string; hi: string };
      return [{ key: "n", label: "个数", ph: v.n }, { key: "lo", label: "下界", ph: v.lo }, { key: "hi", label: "上界", ph: v.hi }];
    }
    case "Points": {
      const v = k.Points as { n: string; xlo: string; xhi: string; ylo: string; yhi: string };
      return [
        { key: "n", label: "个数", ph: v.n },
        { key: "xlo", label: "x 下界", ph: v.xlo },
        { key: "xhi", label: "x 上界", ph: v.xhi },
        { key: "ylo", label: "y 下界", ph: v.ylo },
        { key: "yhi", label: "y 上界", ph: v.yhi },
      ];
    }
    case "Tree": {
      const v = k.Tree as { n: string };
      return [{ key: "n", label: "顶点数", ph: v.n }];
    }
    case "Graph": {
      const v = k.Graph as { n: string; m: string };
      return [{ key: "n", label: "顶点数", ph: v.n }, { key: "m", label: "边数", ph: v.m }];
    }
    default:
      return [];
  }
}

/** 编辑单个文本字段。 */
export function editField(kind: VarKind, key: string, value: string): VarKind {
  const k = kind as Record<string, unknown>;
  const entry = Object.entries(k)[0];
  const [kname, inner] = entry;
  const clone = { ...(inner as Record<string, unknown>), [key]: value };
  return { [kname]: clone } as unknown as VarKind;
}

/** 读取单个文本字段的当前值。 */
export function kindFieldValue(kind: VarKind, key: string): string {
  const k = kind as Record<string, unknown>;
  const inner = Object.values(k)[0] as Record<string, unknown>;
  const v = inner[key];
  return v == null ? "" : String(v);
}

export function setElemType(kind: VarKind, t: ElemType): VarKind {
  const k = kind as Record<string, unknown>;
  const inner = { ...(k.Array as object), elem_type: t };
  return { Array: inner } as unknown as VarKind;
}

export function setGraphFlag(kind: VarKind, key: "directed" | "connected", v: boolean): VarKind {
  const k = kind as Record<string, unknown>;
  const inner = { ...(k.Graph as object), [key]: v };
  return { Graph: inner } as unknown as VarKind;
}

export function setGtype(kind: VarKind, gtype: string): VarKind {
  const k = kind as Record<string, unknown>;
  const inner = { ...(k.Graph as object), gtype };
  return { Graph: inner } as unknown as VarKind;
}

export function setWeight(kind: VarKind, w: Weight | null): VarKind {
  const k = kind as Record<string, unknown>;
  const entry = Object.entries(k)[0];
  const [kname, inner] = entry;
  const clone = { ...(inner as Record<string, unknown>), w };
  return { [kname]: clone } as unknown as VarKind;
}

export function makeItem(kind: VarKind): Item {
  return { name: "", kind, line: 0 };
}

/** 字符集快捷预设（字符串项用）。 */
export const CHARSET_LOWER = "abcdefghijklmnopqrstuvwxyz";
export const CHARSET_UPPER = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
export const CHARSET_DIGITS = "0123456789";

/** 预设组合拼接（固定顺序：小写+大写+数字）。 */
export function presetsToCharset(checks: { lower: boolean; upper: boolean; digits: boolean }): string {
  let s = "";
  if (checks.lower) s += CHARSET_LOWER;
  if (checks.upper) s += CHARSET_UPPER;
  if (checks.digits) s += CHARSET_DIGITS;
  return s;
}

/** 从 charset 反推预设勾选；无法完全匹配时返回 null（走自定义）。 */
export function charsetToPresets(
  charset: string,
): { lower: boolean; upper: boolean; digits: boolean } | null {
  const combos: [string, { lower: boolean; upper: boolean; digits: boolean }][] = [
    [CHARSET_LOWER + CHARSET_UPPER + CHARSET_DIGITS, { lower: true, upper: true, digits: true }],
    [CHARSET_LOWER + CHARSET_UPPER, { lower: true, upper: true, digits: false }],
    [CHARSET_LOWER + CHARSET_DIGITS, { lower: true, upper: false, digits: true }],
    [CHARSET_UPPER + CHARSET_DIGITS, { lower: false, upper: true, digits: true }],
    [CHARSET_LOWER, { lower: true, upper: false, digits: false }],
    [CHARSET_UPPER, { lower: false, upper: true, digits: false }],
    [CHARSET_DIGITS, { lower: false, upper: false, digits: true }],
    ["", { lower: false, upper: false, digits: false }],
  ];
  for (const [s, c] of combos) {
    if (charset === s) return c;
  }
  return null;
}
