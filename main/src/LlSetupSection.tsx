// "Set up LL" sidebar section, shared by the Solver and Combiner tabs.

import type { MutableRefObject } from "react";
import { useEffect, useId, useLayoutEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { FtoViewerApi } from "./FtoViewer";
import LlThumbnail from "./LlThumbnail";
import { CP_OPTIONS, EP_OPTIONS } from "./llOptions";
import {
  applyU,
  applyUPrime,
  EMPTY_LL_SELECTION,
  formatResets,
  planLlOption,
  sameFacelets,
  type CpOption,
  type EpOption,
  type LlGroup,
  type LlPlan,
  type LlSelection,
} from "./llSetup";

type Props = {
  facelets: number[];
  viewerApiRef: MutableRefObject<FtoViewerApi | null>;
};

function LlSetupSection({ facelets, viewerApiRef }: Props) {
  const [open, setOpen] = useState(false);
  const [group, setGroup] = useState<LlGroup>("EP");
  const pickerId = useId();
  const trigger = useRef<HTMLButtonElement>(null);
  const picker = useRef<HTMLDivElement>(null);
  const tabs = useRef<Array<HTMLButtonElement | null>>([]);
  const caseList = useRef<HTMLDivElement>(null);
  const cancelReset = useRef<HTMLButtonElement>(null);
  const [selection, setSelection] = useState<LlSelection>(EMPTY_LL_SELECTION);
  const [pending, setPending] = useState<LlPlan | null>(null);
  const prevRef = useRef<number[] | null>(null);
  const expectedRef = useRef<number[] | null>(null);

  function closePicker(restoreFocus = false) {
    setOpen(false);
    setPending(null);
    if (restoreFocus) trigger.current?.focus();
  }

  useLayoutEffect(() => {
    if (!open) return;
    function position() {
      const panel = picker.current;
      const anchor = trigger.current;
      if (!panel || !anchor) return;
      if (!anchor.getClientRects().length) { closePicker(); return; }
      const rect = anchor.getBoundingClientRect();
      const scale = panel.getBoundingClientRect().width / panel.offsetWidth || 1;
      const gap = 12;
      panel.style.width = `${Math.min(360, (window.innerWidth - 2 * gap) / scale)}px`;
      panel.style.maxHeight = `${Math.min(560, (window.innerHeight - 2 * gap) / scale)}px`;
      const box = panel.getBoundingClientRect();
      const left = rect.left - box.width - gap >= gap
        ? rect.left - box.width - gap
        : rect.right + box.width + gap <= window.innerWidth - gap
          ? rect.right + gap
          : Math.max(gap, Math.min(rect.right - box.width, window.innerWidth - box.width - gap));
      panel.style.left = `${left / scale}px`;
      panel.style.top = `${Math.max(gap, Math.min(rect.top, window.innerHeight - box.height - gap)) / scale}px`;
    }
    position();
    tabs.current[group === "EP" ? 0 : 1]?.focus();
    window.addEventListener("resize", position);
    window.addEventListener("scroll", position, true);
    return () => {
      window.removeEventListener("resize", position);
      window.removeEventListener("scroll", position, true);
    };
  }, [open, group, pending]);

  useEffect(() => {
    if (!open) return;
    function outside(event: PointerEvent) {
      if (!picker.current?.contains(event.target as Node) && !trigger.current?.contains(event.target as Node)) closePicker();
    }
    function escape(event: KeyboardEvent) {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      if (pending) setPending(null);
      else closePicker(true);
    }
    document.addEventListener("pointerdown", outside);
    document.addEventListener("keydown", escape, true);
    return () => {
      document.removeEventListener("pointerdown", outside);
      document.removeEventListener("keydown", escape, true);
    };
  }, [open, pending]);

  useLayoutEffect(() => {
    if (!open) return;
    if (pending) cancelReset.current?.focus();
    else if (!picker.current?.contains(document.activeElement)) tabs.current[group === "EP" ? 0 : 1]?.focus();
  }, [open, pending]);

  function selectGroup(next: LlGroup) {
    setGroup(next);
    caseList.current?.scrollTo(0, 0);
  }

  // Outside edits clear the selections; our own writes and bare AUFs don't.
  useEffect(() => {
    const previous = prevRef.current;
    const expected = expectedRef.current;
    prevRef.current = facelets;
    expectedRef.current = null;
    const preserved =
      sameFacelets(facelets, expected) ||
      (previous !== null &&
        previous.length === 72 &&
        (sameFacelets(facelets, previous) ||
          sameFacelets(facelets, applyU(previous)) ||
          sameFacelets(facelets, applyUPrime(previous))));
    if (!preserved) {
      setSelection(EMPTY_LL_SELECTION);
    }
  }, [facelets]);

  const previews = useMemo(() => {
    const map = new Map<string, number[]>();
    if (!open || facelets.length !== 72) {
      return map;
    }
    for (const option of EP_OPTIONS) {
      map.set(`EP:${option.id}`, planLlOption(facelets, selection, "EP", option).next);
    }
    for (const option of CP_OPTIONS) {
      map.set(`CP:${option.id}`, planLlOption(facelets, selection, "CP", option).next);
    }
    return map;
  }, [open, facelets, selection]);

  function commit(plan: LlPlan) {
    const viewer = viewerApiRef.current;
    if (!viewer) {
      return;
    }
    expectedRef.current = plan.next;
    viewer.setFacelets(plan.next);
    setSelection(plan.selection);
  }

  function choose(group: LlGroup, option: EpOption | CpOption) {
    if (facelets.length !== 72) {
      return;
    }
    const plan = planLlOption(facelets, selection, group, option);
    if (plan.resets.length > 0) {
      setPending(plan);
      return;
    }
    commit(plan);
  }

  function auf(direction: 1 | -1) {
    const viewer = viewerApiRef.current;
    if (!viewer || facelets.length !== 72) {
      return;
    }
    const next = direction === 1 ? applyU(facelets) : applyUPrime(facelets);
    expectedRef.current = next;
    viewer.applyAlgorithmInstant(direction === 1 ? "U" : "U'");
  }

  function renderOption(group: LlGroup, option: EpOption | CpOption) {
    const selected = selection[group] === option.id;
    const preview = selected ? facelets : previews.get(`${group}:${option.id}`);
    return (
      <button
        key={option.id}
        className={selected ? "ll-option selected" : "ll-option"}
        onClick={() => choose(group, option)}
        title={option.label}
        type="button"
        aria-pressed={selected}
      >
        <LlThumbnail facelets={preview ?? facelets} />
        <span>{option.label}</span>
      </button>
    );
  }

  return (
    <>
      <section className="tool-section ll-setup">
        <div className="section-heading">
          <h2>Set up LL</h2>
          <button ref={trigger} type="button" className="ll-picker-trigger"
            aria-expanded={open} aria-controls={open ? pickerId : undefined} aria-haspopup="dialog"
            onClick={() => { if (open) closePicker(); else setOpen(true); }}>
            Choose cases
          </button>
        </div>
      </section>

      {open && createPortal(
        <div ref={picker} id={pickerId} role="dialog" aria-labelledby={`${pickerId}-title`}
          className="ll-case-picker" onKeyDown={(event) => event.stopPropagation()}>
          <div className="ll-picker-header">
            <h2 id={`${pickerId}-title`}>Set up LL</h2>
            <button type="button" className="ll-picker-close" onClick={() => closePicker(true)}>Close</button>
          </div>
          <div className="ll-picker-toolbar">
            <div role="tablist" aria-label="Last-layer cases" className="ll-picker-tabs">
              {(["EP", "CP"] as const).map((value, index) => <button key={value}
                ref={(element) => { tabs.current[index] = element; }}
                type="button" role="tab" id={`${pickerId}-${value}`}
                aria-selected={group === value} aria-controls={`${pickerId}-cases`}
                tabIndex={group === value ? 0 : -1} disabled={pending !== null}
                onClick={() => selectGroup(value)}
                onKeyDown={(event) => {
                  if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
                  event.preventDefault();
                  const next = event.key === "Home" ? 0 : event.key === "End" ? 1 : 1 - index;
                  selectGroup(next === 0 ? "EP" : "CP");
                  tabs.current[next]?.focus();
                }}>
                {value === "EP" ? "L6X" : "L3C"}
              </button>)}
            </div>
            <div className="ll-picker-auf" role="group" aria-label="Adjust U alignment">
              <span>AUF</span>
              <button type="button" disabled={pending !== null} onClick={() => auf(1)}>U</button>
              <button type="button" disabled={pending !== null} onClick={() => auf(-1)}>U'</button>
            </div>
          </div>
          <div ref={caseList} className="ll-picker-content" role="tabpanel" id={`${pickerId}-cases`} aria-labelledby={`${pickerId}-${group}`}>
            {pending ? <div className="ll-picker-confirm" role="alert" aria-labelledby={`${pickerId}-reset`}>
              <h3 id={`${pickerId}-reset`}>Reset required</h3>
              <p>This will reset {formatResets(pending.resets)} to solved. Continue?</p>
              <div className="ll-picker-confirm-actions">
                <button ref={cancelReset} type="button" onClick={() => setPending(null)}>Cancel</button>
                <button type="button" className="primary" onClick={() => { commit(pending); setPending(null); }}>Continue</button>
              </div>
            </div> : <div>
              {group === "EP" ? <div className="ll-option-grid">
                {EP_OPTIONS.map((option) => renderOption("EP", option))}
              </div> : <div className="ll-option-grid ll-corner-grid">
                {CP_OPTIONS.map((option) => renderOption("CP", option))}
              </div>}
            </div>}
          </div>
        </div>,
        trigger.current?.closest("dialog") ?? document.body,
      )}
    </>
  );
}

export default LlSetupSection;
