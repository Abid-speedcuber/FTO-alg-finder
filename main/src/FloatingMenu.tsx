import { useEffect, useLayoutEffect, useRef, type ReactNode, type RefObject } from "react";
import { createPortal } from "react-dom";

// A portal escapes the control pane's scroll clipping; z-index alone cannot.
export default function FloatingMenu({ anchor, onClose, children }: {
  anchor: RefObject<HTMLButtonElement>; onClose: () => void; children: ReactNode;
}) {
  const panel = useRef<HTMLDivElement>(null);
  const dismiss = useRef(onClose);
  dismiss.current = onClose;
  useLayoutEffect(() => {
    function place() {
      const button = anchor.current, menu = panel.current;
      if (!button || !menu) return;
      if (!button.getClientRects().length) { dismiss.current(); return; }
      const scale = menu.getBoundingClientRect().width / menu.offsetWidth || 1;
      const gap = 8;
      menu.style.width = `${Math.min(280, (window.innerWidth - gap * 2) / scale)}px`;
      menu.style.maxHeight = `${(window.innerHeight - gap * 2) / scale}px`;
      const rect = button.getBoundingClientRect();
      const box = menu.getBoundingClientRect();
      const left = Math.max(gap, Math.min(rect.right - box.width, window.innerWidth - box.width - gap));
      const below = rect.bottom + gap;
      const top = below + box.height <= window.innerHeight - gap ? below
        : Math.max(gap, Math.min(rect.top - box.height - gap, window.innerHeight - box.height - gap));
      menu.style.left = `${left / scale}px`;
      menu.style.top = `${top / scale}px`;
    }
    place();
    window.addEventListener("resize", place);
    window.addEventListener("scroll", place, true);
    return () => { window.removeEventListener("resize", place); window.removeEventListener("scroll", place, true); };
  }, [anchor]);
  useEffect(() => {
    function escape(event: KeyboardEvent) {
      if (event.key !== "Escape") return;
      event.preventDefault(); event.stopPropagation(); dismiss.current(); anchor.current?.focus();
    }
    document.addEventListener("keydown", escape, true);
    return () => document.removeEventListener("keydown", escape, true);
  }, [anchor]);
  return createPortal(<>
    <div className="menu-backdrop" onClick={onClose} />
    <div ref={panel} className="instance-menu-panel floating-instance-menu" role="group" aria-label="Solver instances">{children}</div>
  </>, document.body);
}
