import { useEffect, useId, useRef, useState } from "react";

type Option = { value: string; label: string };
export default function BatchSelect({ label, value, options, disabled, onChange, displayLabel }: {
  label: string; value: string; displayLabel?: string; options: Option[]; disabled?: boolean; onChange: (value: string) => void;
}) {
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const id = useId();
  const [open, setOpen] = useState(false);
  const [index, setIndex] = useState(0);
  const selected = Math.max(0, options.findIndex(option => option.value === value));
  useEffect(() => {
    if (!open) return;
    function outside(event: PointerEvent) { if (!root.current?.contains(event.target as Node)) setOpen(false); }
    document.addEventListener("pointerdown", outside);
    root.current?.querySelector<HTMLElement>(`[data-option="${index}"]`)?.scrollIntoView({ block: "nearest" });
    return () => document.removeEventListener("pointerdown", outside);
  }, [open, index]);
  function choose(next: number) {
    if (options[next]) onChange(options[next].value);
    setOpen(false); trigger.current?.focus();
  }
  return <div className="batch-select" ref={root} onBlur={event => { if (!event.currentTarget.contains(event.relatedTarget as Node)) setOpen(false); }}>
    <button type="button" ref={trigger} className="batch-select-trigger" role="combobox" aria-label={label} disabled={disabled || !options.length}
      aria-haspopup="listbox" aria-expanded={open} aria-controls={open ? id : undefined} aria-activedescendant={open ? `${id}-${index}` : undefined}
      onClick={() => { setIndex(selected); setOpen(!open); }} onKeyDown={event => {
        if (["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
          event.preventDefault(); setOpen(true);
          setIndex(event.key === "Home" ? 0 : event.key === "End" ? options.length - 1 : !open ? selected : (index + (event.key === "ArrowDown" ? 1 : -1) + options.length) % options.length);
        } else if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); setOpen(false); }
        else if (open && ["Enter", " "].includes(event.key)) { event.preventDefault(); choose(index); }
        else if (event.key.length === 1 && !event.ctrlKey && !event.metaKey && event.key !== " ") {
          const next = options.findIndex((option, i) => i > index && option.label.toLowerCase().startsWith(event.key.toLowerCase()));
          const fallback = options.findIndex(option => option.label.toLowerCase().startsWith(event.key.toLowerCase()));
          if (next >= 0 || fallback >= 0) { event.preventDefault(); setOpen(true); setIndex(next >= 0 ? next : fallback); }
        }
      }}><span>{displayLabel ?? options.find(option => option.value === value)?.label ?? "Select an instance"}</span><span aria-hidden="true">⌄</span></button>
    {open && <ul id={id} role="listbox" aria-label={label} className="batch-select-options">
      {options.map((option, i) => <li key={option.value} id={`${id}-${i}`} role="option" aria-selected={option.value === value} data-option={i}
        className={i === index ? "is-active" : ""} onPointerMove={() => setIndex(i)} onMouseDown={event => event.preventDefault()} onClick={() => choose(i)}>
        <span>{option.label}</span><span aria-hidden="true">{option.value === value ? "✓" : ""}</span>
      </li>)}
    </ul>}
  </div>;
}
