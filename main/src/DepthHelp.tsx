import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

export default function DepthHelp() {
  const [open, setOpen] = useState(false);
  const [position, setPosition] = useState({ top: 0, left: 0 });
  const button = useRef<HTMLButtonElement>(null);
  const popup = useRef<HTMLDivElement>(null);
  const pinned = useRef(false);
  const closeTimer = useRef<ReturnType<typeof setTimeout>>();

  function cancelClose() { clearTimeout(closeTimer.current); }
  function show() { cancelClose(); setOpen(true); }
  function close() { cancelClose(); pinned.current = false; setOpen(false); }
  function leave() {
    if (!pinned.current) closeTimer.current = setTimeout(close, 150);
  }

  useLayoutEffect(() => {
    if (!open) return;
    function place() {
      if (!button.current || !popup.current) return;
      const anchor = button.current.getBoundingClientRect();
      const box = popup.current.getBoundingClientRect();
      const left = Math.max(8, Math.min(anchor.right - box.width, window.innerWidth - box.width - 8));
      const top = anchor.top >= box.height + 16 ? anchor.top - box.height - 8 : anchor.bottom + 8;
      setPosition({ left, top: Math.max(8, Math.min(top, window.innerHeight - box.height - 8)) });
    }
    place();
    window.addEventListener("resize", place);
    window.addEventListener("scroll", place, true);
    return () => {
      window.removeEventListener("resize", place);
      window.removeEventListener("scroll", place, true);
    };
  }, [open]);

  useEffect(() => {
    if (!open) return;
    function dismiss(event: PointerEvent) {
      if (!button.current?.contains(event.target as Node) && !popup.current?.contains(event.target as Node)) close();
    }
    function escape(event: KeyboardEvent) {
      if (event.key === "Escape") { event.stopPropagation(); close(); }
    }
    document.addEventListener("pointerdown", dismiss);
    document.addEventListener("keydown", escape, true);
    return () => {
      document.removeEventListener("pointerdown", dismiss);
      document.removeEventListener("keydown", escape, true);
    };
  }, [open]);

  useEffect(() => () => clearTimeout(closeTimer.current), []);

  return <>
    <button ref={button} className="depth-info-button" type="button" aria-label="About depth selection"
      aria-expanded={open} aria-controls={open ? "depth-help-popup" : undefined}
      aria-describedby={open ? "depth-help-popup" : undefined}
      onMouseEnter={show} onMouseLeave={leave} onBlur={close}
      onClick={(event) => { event.currentTarget.focus(); if (pinned.current) close(); else { pinned.current = true; show(); } }}>i</button>
    {open ? createPortal(
      <div ref={popup} id="depth-help-popup" role="tooltip" className="depth-help-popup" style={position}
        onMouseEnter={cancelClose} onMouseLeave={leave}>
        <p><code>12-15, 17, 20</code>: depths and inclusive ranges (0–255).</p>
        <p><strong>All:</strong> every selected depth. <strong>Single:</strong> lowest first, stop on success.</p>
        <p><code>auto</code> or blank: first successful depth.</p>
      </div>, document.body,
    ) : null}
  </>;
}
