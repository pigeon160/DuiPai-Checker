import { invoke } from "@tauri-apps/api/core";

/** 与 duipai-core 的 DslError 对应。line 为 1 起行号，可为 null。 */
export interface DslError {
  line: number | null;
  message: string;
}

export type ElemType = "Int" | "Float";

/** 多赋值中的单个项（一行多个数，每项 name = expr）。 */
export interface MultiPart {
  name: string;
  /** 标量表达式原文 */
  expr: string;
}

export type VarKind =
  | { Int: { min: string; max: string } }
  | { Multi: { parts: MultiPart[] } }
  | { Scalar: { expr: string } }
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

// --------------------------------------------------------------------------- //
// Phase 3：生成与对拍
// --------------------------------------------------------------------------- //

/** 生成数据预览。seed 为 null 时随机。 */
export function generateData(config: Config, seed: number | null): Promise<string> {
  return invoke<string>("generate_data", { config, seed });
}

/** 导出文本到指定路径。 */
export function saveTextFile(path: string, content: string): Promise<void> {
  return invoke<void>("save_text_file", { path, content });
}

/** 读取文本文件（源码预览）。 */
export function readTextFile(path: string): Promise<string> {
  return invoke<string>("read_text_file", { path });
}

/** 编译 C++ 源码。 */
export function compileProgram(
  source: string,
  workdir: string,
  name: string,
  compiler: string,
  flags: string,
): Promise<string> {
  return invoke<string>("compile_program", { source, workdir, name, compiler, flags });
}

export type RunStatus = "Ok" | "Timeout" | "Memory" | "Error";

export interface RunResult {
  status: RunStatus;
  returncode: number | null;
  stdout: number[];
  stderr: number[];
  error: string;
  elapsed: number;
  peak_bytes: number;
}

export function runProgramIpc(
  cmd: string,
  dir: string,
  input: string,
  timeout: number,
  memoryLimitMb: number | null = null,
): Promise<RunResult> {
  return invoke<RunResult>("run_program_ipc", {
    cmd,
    dir,
    input,
    timeout,
    memoryLimitMb,
  });
}

export type ProgMode = "RunCmd" | "CppSource";

export interface ProgramSpec {
  mode: ProgMode;
  cmd: string;
  dir: string;
  label: string;
}

export type GenMode = "Builtin" | "External";

export interface CheckParams {
  sol: ProgramSpec;
  brute: ProgramSpec;
  gen_mode: GenMode;
  ext: ProgramSpec | null;
  total: number;
  timeout: number;
  memory_limit_mb: number | null;
  seed: number | null;
  ignore_ws: boolean;
  compiler: string;
  compile_flags: string;
  config: Config;
}

export interface CheckStats {
  pass: number;
  wa: number;
  tle: number;
  re: number;
  mle: number;
  error: number;
  tested: number;
}

export type CheckEvent =
  | { kind: "log"; msg: string }
  | { kind: "status"; tested: number; total: number }
  | {
      kind: "finish";
      stats: CheckStats;
      tested: number;
      reason: string;
      fail_dir: string | null;
    };

export function duipaiStart(params: CheckParams): Promise<void> {
  return invoke<void>("duipai_start", { params });
}

export function duipaiCancel(): Promise<void> {
  return invoke<void>("duipai_cancel");
}

export function duipaiRunning(): Promise<boolean> {
  return invoke<boolean>("duipai_running");
}
