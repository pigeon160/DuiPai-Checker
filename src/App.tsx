import { useCallback, useEffect, useRef, useState } from "react";
import * as monaco from "monaco-editor";
import {
  dslParseChecked,
  dslSerialize,
  ping,
  type Config,
  type DslError,
  type GenMode,
  type Item,
  type ProgramSpec,
} from "./api";
import { registerDslLanguage } from "./dslLanguage";
import DslEditor from "./components/DslEditor";
import VariableList from "./components/VariableList";
import GeneratePanel from "./components/GeneratePanel";
import CheckPanel from "./components/CheckPanel";
import NlPanel from "./components/NlPanel";
import { Panel, SplitHandle } from "./components/Panel";

const COLLAPSED_KEY = "duipai_collapsed";
const HEIGHTS_KEY = "duipai_heights";
const PANEL_COUNT = 5;

function loadJson<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    return raw ? (JSON.parse(raw) as T) : fallback;
  } catch {
    return fallback;
  }
}

const SAMPLE_DSL = "";

export default function App() {
  const [status, setStatus] = useState("正在连接后端……");
  const [dsl, setDsl] = useState(SAMPLE_DSL);
  const [items, setItems] = useState<Item[]>([]);
  const [repeatEnabled, setRepeatEnabled] = useState(false);
  const [repeatCount, setRepeatCount] = useState("1");
  const [errors, setErrors] = useState<DslError[]>([]);
  const [editorFocused, setEditorFocused] = useState(false);
  const [ready, setReady] = useState(false);
  const [genMode, setGenMode] = useState<GenMode>("Builtin");
  const [ext, setExt] = useState<ProgramSpec>({
    mode: "RunCmd",
    cmd: "",
    dir: "",
    label: "外置生成器",
  });
  const editorRef = useRef<monaco.editor.IStandaloneCodeEditor | null>(null);
  const itemsRef = useRef(items);
  itemsRef.current = items;
  const dslRef = useRef(dsl);
  dslRef.current = dsl;

  // 面板折叠 / 拖拽高度（localStorage 持久化）
  const [collapsed, setCollapsed] = useState<boolean[]>(() =>
    loadJson(COLLAPSED_KEY, Array(PANEL_COUNT).fill(false)),
  );
  const [heights, setHeights] = useState<(number | null)[]>(Array(PANEL_COUNT).fill(null));
  useEffect(() => {
    localStorage.setItem(COLLAPSED_KEY, JSON.stringify(collapsed));
  }, [collapsed]);
  useEffect(() => {
    localStorage.setItem(HEIGHTS_KEY, JSON.stringify(heights));
  }, [heights]);

  const togglePanel = (i: number) =>
    setCollapsed((c) => c.map((v, j) => (j === i ? !v : v)));

  /** 拖拽分隔条 i：调整上方面板高度 */
  const resizePanel = (i: number) => (delta: number) => {
    const base =
      heights[i] ?? document.getElementById(`panel-${i}`)?.offsetHeight ?? 300;
    const next = Math.min(2000, Math.max(60, base + delta));
    setHeights((h) => {
      const n = [...h];
      n[i] = next;
      return n;
    });
  };

  const resetHeight = (i: number) =>
    setHeights((h) => {
      const n = [...h];
      n[i] = null;
      return n;
    });

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

  // 图形化 -> DSL（防抖 200ms；编辑器聚焦时不覆盖——用户正在写 DSL）
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
    }, 200);
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

  // Monaco 会吞掉滚轮事件：滚动到顶/底且方向一致时，把滚动转发给整页。
  // 监听器在 DslEditor onMount 中绑定（见 components/DslEditor.tsx）。

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
        <Panel
          id={0}
          title="图形化变量列表（修改自动同步到下方 DSL）"
          basis={heights[0]}
          collapsed={collapsed[0]}
          onToggle={() => togglePanel(0)}
        >
          <VariableList
            items={items}
            repeatEnabled={repeatEnabled}
            repeatCount={repeatCount}
            onChangeItems={setItems}
            onToggleRepeat={setRepeatEnabled}
            onChangeRepeatCount={setRepeatCount}
          />
        </Panel>
        <SplitHandle
          onResize={resizePanel(0)}
          onReset={() => resetHeight(0)}
          disabled={collapsed[0] || collapsed[1]}
        />

        <Panel
          id={1}
          title="DSL 编辑器"
          basis={heights[1]}
          collapsed={collapsed[1]}
          onToggle={() => togglePanel(1)}
          actions={
            <button onClick={() => applyFromDsl()} title="Ctrl+Enter">
              应用（解析为图形化列表）
            </button>
          }
        >
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
        </Panel>
        <SplitHandle
          onResize={resizePanel(1)}
          onReset={() => resetHeight(1)}
          disabled={collapsed[1] || collapsed[2]}
        />

        <Panel
          id={2}
          title="数据生成预览"
          basis={heights[2]}
          collapsed={collapsed[2]}
          onToggle={() => togglePanel(2)}
        >
          <GeneratePanel config={buildConfig()} genMode={genMode} ext={genMode === "External" ? ext : null} />
        </Panel>
        <SplitHandle
          onResize={resizePanel(2)}
          onReset={() => resetHeight(2)}
          disabled={collapsed[2] || collapsed[3]}
        />

        <Panel
          id={3}
          title="对拍"
          basis={heights[3]}
          collapsed={collapsed[3]}
          onToggle={() => togglePanel(3)}
        >
          <CheckPanel
            config={buildConfig()}
            genMode={genMode}
            ext={ext}
            onGenMode={setGenMode}
            onExt={setExt}
          />
        </Panel>
        <SplitHandle
          onResize={resizePanel(3)}
          onReset={() => resetHeight(3)}
          disabled={collapsed[3] || collapsed[4]}
        />

        <Panel
          id={4}
          title="自然语言 → DSL"
          basis={heights[4]}
          collapsed={collapsed[4]}
          onToggle={() => togglePanel(4)}
        >
          <NlPanel
            onLoadDsl={(dslText) => {
              setDsl(dslText);
              setEditorFocused(false);
            }}
          />
        </Panel>
      </main>
    </div>
  );
}
