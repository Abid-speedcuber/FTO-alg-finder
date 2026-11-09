import type { CenterTargets } from "./FtoViewer";

export type SearchCheckpoint = {
  version: 1; depth: number; input: number[];
  tasks: { root: number; base: number; path: number[]; next: number[]; entered: boolean[] }[];
};
export type BatchCase = {
  id: string; setup: string; facelets?: number[] | null; centerTargets?: CenterTargets | null;
  deferredDepth?: number | null; checkpoint?: SearchCheckpoint | null;
  completedDepths: number[]; firstDepth: number | null;
  solutions: { depth: number; algorithm: string }[]; nodes: number; error: string | null;
};
export type BatchProject = {
  version: 1; id: string; name: string; updatedAt: number; cases: BatchCase[];
  settings: { moves: string[]; instanceMoves: string[]; depth: string; threads: number; slowCaseSeconds: number; all: boolean; lastLayer: boolean };
  status: "idle" | "running" | "pausing" | "paused" | "done";
};
export const newCase = (setup = ""): BatchCase => ({ id: crypto.randomUUID(), setup, deferredDepth: null, checkpoint: null, completedDepths: [], firstDepth: null, solutions: [], nodes: 0, error: null });
export const clearCase = (entry: BatchCase): BatchCase => ({ ...entry, deferredDepth: null, checkpoint: null, completedDepths: [], firstDepth: null, solutions: [], nodes: 0, error: null });
export function createProject(moves: string[], instanceMoves: string[]): BatchProject {
  return { version: 1, id: crypto.randomUUID(), name: "Untitled batch", updatedAt: Date.now(), cases: [newCase()],
    settings: { moves: [...moves], instanceMoves: [...instanceMoves], depth: "auto", threads: 1, slowCaseSeconds: 10, all: true, lastLayer: true }, status: "idle" };
}
export function hasCase(entry: BatchCase) { return Boolean(entry.facelets || entry.setup.trim()); }

// Text cells are escaped and formula-prefixed input is neutralized for spreadsheet apps.
function csvCell(value: unknown) {
  let text = String(value ?? "");
  if (/^[\s]*[=+\-@]/.test(text)) text = "'" + text;
  return `"${text.replace(/"/g, '""')}"`;
}
export function projectCsv(project: BatchProject): string {
  const rows: unknown[][] = [["Case", "Setup / state", "Depth", "Solution", "First solution depth", "Completed depths", "Status", "Nodes", "Allowed moves", "Selected depths", "Last layer", "Deferred depth"]];
  project.cases.forEach((entry, index) => {
    if (!hasCase(entry)) return;
    const state = entry.facelets ? JSON.stringify({ facelets: entry.facelets, centerTargets: entry.centerTargets }) : entry.setup;
    const status = entry.error || (entry.solutions.length ? "Solved" : project.status === "done" ? "No solution in selected depths" : entry.deferredDepth != null ? `Deferred at depth ${entry.deferredDepth}` : "Pending");
    for (const solution of entry.solutions.length ? entry.solutions : [null]) {
      rows.push([index + 1, state, solution?.depth, solution?.algorithm, entry.firstDepth, entry.completedDepths.join(", "), status, entry.nodes, project.settings.moves.join(" "), project.settings.depth || "auto", project.settings.lastLayer ? "Yes" : "No", entry.deferredDepth]);
    }
  });
  return "\uFEFF" + rows.map(row => row.map(csvCell).join(",")).join("\r\n") + "\r\n";
}

export function parseProject(text: string): BatchProject {
  const p = JSON.parse(text) as BatchProject;
  if (p?.settings && p.settings.slowCaseSeconds === undefined) p.settings.slowCaseSeconds = 10;
  const integer = (v: unknown) => Number.isInteger(v) && Number(v) >= 0 && Number(v) <= 255;
  if (p.version !== 1 || typeof p.name !== "string" || !p.settings || !Array.isArray(p.cases) ||
      !Array.isArray(p.settings.moves) || !p.settings.moves.every(m => typeof m === "string") ||
      !Array.isArray(p.settings.instanceMoves) || !p.settings.instanceMoves.every(m => typeof m === "string") ||
      typeof p.settings.depth !== "string" || !Number.isInteger(p.settings.threads) || p.settings.threads < 1 || p.settings.threads > 1024 ||
      (!Number.isFinite(p.settings.slowCaseSeconds) || p.settings.slowCaseSeconds < 0 || p.settings.slowCaseSeconds > 86400) ||
      typeof p.settings.all !== "boolean" || typeof p.settings.lastLayer !== "boolean") throw Error("Invalid batch project.");
  const ids = new Set<string>();
  for (const c of p.cases) {
    if (typeof c.id !== "string" || ids.has(c.id) || typeof c.setup !== "string" ||
        !Array.isArray(c.completedDepths) || !c.completedDepths.every(integer) ||
        !(c.firstDepth === null || integer(c.firstDepth)) ||
        !(c.deferredDepth == null || integer(c.deferredDepth)) ||
        !Array.isArray(c.solutions) || !c.solutions.every(s => integer(s.depth) && typeof s.algorithm === "string") ||
        !Number.isFinite(c.nodes) || c.nodes < 0 || !(c.error === null || typeof c.error === "string") ||
        (c.facelets != null && (!Array.isArray(c.facelets) || c.facelets.length !== 72 || !c.facelets.every(v => Number.isInteger(v) && v >= 0 && v <= 8)))) throw Error("Invalid case in batch project.");
    if (c.checkpoint != null) {
      const cp = c.checkpoint;
      const byte = (v: unknown) => Number.isInteger(v) && Number(v) >= 0 && Number(v) <= 255;
      if (cp.version !== 1 || !integer(cp.depth) || !Array.isArray(cp.input) || cp.input.length > 512 || !cp.input.every(byte) ||
          !Array.isArray(cp.tasks) || cp.tasks.length > 65536 || cp.tasks.some(t =>
            !t || !Number.isInteger(t.root) || t.root < 0 || t.root > 2 || !byte(t.base) ||
            !Array.isArray(t.path) || t.path.length > cp.depth || t.base > t.path.length || !t.path.every(m => byte(m) && m < 38) ||
            !Array.isArray(t.next) || t.next.length !== t.path.length - t.base + 1 || !t.next.every(n => byte(n) && n <= 38) ||
            !Array.isArray(t.entered) || t.entered.length !== t.next.length || !t.entered.every(e => typeof e === "boolean")
          )) throw Error("Invalid saved search checkpoint.");
    }
    if (c.centerTargets != null) {
      const t = c.centerTargets;
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
    request.onsuccess = () => resolve((request.result as BatchProject[]).map(p => ({ ...p, settings: { ...p.settings, slowCaseSeconds: p.settings.slowCaseSeconds ?? 10 } })).sort((a,b) => b.updatedAt - a.updatedAt));
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
