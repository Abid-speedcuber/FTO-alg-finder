import { invoke as desktopInvoke, isTauri } from "@tauri-apps/api/core";
import { listen as desktopListen } from "@tauri-apps/api/event";
export { isTauri };
type Event<T> = { event: string; payload: T };
type Pending = { resolve: (value: unknown) => void; reject: (error: Error) => void };
const pending = new Map<number, Pending>();
const listeners = new Map<string, Set<(event: Event<unknown>) => void>>();
let worker: Worker | undefined;
let nextId = 0;
let fatal: Error | undefined;
function getWorker() {
  if (fatal) throw fatal;
  if (!worker) {
    worker = new Worker(new URL(`${import.meta.env.BASE_URL}solver-worker.js`, location.href), { type: "module", name: "FTO solver" });
    const fail = (message: string) => {
      fatal = new Error(message);
      for (const request of pending.values()) request.reject(fatal);
      pending.clear();
      for (const listener of listeners.get("solve-error") ?? []) listener({ event: "solve-error", payload: message });
    };
    worker.onerror = event => fail(event.message || "Could not load the WebAssembly solver. Run npm run build:wasm first.");
    worker.onmessage = ({ data }) => {
      if (data.fatal) { fail(data.fatal); return; }
      if (data.id !== undefined) {
        const request = pending.get(data.id);
        pending.delete(data.id);
        if (data.ok) request?.resolve(data.value); else request?.reject(new Error(data.error));
      } else if (data.event) {
        for (const listener of listeners.get(data.event) ?? []) listener(data);
      } else if (data.diagnostic) console.info("[FTO WASM]", data.diagnostic);
    };
  }
  return worker;
}
export async function invoke<T = unknown>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  if (isTauri()) return desktopInvoke<T>(command, args);
  const runtime = getWorker();
  const id = ++nextId;
  return new Promise<T>((resolve, reject) => {
    pending.set(id, { resolve: value => resolve(value as T), reject });
    runtime.postMessage({ id, command, args });
  });
}
export async function listen<T>(event: string, handler: (event: Event<T>) => void): Promise<() => void> {
  if (isTauri()) return desktopListen<T>(event, handler);
  const group = listeners.get(event) ?? new Set();
  const callback = handler as (event: Event<unknown>) => void;
  group.add(callback); listeners.set(event, group);
  return () => { group.delete(callback); if (!group.size) listeners.delete(event); };
}
