import { rotateAlgorithm, normalizeMoveSelection, type ExecutionHold } from "./rotation";
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import FtoViewer, { type CenterTargets, type FtoViewerApi, type IgnoredStickerState } from "./FtoViewer";
import LlSetupSection from "./LlSetupSection";
import DepthHelp from "./DepthHelp";
import BatchCaseInput, { type BatchCaseInputApi } from "./BatchCaseInput";
import { parseDepthSelection } from "./depthSelection";
import { clearCase, createProject, downloadFile, hasCase, newCase, parseProject, projectCsv, savedProjects, saveProject, type BatchCase, type BatchProject, type BatchMoveSet, type BatchProgress, moveSetsOf, effectiveMoveSets, coveringMoveSet, changeMoveSets, progressForSet, mergeSetProgress, nextBatchAttempt, constrainBatchInstance, deleteProject } from "./batchProject";

type BatchInstance = { id: string; name: string; moves: string[]; banned: string[] };
type Props = { instances: BatchInstance[]; activeInstanceId: string; onSelectInstance: (id: string) => void; accelerationControl?: React.ReactNode; executionHold: ExecutionHold; active: boolean; moves: string[]; instanceMoves: string[]; faceColors: string[]; useGpu: boolean; onRunning: (running: boolean) => void };
function normalizeProject(project: BatchProject): BatchProject {
  const pool = [...new Set(normalizeMoveSelection(project.settings.instanceMoves))];
  const sets = moveSetsOf(project.settings).map(set => ({ ...set, moves: [...new Set(normalizeMoveSelection(set.moves))] }));
  const normalized = constrainBatchInstance(changeMoveSets(project, sets), pool, project.settings.instanceId);
  return { ...normalized, status: moveSetsOf(project.settings).some((set,i) => JSON.stringify(set.moves) !== JSON.stringify(sets[i].moves) || set.moves.some(move => !pool.includes(move))) ? "paused" : project.status,
    settings: { ...normalized.settings, slowCaseSeconds: !project.settings.moveSets && project.settings.slowCaseSeconds === 10 ? 300 : project.settings.slowCaseSeconds ?? 300 } };
}
type BatchEvent = { runId: string; kind: string; caseId?: string; depth?: number; algorithm?: string; case?: BatchCase; message?: string };

function MoveSetEditor({ initial, executionHold, onSave, onClose }: {
  initial: BatchMoveSet; executionHold: ExecutionHold;
  onSave: (set: BatchMoveSet) => void; onClose: () => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [draft, setDraft] = useState(initial);
  useEffect(() => { dialog.current?.showModal(); return () => dialog.current?.close(); }, []);
  return <dialog ref={dialog} className="batch-moveset-dialog" aria-labelledby="batch-moveset-title" onCancel={onClose}>
    <div className="modal-title-row"><h2 id="batch-moveset-title">Edit allowed move set</h2><button type="button" onClick={onClose}>Close</button></div>
    <div className="batch-moves">{draft.instanceMoves.map(move => <button type="button" key={move}
      className={draft.moves.includes(move) ? "selected" : ""} aria-pressed={draft.moves.includes(move)}
      onClick={() => setDraft(p => ({ ...p, moves: p.moves.includes(move) ? p.moves.filter(m => m !== move) : [...p.moves,move] }))}>
      {rotateAlgorithm(move, executionHold)}
    </button>)}</div>
    <div className="batch-move-actions"><button type="button" onClick={() => setDraft(p => ({ ...p, moves: [...p.instanceMoves] }))}>All</button>
      <button type="button" onClick={() => setDraft(p => ({ ...p, moves: [] }))}>None</button>
      <button type="button" onClick={() => setDraft(p => ({ ...p, moves: p.instanceMoves.filter(move => !p.moves.includes(move)) }))}>Invert</button>
      </div>
    <div className="modal-footer"><button type="button" onClick={onClose}>Cancel</button><button type="button" className="primary" disabled={!draft.moves.length} onClick={() => onSave(draft)}>Save move set</button></div>
  </dialog>;
}

function DeleteBatchDialog({ name, deleting, onDelete, onClose }: { name: string; deleting: boolean; onDelete: () => void; onClose: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => { dialog.current?.showModal(); return () => dialog.current?.close(); }, []);
  return <dialog ref={dialog} className="batch-moveset-dialog" aria-labelledby="delete-batch-title" onCancel={event => { event.preventDefault(); if (!deleting) onClose(); }}>
    <h2 id="delete-batch-title">Delete batch?</h2><p>Delete “{name || "Untitled batch"}” and its saved cases and search progress from this device? Downloaded project files are kept.</p>
    <div className="modal-footer"><button type="button" disabled={deleting} onClick={onClose}>Cancel</button><button type="button" className="danger" disabled={deleting} onClick={onDelete}>{deleting ? "Deleting…" : "Delete batch"}</button></div>
  </dialog>;
}

function CaseEditor({ entry, number, lastLayer, faceColors, onSave, onClose }: {
  entry: BatchCase; number: number; lastLayer: boolean; faceColors: string[];
  onSave: (facelets: number[], targets: CenterTargets, ignoredStickerState: IgnoredStickerState) => void; onClose: () => void;
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
      onSave(colors, targets, api.getIgnoredStickerState());
    } catch (e) { setError(String(e)); }
  }
  return <dialog ref={dialog} className="batch-visual-dialog" aria-labelledby="batch-case-title" onCancel={onClose}>
    <div className="batch-dialog-header"><h2 id="batch-case-title">Case {number}</h2><button type="button" aria-label="Close visual editor" onClick={onClose}>×</button></div>
    <div className="batch-visual-layout">
      <FtoViewer setup={setup} inputMode="setup" applySignal={apply} applyFromSolved lastLayerMode={lastLayer}
        onFacelets={faceletChange} onCenterTargets={targetChange} faceColors={faceColors} viewerApiRef={viewer}
        initialState={entry.facelets ? { facelets: entry.facelets, centerTargets: entry.centerTargets ?? undefined, ignoredStickerState: entry.ignoredStickerState } : undefined} />
      <div className="batch-visual-tools">
        <label>Setup moves<input value={setup} onChange={e => setSetup(e.target.value)} /></label>
        <button type="button" onClick={() => setApply(v => v + 1)}>Apply setup</button>
        <LlSetupSection facelets={facelets} viewerApiRef={viewer} />
      </div>
    </div>
    {error && <p className="batch-error" role="alert">{error}</p>}
    <div className="batch-dialog-actions"><button onClick={onClose}>Cancel</button><button className="primary" onClick={save}>Save case</button></div>
  </dialog>;
}

export default function BatchSolver({ instances, activeInstanceId, onSelectInstance, accelerationControl, executionHold, active, moves, instanceMoves, faceColors, useGpu, onRunning }: Props) {
  function newProject() {
    const p = createProject(moves, instanceMoves);
    return { ...constrainBatchInstance(p, instanceMoves, activeInstanceId), status: "idle" as const };
  }
  const [project, setProject] = useState<BatchProject>(() => newProject());
  const [projects, setProjects] = useState<BatchProject[]>([]);
  const [ready, setReady] = useState(false);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  const [storageError, setStorageError] = useState("");
  const [exportPath, setExportPath] = useState("");
  const [activeCases, setActiveCases] = useState<Record<string, number>>({});
  const [saved, setSaved] = useState(false);
  const [editing, setEditing] = useState<string | null>(null);
  const [tab, setTab] = useState<"input" | "output">("input");
  const [editingMoveSet, setEditingMoveSet] = useState<BatchMoveSet | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const deletedIds = useRef(new Set<string>());
  const [runPlan, setRunPlan] = useState<{ tick: number; generation: number } | null>(null);
  const generation = useRef(0);
  const activeSet = useRef<string | null>(null);
  const [activeSetId, setActiveSetId] = useState<string | null>(null);
  const [filter, setFilter] = useState("");
  const [starting, setStarting] = useState(false);
  const [listenersReady, setListenersReady] = useState(false);
  const runId = useRef<string | null>(null);
  const latest = useRef(project); latest.current = project;
  const caseInput = useRef<BatchCaseInputApi>(null);
  const fileInput = useRef<HTMLInputElement>(null);
  const running = project.status === "running" || project.status === "pausing";
  const selection = parseDepthSelection(project.settings.depth);
  const moveSets = moveSetsOf(project.settings);
  const retainedSets = effectiveMoveSets(moveSets);
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
        const entries = (await savedProjects()).map(normalizeProject);
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
      if (saving || latest.current === persisted || deletedIds.current.has(latest.current.id)) return;
      const snapshot = latest.current;
      saving = true;
      try {
        await saveProject(snapshot);
        persisted = snapshot;
        if (!disposed && !deletedIds.current.has(snapshot.id)) {
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
    if (ready && !deletedIds.current.has(project.id) && (project.status === "paused" || project.status === "done")) {
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
        if (e.kind === "solution" && e.caseId && e.depth !== undefined && e.algorithm !== undefined && activeSet.current) {
          const setId = activeSet.current;
          setProject(p => ({ ...p, updatedAt: Date.now(), cases: p.cases.map(c => {
            if (c.id !== e.caseId) return c;
            const sets = moveSetsOf(p.settings);
            const progress = progressForSet(c, setId, sets[0].id);
            if (progress.solutions.some(s => s.depth === e.depth && s.algorithm === e.algorithm)) return c;
            return mergeSetProgress(c, setId, { ...progress, solutions: [...progress.solutions, { depth: e.depth!, algorithm: e.algorithm! }] }, sets, p.settings.all);
          }) }));
        }
        if (e.kind === "case" && e.case && activeSet.current) {
          const setId = activeSet.current;
          setActiveCases(cases => { const next = { ...cases }; delete next[e.case!.id]; return next; });
          setProject(p => ({ ...p, updatedAt: Date.now(), cases: p.cases.map(c => c.id === e.case!.id
            ? mergeSetProgress(c, setId, e.case!, moveSetsOf(p.settings), p.settings.all) : c) }));
        }
        if (["done", "paused", "error"].includes(e.kind)) {
          runId.current = null; setActiveCases({});
          if (e.kind === "done") {
            setRunPlan(plan => {
              if (!plan || plan.generation !== generation.current) return null;
              return { ...plan, tick: plan.tick + 1 };
            });
          } else {
            generation.current++; setRunPlan(null); activeSet.current = null; setActiveSetId(null);
            setProject(p => ({ ...p, updatedAt: Date.now(), status: "paused" }));
            setMessage(e.message ?? "Paused");
          }
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
        cases: changed.some(key => key !== "threads" && key !== "slowCaseSeconds" && key !== "temporaryTables" && key !== "tableMemoryMiB") ? p.cases.map(clearCase) : p.cases,
        status: "paused", updatedAt: Date.now() };
    });
  }
  function focus(id: string) { requestAnimationFrame(() => caseInput.current?.focusCase(id)); }
  async function exportFile(kind: "csv" | "project") {
    const snapshot = latest.current;
    const content = kind === "csv" ? projectCsv(snapshot) : JSON.stringify(snapshot, null, 2);
    try {
      if (isTauri()) setExportPath(await invoke<string>("export_batch_file", { name: snapshot.name || "batch", kind, content }));
      else downloadFile(`${snapshot.name || "batch"}.${kind === "csv" ? "csv" : "ftobatch.json"}`, content, kind === "csv" ? "text/csv;charset=utf-8" : "application/json");
    } catch (e) { setError(String(e)); }
  }
  function start() {
    if (running || !ready || !listenersReady || deleting) return;
    if (selection.error) { setError(selection.error); return; }
    if (!retainedSets.length) { setError("Add a nonempty allowed move set."); return; }
    setError(""); setMessage("Starting…"); setTab("output");
    setProject(p => ({ ...p, status: "running", updatedAt: Date.now() }));
    setRunPlan({ tick: 0, generation: ++generation.current });
  }
  useEffect(() => {
    if (!runPlan) return;
    const snapshot = latest.current;
    const attempt = nextBatchAttempt(snapshot, parseDepthSelection(snapshot.settings.depth).depths);
    if (!attempt) {
      setRunPlan(null); activeSet.current = null; setActiveSetId(null);
      setProject(p => ({ ...p, status: "done", updatedAt: Date.now() }));
      setMessage("Batch complete");
      return;
    }
    const plan = runPlan;
    const sets = moveSetsOf(snapshot.settings);
    const set = sets.find(set => set.id === attempt.moveSetId);
    const entry = snapshot.cases.find(entry => entry.id === attempt.caseId);
    if (!set || !entry) return;
    const id = crypto.randomUUID(); runId.current = id;
    activeSet.current = set.id; setActiveSetId(set.id); setStarting(true);
    setMessage("Starting…");
    void (async () => {
      try {
        await saveProject(snapshot);
        if (generation.current !== plan.generation) return;
        await invoke("start_batch", { request: {
          runId: id, cases: snapshot.cases.filter(hasCase).map(c => ({ ...c, ...progressForSet(c, set.id, sets[0].id), moveSetResults: undefined })),
          allowedMoves: set.moves, instanceMoves: snapshot.settings.instanceMoves,
          depths: parseDepthSelection(snapshot.settings.depth).depths,
          temporaryTables: snapshot.settings.temporaryTables ?? true, tableMemoryMiB: snapshot.settings.tableMemoryMiB ?? 0,
          findAll: snapshot.settings.all, lastLayerMode: snapshot.settings.lastLayer,
          threads: snapshot.settings.threads, slowCaseSeconds: snapshot.settings.slowCaseSeconds, useGpu,
        } });
        if (generation.current !== plan.generation) await invoke("pause_batch");
      } catch (e) {
        if (generation.current !== plan.generation) return;
        generation.current++; runId.current = null; setRunPlan(null); activeSet.current = null; setActiveSetId(null);
        setError(String(e)); setMessage("Paused");
        setProject(p => ({ ...p, status: "paused", updatedAt: Date.now() }));
      } finally { setStarting(false); }
    })();
  }, [runPlan]);
  async function pause() {
    generation.current++; setRunPlan(null);
    setProject(p => ({ ...p, status: "pausing" }));
    try {
      await invoke("pause_batch");
      if (!(await invoke<boolean>("batch_status"))) {
        runId.current = null; activeSet.current = null; setActiveSetId(null); setActiveCases({});
        setProject(p => ({ ...p, status: "paused", updatedAt: Date.now() })); setMessage("Paused");
      }
    } catch (e) { setError(String(e)); setProject(p => ({ ...p, status: "paused" })); }
  }
  async function removeProject() {
    if (running || deleting) return;
    const id = project.id;
    setDeleting(true); deletedIds.current.add(id);
    try {
      await deleteProject(id);
      const remaining = (await savedProjects()).filter(p => p.id !== id).map(normalizeProject);
      setProjects(remaining); setProject(remaining[0] ?? newProject());
      setMessage(""); setError(""); setEditing(null); setFilter(""); setTab("input"); setConfirmDelete(false);
    } catch (e) { deletedIds.current.delete(id); setError(`Could not delete batch: ${String(e)}`); }
    finally { setDeleting(false); }
  }
  function updateMoveSets(sets: BatchMoveSet[]) {
    setProject(p => constrainBatchInstance(changeMoveSets(p, sets), p.settings.instanceMoves, p.settings.instanceId)); setMessage("");
  }
  function reorderMoveSet(index: number, offset: number) {
    const next = [...moveSets]; [next[index],next[index+offset]] = [next[index+offset],next[index]]; updateMoveSets(next);
  }
  async function switchProject(next: BatchProject) {
    next = normalizeProject(next);
    try { if (!deletedIds.current.has(latest.current.id)) await saveProject(latest.current); setTab("input"); setFilter(""); setProject({ ...next, status: next.status === "done" ? "done" : "paused" }); setError(""); setMessage(""); }
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
      <button disabled={!ready || running || deleting} onClick={() => void switchProject(newProject())}>New batch</button>
      <button className="danger" disabled={!ready || running || deleting} onClick={() => setConfirmDelete(true)}>Delete batch</button>
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
        <div className="batch-workspace-tabs" role="tablist" aria-label="Batch workspace" onKeyDown={event => {
          if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
          event.preventDefault();
          const next = event.key === "Home" ? "input" : event.key === "End" ? "output" : tab === "input" ? "output" : "input";
          setTab(next); document.getElementById(`batch-${next}-tab`)?.focus();
        }}>
          <button id="batch-input-tab" role="tab" tabIndex={tab === "input" ? 0 : -1} aria-selected={tab === "input"} aria-controls="batch-input-panel" onClick={() => setTab("input")}>Input <span>{count} cases</span></button>
          <button id="batch-output-tab" role="tab" tabIndex={tab === "output" ? 0 : -1} aria-selected={tab === "output"} aria-controls="batch-output-panel" onClick={() => setTab("output")}>Output <span>{solutionCount} solutions</span></button>
        </div>
        <section id="batch-input-panel" role="tabpanel" aria-labelledby="batch-input-tab" className="batch-cases-panel" hidden={tab !== "input"}>
          <div className="batch-section-header"><h2>Cases <span>{count}</span></h2><button disabled={running || !ready} onClick={() => { const c = newCase(); updateCases(entries => [...entries,c]); focus(c.id); }}>+ Add case</button></div>
          <BatchCaseInput key={project.id} ref={caseInput} entries={project.cases} disabled={running || !ready}
            activeCases={activeCases} onChange={entries => updateCases(() => entries)} onEdit={setEditing} />
        </section>
        <section id="batch-output-panel" role="tabpanel" aria-labelledby="batch-output-tab" className="batch-output-panel" hidden={tab !== "output"}>
          <div className="batch-section-header"><h2>Output <span>{solutionCount} solutions</span></h2><button disabled={!count} onClick={() => void exportFile("csv")}>Export CSV</button></div>
          <div className={`batch-output-status${accelerationControl ? " has-acceleration" : ""}`}>{accelerationControl}<div className="batch-progress" role="status"><span>{solved}/{count} cases solved</span><span>{(activeSetId && running ? `Move set ${moveSets.findIndex(set => set.id === activeSetId) + 1} · ` : "") + (message || (project.status === "done" ? "Batch complete" : project.status === "paused" ? "Paused" : "Ready"))}</span></div></div>
          <input className="batch-output-filter" aria-label="Filter output" placeholder="Filter by case number or setup" value={filter} onChange={e => setFilter(e.target.value)} />
          <div className="batch-output" tabIndex={0} aria-label="Batch results">
            {!count && <div className="batch-output-empty"><h3>No results yet</h3><p>Add your cases in Input, choose the allowed move sets, and start the batch.</p><button type="button" onClick={() => setTab("input")}>Go to Input</button></div>}
            {project.cases.map((entry,index) => hasCase(entry) && (!filter || String(index+1) === filter.trim() || entry.setup.toLowerCase().includes(filter.toLowerCase())) &&
              <article className="batch-result" key={entry.id}>
                <div className="batch-result-heading"><strong>Case {index+1}</strong><code>{entry.facelets ? "Visual state" : entry.setup}</code><span>{entry.firstDepth !== null ? `Best ${project.status === "done" ? "depth " : "so far: "}${entry.firstDepth}` : ""}</span></div>
                {entry.error ? <p className="batch-error">{entry.error}</p> : entry.solutions.length ? entry.solutions.map((s,i) => <div className="batch-solution" key={`${s.depth}:${s.algorithm}`}><span>{s.depth}</span><code>{rotateAlgorithm(s.algorithm, executionHold) || "(solved)"}</code>{s.moveSetIds && <span className="batch-solution-sets">{s.moveSetIds.map(id => `Set ${moveSets.findIndex(set => set.id === id) + 1}`).join(", ")}</span>}<button title="Copy solution" aria-label={`Copy case ${index+1} solution ${i+1}`} onClick={() => void navigator.clipboard.writeText(rotateAlgorithm(s.algorithm, executionHold)).catch(e => setError(String(e)))}>Copy</button></div>) : <p>{entry.deferredDepth != null ? `Deferred at depth ${entry.deferredDepth}` : project.status === "done" ? "No solution in selected depths" : "Pending"}</p>}
                {entry.moveSetResults && <div className="batch-set-statuses">{retainedSets.map(set => {
                  const p = entry.moveSetResults?.[set.id];
                  return <span key={set.id}>Set {moveSets.indexOf(set)+1}: {p?.error ?? (activeSetId === set.id && activeCases[entry.id] !== undefined ? `searching depth ${activeCases[entry.id]}` : p?.checkpoint ? `depth ${p.checkpoint.depth} saved` : p?.firstDepth != null ? `first depth ${p.firstDepth}` : project.status === "done" ? "no solution in selected depths" : "pending")}</span>;
                })}</div>}
              </article>)}
          </div>
        </section>
      </div>
      <aside className="batch-sidebar">
        <section className="tool-section">
          <label className="field">Instance<select aria-label="Batch instance" disabled={running || !ready} value={project.settings.instanceId ?? instances.find(instance => instance.moves.length === project.settings.instanceMoves.length && instance.moves.every(move => project.settings.instanceMoves.includes(move)))?.id ?? ""} onChange={e => {
            const instance = instances.find(instance => instance.id === e.target.value);
            if (!instance) return;
            onSelectInstance(instance.id);
            setProject(p => constrainBatchInstance(p, normalizeMoveSelection(instance.moves), instance.id)); setMessage("");
          }}>
            <option value="" disabled>Saved instance</option>
            {instances.map(instance => <option key={instance.id} value={instance.id}>{instance.name}</option>)}
          </select></label>
          <fieldset disabled={running || !ready}>
            <div className="batch-moveset-list">{moveSets.map((set,index) => {
              const covered = coveringMoveSet(moveSets, index);
              return <section key={set.id} className={`batch-moveset-card${covered ? " is-covered" : ""}`}>
                <h2>Allowed move set {index+1}</h2>
                <div className="batch-selected-moves">{set.moves.map(move => <code key={move}>{rotateAlgorithm(move, executionHold)}</code>)}{!set.moves.length && <span>No moves selected</span>}</div>
                {covered && <p className="batch-control-hint">Covered by move set {moveSets.indexOf(covered)+1}; skipped.</p>}
                <div className="batch-move-actions"><button type="button" onClick={() => setEditingMoveSet(set)}>Edit</button>
                  <button type="button" onClick={() => updateMoveSets(moveSets.filter(other => other.id !== set.id))}>Delete</button>
                  <button type="button" aria-label={`Increase priority of move set ${index+1}`} disabled={index === 0} onClick={() => reorderMoveSet(index,-1)}>↑</button>
                  <button type="button" aria-label={`Decrease priority of move set ${index+1}`} disabled={index === moveSets.length-1} onClick={() => reorderMoveSet(index,1)}>↓</button></div>
              </section>;
            })}</div>
            <button type="button" className="batch-add-moveset" onClick={() => setEditingMoveSet({ id: crypto.randomUUID(), moves: [...project.settings.instanceMoves], instanceMoves: [...project.settings.instanceMoves], instanceId: project.settings.instanceId })}>+ Add another move set</button>
          </fieldset>
        </section>
        <section className="tool-section solve-options">
          <div className="section-heading"><h2>Search</h2></div>
          <fieldset disabled={running || !ready}>
            <div className="search-grid"><div className="depth-field"><div className="depth-label-row"><label htmlFor="batch-depth">Depth</label><DepthHelp /></div><input id="batch-depth" placeholder="auto" value={project.settings.depth} aria-invalid={Boolean(selection.error)} onChange={e => changeSettings({ depth: e.target.value })} /></div>
              <label>Threads<input type="number" min="1" max="1024" value={project.settings.threads} onChange={e => changeSettings({ threads: Math.max(1, Math.min(1024, Number(e.target.value) || 1)) })} /></label></div>
            <label className="batch-defer-label" title="Defer slow first-pass searches until other cases catch up to that depth. 0 disables deferral. Deferred searches resume to completion.">Defer after (minutes)
              <input aria-label="Defer slow cases after minutes" type="number" min="0" max="1440" step="0.5" value={project.settings.slowCaseSeconds / 60} onChange={e => changeSettings({ slowCaseSeconds: Math.max(0, Math.min(86400, (Number(e.target.value) || 0) * 60)) })} />
            </label>
            <label className="check"><input type="checkbox" checked={project.settings.temporaryTables ?? true} onChange={e => changeSettings({ temporaryTables: e.target.checked })} />Temporary tables for large or deep batches</label>
            {(project.settings.temporaryTables ?? true) && <label className="batch-defer-label">Table memory budget (MiB)<input aria-label="Temporary table memory budget in MiB" type="number" min="0" max="131072" placeholder="Auto" value={project.settings.tableMemoryMiB || ""} onChange={e => changeSettings({ tableMemoryMiB: Math.max(0,Math.min(131072,Math.floor(Number(e.target.value)||0))) })} /></label>}
            <label className="check"><input type="checkbox" checked={project.settings.all} onChange={e => changeSettings({ all: e.target.checked })} />All solutions</label>
            <label className="check"><input type="checkbox" checked={project.settings.lastLayer} onChange={e => changeSettings({ lastLayer: e.target.checked })} />Last layer mode</label>
          </fieldset>
          {selection.error && <p className="batch-error">{selection.error}</p>}
          <div className="batch-search-actions"><button className={running ? "primary stop" : "primary"} disabled={!ready || !listenersReady || starting || project.status === "pausing" || (!running && (!count || !retainedSets.length || !!selection.error))} onClick={() => void (running ? pause() : start())}>{project.status === "pausing" ? "Pausing…" : running ? "Pause" : project.status === "idle" ? "Solve batch" : "Resume batch"}</button>
          <button disabled={running || !ready} onClick={() => { updateCases(entries => entries.map(clearCase)); setMessage(""); }}>Reset results</button></div>
        </section>
      </aside>
    </div>
    {active && editingMoveSet && <MoveSetEditor initial={editingMoveSet} executionHold={executionHold} onClose={() => setEditingMoveSet(null)} onSave={set => { updateMoveSets(moveSets.some(old => old.id === set.id) ? moveSets.map(old => old.id === set.id ? set : old) : [...moveSets,set]); setEditingMoveSet(null); }} />}
    {active && confirmDelete && <DeleteBatchDialog name={project.name} deleting={deleting} onClose={() => setConfirmDelete(false)} onDelete={() => void removeProject()} />}
    {active && editEntry && !running && <CaseEditor key={editEntry.id} entry={editEntry} number={editNumber} lastLayer={project.settings.lastLayer} faceColors={faceColors} onClose={() => { setEditing(null); focus(editEntry.id); }} onSave={(facelets, centerTargets, ignoredStickerState) => { updateCases(entries => entries.map(c => c.id === editEntry.id ? clearCase({ ...c, facelets, centerTargets, ignoredStickerState }) : c)); setEditing(null); }} />}
  </section>;
}
