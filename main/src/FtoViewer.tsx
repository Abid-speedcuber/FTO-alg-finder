import { invertInputAlgorithm, resolveInputAlgorithm } from "./rotation";
import type { MutableRefObject, ReactNode } from "react";
import { useEffect, useRef, useState } from "react";

export type IgnoredStickerState = Array<{ color: number; mark: "fixed" | "top" | null } | null>;

export type FtoViewerApi = {
  applyAlgorithm(algorithm: string): void;
  applyAlgorithmInstant(algorithm: string): void;
  getFacelets(): number[];
  setFacelets(facelets: number[], ignoredStickerState?: IgnoredStickerState): void;
  getIgnoredStickerState(): IgnoredStickerState;
  setKeyboardEnabled(enabled: boolean): void;
  getFaceletTransition(algorithm: string): number[] | null;
  getCenterTargets(): CenterTargets;
  setCenterTargets(targets: CenterTargets): void;
  setPieceSelectionMode(mode: "undefine" | "entitle" | null): void;
  setMode(mode: "pan" | "paint" | "swap"): void;
  setColor(color: number): void;
  setFaceColors(colors: string[]): void;
  setLastLayerMode(enabled: boolean): void;
  resetPuzzle(): void;
  resetView(): void;
  dispose(): void;
};

declare global {
  interface Window {
    createFtoViewer?: (
      container: HTMLElement,
      options?: {
        keyboard?: boolean;
        leftClickUndefine?: boolean;
        resolveAlgorithm?: (algorithm: string) => string;
        faceColors?: string[];
        onFacelets?: (facelets: number[]) => void;
        onCenterTargets?: (targets: CenterTargets) => void;
      },
    ) => FtoViewerApi;
  }
}

export type CenterTargets = {
  uf: Array<number | null>;
  rl: Array<number | null>;
  ufSources: Array<number | null>;
  rlSources: Array<number | null>;
  rlTopSources: number[][];
  ufSlots?: number[][];
  rlSlots?: number[][];
};

type Props = {
  setup: string;
  inputMode: "setup" | "alg";
  applySignal: number;
  applyFromSolved?: boolean;
  lastLayerMode: boolean;
  onFacelets: (facelets: number[]) => void;
  onCenterTargets: (targets: CenterTargets) => void;
  faceColors?: string[];
  stateText?: string;
  stateInvalid?: boolean;
  onInvalidStateClick?: () => void;
  viewerApiRef?: MutableRefObject<FtoViewerApi | null>;
  footer?: ReactNode;
  active?: boolean;
  pieceSelection?: boolean | "undefine" | "entitle";
  applyBase?: Props["initialState"];
  initialState?: { facelets: number[]; centerTargets?: CenterTargets; ignoredStickerState?: IgnoredStickerState };
};

function FtoViewer({
  setup,
  inputMode,
  applySignal,
  applyFromSolved = false,
  lastLayerMode,
  onFacelets,
  onCenterTargets,
  faceColors = ["#ffff00", "#0000ff", "#ff0000", "#800080", "#ffffff", "#00a050", "#808080", "#ff8800"],
  stateText,
  stateInvalid = false,
  onInvalidStateClick,
  viewerApiRef,
  footer,
  active = true,
  initialState,
  applyBase,
  pieceSelection = false,
}: Props) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const viewerRef = useRef<FtoViewerApi | null>(null);
  const [inputError, setInputError] = useState("");
  const [mode, setModeState] = useState<"pan" | "paint" | "swap">("pan");
  const [selectedColor, setSelectedColorState] = useState(0);
  const activeRef = useRef(active);
  activeRef.current = active;

  useEffect(() => {
    if (!hostRef.current || !window.createFtoViewer) {
      return;
    }

    const viewer = window.createFtoViewer(hostRef.current, {
      faceColors,
      leftClickUndefine: Boolean(pieceSelection),
      resolveAlgorithm: resolveInputAlgorithm,
      onFacelets,
      onCenterTargets,
    });
    viewerRef.current = viewer;
    viewer.setKeyboardEnabled(activeRef.current && !pieceSelection);
    if (viewerApiRef) {
      viewerApiRef.current = viewer;
    }

    return () => {
      viewer.dispose();
      viewerRef.current = null;
      if (viewerApiRef && viewerApiRef.current === viewer) {
        viewerApiRef.current = null;
      }
    };
  }, [onFacelets, onCenterTargets, viewerApiRef]);

  useEffect(() => {
    viewerRef.current?.setMode(mode);
  }, [mode]);

  useEffect(() => {
    viewerRef.current?.setColor(selectedColor);
  }, [selectedColor]);

  useEffect(() => {
    viewerRef.current?.setFaceColors(faceColors);
  }, [faceColors]);

  useEffect(() => {
    viewerRef.current?.setLastLayerMode(lastLayerMode);
  }, [lastLayerMode]);

  useEffect(() => {
    viewerRef.current?.setPieceSelectionMode(pieceSelection ? pieceSelection === "entitle" ? "entitle" : "undefine" : null);
  }, [pieceSelection]);

  // Only the visible tab's viewer listens to the keyboard; refit on show.
  useEffect(() => {
    viewerRef.current?.setKeyboardEnabled(active && !pieceSelection);
    if (active) {
      window.dispatchEvent(new Event("resize"));
    }
  }, [active]);

  const initialStateRef = useRef(initialState);
  useEffect(() => {
    const initial = initialStateRef.current;
    if (initial) {
      viewerRef.current?.setFacelets(initial.facelets, initial.ignoredStickerState);
      if (initial.centerTargets) viewerRef.current?.setCenterTargets(initial.centerTargets);
    }
  }, []);

  const applyStateRef = useRef({ setup, inputMode, applyFromSolved, applyBase });
  applyStateRef.current = { setup, inputMode, applyFromSolved, applyBase };

  useEffect(() => {
    if (applySignal > 0) {
      const { setup: currentSetup, inputMode: currentMode, applyFromSolved: fromSolved, applyBase: base } = applyStateRef.current;
      try {
        const algorithm = currentMode === "alg" ? invertInputAlgorithm(currentSetup) : resolveInputAlgorithm(currentSetup);
        if (fromSolved) {
          viewerRef.current?.resetPuzzle();
          if (base) {
            viewerRef.current?.setFacelets(base.facelets, base.ignoredStickerState);
            if (base.centerTargets) viewerRef.current?.setCenterTargets(base.centerTargets);
          }
        }
        viewerRef.current?.applyAlgorithmInstant(algorithm);
        setInputError("");
      } catch (error) {
        setInputError(String(error instanceof Error ? error.message : error));
      }
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [applySignal]);

  function setPanMode() {
    setModeState("pan");
  }

  function setSwapMode() {
    setModeState("swap");
  }

  function selectColor(color: number) {
    setSelectedColorState(color);
    setModeState("paint");
  }

  function resetPuzzle() {
    viewerRef.current?.resetPuzzle();
  }

  function resetView() {
    viewerRef.current?.resetView();
  }

  return (
    <div className="fto-viewer-panel">
      <div className={`fto-stage${stateText ? " has-state" : ""}`}>
        <div ref={hostRef} className="fto-canvas-host" />
        {stateText && stateInvalid && onInvalidStateClick ? (
          <button type="button" className="viewer-state invalid" onClick={onInvalidStateClick}
            aria-label="Invalid state: show all problems" aria-haspopup="dialog">
            {stateText}
          </button>
        ) : stateText ? (
          <div className={stateInvalid ? "viewer-state invalid" : "viewer-state"}>
            {stateText}
          </div>
        ) : null}
        {!pieceSelection && <div className="viewer-controls viewer-controls-left">
          {faceColors.slice(0, 6).map((hex, index) => (
            <button
              key={hex}
              className={`swatch ${mode === "paint" && selectedColor === index ? "selected" : ""}`}
              onClick={() => selectColor(index)}
              style={{ ["--swatch" as string]: hex }}
              title={`Color ${index}`}
              aria-label={`Color ${index}`}
            />
          ))}
        </div>}
        <div className="viewer-controls viewer-controls-right">
          {!pieceSelection && faceColors.slice(6).map((hex, offset) => {
            const index = offset + 6;
            return (
              <button
                key={hex}
                className={`swatch ${mode === "paint" && selectedColor === index ? "selected" : ""}`}
                onClick={() => selectColor(index)}
                style={{ ["--swatch" as string]: hex }}
                title={`Color ${index}`}
                aria-label={`Color ${index}`}
              />
            );
          })}
          {!pieceSelection && <button className={mode === "pan" ? "selected" : ""} onClick={setPanMode}>Pan</button>}
          {!pieceSelection && <button className={mode === "swap" ? "selected" : ""} onClick={setSwapMode}>Swap</button>}
          <button onClick={resetView}>View</button>
          <button onClick={resetPuzzle}>{pieceSelection ? "Define all" : "Reset"}</button>
        </div>
      </div>
      {inputError && <p role="alert">{inputError}</p>}
      {footer}
    </div>
  );
}

export default FtoViewer;
