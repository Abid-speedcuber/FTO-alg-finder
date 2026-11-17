import type { CenterTargets, IgnoredStickerState } from "./FtoViewer";

export type SearchCheckpoint = {
  version: 1; depth: number; input: number[];
  tasks: { root: number; base: number; path: number[]; next: number[]; entered: boolean[] }[];
};
export type BatchMoveSet = { id: string; moves: string[]; instanceMoves: string[]; instanceId?: string };
export type BatchSolution = { depth: number; algorithm: string; moveSetIds?: string[] };
export type BatchProgress = {
  deferredDepth?: number | null; checkpoint?: SearchCheckpoint | null;
  completedDepths: number[]; firstDepth: number | null;
  solutions: BatchSolution[]; nodes: number; error: string | null;
};
export type BatchCase = {
  id: string; setup: string; facelets?: number[] | null; centerTargets?: CenterTargets | null;
  ignoredStickerState?: IgnoredStickerState;
  deferredDepth?: number | null; checkpoint?: SearchCheckpoint | null;
  completedDepths: number[]; firstDepth: number | null;
  solutions: BatchSolution[]; nodes: number; error: string | null;
  moveSetResults?: Record<string, BatchProgress>;
};
export type BatchProject = {
  version: 1 | 2; id: string; name: string; updatedAt: number; cases: BatchCase[];
  settings: { moves: string[]; instanceMoves: string[]; instanceId?: string; moveSets?: BatchMoveSet[]; depth: string; threads: number; slowCaseSeconds: number; all: boolean; lastLayer: boolean; temporaryTables?: boolean; tableMemoryMiB?: number };
  status: "idle" | "running" | "pausing" | "paused" | "done";
};
export const newCase = (setup = ""): BatchCase => ({ id: crypto.randomUUID(), setup, deferredDepth: null, checkpoint: null, completedDepths: [], firstDepth: null, solutions: [], nodes: 0, error: null });
export const clearCase = (entry: BatchCase): BatchCase => ({ ...entry, moveSetResults: undefined, deferredDepth: null, checkpoint: null, completedDepths: [], firstDepth: null, solutions: [], nodes: 0, error: null });
export function createProject(moves: string[], instanceMoves: string[]): BatchProject {
  return { version: 2, id: crypto.randomUUID(), name: "Untitled batch", updatedAt: Date.now(), cases: [newCase()],
    settings: { moves: [...moves], instanceMoves: [...instanceMoves], moveSets: [{ id: crypto.randomUUID(), moves: [...moves], instanceMoves: [...instanceMoves] }], depth: "auto", threads: 1, slowCaseSeconds: 300, all: true, lastLayer: true, temporaryTables: true, tableMemoryMiB: 0 }, status: "idle" };
}
export function hasCase(entry: BatchCase) { return Boolean(entry.facelets || entry.setup.trim()); }

export function moveSetsOf(settings: BatchProject["settings"]): BatchMoveSet[] {
  return settings.moveSets ?? [{ id: "legacy", moves: settings.moves, instanceMoves: settings.instanceMoves }];
}
export function coveringMoveSet(sets: BatchMoveSet[], index: number): BatchMoveSet | undefined {
  const current = new Set(sets[index].moves);
  if (!current.size) return undefined;
  return sets.find((other, otherIndex) => {
    const pool = new Set(other.moves);
    return otherIndex !== index && (pool.size > current.size || (pool.size === current.size && otherIndex < index)) && [...current].every(move => pool.has(move));
  });
}
export function effectiveMoveSets(sets: BatchMoveSet[]): BatchMoveSet[] {
  return sets.filter((set, index) => set.moves.length > 0 && !coveringMoveSet(sets, index));
}
export function emptyProgress(): BatchProgress {
  return { completedDepths: [], firstDepth: null, solutions: [], nodes: 0, error: null, deferredDepth: null, checkpoint: null };
}
export function progressForSet(entry: BatchCase, setId: string, legacyId: string): BatchProgress {
  if (entry.moveSetResults) return Object.prototype.hasOwnProperty.call(entry.moveSetResults, setId) ? entry.moveSetResults[setId] : emptyProgress();
  if (setId !== legacyId) return emptyProgress();
  const { completedDepths, firstDepth, solutions, nodes, error, deferredDepth, checkpoint } = entry;
  return { completedDepths, firstDepth, solutions, nodes, error, deferredDepth, checkpoint };
}
export type BatchAttempt = { caseId: string; moveSetId: string; depth: number };
/** Keep a move set resident; finish its depths in case order before the next set. */
export function nextBatchAttempt(project: BatchProject, depths: number[] | null): BatchAttempt | null {
  const sets = moveSetsOf(project.settings);
  const entries = project.cases.filter(hasCase);
  for (const set of effectiveMoveSets(sets)) {
    for (const depth of depths ?? Array.from({ length: 256 }, (_, depth) => depth)) {
      let deferred: BatchAttempt | null = null;
      for (const entry of entries) {
        const p = progressForSet(entry, set.id, sets[0].id);
        if (p.error || p.completedDepths.includes(depth)) continue;
        if (p.firstDepth != null && (depths === null || !project.settings.all)) continue;
        const attempt = { caseId: entry.id, moveSetId: set.id, depth };
        if (p.deferredDepth === depth) deferred ??= attempt;
        else return attempt;
      }
      if (deferred) return deferred;
    }
  }
  return null;
}
export function aggregateCase(entry: BatchCase, results: Record<string, BatchProgress>, sets: BatchMoveSet[], findAll = true): BatchCase {
  const active = effectiveMoveSets(sets);
  const progress = active.map(set => Object.prototype.hasOwnProperty.call(results, set.id) ? results[set.id] : emptyProgress());
  const algorithms = new Map<string, BatchSolution>();
  active.forEach((set, i) => (findAll ? progress[i].solutions : [...progress[i].solutions].sort((a,b) => a.depth-b.depth).slice(0,1)).forEach(solution => {
    const key = `${solution.depth}:${solution.algorithm}`;
    const prior = algorithms.get(key);
    if (prior) { if (!prior.moveSetIds!.includes(set.id)) prior.moveSetIds!.push(set.id); }
    else algorithms.set(key, { depth: solution.depth, algorithm: solution.algorithm, moveSetIds: [set.id] });
  }));
  const ranked = [...algorithms.values()].sort((a,b) => a.depth - b.depth);
  const solutions = ranked;
  const firstDepth = solutions.length ? Math.min(...solutions.map(s => s.depth)) : null;
  const held = progress.flatMap(p => p.deferredDepth == null ? [] : [p.deferredDepth]);
  return { ...entry, moveSetResults: results, solutions, firstDepth,
    nodes: progress.reduce((total,p) => total + p.nodes,0),
    completedDepths: progress.length ? progress[0].completedDepths.filter(depth => progress.every(p => p.completedDepths.includes(depth))) : [],
    checkpoint: progress.length === 1 ? progress[0].checkpoint : null,
    deferredDepth: held.length ? Math.min(...held) : null,
    error: progress.length && progress.every(p => p.error) ? progress[0].error : null,
  };
}
export function mergeSetProgress(entry: BatchCase, setId: string, incoming: BatchProgress, sets: BatchMoveSet[], findAll = true): BatchCase {
  const results = entry.moveSetResults ?? { [sets[0].id]: progressForSet(entry, sets[0].id, sets[0].id) };
  return aggregateCase(entry, { ...results, [setId]: incoming }, sets, findAll);
}
export function changeMoveSets(project: BatchProject, sets: BatchMoveSet[]): BatchProject {
  const prior = moveSetsOf(project.settings);
  const key = (moves: string[]) => [...new Set(moves)].sort().join(" ");
  return { ...project, version: 2, status: "paused", updatedAt: Date.now(), settings: { ...project.settings, moveSets: sets,
    moves: sets[0]?.moves ?? [], instanceMoves: sets[0]?.instanceMoves ?? project.settings.instanceMoves },
    cases: project.cases.map(entry => {
      const results: Record<string, BatchProgress> = {};
      for (const set of sets) {
        const old = prior.find(old => old.id === set.id);
        if (old && key(old.moves) === key(set.moves)) results[set.id] = progressForSet(entry, set.id, prior[0].id);
      }
      return aggregateCase(entry, results, sets, project.settings.all);
    }),
  };
}

/** One instance pool for the entire batch; changing it clips incompatible generators. */
export function constrainBatchInstance(project: BatchProject, pool: string[], instanceId?: string): BatchProject {
  const result = changeMoveSets(project, moveSetsOf(project.settings).map(set => ({ ...set,
    moves: set.moves.filter(move => pool.includes(move)), instanceMoves: [...pool], instanceId,
  })));
  return { ...result, settings: { ...result.settings, instanceMoves: [...pool], instanceId } };
}

// Text cells are escaped and formula-prefixed input is neutralized for spreadsheet apps.
function csvCell(value: unknown) {
  let text = String(value ?? "");
  if (/^[\s]*[=+\-@]/.test(text)) text = "'" + text;
  return `"${text.replace(/"/g, '""')}"`;
}
export function projectCsv(project: BatchProject): string {
  const rows: unknown[][] = [["Case", "Setup / state", "Depth", "Solution", "First solution depth", "Completed depths", "Status", "Nodes", "Allowed moves", "Selected depths", "Last layer", "Deferred depth", "Move sets"]];
  project.cases.forEach((entry, index) => {
    if (!hasCase(entry)) return;
    const state = entry.facelets ? JSON.stringify({ facelets: entry.facelets, centerTargets: entry.centerTargets, ignoredStickerState: entry.ignoredStickerState }) : entry.setup;
    const status = entry.error || (entry.solutions.length ? "Solved" : project.status === "done" ? "No solution in selected depths" : entry.deferredDepth != null ? `Deferred at depth ${entry.deferredDepth}` : "Pending");
    for (const solution of entry.solutions.length ? entry.solutions : [null]) {
      rows.push([index + 1, state, solution?.depth, solution?.algorithm, entry.firstDepth, entry.completedDepths.join(", "), status, entry.nodes, effectiveMoveSets(moveSetsOf(project.settings)).filter(set => !solution?.moveSetIds || solution.moveSetIds.includes(set.id)).map(set => set.moves.join(" ")).join(" | "), project.settings.depth || "auto", project.settings.lastLayer ? "Yes" : "No", entry.deferredDepth, solution?.moveSetIds?.map(id => moveSetsOf(project.settings).findIndex(set => set.id === id) + 1).join(" | ") ?? ""]);
    }
  });
  return "\uFEFF" + rows.map(row => row.map(csvCell).join(",")).join("\r\n") + "\r\n";
}

function validateProgress(c: BatchProgress) {
  const integer = (v: unknown) => Number.isInteger(v) && Number(v) >= 0 && Number(v) <= 255;
  if (!c || !Array.isArray(c.completedDepths) || !c.completedDepths.every(integer) ||
      !(c.firstDepth === null || integer(c.firstDepth)) || !(c.deferredDepth == null || integer(c.deferredDepth)) ||
      !Array.isArray(c.solutions) || !c.solutions.every(s => s && integer(s.depth) && typeof s.algorithm === "string" &&
        (s.moveSetIds === undefined || (Array.isArray(s.moveSetIds) && s.moveSetIds.every(id => typeof id === "string")))) ||
      !Number.isFinite(c.nodes) || c.nodes < 0 || !(c.error === null || typeof c.error === "string")) throw Error("Invalid saved move-set progress.");
  if (c.checkpoint != null) {
      const cp = c.checkpoint;
      const byte = (v: unknown) => Number.isInteger(v) && Number(v) >= 0 && Number(v) <= 255;
      if (cp.version !== 1 || !integer(cp.depth) || !Array.isArray(cp.input) || cp.input.length > 512 || !cp.input.every(byte) ||
          !Array.isArray(cp.tasks) || cp.tasks.length > 65536 || cp.tasks.some(t =>
            !t || !Number.isInteger(t.root) || t.root < 0 || t.root > 2 || !byte(t.base) ||
            !Array.isArray(t.path) || t.path.length > cp.depth || t.base > t.path.length || !t.path.every(m => byte(m) && m < 48) ||
            !Array.isArray(t.next) || t.next.length !== t.path.length - t.base + 1 || !t.next.every(n => byte(n) && n <= 48) ||
            !Array.isArray(t.entered) || t.entered.length !== t.next.length || !t.entered.every(e => typeof e === "boolean")
          )) throw Error("Invalid saved search checkpoint.");
    }
}

export function parseProject(text: string): BatchProject {
  const p = JSON.parse(text) as BatchProject;
  if (p?.settings && p.settings.slowCaseSeconds === undefined) p.settings.slowCaseSeconds = 300;
  const integer = (v: unknown) => Number.isInteger(v) && Number(v) >= 0 && Number(v) <= 255;
  if (![1, 2].includes(p.version) || typeof p.name !== "string" || !p.settings || !Array.isArray(p.cases) ||
      !Array.isArray(p.settings.moves) || !p.settings.moves.every(m => typeof m === "string") ||
      !Array.isArray(p.settings.instanceMoves) || !p.settings.instanceMoves.every(m => typeof m === "string") ||
      (p.settings.instanceId !== undefined && typeof p.settings.instanceId !== "string") ||
      typeof p.settings.depth !== "string" || !Number.isInteger(p.settings.threads) || p.settings.threads < 1 || p.settings.threads > 1024 ||
      (!Number.isFinite(p.settings.slowCaseSeconds) || p.settings.slowCaseSeconds < 0 || p.settings.slowCaseSeconds > 86400) ||
      (p.settings.temporaryTables !== undefined && typeof p.settings.temporaryTables !== "boolean") ||
      (p.settings.tableMemoryMiB !== undefined && (!Number.isInteger(p.settings.tableMemoryMiB) || p.settings.tableMemoryMiB < 0 || p.settings.tableMemoryMiB > 131072)) ||
      typeof p.settings.all !== "boolean" || typeof p.settings.lastLayer !== "boolean") throw Error("Invalid batch project.");
  if (p.settings.moveSets !== undefined) {
    const sets = p.settings.moveSets;
    if (!Array.isArray(sets) || new Set(sets.map(set => set?.id)).size !== sets.length || sets.some(set =>
      !set || typeof set.id !== "string" || !set.id || ["__proto__", "constructor", "prototype"].includes(set.id) || (set.instanceId !== undefined && typeof set.instanceId !== "string") ||
      !Array.isArray(set.moves) || !set.moves.every(m => typeof m === "string") ||
      !Array.isArray(set.instanceMoves) || !set.instanceMoves.every(m => typeof m === "string") ||
      set.moves.some(move => !set.instanceMoves.includes(move)))) throw Error("Invalid allowed move sets.");
  }
  const ids = new Set<string>();
  for (const c of p.cases) {
    if (typeof c.id !== "string" || ids.has(c.id) || typeof c.setup !== "string" ||
        !Array.isArray(c.completedDepths) || !c.completedDepths.every(integer) ||
        !(c.firstDepth === null || integer(c.firstDepth)) ||
        !(c.deferredDepth == null || integer(c.deferredDepth)) ||
        !Array.isArray(c.solutions) || !c.solutions.every(s => integer(s.depth) && typeof s.algorithm === "string") ||
        !Number.isFinite(c.nodes) || c.nodes < 0 || !(c.error === null || typeof c.error === "string") ||
        (c.facelets != null && (!Array.isArray(c.facelets) || c.facelets.length !== 72 || !c.facelets.every(v => Number.isInteger(v) && v >= 0 && v <= 8)))) throw Error("Invalid case in batch project.");
    if (c.ignoredStickerState !== undefined && (!Array.isArray(c.ignoredStickerState) || c.ignoredStickerState.length !== 72 ||
        c.ignoredStickerState.some(saved => saved !== null && (!saved || !Number.isInteger(saved.color) || saved.color < 0 || saved.color > 7 ||
          ![null, "fixed", "top"].includes(saved.mark))))) throw Error("Invalid remembered sticker colors.");
    validateProgress(c);
    const setIds = new Set(moveSetsOf(p.settings).map(set => set.id));
    if (c.solutions.some(solution => solution.moveSetIds?.some(id => !setIds.has(id)))) throw Error("Unknown solution move set.");
    if (c.moveSetResults !== undefined) {
      if (!c.moveSetResults || typeof c.moveSetResults !== "object" || Array.isArray(c.moveSetResults)) throw Error("Invalid move-set results.");
      for (const [id, progress] of Object.entries(c.moveSetResults)) {
        if (!setIds.has(id)) throw Error("Unknown progress move set.");
        validateProgress(progress);
      }
    }
    if (c.centerTargets != null) {
      const t = c.centerTargets;
      for (const groups of [t.ufSlots, t.rlSlots]) {
        if (groups !== undefined && (!Array.isArray(groups) || groups.length !== 4 ||
            groups.some((slots, color) => !Array.isArray(slots) || new Set(slots).size !== slots.length ||
              slots.some(slot => !Number.isInteger(slot) || slot < color * 3 || slot >= color * 3 + 3)))) {
          throw Error("Invalid entitled center slots.");
        }
      }
      if (![t.uf,t.rl,t.ufSources,t.rlSources].every(a => Array.isArray(a) && a.length === 4 && a.every(v => v === null || (Number.isInteger(v) && Number(v) >= 0 && Number(v) < 12))) ||
          !Array.isArray(t.rlTopSources) || t.rlTopSources.length !== 4 || !t.rlTopSources.every(a => Array.isArray(a) && a.every(v => Number.isInteger(v) && v >= 0 && v < 12))) throw Error("Invalid center targets.");
    }
    ids.add(c.id);
  }
  return { ...p, id: crypto.randomUUID(), status: "paused", updatedAt: Date.now() };
}

let dbPromise: Promise<IDBDatabase> | undefined;
function database() {
  return dbPromise ??= new Promise<IDBDatabase>((resolve, reject) => {
    const request = indexedDB.open("fto-batch-projects", 1);
    request.onupgradeneeded = () => request.result.createObjectStore("projects", { keyPath: "id" });
    request.onsuccess = () => resolve(request.result); request.onerror = () => { dbPromise = undefined; reject(request.error); };
  });
}
export async function savedProjects(): Promise<BatchProject[]> {
  const db = await database();
  return new Promise((resolve, reject) => {
    const request = db.transaction("projects").objectStore("projects").getAll();
    request.onsuccess = () => resolve((request.result as BatchProject[]).map(p => ({ ...p, settings: { ...p.settings, slowCaseSeconds: p.settings.slowCaseSeconds ?? 300 } })).sort((a,b) => b.updatedAt - a.updatedAt));
    request.onerror = () => reject(request.error);
  });
}
export async function saveProject(project: BatchProject) {
  const db = await database();
  return new Promise<void>((resolve, reject) => {
    const transaction = db.transaction("projects", "readwrite");
    transaction.objectStore("projects").put(project);
    transaction.oncomplete = () => resolve(); transaction.onerror = () => reject(transaction.error); transaction.onabort = () => reject(transaction.error);
  });
}
export function downloadFile(name: string, content: string, type: string) {
  const url = URL.createObjectURL(new Blob([content], { type }));
  const a = document.createElement("a"); a.href = url; a.download = name; a.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

export async function deleteProject(id: string) {
  const db = await database();
  return new Promise<void>((resolve, reject) => {
    const transaction = db.transaction("projects", "readwrite");
    transaction.objectStore("projects").delete(id);
    transaction.oncomplete = () => resolve();
    transaction.onerror = () => reject(transaction.error);
    transaction.onabort = () => reject(transaction.error);
  });
}
