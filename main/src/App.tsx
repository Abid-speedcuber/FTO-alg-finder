import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import FtoViewer, { type CenterTargets } from "./FtoViewer";

type CubieState = {
  cp: number[];
  co: number[];
  ep: number[];
  uf: number[];
  rl: number[];
};

type SolveResult = {
  nodes: number;
  solutions: string[];
};

type SolveLine = {
  kind: string;
  text: string;
};

type TerminalLine = {
  id: number;
  kind: string;
  text: string;
};

const moves = [
  "U", "U'", "F", "F'", "BR", "BR'", "BL", "BL'", "D", "D'", "B", "B'",
  "R", "R'", "L", "L'", "Uw", "Uw'", "Fw", "Fw'", "Rw", "Rw'", "Lw", "Lw'", "M", "M'",
  "(R U R')", "(R U' R')", "(R' U R)", "(R' U' R)",
];

const solved: CubieState = {
  cp: [0, 1, 2, 3, 4, 5],
  co: [0, 0, 0, 0, 0, 0],
  ep: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
  uf: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
  rl: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
};

function emptyCenterTargets(): CenterTargets {
  return {
    uf: [null, null, null, null],
    rl: [null, null, null, null],
    ufSources: [null, null, null, null],
    rlSources: [null, null, null, null],
    rlTopSources: [[], [], [], []],
  };
}

function App() {
  const [setup, setSetup] = useState("");
  const [inputMode, setInputMode] = useState<"setup" | "alg">("setup");
  const [applySignal, setApplySignal] = useState(0);
  const [facelets, setFacelets] = useState<number[]>([]);
  const [centerTargets, setCenterTargets] = useState<CenterTargets>(emptyCenterTargets);
  const [cubieState, setCubieState] = useState<CubieState | null>(solved);
  const [stateError, setStateError] = useState("");
  const [banned, setBanned] = useState<Set<string>>(new Set());
  const [depth, setDepth] = useState("");
  const [all, setAll] = useState(true);
  const [restrictedPruning, setRestrictedPruning] = useState(false);
  const [lastLayerMode, setLastLayerMode] = useState(false);
  const [threads, setThreads] = useState("1");
  const [status, setStatus] = useState("Idle");
  const [result, setResult] = useState<SolveResult | null>(null);
  const [running, setRunning] = useState(false);
  const [terminal, setTerminal] = useState<TerminalLine[]>([]);
  const validationRun = useRef(0);
  const lineId = useRef(0);
  const terminalRef = useRef<HTMLDivElement | null>(null);

  const allowedMoves = useMemo(() => moves.filter((move) => !banned.has(move)), [banned]);
  const stateLabel = stateError ? "Invalid state" : "Valid state";

  const handleFacelets = useCallback((nextFacelets: number[]) => {
    setFacelets(nextFacelets);
  }, []);

  const appendLine = useCallback((kind: string, text: string) => {
    const id = ++lineId.current;
    setTerminal((lines) => {
      const next = [...lines, { id, kind, text }];
      return next.length > 500 ? next.slice(next.length - 500) : next;
    });
  }, []);

  const clearTerminal = useCallback(() => {
    setTerminal([]);
  }, []);

  useEffect(() => {
    if (facelets.length !== 72) {
      return;
    }
    const run = ++validationRun.current;
    invoke<CubieState>("validate_facelets", { facelets })
      .then((state) => {
        if (run !== validationRun.current) {
          return;
        }
        setCubieState(state);
        setStateError("");
      })
      .catch((error) => {
        if (run !== validationRun.current) {
          return;
        }
        setCubieState(null);
        setStateError(String(error));
      });
  }, [facelets]);

  useEffect(() => {
    let disposed = false;
    let unlisteners: (() => void)[] = [];
    (async () => {
      const fns = await Promise.all([
        listen<SolveLine>("solve-line", (event) => {
          appendLine(event.payload.kind, event.payload.text);
        }),
        listen<SolveResult>("solve-result", (event) => {
          setResult(event.payload);
          setRunning(false);
          setStatus(event.payload.solutions.length ? "Done" : "No solution");
        }),
        listen<string>("solve-error", (event) => {
          appendLine("error", `error: ${event.payload}`);
          setRunning(false);
          setStatus(String(event.payload));
        }),
        listen("solve-cancelled", () => {
          appendLine("cancel", "solve cancelled");
          setRunning(false);
          setStatus("Stopped");
        }),
      ]);
      if (disposed) {
        fns.forEach((fn) => fn());
      } else {
        unlisteners = fns;
      }
    })();
    return () => {
      disposed = true;
      unlisteners.forEach((fn) => fn());
    };
  }, [appendLine]);

  useEffect(() => {
    if (terminalRef.current) {
      terminalRef.current.scrollTop = terminalRef.current.scrollHeight;
    }
  }, [terminal]);

  function toggleMove(move: string) {
    setBanned((current) => {
      const next = new Set(current);
      if (next.has(move)) {
        next.delete(move);
      } else {
        next.add(move);
      }
      return next;
    });
  }

  function allowAllMoves() {
    setBanned(new Set());
  }

  function invertMoveSelection() {
    setBanned((current) => new Set(moves.filter((move) => !current.has(move))));
  }

  async function solve() {
    if (!cubieState || running) {
      return;
    }
    const trimmedDepth = depth.trim();
    if (trimmedDepth && !/^\d+$/.test(trimmedDepth)) {
      setStatus("Depth must be empty or numeric");
      return;
    }
    setRunning(true);
    setStatus("Solving");
    setResult(null);
    clearTerminal();
    appendLine(
      "info",
      `solve (depth=${trimmedDepth || "auto"} all=${all ? "on" : "off"} threads=${threads})`,
    );
    try {
      await invoke("solve_fto", {
        request: {
          scramble: null,
          state: null,
          facelets,
          centerTargets,
          allowedMoves,
          maxDepth: trimmedDepth ? Number(trimmedDepth) : null,
          findAll: all,
          restrictedPruning,
          lastLayerMode,
          threads: Number(threads),
        },
      });
    } catch (error) {
      appendLine("error", `error: ${String(error)}`);
      setStatus(String(error));
      setRunning(false);
    }
  }

  async function stopSolve() {
    await invoke("stop_solve");
    setStatus("Stopping");
  }

  return (
    <main>
      <header className="app-header">
        <div>
          <h1>FTO Alg Finder</h1>
          <p>By Abid Ibn Ashraf</p>
        </div>
        <div className="status-strip">
          <span className={`pill ${running ? "pill-running" : ""}`}>{status}</span>
          <span className={`pill ${stateError ? "pill-bad" : "pill-good"}`}>{stateLabel}</span>
          <span className="pill">{allowedMoves.length}/26 moves</span>
          {result ? <span className="pill">{result.nodes.toLocaleString()} nodes</span> : null}
        </div>
      </header>

      <section className="workspace">
        <div className="input-pane">
          <div className="setup-row">
            <label className="field">
              <span className="input-mode-toggle">
                <button
                  type="button"
                  className={inputMode === "setup" ? "active" : ""}
                  onClick={() => setInputMode("setup")}
                >
                  Setup Move
                </button>
                <span>/</span>
                <button
                  type="button"
                  className={inputMode === "alg" ? "active" : ""}
                  onClick={() => setInputMode("alg")}
                >
                  alg
                </button>
              </span>
              <input
                value={setup}
                onChange={(event) => setSetup(event.target.value)}
                spellCheck={false}
                autoComplete="off"
              />
            </label>
            <button className="secondary" onClick={() => setApplySignal((current) => current + 1)}>Apply</button>
          </div>

          <FtoViewer
            setup={setup}
            inputMode={inputMode}
            applySignal={applySignal}
            lastLayerMode={lastLayerMode}
            onFacelets={handleFacelets}
            onCenterTargets={setCenterTargets}
            footer={
              <div className="terminal" ref={terminalRef}>
                {terminal.length === 0 ? (
                  <div className="terminal-placeholder">solutions and solver logs will appear here</div>
                ) : (
                  terminal.map((line) => (
                    <div key={line.id} className={`terminal-line terminal-${line.kind}`}>
                      {line.text}
                    </div>
                  ))
                )}
              </div>
            }
          />
        </div>

        <div className="control-pane">
          <section className="tool-section">
            <div className="section-heading">
              <h2>Move Set</h2>
              <div className="mini-actions">
                <button onClick={allowAllMoves}>All</button>
                <button onClick={invertMoveSelection}>Invert</button>
              </div>
            </div>
            <div className="move-grid">
              {moves.map((move) => (
                <button
                  key={move}
                  className={banned.has(move) ? "move-toggle banned" : "move-toggle"}
                  onClick={() => toggleMove(move)}
                >
                  {move}
                </button>
              ))}
            </div>
          </section>

          <section className="tool-section solve-options">
            <div className="section-heading">
              <h2>Search</h2>
            </div>
            <div className="search-grid">
            <label>
              Depth
              <input
                value={depth}
                onChange={(event) => setDepth(event.target.value.replace(/\D/g, ""))}
                inputMode="numeric"
                placeholder="auto"
              />
            </label>
            <label>
              Threads
              <input value={threads} onChange={(event) => setThreads(event.target.value)} inputMode="numeric" />
            </label>
            </div>
            <div className="option-list">
            <label className="check">
              <input type="checkbox" checked={all} onChange={(event) => setAll(event.target.checked)} />
              All solutions
            </label>
            <label className="check">
              <input
                type="checkbox"
                checked={lastLayerMode}
                onChange={(event) => setLastLayerMode(event.target.checked)}
              />
              Last layer mode
            </label>
            <label className="check">
              <input
                type="checkbox"
                checked={restrictedPruning}
                onChange={(event) => setRestrictedPruning(event.target.checked)}
              />
              Restricted pruning
            </label>
            </div>
            <div className="actions-row">
            <button
              className={running ? "primary stop" : "primary"}
              onClick={running ? stopSolve : solve}
              disabled={!running && (!cubieState || allowedMoves.length === 0)}
            >
              {running ? "Stop" : "Solve"}
            </button>
            </div>
            {stateError ? <div className="state-error">{stateError}</div> : <div className="state-ok">valid FTO state</div>}
          </section>
        </div>
      </section>
    </main>
  );
}

export default App;
