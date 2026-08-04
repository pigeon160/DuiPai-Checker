import { useCallback, useEffect, useRef, useState } from "react";
import * as monaco from "monaco-editor";
import {
  dslParseChecked,
  dslSerialize,
  ping,
  type Config,
  type DslError,
  type Item,
} from "./api";
import { registerDslLanguage } from "./dslLanguage";
import DslEditor from "./components/DslEditor";
import VariableList from "./components/VariableList";
import GeneratePanel from "./components/GeneratePanel";
import CheckPanel from "./components/CheckPanel";

const SAMPLE_DSL = `# 多测模式：重复 3 次
n = int(1, 100)
a = ints(n, 1, 100)
p = perm(n)
s = str(10, "ab")
t = tree(n, w=int(1, 100))
g = graph(n, 50, 1, 0, w=int(1, 9))
`;

export default function App() {
  const [status, setStatus] = useState("正在连接后端……");
  const [dsl, setDsl] = useState(SAMPLE_DSL);
  const [items, setItems] = useState<Item[]>([]);
  const [repeatEnabled, setRepeatEnabled] = useState(false);
  const [repeatCount, setRepeatCount] = useState("1");
  const [errors, setErrors] = useState<DslError[]>([]);
  const [editorFocused, setEditorFocused] = useState(false);
  const [ready, setReady] = useState(false);
  const editorRef = useRef<monaco.editor.IStandaloneCodeEditor | null>(null);
  const itemsRef = useRef(items);
  itemsRef.current = items;
  const dslRef = useRef(dsl);
  dslRef.current = dsl;

  // 语言注册（命令补全 + 变量名补全 + Ctrl+Enter 应用）
  useEffect(() => {
    registerDslLanguage(
      () => itemsRef.current.map((i) => i.name),
      () => applyFromDslRef.current(),
    );
  }, []);

  useEffect(() => {
    ping()
      .then((p) => setStatus(`后端连通：${p}`))
      .catch((e) => setStatus(`后端连接失败：${String(e)}`));
  }, []);

  // 首次加载：解析示例 DSL -> GUI
  useEffect(() => {
    dslParseChecked(SAMPLE_DSL)
      .then((r) => {
        setItems(r.config.items);
        setRepeatEnabled(r.config.repeat?.enabled ?? false);
        setRepeatCount(r.config.repeat?.count ?? "1");
        setErrors(r.errors);
      })
      .catch(() => {})
      .finally(() => setReady(true));
  }, []);

  // 图形化 -> DSL（防抖 300ms；用户聚焦 DSL 编辑器时不覆盖）
  useEffect(() => {
    if (!ready || editorFocused) return;
    const t = setTimeout(async () => {
      try {
        const cfg: Config = {
          repeat: repeatEnabled ? { enabled: true, count: repeatCount } : null,
          items,
        };
        const text = await dslSerialize(cfg);
        if (text !== dslRef.current) setDsl(text);
      } catch {
        // 序列化失败（理论上不发生），保留当前文本
      }
    }, 300);
    return () => clearTimeout(t);
  }, [items, repeatEnabled, repeatCount, editorFocused, ready]);

  // 实时校验（DSL 文本变化后防抖 400ms，仅更新错误标记，不动 GUI）
  useEffect(() => {
    const t = setTimeout(async () => {
      try {
        const r = await dslParseChecked(dsl);
        setErrors(r.errors);
      } catch (e) {
        setErrors([e as DslError]);
      }
    }, 400);
    return () => clearTimeout(t);
  }, [dsl]);

  // DSL -> 图形化（点“应用”）
  const applyFromDsl = useCallback(async () => {
    try {
      const r = await dslParseChecked(dsl);
      setItems(r.config.items);
      setRepeatEnabled(r.config.repeat?.enabled ?? false);
      setRepeatCount(r.config.repeat?.count ?? "1");
      setErrors(r.errors);
    } catch (e) {
      setErrors([e as DslError]);
    }
  }, [dsl]);
  const applyFromDslRef = useRef(applyFromDsl);
  applyFromDslRef.current = applyFromDsl;

  const clean = errors.length === 0;

  const buildConfig = (): Config => ({
    repeat: repeatEnabled ? { enabled: true, count: repeatCount } : null,
    items,
  });

  return (
    <div className="app">
      <header>
        <h1>对拍检查器</h1>
        <span className="status">{status}</span>
        <span className={`badge ${clean ? "ok" : "err"}`}>
          {clean ? "DSL 校验通过" : `${errors.length} 个错误`}
        </span>
      </header>

      <main className="split">
        <section className="panel top">
          <div className="panel-head">
            <h2>图形化变量列表（修改自动同步到下方 DSL）</h2>
          </div>
          <VariableList
            items={items}
            repeatEnabled={repeatEnabled}
            repeatCount={repeatCount}
            onChangeItems={setItems}
            onToggleRepeat={setRepeatEnabled}
            onChangeRepeatCount={setRepeatCount}
          />
        </section>

        <section className="panel mid">
          <div className="panel-head">
            <h2>DSL 编辑器</h2>
            <div className="head-actions">
              <button onClick={() => applyFromDsl()} title="Ctrl+Enter">
                应用（解析为图形化列表）
              </button>
            </div>
          </div>
          <div className="editor-wrap">
            <DslEditor
              value={dsl}
              onChange={setDsl}
              onFocusChange={setEditorFocused}
              errors={errors}
              editorRef={editorRef}
            />
          </div>
          {errors.length > 0 && (
            <ul className="err-list">
              {errors.map((e, i) => (
                <li key={i}>
                  {e.line != null && <b>第 {e.line} 行：</b>}
                  {e.message}
                </li>
              ))}
            </ul>
          )}
        </section>

        <section className="panel gen">
          <div className="panel-head">
            <h2>数据生成预览</h2>
          </div>
          <GeneratePanel config={buildConfig()} />
        </section>

        <section className="panel check">
          <div className="panel-head">
            <h2>对拍</h2>
          </div>
          <CheckPanel config={buildConfig()} />
        </section>
      </main>
    </div>
  );
}
