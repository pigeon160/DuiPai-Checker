import { useCallback, useEffect, useRef, useState } from "react";
import * as monaco from "monaco-editor";
import { open, save } from "@tauri-apps/plugin-dialog";
import {
  dslParseChecked,
  dslSerialize,
  ping,
  readTextFile,
  saveTextFile,
  type Config,
  type DslError,
  type GenMode,
  type Item,
  type ProgramSpec,
} from "./api";
import { registerDslLanguage } from "./dslLanguage";
import { DSL_TEMPLATES, type DslTemplate } from "./templates";
import DslEditor from "./components/DslEditor";
import VariableList from "./components/VariableList";
import GeneratePanel from "./components/GeneratePanel";
import CheckPanel from "./components/CheckPanel";
import NlPanel from "./components/NlPanel";
import { Panel, SplitHandle } from "./components/Panel";

const COLLAPSED_KEY = "duipai_collapsed";
const CUSTOM_TPL_KEY = "duipai_custom_templates";
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

  // ---- 模板库与配置管理 ----
  const [customTpls, setCustomTpls] = useState<DslTemplate[]>(() =>
    loadJson(CUSTOM_TPL_KEY, [] as DslTemplate[]),
  );
  const [tplSelect, setTplSelect] = useState("i:0");
  const [saveName, setSaveName] = useState("");
  const [saveErr, setSaveErr] = useState("");
  useEffect(() => {
    localStorage.setItem(CUSTOM_TPL_KEY, JSON.stringify(customTpls));
  }, [customTpls]);

  /** 载入下拉选中的模板（内置 i:<idx> 或我的模板 c:<name>）。 */
  const loadTemplate = () => {
    if (tplSelect.startsWith("i:")) {
      const idx = Number(tplSelect.slice(2));
      setDsl(DSL_TEMPLATES[idx]?.dsl ?? "");
    } else if (tplSelect.startsWith("c:")) {
      const name = tplSelect.slice(2);
      const t = customTpls.find((x) => x.name === name);
      if (t) setDsl(t.dsl);
    }
    setEditorFocused(false);
  };

  /** 当前 DSL 另存为我的模板（同名覆盖）。 */
  const saveAsTemplate = () => {
    const name = saveName.trim() || `模板 ${new Date().toLocaleTimeString("zh-CN", { hour12: false })}`;
    setCustomTpls((s) => {
      const next = s.filter((x) => x.name !== name);
      next.push({ name, dsl: dslRef.current });
      return next;
    });
    setSaveName("");
    setSaveErr("");
  };

  const deleteTemplate = (name: string) => {
    setCustomTpls((s) => s.filter((x) => x.name !== name));
    setTplSelect("i:0");
  };

  const exportDsl = async () => {
    const path = await save({
      title: "导出 DSL",
      defaultPath: "input.dsl",
      filters: [{ name: "DSL", extensions: ["dsl", "txt"] }],
    });
    if (!path) return;
    try {
      await saveTextFile(path, dslRef.current);
    } catch (e) {
      setSaveErr(`导出失败：${String(e)}`);
    }
  };

  const importDsl = async () => {
    const path = await open({
      title: "导入 DSL",
      multiple: false,
      filters: [{ name: "DSL", extensions: ["dsl", "txt"] }],
    });
    if (!path || Array.isArray(path)) return;
    try {
      const text = await readTextFile(path);
      setDsl(text);
      setEditorFocused(false);
    } catch (e) {
      setSaveErr(`导入失败：${String(e)}`);
    }
  };

  // 面板折叠（localStorage 持久化）
  // 旧版本可能存了更少面板的数组：长度不符时整体重置，避免折叠失效
  const [collapsed, setCollapsed] = useState<boolean[]>(() => {
    const c = loadJson(COLLAPSED_KEY, Array(PANEL_COUNT).fill(false));
    return c.length === PANEL_COUNT ? c : Array(PANEL_COUNT).fill(false);
  });
  useEffect(() => {
    localStorage.setItem(COLLAPSED_KEY, JSON.stringify(collapsed));
  }, [collapsed]);

  const togglePanel = (i: number) =>
    setCollapsed((c) => c.map((v, j) => (j === i ? !v : v)));

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
        const cfg: Config = { items };
        const text = await dslSerialize(cfg);
        if (text !== dslRef.current) setDsl(text);
      } catch {
        // 序列化失败（理论上不发生），保留当前文本
      }
    }, 200);
    return () => clearTimeout(t);
  }, [items, editorFocused, ready]);

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
      setErrors(r.errors);
    } catch (e) {
      setErrors([e as DslError]);
    }
  }, [dsl]);
  const applyFromDslRef = useRef(applyFromDsl);
  applyFromDslRef.current = applyFromDsl;

  const clean = errors.length === 0;

  const buildConfig = (): Config => ({ items });

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
          collapsed={collapsed[0]}
          onToggle={() => togglePanel(0)}
        >
          <VariableList items={items} onChangeItems={setItems} />
        </Panel>
        <SplitHandle />

        <Panel
          id={1}
          title="DSL 编辑器"
          collapsed={collapsed[1]}
          onToggle={() => togglePanel(1)}
          actions={
            <>
              <select
                className="toolbar-select"
                value={tplSelect}
                onChange={(e) => setTplSelect(e.target.value)}
                title="常用题型模板（含我的模板）"
              >
                <optgroup label="内置模板">
                  {DSL_TEMPLATES.map((t, i) => (
                    <option key={t.name} value={`i:${i}`}>
                      {t.name}
                    </option>
                  ))}
                </optgroup>
                {customTpls.length > 0 && (
                  <optgroup label="我的模板">
                    {customTpls.map((t) => (
                      <option key={t.name} value={`c:${t.name}`}>
                        {t.name}
                      </option>
                    ))}
                  </optgroup>
                )}
              </select>
              <button onClick={loadTemplate} title="载入选中的模板">
                载入模板
              </button>
              <input
                className="save-name-input"
                value={saveName}
                onChange={(e) => setSaveName(e.target.value)}
                placeholder="另存为…"
                title="把当前 DSL 另存为我的模板（本机，同名覆盖）"
              />
              <button onClick={saveAsTemplate} title="把当前 DSL 存为模板">
                另存为模板
              </button>
              {customTpls.length > 0 && (
                <select
                  className="toolbar-select"
                  value=""
                  onChange={(e) => {
                    if (e.target.value) deleteTemplate(e.target.value);
                  }}
                  title="删除我的模板"
                >
                  <option value="">删除模板…</option>
                  {customTpls.map((s) => (
                    <option key={s.name} value={s.name}>
                      {s.name}
                    </option>
                  ))}
                </select>
              )}
              <button onClick={importDsl} title="从 .dsl 文件导入">
                导入
              </button>
              <button onClick={exportDsl} title="导出为 .dsl 文件">
                导出
              </button>
              <button onClick={() => applyFromDsl()} title="Ctrl+Enter">
                应用（解析为图形化列表）
              </button>
            </>
          }
        >
          {saveErr && <div className="gen-error">{saveErr}</div>}
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
                <li
                  key={i}
                  className={e.line != null ? "err-jump" : undefined}
                  title={e.line != null ? "点击跳转到该行" : undefined}
                  onClick={() => {
                    if (e.line == null) return;
                    const ed = editorRef.current;
                    if (!ed) return;
                    ed.revealPositionInCenter({ lineNumber: e.line, column: 1 });
                    ed.setPosition({ lineNumber: e.line, column: 1 });
                    ed.focus();
                  }}
                >
                  {e.line != null && <b>第 {e.line} 行：</b>}
                  {e.message}
                </li>
              ))}
            </ul>
          )}
        </Panel>
        <SplitHandle />

        <Panel
          id={2}
          title="自然语言 → DSL"
          collapsed={collapsed[2]}
          onToggle={() => togglePanel(2)}
        >
          <NlPanel
            onLoadDsl={(dslText) => {
              setDsl(dslText);
              setEditorFocused(false);
            }}
          />
        </Panel>
        <SplitHandle />

        <Panel
          id={3}
          title="数据生成预览"
          collapsed={collapsed[3]}
          onToggle={() => togglePanel(3)}
        >
          <GeneratePanel config={buildConfig()} genMode={genMode} ext={genMode === "External" ? ext : null} />
        </Panel>
        <SplitHandle />

        <Panel
          id={4}
          title="对拍"
          collapsed={collapsed[4]}
          onToggle={() => togglePanel(4)}
        >
          <CheckPanel
            config={buildConfig()}
            genMode={genMode}
            ext={ext}
            onGenMode={setGenMode}
            onExt={setExt}
          />
        </Panel>
      </main>
    </div>
  );
}
