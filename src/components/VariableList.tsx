import { useRef, useState } from "react";
import type { Item, RepeatBlock, VarKind } from "../api";
import VariableRow from "./VariableRow";
import { KIND_ORDER, makeItem } from "../kindMeta";

interface Props {
  items: Item[];
  repeat: RepeatBlock | null;
  onChangeItems: (items: Item[]) => void;
  onChangeRepeat: (repeat: RepeatBlock | null) => void;
}

export default function VariableList({
  items,
  repeat,
  onChangeItems,
  onChangeRepeat,
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
  // 全部已用名字（自动命名去重用）
  const usedNames = new Set([...nameCount.keys()].filter((n) => n !== ""));

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
    const item = { ...makeItem(newKind), name };
    // 行类型：行内项默认名（n/m）也自动重命名为全局未用名，避免跨行重名
    if (Object.keys(newKind)[0] === "Line") {
      const line = (newKind as { Line: { rows: string; items: { name: string }[] } }).Line;
      const nextItems = line.items.map((it) => {
        while (used.has(name)) {
          n += 1;
          name = `v${n}`;
        }
        used.add(name);
        return { ...it, name };
      });
      item.kind = { Line: { ...line, items: nextItems } } as typeof item.kind;
    }
    onChangeItems([...items, item]);
  };

  const reorder = (from: number, to: number) => {
    if (from === to) return;
    const next = [...items];
    const [moved] = next.splice(from, 1);
    next.splice(to, 0, moved);
    onChangeItems(next);
  };

  /** 上/下移动一行（需求：变量顺序可调）。 */
  const move = (i: number, dir: -1 | 1) => reorder(i, i + dir);

  return (
    <div className="var-panel">
      <div className="var-toolbar">
        {repeat ? (
          <label className="repeat-box" title="repeat 块：整体重复 N 次，变量每轮覆盖；块内语句在 DSL 编辑器编辑">
            repeat (
            <input
              className="repeat-count"
              value={repeat.count}
              onChange={(e) => onChangeRepeat({ ...repeat, count: e.target.value })}
              placeholder="N"
              spellCheck={false}
            />
            )
            <button
              className="btn-secondary"
              onClick={() => onChangeRepeat(null)}
              title="移除 repeat 块（块内语句会丢失，请先在 DSL 编辑器保存）"
            >
              移除
            </button>
          </label>
        ) : (
          <button className="btn-secondary" onClick={() => onChangeRepeat({ count: "3", items: [] })}>
            ＋ repeat 块
          </button>
        )}
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
          index={i}
          total={items.length}
          onMove={move}
          item={item}
          dragging={overIndex === i}
          usedNames={usedNames}
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
