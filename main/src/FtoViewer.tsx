import type { MutableRefObject, ReactNode } from "react";
import { useEffect, useRef, useState } from "react";

export type FtoViewerApi = {
  applyAlgorithm(algorithm: string): void;
  applyAlgorithmInstant(algorithm: string): void;
  getFacelets(): number[];
  setFacelets(facelets: number[]): void;
  setKeyboardEnabled(enabled: boolean): void;
  getFaceletTransition(algorithm: string): number[] | null;
  getCenterTargets(): CenterTargets;
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
};

type Props = {
  setup: string;
  inputMode: "setup" | "alg";
  applySignal: number;
  lastLayerMode: boolean;
  onFacelets: (facelets: number[]) => void;
  onCenterTargets: (targets: CenterTargets) => void;
  faceColors?: string[];
  stateText?: string;
  stateInvalid?: boolean;
  viewerApiRef?: MutableRefObject<FtoViewerApi | null>;
  footer?: ReactNode;
  active?: boolean;
};

function FtoViewer({
  setup,
  inputMode,
  applySignal,
  lastLayerMode,
  onFacelets,
  onCenterTargets,
  faceColors = ["#ffff00", "#0000ff", "#ff0000", "#800080", "#ffffff", "#00a050", "#808080", "#ff8800"],
  stateText,
  stateInvalid = false,
  viewerApiRef,
  footer,
  active = true,
}: Props) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const viewerRef = useRef<FtoViewerApi | null>(null);
  const [mode, setModeState] = useState<"pan" | "paint" | "swap">("pan");
  const [selectedColor, setSelectedColorState] = useState(0);
  const activeRef = useRef(active);
  activeRef.current = active;

  useEffect(() => {
    if (!hostRef.current || !window.createFtoViewer) {
      return;
    }

    const viewer = window.createFtoViewer(hostRef.current, {
      onFacelets,
      onCenterTargets,
    });
    viewerRef.current = viewer;
    viewer.setKeyboardEnabled(activeRef.current);
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

  // Only the visible tab's viewer listens to the keyboard; refit on show.
  useEffect(() => {
    viewerRef.current?.setKeyboardEnabled(active);
    if (active) {
      window.dispatchEvent(new Event("resize"));
    }
  }, [active]);

  const applyStateRef = useRef({ setup, inputMode });
  applyStateRef.current = { setup, inputMode };

  useEffect(() => {
    if (applySignal > 0) {
      const { setup: currentSetup, inputMode: currentMode } = applyStateRef.current;
      viewerRef.current?.applyAlgorithmInstant(
        currentMode === "alg" ? invertAlgorithm(currentSetup) : currentSetup,
      );
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
      <div className="fto-stage">
        <div ref={hostRef} className="fto-canvas-host" />
        {stateText ? (
          <div className={stateInvalid ? "viewer-state invalid" : "viewer-state"}>
            {stateText}
          </div>
        ) : null}
        <div className="viewer-controls viewer-controls-left">
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
        </div>
        <div className="viewer-controls viewer-controls-right">
          {faceColors.slice(6).map((hex, offset) => {
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
          <button className={mode === "pan" ? "selected" : ""} onClick={setPanMode}>Pan</button>
          <button className={mode === "swap" ? "selected" : ""} onClick={setSwapMode}>Swap</button>
          <button onClick={resetView}>View</button>
          <button onClick={resetPuzzle}>Reset</button>
        </div>
      </div>
      {footer}
    </div>
  );
}

function invertAlgorithm(algorithm: string) {
  return algorithm
    .trim()
    .split(/\s+/)
    .filter(Boolean)
    .reverse()
    .map(invertMove)
    .join(" ");
}

function invertMove(move: string) {
  if (move.endsWith("'")) {
    return move.slice(0, -1);
  }
  if (move.endsWith("i")) {
    return move.slice(0, -1);
  }
  return `${move}'`;
}

export default FtoViewer;
