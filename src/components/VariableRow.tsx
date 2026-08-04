import { useEffect, useState } from "react";
import type { Item, LineItem, LineItemKind, VarKind, Weight, ElemType } from "../api";
import { exprEval } from "../api";
import {
  charsetToPresets,
  editField,
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
  return (
    <span className={`multi-part${nameErr ? " invalid" : ""}`}>
      <input
        className="name-input small-name"
        value={item.name}
        onChange={(e) => onChange({ ...item, name: e.target.value })}
        placeholder="名字"
        title={nameErr ?? "该数名字"}
        spellCheck={false}
      />
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

/** 字符串项：长度 + 字符集快捷预设（多选）。 */
function StrEditor({
  len,
  charset,
  onChange,
}: {
  len: string;
  charset: string;
  onChange: (len: string, charset: string) => void;
}) {
  const presets = charsetToPresets(charset);
  const [custom, setCustom] = useState(presets ? "" : charset);
  const [usingCustom, setUsingCustom] = useState(!presets);
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
      <span className="charset-presets" title="字符集快捷预设（可多选）">
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
              checked={usingCustom ? false : (presets?.[k0] ?? false)}
              disabled={usingCustom}
              onChange={(e) => {
                const cur = presets ?? { lower: false, upper: false, digits: false };
                const next = { ...cur, [k0]: e.target.checked };
                if (!next.lower && !next.upper && !next.digits) {
                  onChange(len, "abcdefghijklmnopqrstuvwxyz");
                  return;
                }
                onChange(len, presetsToCharset(next));
              }}
            />
            {label}
          </label>
        ))}
        <label className="preset-label">
          <input
            type="checkbox"
            checked={usingCustom}
            onChange={(e) => {
              setUsingCustom(e.target.checked);
              if (e.target.checked) {
                setCustom(charset);
              } else {
                onChange(len, "abcdefghijklmnopqrstuvwxyz");
              }
            }}
          />
          自定义
        </label>
        {usingCustom && (
          <input
            className="field-input charset-input"
            value={custom}
            onChange={(e) => {
              setCustom(e.target.value);
              onChange(len, e.target.value);
            }}
            placeholder="自定义字符集"
            spellCheck={false}
          />
        )}
      </span>
    </>
  );
}

/** 行块表单：行级重复 + 行内项列表。 */
function LineForm({
  kind,
  onKind,
  nameTaken,
}: {
  kind: VarKind;
  onKind: (k: VarKind) => void;
  nameTaken?: (n: string) => boolean;
}) {
  const { rows, items } = (kind as { Line: { rows: string; items: LineItem[] } }).Line;
  const [rowsError, setRowsError] = useState<string | null>(null);
  const repeatOn = rows.trim() !== "" && rows.trim() !== "1";
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

  return (
    <span className="line-form">
      <span className={`multi-part${rowsError ? " invalid" : ""}`}>
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
              className="field-input small"
              value={rows}
              onChange={(e) => setRows(e.target.value)}
              placeholder="N"
              title="重复行数（表达式，可引用前面变量）"
              spellCheck={false}
            />
            <span className="wg-label">行</span>
            {rowsError && <span className="inline-err">{rowsError}</span>}
          </>
        )}
      </span>
      {repeatOn && <span className="repeat-hint">重复行变量名按 n[k] 数组形式引用</span>}
      {items.map((it, i) => (
        <LineItemEditor
          key={i}
          item={it}
          nameTaken={nameTaken}
          onChange={(nit) => setItems(items.map((q, j) => (j === i ? nit : q)))}
          onRemove={() => setItems(items.filter((_, j) => j !== i))}
        />
      ))}
      <button
        onClick={() =>
          setItems([
            ...items,
            { name: "", kind: { Int: { min: "1", max: "100" } } },
          ])
        }
      >
        ＋ 数
      </button>
    </span>
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
    return <LineForm kind={kind} onKind={onKind} nameTaken={nameTaken} />;
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
      k: string | null;
      w: Weight | null;
      val: Weight | null;
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
        <WeightGroup label="边权" w={v.w} onChange={(w) => onKind(setWeight(kind, "w", w))} />
        <WeightGroup label="节点权" w={v.val} onChange={(w) => onKind(setWeight(kind, "val", w))} />
      </>
    );
  }
  if (key === "Tree") {
    const v = k.Tree as { w: Weight | null; val: Weight | null };
    return (
      <>
        <TextFields kind={kind} onKind={onKind} />
        <WeightGroup label="边权" w={v.w} onChange={(w) => onKind(setWeight(kind, "w", w))} />
        <WeightGroup label="节点权" w={v.val} onChange={(w) => onKind(setWeight(kind, "val", w))} />
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
  dragProps,
}: Props) {
  const kindKey = Object.keys(item.kind)[0];
  const isLine = kindKey === "Line";
  const nameErr = isLine ? null : nameError(item.name, nameTaken);

  return (
    <div
      className={`var-row${dragging ? " dragging" : ""}`}
      draggable
      {...dragProps}
    >
      <span className="drag-handle" title="拖拽排序">⠿</span>
      {!isLine && (
        <input
          className={`name-input${nameErr ? " invalid" : ""}`}
          value={item.name}
          onChange={(e) => onName(e.target.value)}
          placeholder="变量名"
          title={nameErr ?? "变量名"}
          spellCheck={false}
        />
      )}
      <span className="kind-badge">{kindLabel(item.kind)}</span>
      <KindForm kind={item.kind} onKind={onKind} nameTaken={nameTaken} />
      <button className="del-btn" onClick={onDelete} title="删除变量">✕</button>
    </div>
  );
}
