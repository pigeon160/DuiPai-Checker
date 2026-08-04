import { useRef, type ReactNode } from "react";

/** 可折叠、可拖拽调高的面板（自然高度，页面整体滚动）。 */
export function Panel({
  id,
  title,
  actions,
  basis,
  collapsed,
  onToggle,
  children,
}: {
  id: number;
  title: string;
  actions?: ReactNode;
  /** 拖拽后的固定高度（px）；null = 自然高度 */
  basis: number | null;
  collapsed: boolean;
  onToggle: () => void;
  children: ReactNode;
}) {
  const style: React.CSSProperties = {
    // 折叠时高度 = 标题栏（children 不渲染）；basis 仅在展开时生效
    flex: collapsed
      ? "0 0 auto"
      : basis != null
        ? `0 0 ${basis}px`
        : "0 0 auto",
  };
  return (
    <section
      id={`panel-${id}`}
      className={`panel${collapsed ? " collapsed" : ""}`}
      style={style}
    >
      <div className="panel-head" onClick={onToggle} title={collapsed ? "展开" : "折叠"}>
        <span className={`chevron ${collapsed ? "closed" : ""}`}>▼</span>
        <h2>{title}</h2>
        {actions && (
          <div className="head-actions" onClick={(e) => e.stopPropagation()}>
            {actions}
          </div>
        )}
      </div>
      {!collapsed && children}
    </section>
  );
}

/** 面板间分隔条：拖拽调上方面板高度，双击恢复弹性。 */
export function SplitHandle({
  onResize,
  onReset,
  disabled,
}: {
  onResize: (delta: number) => void;
  onReset: () => void;
  disabled?: boolean;
}) {
  const dragging = useRef(false);
  const lastY = useRef(0);

  const onMouseDown = (e: React.MouseEvent) => {
    if (disabled) return;
    e.preventDefault();
    dragging.current = true;
    lastY.current = e.clientY;
    const move = (ev: MouseEvent) => {
      if (!dragging.current) return;
      onResize(ev.clientY - lastY.current);
      lastY.current = ev.clientY;
    };
    const up = () => {
      dragging.current = false;
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };

  return (
    <div
      className={`split-handle${disabled ? " disabled" : ""}`}
      onMouseDown={onMouseDown}
      onDoubleClick={onReset}
      title="拖拽调整高度，双击恢复自动"
    >
      <div className="handle-grip" />
    </div>
  );
}
