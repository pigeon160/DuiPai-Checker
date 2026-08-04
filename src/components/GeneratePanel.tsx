import { useRef, useState } from "react";
import { generateData, saveTextFile, type Config, type DslError } from "../api";
import { save } from "@tauri-apps/plugin-dialog";

interface Props {
  config: Config;
}

export default function GeneratePanel({ config }: Props) {
  const [seed, setSeed] = useState("");
  const [output, setOutput] = useState("");
  const [error, setError] = useState<DslError | null>(null);
  const [loading, setLoading] = useState(false);
  const lastConfigRef = useRef("");

  const onGenerate = async () => {
    setLoading(true);
    setError(null);
    try {
      const s = seed.trim() === "" ? null : Number(seed.trim());
      if (seed.trim() !== "" && !Number.isInteger(s)) {
        setError({ line: null, message: "种子必须是整数或留空" });
        return;
      }
      const text = await generateData(config, s as number | null);
      setOutput(text);
      lastConfigRef.current = JSON.stringify(config);
    } catch (e) {
      setError(e as DslError);
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
  const stale = output !== "" && lastConfigRef.current !== JSON.stringify(config);

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
          {loading ? "生成中…" : "生成样例"}
        </button>
        <button onClick={onCopy} disabled={!output}>
          复制
        </button>
        <button onClick={onExport} disabled={!output}>
          导出…
        </button>
        {stale && <span className="stale-hint">配置已变，请重新生成</span>}
      </div>
      {error && <div className="gen-error">{error.message}</div>}
      <pre className="gen-output">{output}</pre>
    </div>
  );
}
