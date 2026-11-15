import { useEffect, useRef, useState } from "react";
import FtoViewer, { type CenterTargets, type FtoViewerApi } from "./FtoViewer";

export type PruningGoal = { facelets: number[]; centerTargets: CenterTargets };
type Props = { initial: PruningGoal | null; lastLayer: boolean; faceColors: string[]; onSave: (goal: PruningGoal | null) => void; onClose: () => void };

export default function PruningPieces({ initial, lastLayer, faceColors, onSave, onClose }: Props) {
  const dialog = useRef<HTMLDialogElement>(null);
  const viewer = useRef<FtoViewerApi | null>(null);
  const [facelets, setFacelets] = useState(initial?.facelets ?? Array.from({ length: 72 }, (_, i) => Math.floor(i / 9)));
  const [, setTargets] = useState<CenterTargets | null>(initial?.centerTargets ?? null);
  useEffect(() => { dialog.current?.showModal(); return () => dialog.current?.close(); }, []);
  function save() {
    const api = viewer.current;
    if (!api) return;
    const colors = api.getFacelets();
    onSave(colors.includes(8) ? { facelets: colors, centerTargets: api.getCenterTargets() } : null);
  }
  return <dialog ref={dialog} className="pruning-pieces-modal" aria-labelledby="pruning-pieces-title" onCancel={onClose}>
    <div className="modal-title-row"><h2 id="pruning-pieces-title">Defined pieces for this table</h2><button className="secondary" onClick={onClose}>Cancel</button></div>
    <p>Left-click a piece to undefine it. Click again to restore it.</p>
    <FtoViewer setup="" inputMode="setup" applySignal={0} lastLayerMode={lastLayer} pieceSelection
      initialState={initial ?? undefined} faceColors={faceColors} viewerApiRef={viewer}
      onFacelets={setFacelets} onCenterTargets={setTargets} />
    <div className="modal-footer"><button className="primary" onClick={save}>Confirm</button></div>
  </dialog>;
}
