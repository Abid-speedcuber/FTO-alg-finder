import { useEffect, useRef, useState } from "react";
import FtoViewer, { type CenterTargets, type FtoViewerApi, type IgnoredStickerState } from "./FtoViewer";

export type PruningGoal = { facelets: number[]; centerTargets: CenterTargets; ignoredStickerState?: IgnoredStickerState };
type Props = { initial: PruningGoal | null; lastLayer: boolean; faceColors: string[]; onSave: (goal: PruningGoal | null) => void; onClose: () => void; title?: string };

export default function PruningPieces({ initial, lastLayer, faceColors, onSave, onClose, title = "Defined pieces for this table" }: Props) {
  const dialog = useRef<HTMLDialogElement>(null);
  const viewer = useRef<FtoViewerApi | null>(null);
  const [selection, setSelection] = useState<"undefine" | "entitle">("undefine");
  const [facelets, setFacelets] = useState(initial?.facelets ?? Array.from({ length: 72 }, (_, i) => Math.floor(i / 9)));
  const [targets, setTargets] = useState<CenterTargets | null>(initial?.centerTargets ?? null);
  useEffect(() => { dialog.current?.showModal(); return () => dialog.current?.close(); }, []);
  function save() {
    const api = viewer.current;
    if (!api) return;
    const colors = api.getFacelets();
    const targets = api.getCenterTargets();
    const entitled = [...(targets.ufSlots ?? []), ...(targets.rlSlots ?? [])].some(slots => slots.length);
    onSave(colors.includes(8) || entitled ? { facelets: colors, centerTargets: api.getCenterTargets(), ignoredStickerState: api.getIgnoredStickerState() } : null);
  }
  return <dialog ref={dialog} className="pruning-pieces-modal" aria-labelledby="pruning-pieces-title" onCancel={onClose}>
    <header className="piece-dialog-header"><div><h2 id="pruning-pieces-title">{title}</h2>
      <p>Choose the pieces to keep and the triangle slots to fill.</p></div>
      <button type="button" className="piece-dialog-close" aria-label="Close piece selection" onClick={onClose}>×</button></header>
    <div className="piece-dialog-body">
    <aside className="piece-selection-tools"><div className="piece-tools-label">Selection tools</div>
    <div className="pruning-tabs" role="group" aria-label="Partial position controls">
      <button type="button" className={selection === "undefine" ? "primary" : "secondary"} aria-pressed={selection === "undefine"} onClick={() => setSelection("undefine")}>Undefine pieces</button>
      <button type="button" className={selection === "entitle" ? "primary" : "secondary"} aria-pressed={selection === "entitle"} onClick={() => setSelection("entitle")}>Entitle slots</button>
    </div>
    <p className="piece-selection-hint" id="piece-selection-hint">{selection === "undefine" ? "Click a piece to undefine or restore it. Shift-click a triangle slot to entitle it." : "Click a triangle slot to entitle it or remove its entitlement. Gold outlines mark the selected destinations."}</p>
    </aside>
    <FtoViewer setup="" inputMode="setup" applySignal={0} lastLayerMode={lastLayer} pieceSelection={selection}
      initialState={initial ?? undefined} faceColors={faceColors} viewerApiRef={viewer}
      onFacelets={setFacelets} onCenterTargets={setTargets} />
    </div>
    <footer className="piece-dialog-footer"><span>{facelets.includes(8) ? "Partial position" : "All pieces defined"}{[...(targets?.ufSlots ?? []), ...(targets?.rlSlots ?? [])].some(slots => slots.length) ? " · Slots entitled" : ""}</span>
      <div className="modal-footer"><button type="button" className="secondary" onClick={onClose}>Cancel</button><button type="button" className="primary" onClick={save}>Confirm</button></div>
    </footer>
  </dialog>;
}
