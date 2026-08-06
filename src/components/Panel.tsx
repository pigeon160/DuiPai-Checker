import { type ReactNode } from "react";

/** 可折叠面板（自然高度，页面整体滚动）。点击标题折叠/展开。 */
export function Panel({
  id,
  title,
  actions,
  collapsed,
  onToggle,
  children,
}: {
  id: number;
  title: string;
  actions?: ReactNode;
  collapsed: boolean;
  onToggle: () => void;
  children: ReactNode;
}) {
  return (
    <section id={`panel-${id}`} className={`panel${collapsed ? " collapsed" : ""}`}>
      <div className="panel-head" onClick={onToggle} title={collapsed ? "展开" : "折叠"}>
        <span className="panel-accent" />
        <span className={`chevron ${collapsed ? "closed" : ""}`}>▸</span>
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

/** 面板间纯视觉分隔线（不可拖拽）。 */
export function SplitHandle() {
  return (
    <div className="split-handle">
      <div className="handle-grip" />
    </div>
  );
}
