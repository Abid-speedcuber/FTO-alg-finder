import type { MutableRefObject, ReactNode } from "react";
import { useEffect, useRef, useState } from "react";

export type FtoViewerApi = {
  applyAlgorithm(algorithm: string): void;
  applyAlgorithmInstant(algorithm: string): void;
  getFacelets(): number[];
  getFaceletTransition(algorithm: string): number[] | null;
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
      },
    ) => FtoViewerApi;
  }
}

type Props = {
  scramble: string;
  scrambleSignal: number;
  apiRef: MutableRefObject<FtoViewerApi | null>;
  footer?: ReactNode;
};

const colorHex = ["#ffff00", "#0000ff", "#ff0000", "#800080", "#ffffff", "#00a050", "#808080", "#ff8800"];

function FtoViewer({ scramble, scrambleSignal, apiRef, footer }: Props) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const viewerRef = useRef<FtoViewerApi | null>(null);
  const [mode, setModeState] = useState<"pan" | "paint">("pan");
  const [selectedColor, setSelectedColorState] = useState(0);

  useEffect(() => {
    if (!hostRef.current || !window.createFtoViewer) {
      return;
    }

    const viewer = window.createFtoViewer(hostRef.current, {});
    viewerRef.current = viewer;
    apiRef.current = viewer;

    return () => {
      viewer.dispose();
      viewerRef.current = null;
      if (apiRef.current === viewer) {
        apiRef.current = null;
      }
    };
  }, [apiRef]);

  useEffect(() => {
    viewerRef.current?.setMode(mode);
  }, [mode]);

  useEffect(() => {
    viewerRef.current?.setColor(selectedColor);
  }, [selectedColor]);

  const applyStateRef = useRef(scramble);
  applyStateRef.current = scramble;

  useEffect(() => {
    if (scrambleSignal > 0) {
      viewerRef.current?.applyAlgorithmInstant(applyStateRef.current);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scrambleSignal]);

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

export default FtoViewer;