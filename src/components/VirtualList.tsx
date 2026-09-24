import { type ReactNode, useLayoutEffect, useRef, useState } from "react";

interface Props<T> {
  items: T[];
  rowHeight: number;
  height: number;
  render: (item: T, index: number) => ReactNode;
  /** Keep the view pinned to the newest item while the user is at the bottom. */
  followTail?: boolean;
  className?: string;
}

/** Minimal fixed-row-height virtualized list: only visible rows are rendered. */
export function VirtualList<T>({ items, rowHeight, height, render, followTail = true, className }: Props<T>) {
  const ref = useRef<HTMLDivElement>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const pinned = useRef(true);

  useLayoutEffect(() => {
    const el = ref.current;
    if (el && followTail && pinned.current) {
      el.scrollTop = el.scrollHeight;
      setScrollTop(el.scrollTop);
    }
  }, [items.length, followTail]);

  const overscan = 6;
  const first = Math.max(0, Math.floor(scrollTop / rowHeight) - overscan);
  const last = Math.min(items.length, Math.ceil((scrollTop + height) / rowHeight) + overscan);

  return (
    <div
      ref={ref}
      className={className}
      style={{ height, overflowY: "auto", position: "relative" }}
      onScroll={(e) => {
        const el = e.currentTarget;
        setScrollTop(el.scrollTop);
        pinned.current = el.scrollTop + el.clientHeight >= el.scrollHeight - rowHeight;
      }}
    >
      <div style={{ height: items.length * rowHeight, position: "relative" }}>
        {items.slice(first, last).map((item, i) => (
          <div
            key={first + i}
            style={{ position: "absolute", top: (first + i) * rowHeight, left: 0, right: 0, height: rowHeight }}
          >
            {render(item, first + i)}
          </div>
        ))}
      </div>
    </div>
  );
}
