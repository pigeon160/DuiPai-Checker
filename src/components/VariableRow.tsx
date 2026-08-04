import type { Item, VarKind, Weight, ElemType } from "../api";
import {
  editField,
  kindFieldValue,
  kindFields,
  kindLabel,
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

function KindForm({ kind, onKind }: { kind: VarKind; onKind: (k: VarKind) => void }) {
  const key = Object.keys(kind)[0];
  const k = kind as Record<string, unknown>;

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
            <WeightGroup label="边权" w={v.w} onChange={(w) => onKind(setWeight(kind, "w", w))} />
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
        {(v.gtype === "General" || v.gtype === "Ring" || v.gtype === "BaseRing") && (
          <WeightGroup label="节点权" w={v.val} onChange={(w) => onKind(setWeight(kind, "val", w))} />
        )}
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

export default function VariableRow({ item, dragging, onName, onKind, onDelete, dragProps }: Props) {
  return (
    <div
      className={`var-row${dragging ? " dragging" : ""}`}
      draggable
      {...dragProps}
    >
      <span className="drag-handle" title="拖拽排序">⠿</span>
      <input
        className="name-input"
        value={item.name}
        onChange={(e) => onName(e.target.value)}
        placeholder="变量名"
        spellCheck={false}
      />
      <span className="kind-badge">{kindLabel(item.kind)}</span>
      <KindForm kind={item.kind} onKind={onKind} />
      <button className="del-btn" onClick={onDelete} title="删除变量">✕</button>
    </div>
  );
}
