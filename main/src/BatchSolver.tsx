import { useCallback, useEffect, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import FtoViewer, { type CenterTargets, type FtoViewerApi } from "./FtoViewer";
import LlSetupSection from "./LlSetupSection";
import DepthHelp from "./DepthHelp";
import { parseDepthSelection } from "./depthSelection";
import { clearCase, createProject, downloadFile, hasCase, newCase, parseProject, projectCsv, savedProjects, saveProject, type BatchCase, type BatchProject } from "./batchProject";

type Props = { active: boolean; moves: string[]; instanceMoves: string[]; faceColors: string[]; useGpu: boolean; onRunning: (running: boolean) => void };
type BatchEvent = { runId: string; kind: string; caseId?: string; depth?: number; algorithm?: string; case?: BatchCase; message?: string };

function CaseEditor({ entry, number, lastLayer, faceColors, onSave, onClose }: {
  entry: BatchCase; number: number; lastLayer: boolean; faceColors: string[];
  onSave: (facelets: number[], targets: CenterTargets) => void; onClose: () => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const viewer = useRef<FtoViewerApi | null>(null);
  const [facelets, setFacelets] = useState<number[]>([]);
  const [setup, setSetup] = useState(entry.setup);
  const [apply, setApply] = useState(entry.facelets ? 0 : 1);
  const [error, setError] = useState("");
  const faceletChange = useCallback((values: number[]) => setFacelets(values), []);
  const targetChange = useCallback((_targets: CenterTargets) => {}, []);
  useEffect(() => { dialog.current?.showModal(); return () => dialog.current?.close(); }, []);
  async function save() {
    const api = viewer.current;
    if (!api) return;
    const colors = api.getFacelets(); const targets = api.getCenterTargets();
    try {
      const response = await invoke<{ errors: unknown[] }>("validate_facelets", { facelets: colors });
      // Native validation also happens on solve. Keep invalid edits in the modal.
      if (response?.errors?.length) { setError("This state has invalid pieces. Correct the colors or center targets before saving."); return; }
      onSave(colors, targets);
    } catch (e) { setError(String(e)); }
  }
  return <dialog ref={dialog} className="batch-visual-dialog" aria-labelledby="batch-case-title" onCancel={onClose}>
    <div className="batch-dialog-header"><h2 id="batch-case-title">Case {number}</h2><button type="button" aria-label="Close visual editor" onClick={onClose}>×</button></div>
    <div className="batch-visual-layout">
      <FtoViewer setup={setup} inputMode="setup" applySignal={apply} lastLayerMode={lastLayer}
        onFacelets={faceletChange} onCenterTargets={targetChange} faceColors={faceColors} viewerApiRef={viewer}
        initialState={entry.facelets ? { facelets: entry.facelets, centerTargets: entry.centerTargets ?? undefined } : undefined} />
      <div className="batch-visual-tools">
        <label>Setup moves<input value={setup} onChange={e => setSetup(e.target.value)} /></label>
        <button type="button" onClick={() => { viewer.current?.resetPuzzle(); setApply(v => v + 1); }}>Apply setup</button>
        <LlSetupSection facelets={facelets} viewerApiRef={viewer} lastLayerMode={lastLayer} />
      </div>
    </div>
    {error && <p className="batch-error" role="alert">{error}</p>}
    <div className="batch-dialog-actions"><button onClick={onClose}>Cancel</button><button className="primary" onClick={save}>Save case</button></div>
  </dialog>;
}

export default function BatchSolver({ active, moves, instanceMoves, faceColors, useGpu, onRunning }: Props) {
  const [project, setProject] = useState<BatchProject>(() => createProject(moves, instanceMoves));
  const [projects, setProjects] = useState<BatchProject[]>([]);
  const [ready, setReady] = useState(false);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  const [storageError, setStorageError] = useState("");
  const [exportPath, setExportPath] = useState("");
  const [activeCases, setActiveCases] = useState<Record<string, number>>({});
  const [saved, setSaved] = useState(false);
  const [editing, setEditing] = useState<string | null>(null);
  const [filter, setFilter] = useState("");
  const [starting, setStarting] = useState(false);
  const [listenersReady, setListenersReady] = useState(false);
  const runId = useRef<string | null>(null);
  const latest = useRef(project); latest.current = project;
  const inputRefs = useRef(new Map<string, HTMLInputElement>());
  const fileInput = useRef<HTMLInputElement>(null);
  const running = project.status === "running" || project.status === "pausing";
  const selection = parseDepthSelection(project.settings.depth);
  const count = project.cases.filter(hasCase).length;
  const solved = project.cases.filter(c => c.solutions.length > 0).length;
  const solutionCount = project.cases.reduce((sum,c) => sum + c.solutions.length, 0);

  useEffect(() => { onRunning(running); }, [running, onRunning]);
  useEffect(() => {
    let disposed = false;
    void (async () => {
      try {
        // A reloaded window cannot retain its old coordinator/event subscription.
        // Stop that run before exposing the persisted checkpoint for editing.
        if (await invoke<boolean>("batch_status")) {
          await invoke("pause_batch");
          while (await invoke<boolean>("batch_status")) await new Promise(resolve => setTimeout(resolve, 100));
        }
        const entries = await savedProjects();
        if (disposed) return;
        setProjects(entries);
        if (entries[0]) setProject({ ...entries[0], status: entries[0].status === "done" ? "done" : "paused" });
      } catch (e) { if (!disposed) setStorageError(`Project recovery: ${String(e)}`); }
      if (!disposed) setReady(true);
    })();
    return () => { disposed = true; };
  }, []);

  useEffect(() => {
    if (!ready) return;
    let disposed = false;
    let saving = false;
    let persisted: BatchProject | null = null;
    async function checkpoint() {
      if (saving || latest.current === persisted) return;
      const snapshot = latest.current;
      saving = true;
      try {
        await saveProject(snapshot);
        persisted = snapshot;
        if (!disposed) {
          setSaved(latest.current === snapshot); setStorageError("");
          setProjects(list => [snapshot, ...list.filter(p => p.id !== snapshot.id)].sort((a,b) => b.updatedAt - a.updatedAt));
        }
      } catch (e) {
        if (!disposed) setStorageError(`Could not save project: ${String(e)}. Download the project to keep a copy.`);
      } finally { saving = false; }
    }
    const timer = setInterval(() => void checkpoint(), 500);
    void checkpoint();
    return () => { disposed = true; clearInterval(timer); };
  }, [ready]);
  useEffect(() => { setSaved(false); }, [project]);
  useEffect(() => {
    if (ready && (project.status === "paused" || project.status === "done")) {
      void saveProject(latest.current).catch(e => setStorageError(`Could not save checkpoint: ${String(e)}`));
    }
  }, [ready, project.status]);

  useEffect(() => {
    let disposed = false; const cleanup: (() => void)[] = [];
    void (async () => {
      const remove = await listen<BatchEvent>("batch-event", ({ payload: e }) => {
        if (e.runId !== runId.current) return;
        if (e.kind === "search" && e.caseId && e.depth !== undefined) setActiveCases(cases => ({ ...cases, [e.caseId!]: e.depth! }));
        if (e.kind === "status") setMessage(e.message ?? "Searching…");
        if (e.kind === "error") setError(e.message ?? "Batch search failed.");
        if (e.kind === "solution" && e.caseId && e.depth !== undefined && e.algorithm !== undefined) {
          setProject(p => ({ ...p, updatedAt: Date.now(), cases: p.cases.map(c => c.id !== e.caseId || c.solutions.some(s => s.depth === e.depth && s.algorithm === e.algorithm) ? c : { ...c, solutions: [...c.solutions, { depth: e.depth!, algorithm: e.algorithm! }] }) }));
        }
        if (e.kind === "case" && e.case) {
          setActiveCases(cases => { const next = { ...cases }; delete next[e.case!.id]; return next; });
          setProject(p => ({ ...p, updatedAt: Date.now(), cases: p.cases.map(c => c.id === e.case!.id ? e.case! : c) }));
        }
        if (["done", "paused", "error"].includes(e.kind)) {
          runId.current = null; setActiveCases({});
          setProject(p => ({ ...p, updatedAt: Date.now(), status: e.kind === "done" ? "done" : "paused" }));
          setMessage(e.kind === "done" ? "Batch complete" : "Paused");
        }
      });
      if (disposed) remove(); else cleanup.push(remove);
      const logRemove = await listen<{ text: string }>("batch-log", ({ payload }) => { if (runId.current) setMessage(payload.text); });
      if (disposed) logRemove(); else { cleanup.push(logRemove); setListenersReady(true); }
    })().catch(e => setError(String(e)));
    return () => { disposed = true; cleanup.forEach(remove => remove()); };
  }, []);

  function updateCases(change: (cases: BatchCase[]) => BatchCase[]) {
    setMessage("");
    setProject(p => ({ ...p, cases: change(p.cases), status: "paused", updatedAt: Date.now() }));
  }
  function changeSettings(settings: Partial<BatchProject["settings"]>) {
    setMessage("");
    // Settings define the search problem. Old checkpoints cannot be used with a different one.
    setProject(p => {
      const changed = (Object.keys(settings) as (keyof typeof settings)[])
        .filter(key => JSON.stringify(p.settings[key]) !== JSON.stringify(settings[key]));
      if (!changed.length) return p;
      return { ...p, settings: { ...p.settings, ...settings },
        cases: changed.some(key => key !== "threads" && key !== "slowCaseSeconds") ? p.cases.map(clearCase) : p.cases,
        status: "paused", updatedAt: Date.now() };
    });
  }
  function focus(id: string) { requestAnimationFrame(() => inputRefs.current.get(id)?.focus()); }
  function insertAfter(id: string, entries = [newCase()]) {
    updateCases(cases => { const index = cases.findIndex(c => c.id === id); return [...cases.slice(0,index+1), ...entries, ...cases.slice(index+1)]; });
    focus(entries[0].id);
  }
  async function exportFile(kind: "csv" | "project") {
    const snapshot = latest.current;
    const content = kind === "csv" ? projectCsv(snapshot) : JSON.stringify(snapshot, null, 2);
    try {
      if (isTauri()) setExportPath(await invoke<string>("export_batch_file", { name: snapshot.name || "batch", kind, content }));
      else downloadFile(`${snapshot.name || "batch"}.${kind === "csv" ? "csv" : "ftobatch.json"}`, content, kind === "csv" ? "text/csv;charset=utf-8" : "application/json");
    } catch (e) { setError(String(e)); }
  }
  async function start() {
    if (running || !ready || !listenersReady) return;
    if (selection.error) { setError(selection.error); return; }
    const id = crypto.randomUUID(); runId.current = id;
    setError(""); setMessage("Starting…"); setStarting(true);
    const snapshot = { ...latest.current, status: "running" as const, updatedAt: Date.now() };
    setProject(snapshot);
    try {
      await saveProject(snapshot);
      await invoke("start_batch", { request: {
        runId: id, cases: snapshot.cases.filter(hasCase), allowedMoves: snapshot.settings.moves,
        instanceMoves: snapshot.settings.instanceMoves, depths: selection.depths,
        findAll: snapshot.settings.all, lastLayerMode: snapshot.settings.lastLayer,
        threads: snapshot.settings.threads, slowCaseSeconds: snapshot.settings.slowCaseSeconds, useGpu,
      } });
    } catch (e) {
      runId.current = null; setError(String(e)); setMessage("Paused");
      setProject(p => ({ ...p, status: "paused", updatedAt: Date.now() }));
    } finally { setStarting(false); }
  }
  async function pause() {
    setProject(p => ({ ...p, status: "pausing" }));
    try { await invoke("pause_batch"); } catch (e) { setError(String(e)); setProject(p => ({ ...p, status: "running" })); }
  }
  async function switchProject(next: BatchProject) {
    try { await saveProject(latest.current); setProject({ ...next, status: next.status === "done" ? "done" : "paused" }); setError(""); setMessage(""); }
    catch (e) { setError(String(e)); }
  }
  const editEntry = project.cases.find(c => c.id === editing);
  const editNumber = project.cases.findIndex(c => c.id === editing) + 1;
  return <section className="batch-workspace" aria-label="Batch solving">
    <div className="batch-project-bar">
      <label className="batch-project-name"><span className="sr-only">Project name</span><input aria-label="Project name" value={project.name} disabled={!ready || running} onChange={e => setProject(p => ({ ...p, name: e.target.value, updatedAt: Date.now() }))} /></label>
      <select aria-label="Saved batch projects" value={project.id} disabled={!ready || running} onChange={e => { const p = projects.find(p => p.id === e.target.value); if (p) void switchProject(p); }}>
        {!projects.some(p => p.id === project.id) && <option value={project.id}>{project.name}</option>}
        {projects.map(p => <option key={p.id} value={p.id}>{p.name || "Untitled batch"}</option>)}
      </select>
      <button disabled={!ready || running} onClick={() => void switchProject(createProject(moves, instanceMoves))}>New batch</button>
      <button disabled={!ready || running} onClick={() => fileInput.current?.click()}>Open project</button>
      <button disabled={!ready} onClick={() => void exportFile("project")}>Download project</button>
      <input hidden ref={fileInput} type="file" accept=".json,.ftobatch.json" onChange={async e => {
        const file = e.target.files?.[0]; e.target.value = ""; if (!file) return;
        try { await switchProject(parseProject(await file.text())); } catch (e) { setError(String(e)); }
      }} />
      <span className="batch-save-status" role="status">{ready ? saved ? "Saved locally" : "Saving…" : "Loading…"}</span>
    </div>
    {exportPath && <p className="batch-export-path" role="status">Saved to {exportPath}<button aria-label="Dismiss export location" onClick={() => setExportPath("")}>×</button></p>}
    {(error || storageError) && <div className="batch-error" role="alert">{error || storageError}</div>}
    <div className="batch-grid">
      <div className="batch-main">
        <section className="batch-cases-panel">
          <div className="batch-section-header"><h2>Cases <span>{count}</span></h2><button disabled={running || !ready} onClick={() => { const c = newCase(); updateCases(entries => [...entries,c]); focus(c.id); }}>+ Add case</button></div>
          <div className="batch-case-list" aria-label="Case setups">
            {project.cases.map((entry, index) => <div className={`batch-case-row${entry.error ? " has-error" : ""}`} key={entry.id}>
              <span className="batch-line-number">{index+1}</span>
              {entry.facelets ? <button className="batch-visual-link" disabled={running} onClick={() => setEditing(entry.id)}>Case {index+1}</button> :
                <input aria-label={`Case ${index+1} setup`} placeholder={index === 0 ? "Setup moves" : ""} value={entry.setup} disabled={running || !ready}
                  ref={el => { if (el) inputRefs.current.set(entry.id,el); else inputRefs.current.delete(entry.id); }}
                  onChange={e => updateCases(entries => entries.map(c => c.id === entry.id ? clearCase({ ...c, setup: e.target.value }) : c))}
                  onPaste={e => {
                    const text = e.clipboardData.getData("text"); if (!/[\r\n]/.test(text)) return;
                    e.preventDefault(); const input = e.currentTarget;
                    const combined = entry.setup.slice(0,input.selectionStart ?? 0) + text.replace(/\r\n?/g,"\n") + entry.setup.slice(input.selectionEnd ?? entry.setup.length);
                    const lines = combined.split("\n"); if (lines[lines.length - 1] === "") lines.pop();
                    const added = lines.map((line,i) => i === 0 ? clearCase({ ...entry, setup: line }) : newCase(line));
                    updateCases(entries => entries.flatMap(c => c.id === entry.id ? added : [c])); focus(added[added.length-1].id);
                  }}
                  onKeyDown={e => {
                    if (e.key === "Tab" && !e.shiftKey) { e.preventDefault(); setEditing(entry.id); }
                    if (e.key === "Enter") { e.preventDefault(); insertAfter(entry.id); }
                    if (e.key === "Backspace" && !entry.setup && project.cases.length > 1) {
                      e.preventDefault(); const previous = project.cases[Math.max(0,index-1)];
                      updateCases(entries => entries.filter(c => c.id !== entry.id)); focus(previous.id);
                    }
                  }} />}
              <span className="batch-case-badge" title={entry.error ?? `${entry.completedDepths.length} depths completed`}>
                {entry.error ? "Invalid" : activeCases[entry.id] !== undefined ? `Searching d${activeCases[entry.id]}` : entry.deferredDepth != null ? `Deferred d${entry.deferredDepth}` : entry.solutions.length ? `${entry.solutions.length} found` : entry.completedDepths.length ? `d${Math.max(...entry.completedDepths)}` : ""}
              </span>
              <button className="batch-remove" aria-label={`Remove case ${index+1}`} disabled={running} onClick={() => updateCases(entries => entries.length === 1 ? [newCase()] : entries.filter(c => c.id !== entry.id))}>×</button>
            </div>)}
          </div>
        </section>
        <section className="batch-output-panel">
          <div className="batch-section-header"><h2>Output <span>{solutionCount} solutions</span></h2><button disabled={!count} onClick={() => void exportFile("csv")}>Export CSV</button></div>
          <div className="batch-progress" role="status"><span>{solved}/{count} cases solved</span><span>{message || (project.status === "done" ? "Batch complete" : project.status === "paused" ? "Paused" : "Ready")}</span></div>
          <input className="batch-output-filter" aria-label="Filter output" placeholder="Filter by case number or setup" value={filter} onChange={e => setFilter(e.target.value)} />
          <div className="batch-output" tabIndex={0} aria-label="Batch results">
            {project.cases.map((entry,index) => hasCase(entry) && (!filter || String(index+1) === filter.trim() || entry.setup.toLowerCase().includes(filter.toLowerCase())) &&
              <article className="batch-result" key={entry.id}>
                <div className="batch-result-heading"><strong>Case {index+1}</strong><code>{entry.facelets ? "Visual state" : entry.setup}</code><span>{entry.firstDepth !== null ? `First depth ${entry.firstDepth}` : ""}</span></div>
                {entry.error ? <p className="batch-error">{entry.error}</p> : entry.solutions.length ? entry.solutions.map((s,i) => <div className="batch-solution" key={`${s.depth}:${s.algorithm}`}><span>{s.depth}</span><code>{s.algorithm || "(solved)"}</code><button title="Copy solution" aria-label={`Copy case ${index+1} solution ${i+1}`} onClick={() => void navigator.clipboard.writeText(s.algorithm).catch(e => setError(String(e)))}>Copy</button></div>) : <p>{entry.deferredDepth != null ? `Deferred at depth ${entry.deferredDepth}` : project.status === "done" ? "No solution in selected depths" : "Pending"}</p>}
              </article>)}
          </div>
        </section>
      </div>
      <aside className="batch-sidebar">
        <section className="tool-section">
          <div className="section-heading"><h2>Allowed moves</h2></div>
          <fieldset disabled={running || !ready} className="batch-move-fieldset">
            <div className="batch-moves">{project.settings.instanceMoves.map(move => <button type="button" key={move} className={project.settings.moves.includes(move) ? "selected" : ""} aria-pressed={project.settings.moves.includes(move)} onClick={() => changeSettings({ moves: project.settings.moves.includes(move) ? project.settings.moves.filter(m => m !== move) : [...project.settings.moves,move] })}>{move}</button>)}</div>
            <div className="batch-move-actions"><button onClick={() => changeSettings({ moves: [...project.settings.instanceMoves] })}>All</button><button onClick={() => changeSettings({ moves: [] })}>None</button><button onClick={() => changeSettings({ moves: [...moves], instanceMoves: [...instanceMoves] })}>Use current instance</button></div>
          </fieldset>
        </section>
        <section className="tool-section solve-options">
          <div className="section-heading"><h2>Search</h2></div>
          <fieldset disabled={running || !ready}>
            <div className="search-grid"><div className="depth-field"><div className="depth-label-row"><label htmlFor="batch-depth">Depth</label><DepthHelp /></div><input id="batch-depth" placeholder="auto" value={project.settings.depth} aria-invalid={Boolean(selection.error)} onChange={e => changeSettings({ depth: e.target.value })} /></div>
              <label>Threads<input type="number" min="1" max="1024" value={project.settings.threads} onChange={e => changeSettings({ threads: Math.max(1, Math.min(1024, Number(e.target.value) || 1)) })} /></label></div>
            <label className="batch-defer-label" title="All solutions: defer slow first-pass searches until other cases catch up to that depth. 0 disables deferral. Deferred searches resume to completion.">Defer after (seconds)
              <input aria-label="Defer slow cases after seconds" type="number" min="0" max="86400" step="1" disabled={!project.settings.all} value={project.settings.slowCaseSeconds} onChange={e => changeSettings({ slowCaseSeconds: Math.max(0, Math.min(86400, Number(e.target.value) || 0)) })} />
            </label>
            <label className="check"><input type="checkbox" checked={project.settings.all} onChange={e => changeSettings({ all: e.target.checked })} />All solutions</label>
            <label className="check"><input type="checkbox" checked={project.settings.lastLayer} onChange={e => changeSettings({ lastLayer: e.target.checked })} />Last layer mode</label>
          </fieldset>
          {selection.error && <p className="batch-error">{selection.error}</p>}
          <div className="batch-search-actions"><button className={running ? "primary stop" : "primary"} disabled={!ready || !listenersReady || starting || project.status === "pausing" || (!running && (!count || !project.settings.moves.length || !!selection.error))} onClick={() => void (running ? pause() : start())}>{project.status === "pausing" ? "Pausing…" : running ? "Pause" : project.status === "idle" ? "Solve batch" : "Resume batch"}</button>
          <button disabled={running || !ready} onClick={() => { updateCases(entries => entries.map(clearCase)); setMessage(""); }}>Reset results</button></div>
        </section>
      </aside>
    </div>
    {active && editEntry && !running && <CaseEditor key={editEntry.id} entry={editEntry} number={editNumber} lastLayer={project.settings.lastLayer} faceColors={faceColors} onClose={() => { setEditing(null); focus(editEntry.id); }} onSave={(facelets, centerTargets) => { updateCases(entries => entries.map(c => c.id === editEntry.id ? clearCase({ ...c, facelets, centerTargets }) : c)); setEditing(null); }} />}
  </section>;
}
