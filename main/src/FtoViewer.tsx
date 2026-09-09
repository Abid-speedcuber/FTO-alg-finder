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
  applySignal: number;
  onFacelets: (facelets: number[]) => void;
};

const colorHex = ["#ffffff", "#ff8800", "#ffff00", "#00ff00", "#0000ff", "#ff0000", "#800080", "#00ffff"];

function FtoViewer({ setup, applySignal, onFacelets }: Props) {
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
      viewerRef.current?.applyAlgorithmInstant(setup);
    }
  }, [applySignal, setup]);

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
      <div ref={hostRef} className="fto-canvas-host" />
      <div className="viewer-controls">
        {colorHex.map((hex, index) => (
          <button
            key={hex}
            className={`swatch ${mode === "paint" && selectedColor === index ? "selected" : ""}`}
            onClick={() => selectColor(index)}
            style={{ ["--swatch" as string]: hex }}
            title={`Color ${index}`}
            aria-label={`Color ${index}`}
          />
        ))}
        <button className={mode === "pan" ? "selected" : ""} onClick={setPanMode}>Pan</button>
        <button onClick={resetPuzzle}>Reset</button>
      </div>
      <div className="move-log">{history.slice(-40).join(" ")}</div>
    </div>
  );
}

export default FtoViewer;
