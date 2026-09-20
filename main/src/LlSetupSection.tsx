// "Set up LL" sidebar section, shared by the Solver and Combiner tabs.

import type { MutableRefObject } from "react";
import { useEffect, useMemo, useRef, useState } from "react";
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
  lastLayerMode?: boolean;
};

function LlSetupSection({ facelets, viewerApiRef, lastLayerMode = false }: Props) {
  const [open, setOpen] = useState(false);
  const [selection, setSelection] = useState<LlSelection>(EMPTY_LL_SELECTION);
  const [pending, setPending] = useState<LlPlan | null>(null);
  const prevRef = useRef<number[] | null>(null);
  const expectedRef = useRef<number[] | null>(null);

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
    viewer.setFacelets(next);
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
      >
        <LlThumbnail facelets={preview ?? facelets} lastLayerMode={lastLayerMode} />
        <span>{option.label}</span>
      </button>
    );
  }

  return (
    <>
      <section className="tool-section ll-setup">
        <div className="section-heading">
          <h2>Set up LL</h2>
          <div className="mini-actions">
            <button
              className="chevron-toggle"
              aria-expanded={open}
              onClick={() => setOpen((value) => !value)}
            >
              {open ? "▴" : "▾"}
            </button>
          </div>
        </div>
        {open ? (
          <>
            <div className="ll-group">
              <span className="ll-group-title">EP</span>
              {EP_OPTIONS.length === 0 ? (
                <div className="hint">No EP cases defined yet (see llOptions.ts).</div>
              ) : (
                <div className="ll-option-grid">
                  {EP_OPTIONS.map((option) => renderOption("EP", option))}
                </div>
              )}
            </div>
            <div className="ll-group">
              <span className="ll-group-title">AUF</span>
              <div className="ll-auf">
                <button onClick={() => auf(1)}>U</button>
                <button onClick={() => auf(-1)}>U'</button>
              </div>
            </div>
            <div className="ll-group">
              <span className="ll-group-title">CP</span>
              {[1, 2].map((row) => {
                const rowOptions = CP_OPTIONS.filter((option) => option.row === row);
                if (rowOptions.length === 0) {
                  return null;
                }
                return (
                  <div className="ll-option-grid" key={row}>
                    {rowOptions.map((option) => renderOption("CP", option))}
                  </div>
                );
              })}
            </div>
          </>
        ) : null}
      </section>

      {pending ? (
        <div className="modal-backdrop">
          <div className="modal">
            <h2>Reset required</h2>
            <p className="modal-subtitle">
              This will reset {formatResets(pending.resets)} to solved. Continue?
            </p>
            <div className="modal-footer">
              <button className="secondary" onClick={() => setPending(null)}>
                Cancel
              </button>
              <button
                className="primary"
                onClick={() => {
                  commit(pending);
                  setPending(null);
                }}
              >
                Continue
              </button>
            </div>
          </div>
        </div>
      ) : null}
    </>
  );
}

export default LlSetupSection;
