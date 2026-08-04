import { useCallback, useEffect, useState } from "react";
import { dslParse, dslSerialize, exprEval, ping, type Config, type DslError } from "./api";

const SAMPLE_DSL = `# 多测模式：重复 3 次
n = int(1, 100)
x = float(0, 1, 4)
a = ints(n, 1, 100)
b = floats(3, 0, 1)
M = matrix(3, n, 0, 1)
F = matf(int(1, 5), n, 0, 1, 4)
`;

export default function App() {
  const [dsl, setDsl] = useState(SAMPLE_DSL);
  const [status, setStatus] = useState("正在连接后端……");
  const [config, setConfig] = useState<Config | null>(null);
  const [error, setError] = useState<DslError | null>(null);
  const [roundtrip, setRoundtrip] = useState<string | null>(null);
  const [exprResult, setExprResult] = useState<string>("");

  useEffect(() => {
    ping()
      .then((p) => setStatus(`后端连通：${p}`))
      .catch((e) => setStatus(`后端连接失败：${String(e)}`));
  }, []);

  const onParse = useCallback(async () => {
    setError(null);
    setConfig(null);
    setRoundtrip(null);
    try {
      setConfig(await dslParse(dsl));
    } catch (e) {
      setError(e as DslError);
    }
  }, [dsl]);

  const onRoundtrip = useCallback(async () => {
    setError(null);
    setRoundtrip(null);
    try {
      const cfg = await dslParse(dsl);
      setConfig(cfg);
      setRoundtrip(await dslSerialize(cfg));
    } catch (e) {
      setError(e as DslError);
    }
  }, [dsl]);

  const onExprEval = useCallback(async () => {
    setExprResult("");
    try {
      const env = { n: 100 };
      const r = await exprEval("2*n + int(1, 5)", env);
      setExprResult(`2*n + int(1,5)（n=100）= ${r}`);
    } catch (e) {
      setExprResult(`求值失败：${(e as DslError).message}`);
    }
  }, []);

  return (
    <div className="page">
      <header>
        <h1>对拍检查器 · Phase 1</h1>
        <span className="status">{status}</span>
      </header>

      <section>
        <h2>DSL 文本</h2>
        <textarea
          value={dsl}
          onChange={(e) => setDsl(e.target.value)}
          rows={10}
          spellCheck={false}
          className="dsl-editor"
        />
        <div className="toolbar">
          <button onClick={onParse}>解析 → IR</button>
          <button onClick={onRoundtrip}>往返（解析 → 序列化）</button>
          <button onClick={onExprEval}>表达式求值测试</button>
        </div>
      </section>

      {error && (
        <section className="error">
          <h2>解析失败</h2>
          <pre>
            {error.line !== null ? `第 ${error.line} 行：` : ""}
            {error.message}
          </pre>
        </section>
      )}

      {config && (
        <section>
          <h2>IR 配置（{config.items.length} 个变量
            {config.repeat?.enabled ? `，多测重复 ${config.repeat.count} 次` : ""}）</h2>
          <pre className="json">{JSON.stringify(config, null, 2)}</pre>
        </section>
      )}

      {roundtrip !== null && (
        <section>
          <h2>序列化回 DSL</h2>
          <pre className="json">{roundtrip}</pre>
          {roundtrip.trim() === dsl.trim() ? (
            <p className="ok">与输入文本一致（往返稳定）</p>
          ) : (
            <p className="warn">与输入文本存在规范化差异（见上方 IR 展示）</p>
          )}
        </section>
      )}

      {exprResult && (
        <section>
          <h2>表达式</h2>
          <pre className="json">{exprResult}</pre>
        </section>
      )}
    </div>
  );
}
