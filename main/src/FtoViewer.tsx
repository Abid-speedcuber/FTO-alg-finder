import type { ReactNode } from "react";
import { useEffect, useRef, useState } from "react";

type FtoViewerApi = {
  applyAlgorithm(algorithm: string): void;
  applyAlgorithmInstant(algorithm: string): void;
  getFacelets(): number[];
  setMode(mode: "pan" | "paint"): void;
  setColor(color: number): void;
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
        onMove?: (history: string[]) => void;
        onFacelets?: (facelets: number[]) => void;
      },
    ) => FtoViewerApi;
  }
}

type Props = {
  setup: string;
  inputMode: "setup" | "alg";
  applySignal: number;
  onFacelets: (facelets: number[]) => void;
  footer?: ReactNode;
};

const colorHex = ["#ffff00", "#0000ff", "#ff0000", "#800080", "#ffffff", "#00a050", "#808080", "#ff8800"];

function FtoViewer({ setup, inputMode, applySignal, onFacelets, footer }: Props) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const viewerRef = useRef<FtoViewerApi | null>(null);
  const [history, setHistory] = useState<string[]>([]);
  const [mode, setModeState] = useState<"pan" | "paint">("pan");
  const [selectedColor, setSelectedColorState] = useState(0);

  useEffect(() => {
    if (!hostRef.current || !window.createFtoViewer) {
      return;
    }

    const viewer = window.createFtoViewer(hostRef.current, {
      onMove: setHistory,
      onFacelets,
    });
    viewerRef.current = viewer;

    return () => {
      viewer.dispose();
      viewerRef.current = null;
    };
  }, [onFacelets]);

  useEffect(() => {
    viewerRef.current?.setMode(mode);
  }, [mode]);

  useEffect(() => {
    viewerRef.current?.setColor(selectedColor);
  }, [selectedColor]);

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
    setHistory([]);
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
          <button onClick={resetPuzzle}>Reset</button>
        </div>
      </div>
      {footer}
      <div className="move-log">{history.slice(-40).join(" ")}</div>
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
