import { useRef, useState } from "react";
import {
  generateData,
  runProgramIpc,
  saveTextFile,
  type Config,
  type DslError,
  type GenMode,
  type ProgramSpec,
} from "../api";
import { save } from "@tauri-apps/plugin-dialog";

interface Props {
  config: Config;
  genMode: GenMode;
  ext: ProgramSpec | null;
}

export default function GeneratePanel({ config, genMode, ext }: Props) {
  const [seed, setSeed] = useState("");
  const [output, setOutput] = useState("");
  const [error, setError] = useState<DslError | null>(null);
  const [loading, setLoading] = useState(false);
  const lastConfigRef = useRef("");

  const parseSeed = (): number | null => {
    if (seed.trim() === "") return null;
    const s = Number(seed.trim());
    if (!Number.isInteger(s)) throw new Error("种子必须是整数或留空");
    return s;
  };

  const onGenerate = async () => {
    setLoading(true);
    setError(null);
    try {
      const s = parseSeed();
      if (genMode === "External") {
        if (!ext || !ext.cmd.trim()) {
          setError({ line: null, message: "请先在“对拍”面板填写外置生成器" });
          return;
        }
        // 外置生成器：追加 --seed（对齐 legacy 行为），stdout 即测试数据
        const cmd =
          s === null
            ? ext.cmd
            : `${ext.cmd} --seed ${s}`;
        const r = await runProgramIpc(cmd, ext.dir, "", 30, null);
        if (r.status === "Timeout") {
          setError({ line: null, message: "外置生成器超时（>30s）" });
          return;
        }
        if (r.status === "Error") {
          setError({ line: null, message: `外置生成器启动失败：${r.error}` });
          return;
        }
        if (r.returncode !== 0) {
          const stderr = new TextDecoder().decode(new Uint8Array(r.stderr));
          setError({ line: null, message: `外置生成器返回码 ${r.returncode}：${stderr.slice(0, 200)}` });
          return;
        }
        const text = new TextDecoder().decode(new Uint8Array(r.stdout));
        setOutput(text);
        lastConfigRef.current = JSON.stringify({ g: genMode, e: ext.cmd });
        return;
      }
      const text = await generateData(config, s);
      setOutput(text);
      lastConfigRef.current = JSON.stringify({ g: genMode, c: config });
    } catch (e) {
      const err = e as Error;
      setError({ line: null, message: err.message ?? String(e) });
    } finally {
      setLoading(false);
    }
  };

  const onCopy = async () => {
    await navigator.clipboard.writeText(output);
  };

  const onExport = async () => {
    const path = await save({
      title: "导出测试数据",
      defaultPath: "test.in",
      filters: [{ name: "文本", extensions: ["in", "txt"] }],
    });
    if (!path) return;
    try {
      await saveTextFile(path, output);
    } catch (e) {
      setError({ line: null, message: `导出失败：${String(e)}` });
    }
  };

  // 配置变化提示（未重新生成时旧数据仍可复制）
  const stale =
    output !== "" &&
    lastConfigRef.current !==
      JSON.stringify(genMode === "External" ? { g: genMode, e: ext?.cmd } : { g: genMode, c: config });

  return (
    <div className="gen-panel">
      <div className="gen-toolbar">
        <label>
          种子
          <input
            className="seed-input"
            value={seed}
            onChange={(e) => setSeed(e.target.value)}
            placeholder="留空=随机"
            title="留空则每次随机（不可复现）"
          />
        </label>
        <button onClick={onGenerate} disabled={loading}>
          {loading
            ? "生成中…"
            : genMode === "External"
              ? "试运行外置生成器"
              : "生成样例"}
        </button>
        <button onClick={onCopy} disabled={!output}>
          复制
        </button>
        <button onClick={onExport} disabled={!output}>
          导出…
        </button>
        {genMode === "External" && (
          <span className="stale-hint">外置模式：生成器标准输出即测试数据（种子非空时追加 --seed）</span>
        )}
        {stale && <span className="stale-hint">配置已变，请重新生成</span>}
      </div>
      {error && <div className="gen-error">{error.message}</div>}
      <pre className="gen-output">{output}</pre>
    </div>
  );
}
