import type { ReactNode } from "react";
import { useEffect, useRef, useState } from "react";

type FtoViewerApi = {
  applyAlgorithm(algorithm: string): void;
  applyAlgorithmInstant(algorithm: string): void;
  getFacelets(): number[];
  getCenterTargets(): CenterTargets;
  setMode(mode: "pan" | "paint"): void;
  setColor(color: number): void;
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
  footer?: ReactNode;
};

const colorHex = ["#ffff00", "#0000ff", "#ff0000", "#800080", "#ffffff", "#00a050", "#808080", "#ff8800"];

function FtoViewer({ setup, inputMode, applySignal, lastLayerMode, onFacelets, onCenterTargets, footer }: Props) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const viewerRef = useRef<FtoViewerApi | null>(null);
  const [mode, setModeState] = useState<"pan" | "paint">("pan");
  const [selectedColor, setSelectedColorState] = useState(0);

  useEffect(() => {
    if (!hostRef.current || !window.createFtoViewer) {
      return;
    }

    const viewer = window.createFtoViewer(hostRef.current, {
      onFacelets,
      onCenterTargets,
    });
    viewerRef.current = viewer;

    return () => {
      viewer.dispose();
      viewerRef.current = null;
    };
  }, [onFacelets, onCenterTargets]);

  useEffect(() => {
    viewerRef.current?.setMode(mode);
  }, [mode]);

  useEffect(() => {
    viewerRef.current?.setColor(selectedColor);
  }, [selectedColor]);

  useEffect(() => {
    viewerRef.current?.setLastLayerMode(lastLayerMode);
  }, [lastLayerMode]);

  useEffect(() => {
    if (applySignal > 0) {
      viewerRef.current?.applyAlgorithmInstant(inputMode === "alg" ? invertAlgorithm(setup) : setup);
    }
  }, [applySignal, inputMode, setup]);

  function setPanMode() {
    setModeState("pan");
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
        <div className="viewer-controls viewer-controls-left">
          {colorHex.slice(0, 5).map((hex, index) => (
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
          {colorHex.slice(5).map((hex, offset) => {
            const index = offset + 5;
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
