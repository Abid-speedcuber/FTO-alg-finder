import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import FtoViewer from "./FtoViewer";

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
];

const solved: CubieState = {
  cp: [0, 1, 2, 3, 4, 5],
  co: [0, 0, 0, 0, 0, 0],
  ep: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
  uf: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
  rl: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
};

function App() {
  const [setup, setSetup] = useState("R U B");
  const [applySignal, setApplySignal] = useState(0);
  const [facelets, setFacelets] = useState<number[]>([]);
  const [cubieState, setCubieState] = useState<CubieState | null>(solved);
  const [stateError, setStateError] = useState("");
  const [banned, setBanned] = useState<Set<string>>(new Set());
  const [depth, setDepth] = useState("8");
  const [exact, setExact] = useState(true);
  const [all, setAll] = useState(false);
  const [restrictedPruning, setRestrictedPruning] = useState(false);
  const [threads, setThreads] = useState("1");
  const [status, setStatus] = useState("Idle");
  const [result, setResult] = useState<SolveResult | null>(null);
  const [running, setRunning] = useState(false);
  const [terminal, setTerminal] = useState<TerminalLine[]>([]);
  const validationRun = useRef(0);
  const lineId = useRef(0);
  const terminalRef = useRef<HTMLDivElement | null>(null);

  const allowedMoves = useMemo(() => moves.filter((move) => !banned.has(move)), [banned]);

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

  async function solve() {
    if (!cubieState || running) {
      return;
    }
    setRunning(true);
    setStatus("Solving");
    setResult(null);
    clearTerminal();
    appendLine(
      "info",
      `solve (depth=${depth} exact=${exact ? "on" : "off"} all=${all ? "on" : "off"} threads=${threads})`,
    );
    try {
      await invoke("solve_fto", {
        request: {
          scramble: null,
          state: null,
          facelets,
          allowedMoves,
          maxDepth: Number(depth),
          exact,
          findAll: all,
          restrictedPruning,
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
      <section className="workspace">
        <div className="input-pane">
          <header>
            <h1>Optimal FTO Solver</h1>
            <div className="status">{status}</div>
          </header>

          <div className="setup-row">
            <label className="field">
              <span>Setup moves</span>
              <input
                value={setup}
                onChange={(event) => setSetup(event.target.value)}
                spellCheck={false}
                autoComplete="off"
              />
            </label>
            <button onClick={() => setApplySignal((current) => current + 1)}>Apply</button>
          </div>

          <FtoViewer
            setup={setup}
            applySignal={applySignal}
            onFacelets={handleFacelets}
            footer={
              <div className="terminal" ref={terminalRef}>
                {terminal.map((line) => (
                  <div key={line.id} className={`terminal-line terminal-${line.kind}`}>
                    {line.text}
                  </div>
                ))}
              </div>
            }
          />
        </div>

        <div className="control-pane">
          <section>
            <h2>Moves</h2>
            <div className="move-grid">
              {moves.map((move) => (
                <button
                  key={move}
                  className={banned.has(move) ? "banned" : ""}
                  onClick={() => toggleMove(move)}
                >
                  {move}
                </button>
              ))}
            </div>
          </section>

          <section className="solve-options">
            <h2>Search</h2>
            <label>
              Depth
              <input value={depth} onChange={(event) => setDepth(event.target.value)} inputMode="numeric" />
            </label>
            <label>
              Threads
              <input value={threads} onChange={(event) => setThreads(event.target.value)} inputMode="numeric" />
            </label>
            <label className="check">
              <input type="checkbox" checked={exact} onChange={(event) => setExact(event.target.checked)} />
              Exact depth
            </label>
            <label className="check">
              <input type="checkbox" checked={all} onChange={(event) => setAll(event.target.checked)} />
              All solutions
            </label>
            <label className="check">
              <input
                type="checkbox"
                checked={restrictedPruning}
                onChange={(event) => setRestrictedPruning(event.target.checked)}
              />
              Restricted pruning
            </label>
            <button
              className="primary"
              onClick={running ? stopSolve : solve}
              disabled={!running && (!cubieState || allowedMoves.length === 0)}
            >
              {running ? "Stop" : "Solve"}
            </button>
            {stateError ? <div className="state-error">{stateError}</div> : <div className="state-ok">valid FTO state</div>}
          </section>
        </div>
      </section>
    </main>
  );
}

export default App;