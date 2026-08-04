import { useRef, useState } from "react";
import type { Item, VarKind } from "../api";
import VariableRow from "./VariableRow";
import { KIND_ORDER, makeItem } from "../kindMeta";

interface Props {
  items: Item[];
  repeatEnabled: boolean;
  repeatCount: string;
  onChangeItems: (items: Item[]) => void;
  onToggleRepeat: (enabled: boolean) => void;
  onChangeRepeatCount: (count: string) => void;
}

export default function VariableList({
  items,
  repeatEnabled,
  repeatCount,
  onChangeItems,
  onToggleRepeat,
  onChangeRepeatCount,
}: Props) {
  const dragIndex = useRef<number>(-1);
  const [overIndex, setOverIndex] = useState<number>(-1);
  const [newKind, setNewKind] = useState<VarKind>(KIND_ORDER[0].kind);

  // 跨行重名检测（名称框红框提示）
  const nameCount = new Map<string, number>();
  for (const it of items) {
    const key = Object.keys(it.kind)[0];
    if (key === "Line") {
      for (const p of (it.kind as { Line: { items: { name: string }[] } }).Line.items) {
        nameCount.set(p.name, (nameCount.get(p.name) ?? 0) + 1);
      }
    } else {
      nameCount.set(it.name, (nameCount.get(it.name) ?? 0) + 1);
    }
  }
  const nameTaken = (n: string) => (nameCount.get(n) ?? 0) > 1;

  // 自动命名：新顶层变量取名 v1/v2/...（避开已有名）
  const addItem = () => {
    const used = new Set<string>();
    for (const it of items) {
      const key = Object.keys(it.kind)[0];
      if (key === "Line") {
        for (const p of (it.kind as { Line: { items: { name: string }[] } }).Line.items) {
          used.add(p.name);
        }
      } else {
        used.add(it.name);
      }
    }
    let n = 1;
    let name = "v1";
    while (used.has(name)) {
      n += 1;
      name = `v${n}`;
    }
    onChangeItems([...items, { ...makeItem(newKind), name }]);
  };

  const reorder = (from: number, to: number) => {
    if (from === to) return;
    const next = [...items];
    const [moved] = next.splice(from, 1);
    next.splice(to, 0, moved);
    onChangeItems(next);
  };

  return (
    <div className="var-panel">
      <div className="var-toolbar">
        <label className="repeat-box">
          <input
            type="checkbox"
            checked={repeatEnabled}
            onChange={(e) => onToggleRepeat(e.target.checked)}
          />
          多测模式
        </label>
        <input
          className="repeat-count"
          disabled={!repeatEnabled}
          value={repeatCount}
          onChange={(e) => onChangeRepeatCount(e.target.value)}
          title="重复次数"
          placeholder="N"
        />
        <span className="toolbar-spacer" />
        <select
          value={JSON.stringify(newKind)}
          onChange={(e) => setNewKind(JSON.parse(e.target.value) as VarKind)}
        >
          {KIND_ORDER.map((o) => (
            <option key={o.label} value={JSON.stringify(o.kind)}>
              {o.label}
            </option>
          ))}
        </select>
        <button className="btn-primary" onClick={addItem}>＋ 添加变量</button>
      </div>

      {items.length === 0 && <p className="empty-hint">还没有变量——点“＋ 添加变量”或直接在下方面板编辑 DSL</p>}

      {items.map((item, i) => (
        <VariableRow
          key={i}
          item={item}
          index={i}
          dragging={overIndex === i}
          onName={(name) => {
            const next = [...items];
            next[i] = { ...item, name };
            onChangeItems(next);
          }}
          onKind={(kind) => {
            const next = [...items];
            next[i] = { ...item, kind };
            onChangeItems(next);
          }}
          onDelete={() => onChangeItems(items.filter((_, j) => j !== i))}
          nameTaken={nameTaken}
          dragProps={{
            onDragStart: (e) => {
              dragIndex.current = i;
              e.dataTransfer.effectAllowed = "move";
              e.dataTransfer.setData("text/plain", String(i));
            },
            onDragOver: (e) => {
              e.preventDefault();
              e.dataTransfer.dropEffect = "move";
              if (overIndex !== i) setOverIndex(i);
            },
            onDrop: (e) => {
              e.preventDefault();
              const from = dragIndex.current;
              if (from >= 0) reorder(from, i);
              setOverIndex(-1);
            },
            onDragEnd: () => {
              dragIndex.current = -1;
              setOverIndex(-1);
            },
          }}
        />
      ))}
    </div>
  );
}
