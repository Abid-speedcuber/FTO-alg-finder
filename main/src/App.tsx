import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import AlgCombiner from "./AlgCombiner";
import FtoViewer, { type CenterTargets, type FtoViewerApi } from "./FtoViewer";
import LlSetupSection from "./LlSetupSection";

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
  "S", "S'", "E", "E'",
  "(R U R')", "(R U' R')", "(R' U R)", "(R' U' R)",
  "(F U F')", "(F U' F')", "(F' U F)", "(F' U' F)",
];

const INSTANCES_KEY = "fto.instances.v1";

type Instance = {
  id: string;
  name: string;
  moves: string[];
  banned: string[];
};

type SetupModalState =
  | { kind: "first"; instance: Instance }
  | { kind: "new" }
  | null;

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

function newInstanceId(): string {
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 9)}`;
}

function bootstrapInstances(): { instances: Instance[]; activeId: string; firstRun: boolean } {
  try {
    const raw = localStorage.getItem(INSTANCES_KEY);
    if (raw) {
      const parsed = JSON.parse(raw) as { instances?: unknown[]; activeId?: unknown };
      if (Array.isArray(parsed.instances)) {
        const instances = parsed.instances.filter(
          (item): item is Instance =>
            !!item &&
            typeof (item as Instance).id === "string" &&
            typeof (item as Instance).name === "string" &&
            Array.isArray((item as Instance).moves) &&
            Array.isArray((item as Instance).banned),
        );
        if (instances.length > 0) {
          const ids = new Set(instances.map((instance) => instance.id));
          const activeId =
            typeof parsed.activeId === "string" && ids.has(parsed.activeId)
              ? parsed.activeId
              : instances[0].id;
          return { instances, activeId, firstRun: false };
        }
      }
    }
  } catch {
    // fall through to fresh bootstrap
  }
  const id = "default";
  return {
    instances: [{ id, name: "Default", moves: [...moves], banned: [] }],
    activeId: id,
    firstRun: true,
  };
}

function InstanceSetupModal({
  title,
  subtitle,
  initialName,
  initialMoves,
  confirmLabel,
  onSave,
  onCancel,
}: {
  title: string;
  subtitle?: string;
  initialName: string;
  initialMoves: string[];
  confirmLabel: string;
  onSave: (name: string, selected: string[]) => void;
  onCancel: () => void;
}) {
  const [name, setName] = useState(initialName);
  const [selected, setSelected] = useState<Set<string>>(() => new Set(initialMoves));

  const toggleMove = (move: string) => {
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(move)) {
        next.delete(move);
      } else {
        next.add(move);
      }
      return next;
    });
  };

  const canSave = name.trim().length > 0 && selected.size > 0;

  return (
    <div className="modal-backdrop">
      <div className="modal">
        <h2>{title}</h2>
        {subtitle ? <p className="modal-subtitle">{subtitle}</p> : null}
        <label className="field">
          <span>Name</span>
          <input
            value={name}
            onChange={(event) => setName(event.target.value)}
            spellCheck={false}
            autoComplete="off"
            placeholder="Instance name"
          />
        </label>
        <div className="modal-moves-heading">
          <div className="field">
            <span>Moves this instance uses</span>
          </div>
          <div className="mini-actions">
            <button className="secondary" onClick={() => setSelected(new Set(moves))}>
              All
            </button>
            <button className="secondary" onClick={() => setSelected(new Set())}>
              None
            </button>
          </div>
        </div>
        <div className="modal-move-grid">
          {moves.map((move) => (
            <button
              key={move}
              className={selected.has(move) ? "move-toggle" : "move-toggle banned"}
              onClick={() => toggleMove(move)}
            >
              {move}
            </button>
          ))}
        </div>
        <div className="modal-footer">
          <button className="secondary" onClick={onCancel}>
            Cancel
          </button>
          <button
            className="primary"
            disabled={!canSave}
            onClick={() => onSave(name, moves.filter((move) => selected.has(move)))}
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}

function App() {
  const [mode, setMode] = useState<"solver" | "combiner">("solver");
  const [boot] = useState(bootstrapInstances);
  const [instances, setInstances] = useState<Instance[]>(boot.instances);
  const [activeId, setActiveId] = useState(boot.activeId);
  const [setupModal, setSetupModal] = useState<SetupModalState>(
    boot.firstRun ? { kind: "first", instance: boot.instances[0] } : null,
  );
  const [menuOpen, setMenuOpen] = useState(false);
  const [contextMenu, setContextMenu] = useState<{ id: string; x: number; y: number } | null>(null);

  const [setup, setSetup] = useState("");
  const [inputMode, setInputMode] = useState<"setup" | "alg">("setup");
  const [applySignal, setApplySignal] = useState(0);
  const [facelets, setFacelets] = useState<number[]>([]);
  const [centerTargets, setCenterTargets] = useState<CenterTargets>(emptyCenterTargets);
  const [cubieState, setCubieState] = useState<CubieState | null>(solved);
  const [stateError, setStateError] = useState("");
  const [depth, setDepth] = useState("");
  const [all, setAll] = useState(true);
  const [restrictedPruning, setRestrictedPruning] = useState(false);
  const [lastLayerMode, setLastLayerMode] = useState(false);
  const viewerApiRef = useRef<FtoViewerApi | null>(null);
  const [threads, setThreads] = useState("1");
  const [status, setStatus] = useState("Idle");
  const [result, setResult] = useState<SolveResult | null>(null);
  const [running, setRunning] = useState(false);
  const [terminal, setTerminal] = useState<TerminalLine[]>([]);
  const validationRun = useRef(0);
  const lineId = useRef(0);
  const terminalRef = useRef<HTMLDivElement | null>(null);

  const activeInstance = instances.find((instance) => instance.id === activeId) ?? instances[0];
  const bannedSet = useMemo(() => new Set(activeInstance.banned), [activeInstance]);
  const allowedMoves = useMemo(
    () => activeInstance.moves.filter((move) => !bannedSet.has(move)),
    [activeInstance, bannedSet],
  );
  const stateLabel = stateError ? "Invalid state" : "Valid state";

  useEffect(() => {
    try {
      localStorage.setItem(INSTANCES_KEY, JSON.stringify({ instances, activeId }));
    } catch {
      // storage unavailable; keep in-memory instances
    }
  }, [instances, activeId]);

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

  function updateInstance(id: string, patch: Partial<Instance>) {
    setInstances((list) => list.map((instance) => (instance.id === id ? { ...instance, ...patch } : instance)));
  }

  function toggleMove(move: string) {
    const id = activeInstance.id;
    setInstances((list) =>
      list.map((instance) => {
        if (instance.id !== id) {
          return instance;
        }
        const banned = instance.banned.includes(move)
          ? instance.banned.filter((entry) => entry !== move)
          : [...instance.banned, move];
        return { ...instance, banned };
      }),
    );
  }

  function allowAllMoves() {
    updateInstance(activeInstance.id, { banned: [] });
  }

  function invertMoveSelection() {
    updateInstance(activeInstance.id, {
      banned: activeInstance.moves.filter((move) => !activeInstance.banned.includes(move)),
    });
  }

  function createInstance(name: string, selected: string[]) {
    const instance: Instance = {
      id: newInstanceId(),
      name: name.trim(),
      moves: selected,
      banned: [],
    };
    setInstances((list) => [...list, instance]);
    setActiveId(instance.id);
  }

  function deleteInstance(id: string) {
    if (instances.length <= 1) {
      return;
    }
    setInstances((list) => list.filter((instance) => instance.id !== id));
    setActiveId((current) => {
      if (current !== id) {
        return current;
      }
      return instances.find((instance) => instance.id !== id)?.id ?? instances[0].id;
    });
  }

  function openContextMenu(instanceId: string, event: React.MouseEvent) {
    event.preventDefault();
    setMenuOpen(false);
    setContextMenu({ id: instanceId, x: event.clientX, y: event.clientY });
  }

  function closeOverlays() {
    setMenuOpen(false);
    setContextMenu(null);
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
      `solve (instance=${activeInstance.name} depth=${trimmedDepth || "auto"} all=${all ? "on" : "off"} threads=${threads})`,
    );
    try {
      await invoke("solve_fto", {
        request: {
          scramble: null,
          state: null,
          facelets,
          centerTargets,
          allowedMoves,
          instanceMoves: activeInstance.moves,
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

  const contextInstance = contextMenu
    ? instances.find((instance) => instance.id === contextMenu.id)
    : undefined;

  return (
    <main>
      <header className="app-header">
        <div>
          <h1>{mode === "combiner" ? "FTO Alg Combiner" : "FTO Alg Finder"}</h1>
          <p>By Abid Ibn Ashraf</p>
        </div>
        <div className="topbar-right">
          <div className="mode-switch">
            <button
              type="button"
              className={mode === "solver" ? "active" : ""}
              onClick={() => setMode("solver")}
            >
              Solver
            </button>
            <button
              type="button"
              className={mode === "combiner" ? "active" : ""}
              onClick={() => setMode("combiner")}
            >
              Combiner
            </button>
          </div>
          {mode === "solver" ? (
            <>
              <div className="status-strip">
                <span className={`pill ${running ? "pill-running" : ""}`}>{status}</span>
                <span className={`pill ${stateError ? "pill-bad" : "pill-good"}`}>{stateLabel}</span>
                <span className="pill">
                  {allowedMoves.length}/{activeInstance.moves.length} moves
                </span>
                {result ? <span className="pill">{result.nodes.toLocaleString()} nodes</span> : null}
              </div>
              <div className="instance-menu">
                <button className="instance-trigger" onClick={() => setMenuOpen((open) => !open)}>
                  <span className="instance-trigger-name">{activeInstance.name}</span>
                  <span className="instance-caret">▾</span>
                </button>
                {menuOpen ? (
                  <>
                    <div className="menu-backdrop" onClick={closeOverlays} />
                    <div className="instance-menu-panel">
                      <div className="instance-menu-label">Instances</div>
                      {instances.map((instance) => (
                        <button
                          key={instance.id}
                          className={instance.id === activeId ? "instance-item active" : "instance-item"}
                          onClick={() => {
                            setActiveId(instance.id);
                            setMenuOpen(false);
                          }}
                          onContextMenu={(event) => openContextMenu(instance.id, event)}
                          title="Right-click to delete"
                        >
                          <span className="instance-item-name">{instance.name}</span>
                          <span className="instance-item-count">{instance.moves.length} moves</span>
                        </button>
                      ))}
                      <div className="instance-menu-divider" />
                      <button
                        className="instance-item new"
                        onClick={() => {
                          setSetupModal({ kind: "new" });
                          setMenuOpen(false);
                        }}
                      >
                        + New instance
                      </button>
                    </div>
                  </>
                ) : null}
              </div>
            </>
          ) : null}
        </div>
        {mode === "solver" && contextMenu ? (
          <>
            <div className="menu-backdrop" onClick={closeOverlays} />
            <div className="context-menu" style={{ left: contextMenu.x, top: contextMenu.y }}>
              <button
                className="context-delete"
                disabled={instances.length <= 1}
                onClick={() => {
                  deleteInstance(contextMenu.id);
                  closeOverlays();
                }}
              >
                Delete {contextInstance?.name ?? "instance"}
              </button>
            </div>
          </>
        ) : null}
      </header>

      {mode === "solver" && setupModal ? (
        setupModal.kind === "first" ? (
          <InstanceSetupModal
            title="Set up your default instance"
            subtitle="Pick the moves you'll actually use. This instance's base pruning table is built only from these moves, and only they are exposed in the move panel."
            initialName={setupModal.instance.name}
            initialMoves={setupModal.instance.moves}
            confirmLabel="Save"
            onSave={(name, selected) => {
              updateInstance(setupModal.instance.id, { name, moves: selected, banned: [] });
              setSetupModal(null);
            }}
            onCancel={() => setSetupModal(null)}
          />
        ) : (
          <InstanceSetupModal
            title="New instance"
            subtitle="Pick a name and the moves this instance will use. Its base pruning table is built only from these moves."
            initialName=""
            initialMoves={[...moves]}
            confirmLabel="Create"
            onSave={(name, selected) => {
              createInstance(name, selected);
              setSetupModal(null);
            }}
            onCancel={() => setSetupModal(null)}
          />
        )
      ) : null}

      <div hidden={mode !== "solver"}>
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
            viewerApiRef={viewerApiRef}
            active={mode === "solver"}
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
              <h2>Move Set ({activeInstance.name})</h2>
              <div className="mini-actions">
                <button onClick={allowAllMoves}>All</button>
                <button onClick={invertMoveSelection}>Invert</button>
              </div>
            </div>
            {activeInstance.moves.length === 0 ? (
              <div className="state-error">This instance has no moves selected. Edit it from the instance menu.</div>
            ) : (
              <div className="move-grid">
                {activeInstance.moves.map((move) => (
                  <button
                    key={move}
                    className={bannedSet.has(move) ? "move-toggle banned" : "move-toggle"}
                    onClick={() => toggleMove(move)}
                  >
                    {move}
                  </button>
                ))}
              </div>
            )}
          </section>

          <LlSetupSection
            facelets={facelets}
            viewerApiRef={viewerApiRef}
            lastLayerMode={lastLayerMode}
          />

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
      </div>
      <div hidden={mode !== "combiner"}>
        <AlgCombiner active={mode === "combiner"} />
      </div>
    </main>
  );
}

export default App;
