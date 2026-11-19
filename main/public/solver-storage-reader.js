// IndexedDB runs on this worker while the Emscripten host synchronously services
// Rust filesystem reads. The page's UI thread is never blocked.
const request = indexedDB.open('fto-wasm-files-v1', 1);
let db;
request.onsuccess = () => { db = request.result; postMessage({ ready: true }); };
request.onerror = () => postMessage({ error: String(request.error) });
request.onupgradeneeded = () => {
  request.result.createObjectStore('chunks');
  request.result.createObjectStore('manifest');
};
onmessage = ({ data }) => {
  const control = new Int32Array(data.control);
  const buffer = new Uint8Array(data.buffer);
  const finish = (value, error = false) => {
    const bytes = error ? new TextEncoder().encode(String(value)) : value;
    buffer.set(bytes.subarray(0, buffer.length));
    Atomics.store(control, 1, Math.min(bytes.length, buffer.length));
    Atomics.store(control, 0, error ? 2 : 1);
    Atomics.notify(control, 0);
  };
  try {
    const read = db.transaction('chunks').objectStore('chunks').get(data.key);
    read.onsuccess = () => {
      if (!(read.result instanceof Uint8Array)) finish(`Missing saved cache chunk: ${data.key}`, true);
      else finish(read.result);
    };
    read.onerror = () => finish(read.error, true);
  } catch (error) { finish(error, true); }
};
