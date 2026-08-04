import { invoke } from "@tauri-apps/api/core";

/** 与 duipai-core 的 DslError 对应。line 为 1 起行号，可为 null。 */
export interface DslError {
  line: number | null;
  message: string;
}

export type ElemType = "Int" | "Float";

export type VarKind =
  | { Int: { min: string; max: string } }
  | { Float: { min: string; max: string; prec: string } }
  | {
      Array: {
        elem_type: ElemType;
        el_min: string;
        el_max: string;
        prec: string;
        rows: string;
        cols: string;
      };
    }
  | { Perm: { n: string } }
  | { String: { rows: string; cols: string; charset: string } }
  | { Binseq: { n: string; k: string } }
  | { Intervals: { n: string; lo: string; hi: string } }
  | {
      Points: {
        n: string;
        xlo: string;
        xhi: string;
        ylo: string;
        yhi: string;
      };
    }
  | { Tree: { n: string; w: Weight | null; val: Weight | null } }
  | {
      Graph: {
        gtype: "General" | "Dag" | "Bipartite" | "Ring" | "BaseRing";
        n: string;
        m: string;
        directed: boolean;
        connected: boolean;
        k: string | null;
        w: Weight | null;
        val: Weight | null;
      };
    };

export interface Weight {
  kind: ElemType;
  min: string;
  max: string;
  prec: string;
}

export interface Item {
  name: string;
  kind: VarKind;
  /** 该语句在 DSL 文本中的行号（1 起）。 */
  line: number;
}

export interface RepeatMode {
  enabled: boolean;
  count: string;
}

export interface Config {
  repeat: RepeatMode | null;
  items: Item[];
}

export function ping(): Promise<string> {
  return invoke<string>("ping");
}

export function dslParse(text: string): Promise<Config> {
  return invoke<Config>("dsl_parse", { text });
}

/** 解析 + 静态校验：语法错误走 Err，校验错误走 errors 列表（不阻断加载）。 */
export interface ParseChecked {
  config: Config;
  errors: DslError[];
}

export function dslParseChecked(text: string): Promise<ParseChecked> {
  return invoke<ParseChecked>("dsl_parse_checked", { text });
}

export function dslSerialize(config: Config): Promise<string> {
  return invoke<string>("dsl_serialize", { config });
}

export function exprEval(
  expr: string,
  env: Record<string, number>,
): Promise<number> {
  return invoke<number>("expr_eval", { expr, env });
}
