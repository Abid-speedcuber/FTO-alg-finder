import { useCallback, useMemo, useRef, useState } from "react";
import FtoViewer, { type FtoViewerApi } from "./FtoViewer";
import LlSetupSection from "./LlSetupSection";
import { DEFAULT_INTERMEDIATE_ALGS } from "./algs";
import {
  MoveEngine,
  cancelTokens,
  countCombinations,
  generateCombos,
  isSolved,
  parseIntermediateAlgs,
  serializeIntermediateAlgs,
  tokenizeAlg,
} from "./combine";

type TerminalLine = {
  id: number;
  kind: string;
  text: string;
};

type Solution = {
  from: string[];
  alg: string;
  rawText: string;
  rawMoves: number;
  moves: number;
};

function AlgCombiner({ active = true }: { active?: boolean }) {
  const [scramble, setScramble] = useState("");
  const [scrambleSignal, setScrambleSignal] = useState(0);
  const [algText, setAlgText] = useState(() => serializeIntermediateAlgs(DEFAULT_INTERMEDIATE_ALGS));
  const [useTriples, setUseTriples] = useState(false);
  const [running, setRunning] = useState(false);
  const [status, setStatus] = useState("Idle");
  const [terminal, setTerminal] = useState<TerminalLine[]>([]);
  const [solutions, setSolutions] = useState<Solution[]>([]);

  const apiRef = useRef<FtoViewerApi | null>(null);
  const cancelRef = useRef(false);
  const foundRef = useRef<Solution[]>([]);
  const lineId = useRef(0);
  const terminalRef = useRef<HTMLDivElement | null>(null);

  const algs = useMemo(() => parseIntermediateAlgs(algText), [algText]);
  const algNames = useMemo(() => Object.keys(algs), [algs]);
  const counts = useMemo(() => countCombinations(algNames.length, useTriples), [algNames.length, useTriples]);

  const [facelets, setFacelets] = useState<number[]>([]);
  const handleFacelets = useCallback((next: number[]) => setFacelets(next), []);
  const handleCenterTargets = useCallback(() => {}, []);

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

  const applyScramble = useCallback(() => {
    if (!scramble.trim()) {
      return;
    }
    setScrambleSignal((current) => current + 1);
  }, [scramble]);

  const resetAlgText = useCallback(() => {
    setAlgText(serializeIntermediateAlgs(DEFAULT_INTERMEDIATE_ALGS));
  }, []);

  async function runSearch() {
    const viewer = apiRef.current;
    if (!viewer || running) {
      return;
    }
    const names = Object.keys(algs);
    if (names.length < 1) {
      appendLine("error", "need at least 1 intermediate alg (one per line, name: moves)");
      return;
    }

    const engine = new MoveEngine((token) => viewer.getFaceletTransition(token));
    const unknown = new Set<string>();
    for (const name of names) {
      for (const token of tokenizeAlg(algs[name])) {
        if (!engine.transition(token)) {
          unknown.add(token);
        }
      }
    }
    if (unknown.size > 0) {
      appendLine("error", `unknown moves: ${[...unknown].join(", ")}`);
      return;
    }

    const useT = useTriples;
    const total = countCombinations(names.length, useT);
    const base = viewer.getFacelets();
    const gen = generateCombos(algs, useT);

    cancelRef.current = false;
    foundRef.current = [];
    setSolutions([]);
    clearTerminal();
    setRunning(true);
    setStatus("Searching");
    appendLine("info", `intermediate algs: ${names.length}`);
    appendLine(
      "info",
      `combos to try: ${total.singles.toLocaleString()} single(s) + ${total.pairs.toLocaleString()} pair(s)${useT ? ` + ${total.triples.toLocaleString()} triple(s)` : ""}`,
    );
    if (isSolved(base)) {
      appendLine("info", "note: the on-screen puzzle is already solved");
    }

    const startedAt = performance.now();
    let tested = 0;
    let sinceProgress = 0;
    let done = false;

    try {
      while (!done && !cancelRef.current) {
        const batchEnd = tested + 2000;
        while (tested < batchEnd) {
          const next = gen.next();
          if (next.done) {
            done = true;
            break;
          }
          const candidate = next.value;
          tested += 1;
          sinceProgress += 1;
          if (engine.solves(base, candidate.tokens)) {
            const cancelled = cancelTokens(candidate.tokens);
            const solution: Solution = {
              from: candidate.names,
              alg: cancelled.join(" "),
              rawText: candidate.text,
              rawMoves: candidate.moves,
              moves: cancelled.length,
            };
            foundRef.current.push(solution);
            setSolutions((prev) => [...prev, solution]);
            appendLine(
              "solution",
              `found (${cancelled.length} moves, from ${candidate.moves}): ${cancelled.join(" ")}  | algs: ${candidate.names.join(" + ")}  | raw: ${candidate.text}`,
            );
          }
          if (sinceProgress >= 25000) {
            sinceProgress = 0;
            appendLine("progress", `... tested ${tested.toLocaleString()} combinations`);
          }
        }
        if (!done && !cancelRef.current) {
          await new Promise((resolve) => setTimeout(resolve, 0));
        }
      }
    } catch (error) {
      appendLine("error", `error: ${String(error)}`);
    }

    const elapsed = (performance.now() - startedAt) / 1000;
    setRunning(false);

    if (cancelRef.current) {
      appendLine("cancel", `stopped after ${tested.toLocaleString()} combinations`);
      setStatus("Stopped");
      return;
    }

    appendLine("info", `tested ${tested.toLocaleString()} combinations in ${elapsed.toFixed(2)}s`);
    const found = foundRef.current
      .slice()
      .sort((a, b) => a.moves - b.moves || a.alg.localeCompare(b.alg));
    setSolutions(found);
    if (found.length === 0) {
      appendLine("done", "not found");
      setStatus("Not found");
    } else {
      appendLine("done", `found ${found.length} solution${found.length === 1 ? "" : "s"}`);
      setStatus("Done");
    }
  }

  function stopSearch() {
    cancelRef.current = true;
    setStatus("Stopping");
  }

  function applySolution(solution: Solution) {
    apiRef.current?.applyAlgorithm(solution.alg);
  }

  return (
    <>
      <section className="workspace">
        <div className="input-pane">
          <div className="setup-row">
            <label className="field">
              <span>Scramble (applied to the puzzle)</span>
              <input
                value={scramble}
                onChange={(event) => setScramble(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") {
                    applyScramble();
                  }
                }}
                placeholder="e.g. BR U BR' U' R L' D"
                spellCheck={false}
                autoComplete="off"
              />
            </label>
            <button className="secondary" onClick={applyScramble}>
              Apply
            </button>
          </div>

          <FtoViewer
            setup={scramble}
            inputMode="setup"
            applySignal={scrambleSignal}
            lastLayerMode={false}
            onFacelets={handleFacelets}
            onCenterTargets={handleCenterTargets}
            viewerApiRef={apiRef}
            active={active}
            footer={
              <div className="terminal" ref={terminalRef}>
                {terminal.length === 0 ? (
                  <div className="terminal-placeholder">combination search logs will appear here</div>
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
              <h2>Intermediate Algs</h2>
              <div className="mini-actions">
                <button onClick={resetAlgText}>Example</button>
              </div>
            </div>
            <p className="hint">One per line: name : moves (# for comments)</p>
            <textarea
              className="alg-editor"
              value={algText}
              onChange={(event) => setAlgText(event.target.value)}
              spellCheck={false}
              autoComplete="off"
            />
            <div className="hint">
              {algNames.length} alg{algNames.length === 1 ? "" : "s"} parsed. U and U' are inserted before, between and
              after each alg automatically.
            </div>
          </section>

          <LlSetupSection facelets={facelets} viewerApiRef={apiRef} />

          <section className="tool-section solve-options">
            <div className="section-heading">
              <h2>Combine</h2>
            </div>
            <div className="option-list">
              <label className="check">
                <input
                  type="checkbox"
                  checked={useTriples}
                  onChange={(event) => setUseTriples(event.target.checked)}
                />
                Also try 3-alg combinations
              </label>
            </div>
            <div className="hint">
              1 alg: 9 U-placements. 2 algs: 27 U-placements per pair. 3 algs: 81 U-placements per triple. Every alg,
              ordered pair and ordered triple is tried, deduped by exact sequence.
            </div>
            <div className="actions-row">
              <button
                className={running ? "primary stop" : "primary"}
                onClick={running ? stopSearch : runSearch}
                disabled={algNames.length < 1 && !running}
              >
                {running ? "Stop" : "Find combinations"}
              </button>
            </div>
            {algNames.length < 1 ? (
              <div className="state-error">Add at least one intermediate alg.</div>
            ) : (
              <div className="state-ok">
                will test {counts.total.toLocaleString()} combinations against the current state
              </div>
            )}
          </section>

          <section className="tool-section">
            <div className="section-heading">
              <h2>Solutions</h2>
              <span className="hint">{solutions.length} found</span>
            </div>
            {solutions.length === 0 ? (
              <div className="hint">No solutions yet.</div>
            ) : (
              <div className="solutions">
                {solutions.map((solution, index) => (
                  <div className="solution-row" key={`${solution.rawText}-${index}`}>
                    <div className="solution-body">
                      <span className="solution-seq">{solution.alg}</span>
                      <span className="solution-meta">
                        {solution.rawMoves} → {solution.moves}m
                      </span>
                      <button className="secondary" onClick={() => applySolution(solution)}>
                        Apply
                      </button>
                    </div>
                    <div className="solution-sub">
                      algs: {solution.from.join(" + ")} · raw: {solution.rawText}
                    </div>
                  </div>
                ))}
              </div>
            )}
          </section>
        </div>
      </section>
    </>
  );
}

export default AlgCombiner;
