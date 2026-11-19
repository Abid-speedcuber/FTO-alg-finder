import { openStorage } from "./solver-storage.js";
// One Emscripten host worker owns the filesystem and shared Rust solver state.
let module;
let storage;
let releaseLock;
let changed = false;
let syncing = false;
let failed = false;
let retryAfter = 0;
const sendError = error => postMessage({ fatal: String(error?.message || error) });
const invoke = (command, args = {}) => JSON.parse(module.ccall('fto_dispatch', 'string', ['string'], [JSON.stringify({ command, args })]));

async function initialize() {
  if (!self.crossOriginIsolated || typeof SharedArrayBuffer === 'undefined') {
    throw new Error('The solver requires cross-origin isolation (COOP: same-origin and COEP: require-corp).');
  }
  if (!navigator.locks) throw new Error('This browser needs the Web Locks API to safely open the solver cache.');
  await new Promise((resolve, reject) => {
    navigator.locks.request('fto-wasm-cache-v1', { ifAvailable: true }, lock => {
      if (!lock) { reject(new Error('The solver is already open in another tab. Close it before opening this tab.')); return; }
      resolve();
      return new Promise(done => { releaseLock = done; });
    }).catch(reject);
  });
  const { default: createModule } = await import('./wasm/fto-web.js');
  module = await createModule({
    mainScriptUrlOrBlob: new URL("./wasm/fto-web.js", self.location.href).href,
    locateFile: file => new URL(`./wasm/${file}`, self.location.href).href,
    print: text => postMessage({ diagnostic: text }),
    printErr: text => postMessage({ diagnostic: text }),
    onAbort: reason => { failed = true; sendError(`Solver runtime aborted: ${reason}. Reload to restart.`); },
  });
  storage = await openStorage(module.FS);
  postMessage({ ready: true });
  setInterval(async () => {
    if (failed || syncing) return;
    try {
      const events = JSON.parse(module.ccall('fto_events', 'string', [], []));
      for (const event of events) {
        if (['solve-result', 'solve-error', 'solve-cancelled', 'pruning-generated', 'bloom-generated'].includes(event.event)
            || (event.event === 'batch-event' && ['done', 'paused', 'error'].includes(event.payload.kind))) changed = true;
        postMessage(event);
      }
      // Cache writes and deletes must finish before taking a cache snapshot.
      if (changed && Date.now() >= retryAfter && !invoke('get_solver_status').value.running) {
        syncing = true;
        try {
          postMessage({ event: 'solve-line', payload: { kind: 'info', text: 'Saving browser tables…' } });
          await storage.save(); changed = false;
          postMessage({ event: 'solve-line', payload: { kind: 'info', text: 'Browser tables saved.' } });
        }
        finally { syncing = false; }
      }
    } catch (error) {
      retryAfter = Date.now() + 30000;
      postMessage({ event: 'solve-line', payload: { kind: 'error', text: `Cache persistence failed: ${error}. Keep this tab open to retain unsaved tables. Retrying in 30 seconds.` } });
    }
  }, 100);
}
const ready = initialize().catch(error => { failed = true; releaseLock?.(); sendError(error); throw error; });
// Serialize commands with filesystem snapshots. Cancellation is never blocked by
// computation: Rust runs the actual jobs on pthread workers.
let commands = Promise.resolve();
onmessage = ({ data }) => {
  commands = commands.then(async () => {
    try {
      await ready;
      if (failed) throw new Error('Solver runtime stopped. Reload to restart.');
      while (syncing) await new Promise(resolve => setTimeout(resolve, 10));
      const result = data.command === "browser_storage_status"
        ? { ok: true, value: storage.status() } : invoke(data.command, data.args);
      if (result.ok && ['delete_pruning_table', 'reset_app_data'].includes(data.command)) changed = true;
      postMessage({ id: data.id, ...result });
    } catch (error) { postMessage({ id: data.id, ok: false, error: String(error?.message || error) }); }
  });
};
