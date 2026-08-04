import { useEffect, useState } from "react";
import type { Item, LineItem, LineItemKind, VarKind, Weight, ElemType } from "../api";
import { exprEval } from "../api";
import {
  CHARSET_DIGITS,
  CHARSET_LOWER,
  CHARSET_UPPER,
  editField,
  kindColor,
  kindFieldValue,
  kindFields,
  kindLabel,
  nameError,
  presetsToCharset,
  setElemType,
  setGraphFlag,
  setGtype,
  setWeight,
} from "../kindMeta";

interface Props {
  item: Item;
  index: number;
  dragging: boolean;
  onName: (name: string) => void;
  onKind: (kind: VarKind) => void;
  onDelete: () => void;
  nameTaken?: (n: string) => boolean;
  /** 全部已用名字（自动命名去重用） */
  usedNames?: Set<string>;
  dragProps: {
    onDragStart: (e: React.DragEvent) => void;
    onDragOver: (e: React.DragEvent) => void;
    onDrop: (e: React.DragEvent) => void;
    onDragEnd: () => void;
  };
}

/** 边权/节点权值编辑组。 */
function WeightGroup({
  label,
  w,
  onChange,
}: {
  label: string;
  w: Weight | null;
  onChange: (w: Weight | null) => void;
}) {
  const mode: "none" | "Int" | "Float" = w?.kind ?? "none";
  return (
    <span className="weight-group">
      <span className="wg-label">{label}</span>
      <select
        value={mode}
        onChange={(e) => {
          const v = e.target.value;
          if (v === "none") onChange(null);
          else if (v === "Int") onChange({ kind: "Int", min: "1", max: "100", prec: "6" });
          else onChange({ kind: "Float", min: "0", max: "1", prec: "6" });
        }}
      >
        <option value="none">无</option>
        <option value="Int">整数</option>
        <option value="Float">浮点</option>
      </select>
      {w && (
        <>
          <input
            className="field-input small"
            title={`${label}最小值`}
            value={w.min}
            onChange={(e) => onChange({ ...w, min: e.target.value })}
            placeholder="min"
          />
          <input
            className="field-input small"
            title={`${label}最大值`}
            value={w.max}
            onChange={(e) => onChange({ ...w, max: e.target.value })}
            placeholder="max"
          />
          {mode === "Float" && (
            <input
              className="field-input small"
              title={`${label}精度`}
              value={w.prec}
              onChange={(e) => onChange({ ...w, prec: e.target.value })}
              placeholder="prec"
            />
          )}
        </>
      )}
    </span>
  );
}

function TextFields({ kind, onKind }: { kind: VarKind; onKind: (k: VarKind) => void }) {
  return (
    <>
      {kindFields(kind).map((f) => (
        <input
          key={f.key}
          className="field-input"
          title={`${f.label}（表达式，可引用前面变量或 int(a,b)）`}
          value={kindFieldValue(kind, f.key)}
          onChange={(e) => onKind(editField(kind, f.key, e.target.value))}
          placeholder={f.ph}
        />
      ))}
    </>
  );
}

/** 行内项类型关键字 ↔ LineItemKind。 */
const ITEM_TYPE_LABELS: [string, string][] = [
  ["Int", "整数"],
  ["Float", "浮点"],
  ["Scalar", "表达式"],
  ["Text", "文本"],
  ["Str", "字符串"],
];

function itemKindKey(kind: LineItemKind): string {
  return Object.keys(kind)[0];
}

/** 切换行内项类型（保留可转换的参数）。 */
function switchItemKind(kind: LineItemKind, target: string): LineItemKind {
  const k = kind as Record<string, unknown>;
  switch (target) {
    case "Int": {
      const min = k.Int ? (k.Int as { min: string }).min : k.Float ? (k.Float as { min: string }).min : "";
      const max = k.Int ? (k.Int as { max: string }).max : k.Float ? (k.Float as { max: string }).max : "";
      return { Int: { min: min || "1", max: max || "100" } };
    }
    case "Float": {
      const min = k.Int ? (k.Int as { min: string }).min : k.Float ? (k.Float as { min: string }).min : "";
      const max = k.Int ? (k.Int as { max: string }).max : k.Float ? (k.Float as { max: string }).max : "";
      const prec = k.Float ? (k.Float as { prec: string }).prec : "6";
      return { Float: { min: min || "0", max: max || "1", prec } };
    }
    case "Scalar": {
      const expr = k.Scalar ? (k.Scalar as { expr: string }).expr : "";
      return { Scalar: { expr: expr || "int(1, 100)" } };
    }
    case "Text":
      return { Text: { text: k.Text ? (k.Text as { text: string }).text : "" } };
    case "Str": {
      const len = k.Str ? (k.Str as { len: string }).len : "10";
      const charset = k.Str ? (k.Str as { charset: string }).charset : "abcdefghijklmnopqrstuvwxyz";
      return { Str: { len, charset } };
    }
    default:
      return kind;
  }
}

/** 行内项编辑：名字 + 类型下拉 + 参数。 */
function LineItemEditor({
  item,
  onChange,
  onRemove,
  nameTaken,
}: {
  item: LineItem;
  onChange: (it: LineItem) => void;
  onRemove: () => void;
  nameTaken?: (n: string) => boolean;
}) {
  const key = itemKindKey(item.kind);
  const k = item.kind as Record<string, unknown>;
  const nameErr = nameError(item.name, nameTaken);
  const [dupErr, setDupErr] = useState<string | null>(null);

  // 重名即时阻止：目标名已被占用时回退输入并短暂提示
  const handleName = (raw: string) => {
    const err = nameError(raw, nameTaken);
    if (err && err.includes("重复")) {
      setDupErr(err);
      setTimeout(() => setDupErr(null), 2000);
      return;
    }
    setDupErr(null);
    onChange({ ...item, name: raw });
  };

  return (
    <span className={`multi-part${nameErr || dupErr ? " invalid" : ""}`}>
      <input
        className="name-input small-name"
        value={item.name}
        onChange={(e) => handleName(e.target.value)}
        placeholder="名字"
        title={dupErr ?? nameErr ?? "该数名字"}
        spellCheck={false}
      />
      {dupErr && <span className="inline-err">{dupErr}</span>}
      <select
        value={key}
        title="行内项类型"
        onChange={(e) => onChange({ ...item, kind: switchItemKind(item.kind, e.target.value) })}
      >
        {ITEM_TYPE_LABELS.map(([v, l]) => (
          <option key={v} value={v}>{l}</option>
        ))}
      </select>
      {key === "Int" && (
        <>
          <input
            className="field-input small"
            value={(k.Int as { min: string }).min}
            onChange={(e) =>
              onChange({ ...item, kind: { Int: { ...(k.Int as object), min: e.target.value } } as LineItemKind })
            }
            placeholder="min"
            title="最小值"
          />
          <input
            className="field-input small"
            value={(k.Int as { max: string }).max}
            onChange={(e) =>
              onChange({ ...item, kind: { Int: { ...(k.Int as object), max: e.target.value } } as LineItemKind })
            }
            placeholder="max"
            title="最大值"
          />
        </>
      )}
      {key === "Float" && (
        <>
          <input
            className="field-input small"
            value={(k.Float as { min: string }).min}
            onChange={(e) =>
              onChange({ ...item, kind: { Float: { ...(k.Float as object), min: e.target.value } } as LineItemKind })
            }
            placeholder="lo"
            title="下界"
          />
          <input
            className="field-input small"
            value={(k.Float as { max: string }).max}
            onChange={(e) =>
              onChange({ ...item, kind: { Float: { ...(k.Float as object), max: e.target.value } } as LineItemKind })
            }
            placeholder="hi"
            title="上界"
          />
          <input
            className="field-input small"
            value={(k.Float as { prec: string }).prec}
            onChange={(e) =>
              onChange({ ...item, kind: { Float: { ...(k.Float as object), prec: e.target.value } } as LineItemKind })
            }
            placeholder="prec"
            title="精度（默认 6）"
          />
        </>
      )}
      {key === "Scalar" && (
        <input
          className="field-input expr-input"
          value={(k.Scalar as { expr: string }).expr}
          onChange={(e) =>
            onChange({ ...item, kind: { Scalar: { expr: e.target.value } } as LineItemKind })
          }
          placeholder="2 * n + a[1]"
          title="自由表达式：常数/引用/int()/float()/算术/数组索引"
          spellCheck={false}
        />
      )}
      {key === "Text" && (
        <input
          className="field-input expr-input"
          value={(k.Text as { text: string }).text}
          onChange={(e) =>
            onChange({ ...item, kind: { Text: { text: e.target.value } } as LineItemKind })
          }
          placeholder="固定文本（如 ---）"
          title="固定字面量，不可引用"
          spellCheck={false}
        />
      )}
      {key === "Str" && (
        <StrEditor
          len={(k.Str as { len: string }).len}
          charset={(k.Str as { charset: string }).charset}
          onChange={(len, charset) => onChange({ ...item, kind: { Str: { len, charset } } as LineItemKind })}
        />
      )}
      <button className="del-btn" onClick={onRemove} title="删除该数">✕</button>
    </span>
  );
}

/** 字符串项：长度 + 字符集快捷预设（预设与自定义可组合，生成时去重）。 */
function StrEditor({
  len,
  charset,
  onChange,
}: {
  len: string;
  charset: string;
  onChange: (len: string, charset: string) => void;
}) {
  // 从 charset 中按序剥离预设段，剩余为自定义字符
  const [presets, custom] = splitPresets(charset);
  const presetOn = presets;
  return (
    <>
      <input
        className="field-input small"
        value={len}
        onChange={(e) => onChange(e.target.value, charset)}
        placeholder="长度"
        title="长度（可为表达式，如 int(3, 5) 区间随机）"
        spellCheck={false}
      />
      <span className="charset-presets" title="字符集快捷预设（多选，可与自定义组合，生成时去重）">
        {(
          [
            ["lower", "全小写"],
            ["upper", "全大写"],
            ["digits", "数字"],
          ] as const
        ).map(([k0, label]) => (
          <label key={k0} className="preset-label">
            <input
              type="checkbox"
              checked={presetOn[k0]}
              onChange={(e) => {
                const next = { ...presetOn, [k0]: e.target.checked };
                onChange(len, presetsToCharset(next) + custom);
              }}
            />
            {label}
          </label>
        ))}
        <span className="wg-label">自定义</span>
        <input
          className="field-input charset-input"
          value={custom}
          onChange={(e) => onChange(len, presetsToCharset(presetOn) + e.target.value)}
          placeholder="额外字符"
          title="自定义字符（与预设组合，可重复，生成时去重）"
          spellCheck={false}
        />
      </span>
    </>
  );
}

/** 从 charset 中按序剥离 LOWER/UPPER/DIGITS 预设段，返回 (预设勾选, 自定义剩余)。 */
function splitPresets(charset: string): [
  { lower: boolean; upper: boolean; digits: boolean },
  string,
] {
  let rest = charset;
  const lower = rest.startsWith(CHARSET_LOWER);
  if (lower) rest = rest.slice(CHARSET_LOWER.length);
  const upper = rest.startsWith(CHARSET_UPPER);
  if (upper) rest = rest.slice(CHARSET_UPPER.length);
  const digits = rest.startsWith(CHARSET_DIGITS);
  if (digits) rest = rest.slice(CHARSET_DIGITS.length);
  return [{ lower, upper, digits }, rest];
}

/** 行块标题控件：重复勾选 + 次数 + 提示 + ＋数。 */
function LineHeadControls({
  kind,
  onKind,
  usedNames,
}: {
  kind: VarKind;
  onKind: (k: VarKind) => void;
  usedNames?: Set<string>;
}) {
  const { rows, items } = (kind as { Line: { rows: string; items: LineItem[] } }).Line;
  const [rowsError, setRowsError] = useState<string | null>(null);
  // 空串也视为重复态：删空后输入框保留，可继续输入（而不是整个消失）
  const repeatOn = rows.trim() !== "1";
  const setRows = (r: string) => onKind({ Line: { rows: r, items } } as unknown as VarKind);
  const setItems = (items: LineItem[]) => onKind({ Line: { rows, items } } as unknown as VarKind);

  useEffect(() => {
    let cancelled = false;
    const t = setTimeout(async () => {
      if (rows.trim() === "" || rows.trim() === "1") {
        if (!cancelled) setRowsError(null);
        return;
      }
      try {
        const v = await exprEval(rows, {});
        if (!cancelled) setRowsError(v < 1 ? "重复行数须 ≥ 1" : null);
      } catch (e) {
        const msg = (e as { message?: string }).message ?? String(e);
        if (msg.includes("未定义的变量")) {
          if (!cancelled) setRowsError(null);
        } else if (!cancelled) {
          setRowsError("行数须为表达式且 ≥ 1");
        }
      }
    }, 350);
    return () => {
      cancelled = true;
      clearTimeout(t);
    };
  }, [rows]);

  // 自动命名：新行内项取名 v1/v2/...（避开全部已用名字，含其他行）
  const nextName = () => {
    let i = 1;
    while ((usedNames?.has(`v${i}`) ?? false) || items.some((it) => it.name === `v${i}`)) {
      i += 1;
    }
    return `v${i}`;
  };

  return (
    <>
      <label className="repeat-toggle" title="重复输出多行">
        <input
          type="checkbox"
          checked={repeatOn}
          onChange={(e) => setRows(e.target.checked ? "2" : "1")}
        />
        重复
      </label>
      {repeatOn && (
        <>
          <input
            className={`field-input small${rowsError ? " invalid" : ""}`}
            value={rows}
            onChange={(e) => setRows(e.target.value)}
            placeholder="N"
            title="重复行数（表达式，可引用前面变量）"
            spellCheck={false}
          />
          <span className="wg-label">行</span>
          {rowsError && <span className="inline-err">{rowsError}</span>}
          <span className="repeat-hint">重复行变量名按 n[k] 数组形式引用</span>
        </>
      )}
      <button
        className="btn-secondary"
        onClick={() =>
          setItems([
            ...items,
            { name: nextName(), kind: { Int: { min: "1", max: "100" } } },
          ])
        }
      >
        ＋ 数
      </button>
    </>
  );
}

/** 行内项列表（各占一行缩进）。 */
function LineItems({
  kind,
  onKind,
  nameTaken,
}: {
  kind: VarKind;
  onKind: (k: VarKind) => void;
  nameTaken?: (n: string) => boolean;
}) {
  const { rows, items } = (kind as { Line: { rows: string; items: LineItem[] } }).Line;
  const setItems = (items: LineItem[]) => onKind({ Line: { rows, items } } as unknown as VarKind);
  return (
    <div className="line-items">
      {items.map((it, i) => (
        <LineItemEditor
          key={i}
          item={it}
          nameTaken={nameTaken}
          onChange={(nit) => setItems(items.map((q, j) => (j === i ? nit : q)))}
          onRemove={() => setItems(items.filter((_, j) => j !== i))}
        />
      ))}
    </div>
  );
}

function KindForm({
  kind,
  onKind,
  nameTaken,
}: {
  kind: VarKind;
  onKind: (k: VarKind) => void;
  nameTaken?: (n: string) => boolean;
}) {
  const key = Object.keys(kind)[0];
  const k = kind as Record<string, unknown>;

  if (key === "Line") {
    return <LineItems kind={kind} onKind={onKind} nameTaken={nameTaken} />;
  }
  if (key === "Array") {
    const v = k.Array as { elem_type: ElemType };
    return (
      <>
        <select
          value={v.elem_type}
          title="元素类型"
          onChange={(e) => onKind(setElemType(kind, e.target.value as ElemType))}
        >
          <option value="Int">整数</option>
          <option value="Float">浮点</option>
        </select>
        <TextFields kind={kind} onKind={onKind} />
      </>
    );
  }
  if (key === "Graph") {
    const v = k.Graph as {
      gtype: string;
      directed: boolean;
      connected: boolean;
      multi: boolean;
      loop_: boolean;
      k: string | null;
      w: Weight | null;
    };
    return (
      <>
        <select value={v.gtype} title="图结构" onChange={(e) => onKind(setGtype(kind, e.target.value))}>
          <option value="General">一般</option>
          <option value="Dag">DAG</option>
          <option value="Bipartite">二分</option>
          <option value="Ring">环</option>
          <option value="BaseRing">基环树</option>
        </select>
        <TextFields kind={kind} onKind={onKind} />
        {v.gtype === "General" && (
          <>
            <select
              value={v.directed ? "1" : "0"}
              title="有向/无向"
              onChange={(e) => onKind(setGraphFlag(kind, "directed", e.target.value === "1"))}
            >
              <option value="1">有向</option>
              <option value="0">无向</option>
            </select>
            <select
              value={v.connected ? "1" : "0"}
              title="连通/任意"
              onChange={(e) => onKind(setGraphFlag(kind, "connected", e.target.value === "1"))}
            >
              <option value="1">连通</option>
              <option value="0">任意</option>
            </select>
          </>
        )}
        {v.gtype === "BaseRing" && (
          <input
            className="field-input small"
            title="环大小 k"
            value={v.k ?? "3"}
            onChange={(e) =>
              onKind({ Graph: { ...v, k: e.target.value } } as unknown as VarKind)
            }
            placeholder="k"
          />
        )}
        {(v.gtype === "General" || v.gtype === "Dag" || v.gtype === "Bipartite") && (
          <>
            <label className="preset-label" title="允许重复边（边数无上限）">
              <input
                type="checkbox"
                checked={v.multi}
                onChange={(e) =>
                  onKind({ Graph: { ...v, multi: e.target.checked } } as unknown as VarKind)
                }
              />
              重边
            </label>
            {v.gtype === "General" && (
              <label className="preset-label" title="允许自环（u 可等于 v）">
                <input
                  type="checkbox"
                  checked={v.loop_}
                  onChange={(e) =>
                    onKind({ Graph: { ...v, loop_: e.target.checked } } as unknown as VarKind)
                  }
                />
                自环
              </label>
            )}
          </>
        )}
        <WeightGroup label="边权" w={v.w} onChange={(w) => onKind(setWeight(kind, w))} />
      </>
    );
  }
  if (key === "Tree") {
    const v = k.Tree as { ttype: string; w: Weight | null };
    return (
      <>
        <select
          value={v.ttype}
          title="树结构"
          onChange={(e) => onKind({ Tree: { ...v, ttype: e.target.value } } as unknown as VarKind)}
        >
          <option value="Random">随机树</option>
          <option value="Star">菊花图</option>
          <option value="Chain">链</option>
        </select>
        <TextFields kind={kind} onKind={onKind} />
        <WeightGroup label="边权" w={v.w} onChange={(w) => onKind(setWeight(kind, w))} />
      </>
    );
  }
  return <TextFields kind={kind} onKind={onKind} />;
}

export default function VariableRow({
  item,
  dragging,
  onName,
  onKind,
  onDelete,
  nameTaken,
  usedNames,
  dragProps,
}: Props) {
  const kindKey = Object.keys(item.kind)[0];
  const isLine = kindKey === "Line";
  const nameErr = isLine ? null : nameError(item.name, nameTaken);
  const badge = kindColor(item.kind);

  return (
    <div
      className={`var-row${isLine ? " line-row" : ""}${dragging ? " dragging" : ""}`}
      draggable
      {...dragProps}
    >
      {isLine ? (
        <>
          <div className="line-head">
            <span className="drag-handle" title="拖拽排序">⠿</span>
            <span className="kind-badge" style={{ background: badge.bg, color: badge.fg }}>
              {kindLabel(item.kind)}
            </span>
            <LineHeadControls kind={item.kind} onKind={onKind} usedNames={usedNames} />
            <button className="del-btn" onClick={onDelete} title="删除变量">✕</button>
          </div>
          <LineItems kind={item.kind} onKind={onKind} nameTaken={nameTaken} />
        </>
      ) : (
        <>
          <span className="drag-handle" title="拖拽排序">⠿</span>
          <input
            className={`name-input${nameErr ? " invalid" : ""}`}
            value={item.name}
            onChange={(e) => {
              // 重名即时阻止（多名字转换逻辑在 handleNameChange 中已保留）
              const err = nameError(e.target.value, nameTaken);
              if (err && err.includes("重复")) {
                return;
              }
              onName(e.target.value);
            }}
            placeholder="变量名"
            title={nameErr ?? "变量名"}
            spellCheck={false}
          />
          {nameErr?.includes("重复") && (
            <span className="inline-err">{nameErr}</span>
          )}
          <span className="kind-badge" style={{ background: badge.bg, color: badge.fg }}>
            {kindLabel(item.kind)}
          </span>
          <KindForm kind={item.kind} onKind={onKind} nameTaken={nameTaken} />
          <button className="del-btn" onClick={onDelete} title="删除变量">✕</button>
        </>
      )}
    </div>
  );
}
