import { useCallback, useEffect, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import {
  duipaiCancel,
  duipaiStart,
  readTextFile,
  type CheckEvent,
  type CheckParams,
  type CheckStats,
  type Config,
  type GenMode,
  type ProgramSpec,
  type ProgMode,
} from "../api";

interface Props {
  config: Config;
  genMode: GenMode;
  ext: ProgramSpec;
  onGenMode: (m: GenMode) => void;
  onExt: (s: ProgramSpec) => void;
}

const EMPTY_STATS: CheckStats = { pass: 0, wa: 0, tle: 0, re: 0, mle: 0, error: 0, tested: 0 };

function ProgramRow({
  label,
  spec,
  onChange,
  onPreview,
  onImport,
}: {
  label: string;
  spec: ProgramSpec;
  onChange: (s: ProgramSpec) => void;
  onPreview: (path: string) => void;
  onImport: (label: string) => void;
}) {
  return (
    <div className="prog-row">
      <span className="prog-label">{label}</span>
      <select
        value={spec.mode}
        onChange={(e) => onChange({ ...spec, mode: e.target.value as ProgMode })}
        title="C++ 源码模式会自动编译"
      >
        <option value="RunCmd">运行命令</option>
        <option value="CppSource">C++ 源码</option>
      </select>
      <input
        className="prog-cmd"
        value={spec.cmd}
        onChange={(e) => onChange({ ...spec, cmd: e.target.value })}
        placeholder={
          spec.mode === "CppSource"
            ? spec.cmd.trim() === ""
              ? "点右侧“导入…”选择 .cpp 文件"
              : "C:\\path\\sol.cpp"
            : "例如 python3 sol.py"
        }
        spellCheck={false}
      />
      {spec.mode === "CppSource" && (
        <>
          <button onClick={() => onImport(label)} title="选择 .cpp 文件并填入路径">
            导入…
          </button>
          {spec.cmd.trim() !== "" && (
            <button onClick={() => onPreview(spec.cmd)} title="读取源码内容预览">
              预览
            </button>
          )}
        </>
      )}
    </div>
  );
}

export default function CheckPanel({ config, genMode, ext, onGenMode, onExt }: Props) {
  const [sol, setSol] = useState<ProgramSpec>({ mode: "RunCmd", cmd: "", dir: "", label: "正解" });
  const [brute, setBrute] = useState<ProgramSpec>({ mode: "RunCmd", cmd: "", dir: "", label: "暴力" });
  const [total, setTotal] = useState("100");
  const [timeoutS, setTimeoutS] = useState("5");
  const [memLimit, setMemLimit] = useState("");
  const [seed, setSeed] = useState("");
  const [ignoreWs, setIgnoreWs] = useState(false);
  const [compiler, setCompiler] = useState("g++");
  const [compileFlags, setCompileFlags] = useState("-O2 -std=c++17");
  const [running, setRunning] = useState(false);
  const [logs, setLogs] = useState<string[]>([]);
  const [stats, setStats] = useState<CheckStats>(EMPTY_STATS);
  const [error, setError] = useState<string>("");
  const [preview, setPreview] = useState<{ path: string; content: string } | null>(null);
  const logBoxRef = useRef<HTMLDivElement>(null);
  const configRef = useRef(config);
  configRef.current = config;
  const solRef = useRef(sol);
  solRef.current = sol;
  const bruteRef = useRef(brute);
  bruteRef.current = brute;
  const extRef = useRef(ext);
  extRef.current = ext;
  const genModeRef = useRef(genMode);
  genModeRef.current = genMode;

  useEffect(() => {
    let unlisten: UnlistenFn | null = null;
    listen<CheckEvent>("check://event", (ev) => {
      const e = ev.payload;
      if (e.kind === "log") {
        setLogs((prev) => [...prev.slice(-1999), e.msg]);
      } else if (e.kind === "status") {
        setStats((s) => ({ ...s, tested: e.tested }));
      } else if (e.kind === "finish") {
        setRunning(false);
        setStats(e.stats);
        const errs = e.stats.re + e.stats.error;
        setLogs((prev) => [
          ...prev.slice(-1999),
          `对拍结束（${e.reason}）：共测试 ${e.tested} 组`,
          `  通过：${e.stats.pass}    不一致(WA)：${e.stats.wa}    超时(TLE)：${e.stats.tle}    内存超限(MLE)：${e.stats.mle}    运行错误(RE/Error)：${errs}`,
          ...(e.fail_dir ? [`现场已保存：${e.fail_dir}`] : []),
        ]);
      }
    }).then((u) => {
      unlisten = u;
    });
    return () => {
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    if (logBoxRef.current) {
      logBoxRef.current.scrollTop = logBoxRef.current.scrollHeight;
    }
  }, [logs]);

  const onPreview = useCallback(async (path: string) => {
    try {
      const content = await readTextFile(path);
      setPreview({ path, content });
    } catch (e) {
      setError(`读取源码失败：${String(e)}`);
    }
  }, []);

  const onImport = useCallback(async (label: string) => {
    const path = await open({
      title: `选择 ${label} 源码`,
      filters: [{ name: "C++ 源码", extensions: ["cpp", "cc", "cxx", "c"] }],
    });
    if (!path || typeof path !== "string") return;
    if (label === "正解") setSol((s) => ({ ...s, mode: "CppSource", cmd: path }));
    else if (label === "暴力") setBrute((s) => ({ ...s, mode: "CppSource", cmd: path }));
    else onExt({ ...extRef.current, mode: "CppSource", cmd: path });
  }, [onExt]);

  const onStart = useCallback(async () => {
    setError("");
    if (!sol.cmd.trim() || !brute.cmd.trim()) {
      setError("请填写正解与暴力的命令或源码路径");
      return;
    }
    if (genModeRef.current === "External" && !extRef.current.cmd.trim()) {
      setError("请填写外置生成器的命令或源码路径");
      return;
    }
    let totalN: number;
    try {
      totalN = Number(total.trim());
      if (!Number.isInteger(totalN)) throw new Error();
    } catch {
      setError("组数必须是整数（-1 表示无限）");
      return;
    }
    const t = Number(timeoutS.trim());
    if (!(t > 0)) {
      setError("超时秒数必须大于 0");
      return;
    }
    let memN: number | null = null;
    if (memLimit.trim() !== "") {
      memN = Number(memLimit.trim());
      if (!Number.isInteger(memN) || memN <= 0) {
        setError("内存限制必须是正整数（MB），或留空表示不限");
        return;
      }
    }
    let seedN: number | null = null;
    if (seed.trim() !== "") {
      seedN = Number(seed.trim());
      if (!Number.isInteger(seedN)) {
        setError("种子必须是整数或留空");
        return;
      }
    }
    const params: CheckParams = {
      sol: { ...solRef.current, label: "正解" },
      brute: { ...bruteRef.current, label: "暴力" },
      gen_mode: genModeRef.current,
      ext: genModeRef.current === "External" ? { ...extRef.current, label: "外置生成器" } : null,
      total: totalN,
      timeout: t,
      memory_limit_mb: memN,
      seed: seedN,
      ignore_ws: ignoreWs,
      compiler: compiler.trim() || "g++",
      compile_flags: compileFlags.trim() || "-O2 -std=c++17",
      config: configRef.current,
    };
    setLogs([]);
    setStats(EMPTY_STATS);
    try {
      await duipaiStart(params);
      setRunning(true);
    } catch (e) {
      setError(String(e));
    }
  }, [sol, brute, total, timeoutS, memLimit, seed, ignoreWs, compiler, compileFlags]);

  const onStop = useCallback(async () => {
    await duipaiCancel();
    setLogs((prev) => [...prev, "收到停止请求，正在停止……"]);
  }, []);

  return (
    <div className="check-panel">
      <div className="prog-group">
        <ProgramRow label="正解" spec={sol} onChange={setSol} onPreview={onPreview} onImport={onImport} />
        <ProgramRow label="暴力" spec={brute} onChange={setBrute} onPreview={onPreview} onImport={onImport} />
        <div className="prog-row">
          <span className="prog-label">数据</span>
          <select
            value={genMode}
            onChange={(e) => onGenMode(e.target.value as GenMode)}
            title="内置生成器使用 DSL 配置；外置生成器的标准输出即测试数据"
          >
            <option value="Builtin">内置生成器（DSL）</option>
            <option value="External">外置生成器</option>
          </select>
          {genMode === "External" && (
            <ProgramRow label="生成" spec={ext} onChange={onExt} onPreview={onPreview} onImport={onImport} />
          )}
        </div>
      </div>
      <div className="param-row">
        <label>组数
          <input className="num-input" value={total} onChange={(e) => setTotal(e.target.value)} title="-1 表示无限" />
        </label>
        <label>超时(s)
          <input className="num-input" value={timeoutS} onChange={(e) => setTimeoutS(e.target.value)} />
        </label>
        <label>内存(MB)
          <input className="num-input" value={memLimit} onChange={(e) => setMemLimit(e.target.value)} placeholder="不限" title="留空表示不限制内存" />
        </label>
        <label>种子
          <input className="num-input" value={seed} onChange={(e) => setSeed(e.target.value)} placeholder="留空随机" />
        </label>
        <label className="ws-label">
          <input type="checkbox" checked={ignoreWs} onChange={(e) => setIgnoreWs(e.target.checked)} />
          忽略行末空格
        </label>
        <label>编译器
          <input className="num-input" value={compiler} onChange={(e) => setCompiler(e.target.value)} />
        </label>
        <label>编译参数
          <input className="flags-input" value={compileFlags} onChange={(e) => setCompileFlags(e.target.value)} />
        </label>
      </div>
      <div className="param-row">
        {!running ? (
          <button className="start-btn" onClick={onStart}>开始对拍</button>
        ) : (
          <button className="stop-btn" onClick={onStop}>停止</button>
        )}
        <span className="stats-text">
          已测试 {stats.tested}
          {total !== "-1" && `/${total}`} ｜ 通过 {stats.pass} ｜ WA {stats.wa} ｜ TLE {stats.tle} ｜ MLE {stats.mle} ｜ RE {stats.re} ｜ 错误 {stats.error}
        </span>
        {error && <span className="param-error">{error}</span>}
      </div>
      <div className="log-box" ref={logBoxRef}>
        {logs.map((l, i) => (
          <div key={i} className="log-line">{l}</div>
        ))}
      </div>

      {preview && (
        <div className="modal-mask" onClick={() => setPreview(null)}>
          <div className="modal" onClick={(e) => e.stopPropagation()}>
            <div className="modal-head">
              <span>{preview.path}</span>
              <button onClick={() => setPreview(null)}>关闭</button>
            </div>
            <pre className="modal-body">{preview.content}</pre>
          </div>
        </div>
      )}
    </div>
  );
}
