import type { Item, VarKind, Weight, ElemType } from "./api";

export type { VarKind };

/** 类型中文名（顺序即“添加变量”下拉顺序）。 */
export const KIND_ORDER: { kind: VarKind; label: string }[] = [
  { kind: { Int: { min: "1", max: "100" } }, label: "整数变量" },
  { kind: { Float: { min: "0", max: "1", prec: "6" } }, label: "浮点变量" },
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
  { kind: { String: { rows: "1", cols: "10", charset: "abcdefghijklmnopqrstuvwxyz" } }, label: "字符串" },
  { kind: { Binseq: { n: "10", k: "5" } }, label: "0/1 序列" },
  { kind: { Intervals: { n: "5", lo: "1", hi: "10" } }, label: "区间" },
  { kind: { Points: { n: "5", xlo: "0", xhi: "9", ylo: "0", yhi: "9" } }, label: "点集" },
  { kind: { Tree: { n: "10", w: null, val: null } }, label: "树" },
  { kind: { Graph: { gtype: "General", n: "10", m: "15", directed: true, connected: false, k: null, w: null, val: null } }, label: "图" },
  { kind: { Graph: { gtype: "Dag", n: "10", m: "15", directed: true, connected: false, k: null, w: null, val: null } }, label: "图（DAG）" },
  { kind: { Graph: { gtype: "Bipartite", n: "10", m: "15", directed: false, connected: false, k: null, w: null, val: null } }, label: "图（二分）" },
  { kind: { Graph: { gtype: "Ring", n: "5", m: "5", directed: false, connected: true, k: null, w: null, val: null } }, label: "环" },
  { kind: { Graph: { gtype: "BaseRing", n: "8", m: "8", directed: false, connected: true, k: "3", w: null, val: null } }, label: "基环树" },
];

export function kindLabel(kind: VarKind): string {
  const key = Object.keys(kind)[0];
  const k = kind as Record<string, unknown>;
  switch (key) {
    case "Int": return "整数变量";
    case "Float": return "浮点变量";
    case "Array": return (k.Array as { elem_type: ElemType }).elem_type === "Int" ? "整数数组" : "浮点数组";
    case "Perm": return "排列";
    case "String": return "字符串";
    case "Binseq": return "0/1 序列";
    case "Intervals": return "区间";
    case "Points": return "点集";
    case "Tree": return "树";
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

function textFields(kind: VarKind): F[] {
  const k = kind as Record<string, unknown>;
  switch (Object.keys(kind)[0]) {
    case "Int": {
      const v = k.Int as { min: string; max: string };
      return [{ key: "min", label: "最小", ph: v.min }, { key: "max", label: "最大", ph: v.max }];
    }
    case "Float": {
      const v = k.Float as { min: string; max: string; prec: string };
      return [{ key: "min", label: "最小", ph: v.min }, { key: "max", label: "最大", ph: v.max }, { key: "prec", label: "精度", ph: v.prec }];
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
    default: return [];
  }
}

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
    case "String": {
      const v = k.String as { rows: string; cols: string; charset: string };
      return [
        { key: "rows", label: "行数", ph: v.rows },
        { key: "cols", label: "长度", ph: v.cols },
        { key: "charset", label: "字符集", ph: v.charset },
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
      return textFields(kind);
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

/** 权值描述 → DSL 片段（与后端 Weight 结构一致）。 */
export function setWeight(kind: VarKind, which: "w" | "val", w: Weight | null): VarKind {
  const k = kind as Record<string, unknown>;
  const entry = Object.entries(k)[0];
  const [kname, inner] = entry;
  const clone = { ...(inner as Record<string, unknown>), [which]: w };
  return { [kname]: clone } as unknown as VarKind;
}

export function makeItem(name: string, kind: VarKind): Item {
  return { name, kind, line: 0 };
}
