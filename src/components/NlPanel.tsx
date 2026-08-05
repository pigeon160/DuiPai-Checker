import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  modelDownload,
  modelLoad,
  modelSetPath,
  modelStatus,
  nlToDsl,
  type ModelProgress,
  type ModelStatus,
  type NlResult,
} from "../api";

interface Props {
  onLoadDsl: (dsl: string) => void;
}

export default function NlPanel({ onLoadDsl }: Props) {
  const [text, setText] = useState("");
  const [result, setResult] = useState<NlResult | null>(null);
  const [converting, setConverting] = useState(false);
  const [convError, setConvError] = useState("");
  const [model, setModel] = useState<ModelStatus | null>(null);
  const [modelPathInput, setModelPathInput] = useState("");
  const [modelMsg, setModelMsg] = useState("");
  const [dlBusy, setDlBusy] = useState(false);
  const [dlPct, setDlPct] = useState<number | null>(null);
  const [loadingModel, setLoadingModel] = useState(false);
  const dlStateRef = useRef<"idle" | "start" | "progress" | "done" | "error">("idle");

  const refreshModel = async () => {
    try {
      const s = await modelStatus();
      setModel(s);
      setModelPathInput(s.path ?? "");
    } catch (e) {
      setModelMsg(`模型状态获取失败：${String(e)}`);
    }
  };

  useEffect(() => {
    refreshModel();
    const un = listen<ModelProgress>("model://progress", (e) => {
      const p = e.payload;
      dlStateRef.current = p.stage;
      if (p.stage === "progress") {
        setDlPct(p.pct ?? null);
        setModelMsg(`下载中 ${p.pct ?? "…"}%`);
        return;
      }
      setModelMsg(
        p.stage === "start"
          ? `正在下载 ${p.file}…（完成后再设置路径）`
          : p.stage === "done"
            ? `下载完成：${p.file}`
            : `下载失败：${p.message}`,
      );
      if (p.stage === "done" || p.stage === "error") {
        setDlBusy(false);
        setDlPct(p.stage === "done" ? 100 : null);
        refreshModel();
      }
    });
    return () => {
      un.then((f) => f());
    };
  }, []);

  const onConvert = async () => {
    setConverting(true);
    setConvError("");
    try {
      const r = await nlToDsl(text);
      setResult(r);
    } catch (e) {
      setConvError(String(e));
    } finally {
      setConverting(false);
    }
  };

  const onSetPath = async () => {
    setModelMsg("");
    try {
      const s = await modelSetPath(modelPathInput);
      setModel(s);
      setModelMsg(s.path ? `模型路径已保存：${s.path}` : "已清除模型路径");
    } catch (e) {
      setModelMsg(String(e));
    }
  };

  const onLoad = async () => {
    setModelMsg("");
    setLoadingModel(true);
    try {
      const s = await modelLoad();
      setModel(s);
      setModelMsg(s.loaded ? "模型加载成功" : "模型未加载");
    } catch (e) {
      setModelMsg(String(e));
    } finally {
      setLoadingModel(false);
    }
  };

  const onDownload = async () => {
    const url = modelPathInput.trim();
    if (!url.startsWith("http")) {
      setModelMsg("请先在路径框输入 https:// 下载地址");
      return;
    }
    setDlBusy(true);
    setModelMsg("");
    try {
      await modelDownload(url);
    } catch (e) {
      setModelMsg(String(e));
      setDlBusy(false);
    }
  };

  const confPct = result ? Math.round(result.confidence * 100) : 0;

  return (
    <div className="nl-panel">
      <div className="nl-top">
        <textarea
          className="nl-input"
          value={text}
          onChange={(e) => setText(e.target.value)}
          placeholder={
            "用自然语言描述输入格式，例如：\n" +
            "「多测，T 组。第一行两个整数 n m，接下来 n 行每行两个整数 a b，" +
            "然后一棵带边权的树，边权 1 到 10^9」\n" +
            "「first line contains n, then n lines each with one integer a」"
          }
        />
        <div className="nl-buttons">
          <button className="btn-primary" onClick={onConvert} disabled={converting || !text.trim()}>
            {converting ? "转换中…" : "转换为 DSL"}
          </button>
          <button
            className="btn-secondary"
            onClick={() => {
              if (result && result.dsl) onLoadDsl(result.dsl);
            }}
            disabled={!result || !result.dsl}
            title="把转换结果载入 DSL 编辑器"
          >
            载入编辑器
          </button>
        </div>
      </div>

      {convError && <div className="gen-error">{convError}</div>}

      {result && (
        <div className="nl-result">
          <div className="nl-meta">
            <span className={`badge conf-${result.confidence >= 0.9 ? "hi" : result.confidence >= 0.5 ? "mid" : "lo"}`}>
              置信度 {confPct}%
            </span>
            <span className={`badge ${result.method === "Model" ? "model" : "rule"}`}>
              {result.method === "Model" ? "模型" : "规则"}
            </span>
            {result.confidence === 0 && <span className="stale-hint">未命中任何规则</span>}
          </div>
          {result.warnings.length > 0 && (
            <ul className="err-list nl-warnings">
              {result.warnings.map((w, i) => (
                <li key={i}>{w}</li>
              ))}
            </ul>
          )}
          {result.dsl ? (
            <pre className="nl-dsl">{result.dsl}</pre>
          ) : (
            <p className="nl-none">没有可用的转换结果。</p>
          )}
        </div>
      )}

      <div className="nl-model">
        <div className="nl-model-head">
          <span className="panel-accent" />
          <h3>本地模型通道</h3>
          {model && (
            <span className={`badge ${model.available ? (model.loaded ? "ok" : "model") : "lo"}`}>
              {model.available ? (model.loaded ? "模型已就绪" : "未加载模型") : "未编译启用（nl-model）"}
            </span>
          )}
        </div>
        <div className="nl-model-row">
          <input
            className="nl-model-path"
            value={modelPathInput}
            onChange={(e) => setModelPathInput(e.target.value)}
            placeholder={
              "模型文件路径（.gguf）或下载地址（https://…）"
            }
            title="填入本地 .gguf 路径，或 https 下载地址"
          />
          <button className="btn-secondary" onClick={onSetPath} disabled={dlBusy}>
            设置路径
          </button>
          <button className="btn-secondary" onClick={onLoad} disabled={!model?.available || dlBusy || loadingModel}>
            {loadingModel ? "加载中…" : "加载"}
          </button>
          <button className="btn-secondary" onClick={onDownload} disabled={dlBusy || loadingModel}>
            {dlBusy ? "下载中…" : "下载模型"}
          </button>
        </div>
        {dlBusy && dlPct != null && (
          <div className="dl-progress">
            <div className="dl-progress-bar" style={{ width: `${dlPct}%` }} />
          </div>
        )}
        {modelMsg && <div className="stale-hint nl-model-msg">{modelMsg}</div>}
        <p className="nl-model-hint">
          模型推理通道（llama.cpp，nl-model 编译特性）：放置 GGUF 模型文件后可加载，
          规则未命中的描述由模型转换。未启用时仅规则引擎工作。
        </p>
      </div>
    </div>
  );
}
