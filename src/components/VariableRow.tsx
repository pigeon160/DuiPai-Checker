import { useEffect, useState } from "react";
import type { Item, MultiPart, VarKind, Weight, ElemType } from "../api";
import { exprEval } from "../api";
import {
  editField,
  kindFieldValue,
  kindFields,
  kindLabel,
  nameError,
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

/** 多赋值表单：每项 name + expr，常驻"＋ 数"与"重复 N 行"（实时校验）。 */
function MultiForm({
  kind,
  onKind,
  nameTaken,
}: {
  kind: VarKind;
  onKind: (k: VarKind) => void;
  nameTaken?: (n: string) => boolean;
}) {
  const { rows, parts } = (kind as { Multi: { rows: string; parts: MultiPart[] } }).Multi;
  const [rowsError, setRowsError] = useState<string | null>(null);
  const setParts = (parts: MultiPart[]) =>
    onKind({ Multi: { rows, parts } } as unknown as VarKind);

  // 重复行数实时校验：语法合法；常量时须 >= 1；引用未定义变量视为合法表达式
  useEffect(() => {
    let cancelled = false;
    const t = setTimeout(async () => {
      if (rows.trim() === "") {
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
    <>
      <span className={`multi-part${rowsError ? " invalid" : ""}`} title="重复输出行数（表达式，默认 1 行）">
        <span className="wg-label">重复</span>
        <input
          className="field-input small"
          value={rows}
          onChange={(e) =>
            onKind({ Multi: { rows: e.target.value, parts } } as unknown as VarKind)
          }
          placeholder="1"
          spellCheck={false}
        />
        <span className="wg-label">行</span>
        {rowsError && <span className="inline-err">{rowsError}</span>}
      </span>
      {parts.map((p, i) => {
        const err = nameError(p.name, nameTaken);
        return (
          <span key={i} className={`multi-part${err ? " invalid" : ""}`}>
            <input
              className="name-input small-name"
              value={p.name}
              onChange={(e) =>
                setParts(parts.map((q, j) => (j === i ? { ...q, name: e.target.value } : q)))
              }
              placeholder={`名${i + 1}`}
              title={err ?? `第 ${i + 1} 个数名字`}
              spellCheck={false}
            />
            <input
              className="field-input expr-input"
              title={`第 ${i + 1} 个数表达式（int()/float()/算术/引用/数组索引均可）`}
              value={p.expr}
              onChange={(e) =>
                setParts(parts.map((q, j) => (j === i ? { ...q, expr: e.target.value } : q)))
              }
              placeholder="int(1, 100)"
              spellCheck={false}
            />
            <button
              className="del-btn"
              onClick={() => setParts(parts.filter((_, j) => j !== i))}
              title="删除该数"
            >
              ✕
            </button>
          </span>
        );
      })}
      <button
        onClick={() =>
          setParts([
            ...parts,
            { name: "", expr: parts[0]?.expr ?? "int(1, 100)" },
          ])
        }
      >
        ＋ 数
      </button>
    </>
  );
}

/** 单值类型 → 表达式文本（多名字转换用）；多行类型返回 null。 */
function kindToExpr(kind: VarKind): string | null {
  const k = kind as Record<string, unknown>;
  switch (Object.keys(kind)[0]) {
    case "Int": {
      const v = k.Int as { min: string; max: string };
      return `int(${v.min}, ${v.max})`;
    }
    case "Float": {
      const v = k.Float as { min: string; max: string; prec: string };
      return v.prec === "6" ? `float(${v.min}, ${v.max})` : `float(${v.min}, ${v.max}, ${v.prec})`;
    }
    case "Scalar": {
      return (k.Scalar as { expr: string }).expr;
    }
    case "Multi": {
      const parts = (k.Multi as { parts: MultiPart[] }).parts;
      return parts[0]?.expr ?? null;
    }
    default:
      return null;
  }
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

  if (key === "Multi") {
    return <MultiForm kind={kind} onKind={onKind} nameTaken={nameTaken} />;
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

/** 允许"＋ 数"转多赋值的单行标量类型 */
const MULTIABLE = new Set(["Int", "Float", "Scalar"]);

export default function VariableRow({
  item,
  dragging,
  onName,
  onKind,
  onDelete,
  nameTaken,
  dragProps,
}: Props) {
  const isMulti = Object.keys(item.kind)[0] === "Multi";
  const kindKey = Object.keys(item.kind)[0];
  const multiParts = isMulti
    ? (item.kind as { Multi: { parts: MultiPart[] } }).Multi.parts
    : [];
  const nameErr = isMulti ? null : nameError(item.name, nameTaken);

  /** 名称框变更：多个名字（空格分隔）自动转为多赋值，单名字转回表达式变量 */
  const handleNameChange = (raw: string) => {
    const names = raw.split(/\s+/).filter(Boolean);
    if (isMulti) {
      const { rows } = (item.kind as { Multi: { rows: string; parts: MultiPart[] } }).Multi;
      if (names.length > 1) {
        const next = names.map((n, i) =>
          i < multiParts.length ? { ...multiParts[i], name: n } : { name: n, expr: multiParts[0]?.expr ?? "int(1, 100)" },
        );
        onKind({ Multi: { rows, parts: next } } as unknown as VarKind);
      } else if (names.length === 1) {
        onKind({ Scalar: { expr: multiParts[0]?.expr ?? "" } } as unknown as VarKind);
      }
      return;
    }
    if (names.length > 1) {
      const expr = kindToExpr(item.kind);
      if (expr === null) return; // 多行类型不支持一行多值，忽略改名
      onKind({ Multi: { rows: "1", parts: names.map((n) => ({ name: n, expr })) } } as unknown as VarKind);
      return;
    }
    onName(raw);
  };

  /** 单值标量行"＋ 数"：转多赋值（保留当前值 + 新 part） */
  const addNumber = () => {
    const expr = kindToExpr(item.kind);
    if (expr === null) return;
    onKind({
      Multi: {
        rows: "1",
        parts: [
          { name: item.name, expr },
          { name: "", expr: "int(1, 100)" },
        ],
      },
    } as unknown as VarKind);
  };

  return (
    <div
      className={`var-row${dragging ? " dragging" : ""}`}
      draggable
      {...dragProps}
    >
      <span className="drag-handle" title="拖拽排序">⠿</span>
      <input
        className={`name-input${nameErr ? " invalid" : ""}`}
        value={isMulti ? multiParts.map((p) => p.name).join(" ") : item.name}
        onChange={(e) => handleNameChange(e.target.value)}
        placeholder={isMulti ? "多个名字（空格分隔）" : "变量名"}
        title={
          nameErr ??
          (isMulti
            ? "空格分隔多个名字，每名一个数"
            : "可输入多个名字（空格分隔）实现一行多个数")
        }
        spellCheck={false}
      />
      <span className="kind-badge">{kindLabel(item.kind)}</span>
      <KindForm kind={item.kind} onKind={onKind} nameTaken={nameTaken} />
      {MULTIABLE.has(kindKey) && (
        <button onClick={addNumber} title="添加第二个数（转一行多赋值）">
          ＋ 数
        </button>
      )}
      <button className="del-btn" onClick={onDelete} title="删除变量">✕</button>
    </div>
  );
}
