import { useRef, useState } from "react";
import type { Item, LineItem, VarKind } from "../api";
import VariableRow from "./VariableRow";
import { KIND_ORDER, makeItem } from "../kindMeta";

interface Props {
  items: Item[];
  onChangeItems: (items: Item[]) => void;
}

/** 收集某作用域内的名字（repeat 块的子项不计入——作用域隔离）。 */
function collectNames(list: Item[], out: Map<string, number>) {
  for (const it of list) {
    const key = Object.keys(it.kind)[0];
    if (key === "Line") {
      for (const p of (it.kind as { Line: { items: LineItem[] } }).Line.items) {
        out.set(p.name, (out.get(p.name) ?? 0) + 1);
      }
    } else if (key !== "Repeat") {
      out.set(it.name, (out.get(it.name) ?? 0) + 1);
    }
  }
}

function nextVarName(used: Set<string>): string {
  let n = 1;
  let name = "v1";
  while (used.has(name)) {
    n += 1;
    name = `v${n}`;
  }
  return name;
}

/** repeat 块行：N 输入 + 块内变量列表（可添加/编辑/移动/删除）。 */
function RepeatRow({
  item,
  index,
  total,
  onMove,
  onChange,
  onDelete,
}: {
  item: Item;
  index: number;
  total: number;
  onMove: (i: number, dir: -1 | 1) => void;
  onChange: (it: Item) => void;
  onDelete: () => void;
}) {
  const rep = (item.kind as { Repeat: { count: string; items: Item[] } }).Repeat;
  const [newKind, setNewKind] = useState<VarKind>(KIND_ORDER[1].kind);
  const dragIndex = useRef<number>(-1);
  const [overIndex, setOverIndex] = useState<number>(-1);
  const sub = rep.items;
  const setSub = (next: Item[]) =>
    onChange({ ...item, kind: { Repeat: { ...rep, items: next } } });

  // 块内名字统计（只算本块：块内变量与块外可重名）
  const subNames = new Map<string, number>();
  collectNames(sub, subNames);
  const subTaken = (n: string) => (subNames.get(n) ?? 0) > 1;
  const subUsed = new Set([...subNames.keys()].filter((n) => n !== ""));

  const addSub = () => {
    const name = nextVarName(subUsed);
    const ni = { ...makeItem(newKind), name };
    // 行类型：行内项默认名也去重
    if (Object.keys(newKind)[0] === "Line") {
      const line = (newKind as { Line: { rows: string; items: { name: string }[] } }).Line;
      const used = new Set(subUsed);
      used.add(name);
      const nextItems = line.items.map((it) => {
        const nm = nextVarName(used);
        used.add(nm);
        return { ...it, name: nm };
      });
      ni.kind = { Line: { ...line, items: nextItems } } as typeof ni.kind;
    }
    setSub([...sub, ni]);
  };

  const subMove = (i: number, dir: -1 | 1) => {
    const next = [...sub];
    const [m] = next.splice(i, 1);
    next.splice(i + dir, 0, m);
    setSub(next);
  };

  return (
    <div className="var-row repeat-row">
      <span className="move-btns">
        <button className="move-btn" disabled={index === 0} onClick={() => onMove(index, -1)} title="上移">
          ↑
        </button>
        <button className="move-btn" disabled={index === total - 1} onClick={() => onMove(index, 1)} title="下移">
          ↓
        </button>
      </span>
      <div className="line-head">
        <span className="kind-badge" style={{ background: "#FEF3C7", color: "#B45309" }}>
          repeat
        </span>
        <span className="repeat-label">repeat (</span>
        <input
          className="field-input small"
          value={rep.count}
          onChange={(e) =>
            onChange({ ...item, kind: { Repeat: { ...rep, count: e.target.value } } })
          }
          placeholder="N"
          title="重复次数（表达式）"
          spellCheck={false}
        />
        <span className="repeat-label">):</span>
        <span className="repeat-hint">块内变量每轮覆盖，块外不可见</span>
        <button className="del-btn" onClick={onDelete} title="删除 repeat 块">✕</button>
      </div>
      <div className="repeat-body">
        {sub.map((si, j) => (
          <VariableRow
            key={j}
            index={j}
            total={sub.length}
            onMove={subMove}
            item={si}
            dragging={overIndex === j}
            usedNames={subUsed}
            onName={(name) => setSub(sub.map((q, k) => (k === j ? { ...q, name } : q)))}
            onKind={(kind) => setSub(sub.map((q, k) => (k === j ? { ...q, kind } : q)))}
            onDelete={() => setSub(sub.filter((_, k) => k !== j))}
            nameTaken={subTaken}
            dragProps={{
              onDragStart: (e) => {
                dragIndex.current = j;
                e.dataTransfer.effectAllowed = "move";
                e.dataTransfer.setData("text/plain", String(j));
              },
              onDragOver: (e) => {
                e.preventDefault();
                e.dataTransfer.dropEffect = "move";
                if (overIndex !== j) setOverIndex(j);
              },
              onDrop: (e) => {
                e.preventDefault();
                const from = dragIndex.current;
                if (from >= 0) {
                  const next = [...sub];
                  const [m] = next.splice(from, 1);
                  next.splice(j, 0, m);
                  setSub(next);
                }
                setOverIndex(-1);
              },
              onDragEnd: () => {
                dragIndex.current = -1;
                setOverIndex(-1);
              },
            }}
          />
        ))}
        <div className="repeat-add">
          <select
            value={JSON.stringify(newKind)}
            onChange={(e) => setNewKind(JSON.parse(e.target.value) as VarKind)}
          >
            {KIND_ORDER.filter((o) => Object.keys(o.kind)[0] !== "Repeat").map((o) => (
              <option key={o.label} value={JSON.stringify(o.kind)}>
                {o.label}
              </option>
            ))}
          </select>
          <button className="btn-primary" onClick={addSub}>＋ 块内添加</button>
        </div>
      </div>
    </div>
  );
}

export default function VariableList({ items, onChangeItems }: Props) {
  const dragIndex = useRef<number>(-1);
  const [overIndex, setOverIndex] = useState<number>(-1);
  const [newKind, setNewKind] = useState<VarKind>(KIND_ORDER[1].kind);

  // 顶层作用域重名检测（repeat 子项不计入）
  const nameCount = new Map<string, number>();
  collectNames(items, nameCount);
  const nameTaken = (n: string) => (nameCount.get(n) ?? 0) > 1;
  const usedNames = new Set([...nameCount.keys()].filter((n) => n !== ""));

  const addItem = () => {
    const used = new Set(usedNames);
    const name = nextVarName(used);
    const item = { ...makeItem(newKind), name };
    if (Object.keys(newKind)[0] === "Line") {
      const line = (newKind as { Line: { rows: string; items: { name: string }[] } }).Line;
      used.add(name);
      const nextItems = line.items.map((it) => {
        const nm = nextVarName(used);
        used.add(nm);
        return { ...it, name: nm };
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

  const move = (i: number, dir: -1 | 1) => reorder(i, i + dir);

  return (
    <div className="var-panel">
      <div className="var-toolbar">
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

      {items.map((item, i) =>
        Object.keys(item.kind)[0] === "Repeat" ? (
          <RepeatRow
            key={i}
            item={item}
            index={i}
            total={items.length}
            onMove={move}
            onChange={(it) => onChangeItems(items.map((q, j) => (j === i ? it : q)))}
            onDelete={() => onChangeItems(items.filter((_, j) => j !== i))}
          />
        ) : (
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
        ),
      )}
    </div>
  );
}
