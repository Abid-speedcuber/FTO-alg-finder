// On-demand filesystem backed by bounded IndexedDB records. Stock IDBFS stores an entire
// file in one value; the normal 211 MiB transition table exceeds Chrome's limit.
const CHUNK_BYTES = 16 * 1024 * 1024;
const DATABASE = 'fto-wasm-files-v1';
function transaction(db, name, mode, operation) {
  return new Promise((resolve, reject) => {
    const tx = db.transaction(name, mode);
    let request;
    try { request = operation(tx.objectStore(name)); }
    catch (error) { tx.abort(); reject(error); return; }
    tx.oncomplete = () => resolve(request?.result);
    tx.onabort = tx.onerror = () => reject(tx.error || request?.error || new Error('Cache transaction failed'));
  });
}
async function openDatabase() {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(DATABASE, 1);
    request.onupgradeneeded = () => {
      request.result.createObjectStore('chunks');
      request.result.createObjectStore('manifest');
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
    request.onblocked = () => reject(new Error('Close other solver tabs to open storage'));
  });
}
export async function openStorage(FS) {
  const db = await openDatabase();
  let manifest = await transaction(db, 'manifest', 'readonly', store => store.get('current')) || { files: {}, directories: [] };
  // Rust's filesystem API is synchronous. A second worker reads IndexedDB
  // asynchronously and signals this host worker through bounded shared buffers.
  const reader = new Worker(new URL('./solver-storage-reader.js', import.meta.url), { type: 'module', name: 'FTO storage' });
  await new Promise((resolve, reject) => {
    reader.onmessage = ({ data }) => data.ready ? resolve() : reject(new Error(data.error));
    reader.onerror = event => reject(new Error(event.message || 'Storage worker failed'));
  });
  const control = new Int32Array(new SharedArrayBuffer(8));
  const shared = new Uint8Array(new SharedArrayBuffer(CHUNK_BYTES));
  let cachedKey;
  let cachedLength = 0;
  let readFailure;
  function chunk(key) {
    if (readFailure) throw readFailure;
    if (cachedKey === key) return shared.subarray(0, cachedLength);
    Atomics.store(control, 0, 0);
    reader.postMessage({ key, control: control.buffer, buffer: shared.buffer });
    if (Atomics.wait(control, 0, 0, 60000) === 'timed-out') {
      readFailure = new Error('IndexedDB read timed out. Reload the solver to retry.');
      throw readFailure;
    }
    const length = Atomics.load(control, 1);
    if (Atomics.load(control, 0) !== 1) throw new Error(new TextDecoder().decode(shared.subarray(0, length)));
    cachedKey = key; cachedLength = length;
    return shared.subarray(0, length);
  }
  const lazy = new WeakMap();
  function makeLazy(path, info) {
    const node = FS.lookupPath(path).node;
    if (lazy.has(node)) return;
    const streamOps = node.stream_ops;
    const nodeOps = node.node_ops;
    function writable() {
      if (!lazy.has(node)) return;
      const data = new Uint8Array(info.size);
      for (let index = 0, offset = 0; index < info.chunks; index++) {
        const bytes = chunk(`${info.id}:${index}`);
        data.set(bytes, offset); offset += bytes.length;
      }
      node.contents = data;
      node.usedBytes = info.size;
      node.stream_ops = streamOps; node.node_ops = nodeOps;
      lazy.delete(node);
    }
    node.contents = null;
    node.usedBytes = info.size;
    lazy.set(node, info);
    node.stream_ops = {
      ...streamOps,
      read(stream, target, offset, length, position) {
        if (!lazy.has(node)) return streamOps.read(stream, target, offset, length, position);
        const count = Math.max(0, Math.min(length, info.size - position));
        for (let done = 0; done < count;) {
          const index = Math.floor((position + done) / CHUNK_BYTES);
          const start = (position + done) % CHUNK_BYTES;
          const bytes = chunk(`${info.id}:${index}`);
          const part = Math.min(count - done, bytes.length - start);
          if (part <= 0) throw new Error(`Incomplete saved cache: ${path}`);
          target.set(bytes.subarray(start, start + part), offset + done); done += part;
        }
        return count;
      },
      write(...args) { writable(); return streamOps.write(...args); },
      mmap: undefined,
    };
    node.node_ops = {
      ...nodeOps,
      setattr(target, attributes) {
        if (attributes.size !== undefined) {
          if (attributes.size === 0) {
            node.contents = null; node.usedBytes = 0;
            node.stream_ops = streamOps; node.node_ops = nodeOps; lazy.delete(node);
          } else writable();
        }
        return nodeOps.setattr(target, attributes);
      },
    };
  }
  FS.mkdir('/fto');
  for (const path of manifest.directories) FS.mkdirTree(path);
  for (const [path, info] of Object.entries(manifest.files)) {
    FS.mkdirTree(path.slice(0, path.lastIndexOf('/')));
    FS.writeFile(path, new Uint8Array(0));
    FS.utime(path, info.mtime, info.mtime);
    makeLazy(path, info);
  }
  return {
    status() {
      let files = 0, lazyFiles = 0, residentFileBytes = 0;
      function walk(directory) {
        for (const name of FS.readdir(directory)) {
          if (name === '.' || name === '..') continue;
          const path = `${directory}/${name}`;
          const node = FS.lookupPath(path).node;
          if (FS.isDir(node.mode)) walk(path);
          else if (FS.isFile(node.mode)) {
            files++; if (lazy.has(node)) lazyFiles++;
            residentFileBytes += node.contents?.byteLength || 0;
          }
        }
      }
      walk('/fto');
      return { files, lazyFiles, residentFileBytes, readBufferBytes: CHUNK_BYTES };
    },
    async save() {
      const next = { files: {}, directories: [] };
      const paths = [];
      function walk(directory) {
        for (const name of FS.readdir(directory)) {
          if (name === '.' || name === '..') continue;
          const path = `${directory}/${name}`;
          const stat = FS.stat(path);
          if (FS.isDir(stat.mode)) { next.directories.push(path); walk(path); }
          else if (FS.isFile(stat.mode)) paths.push([path, stat]);
        }
      }
      walk('/fto');
      for (const [path, stat] of paths) {
        const mtime = stat.mtime.getTime();
        const previous = manifest.files[path];
        if (previous && previous.size === stat.size && previous.mtime === mtime) {
          next.files[path] = previous; continue;
        }
        const info = { id: crypto.randomUUID(), size: stat.size, mtime, chunks: Math.ceil(stat.size / CHUNK_BYTES) };
        const stream = FS.open(path, 'r');
        try {
          const buffer = new Uint8Array(Math.min(CHUNK_BYTES, stat.size));
          for (let index = 0, offset = 0; index < info.chunks; index++) {
            const length = Math.min(CHUNK_BYTES, stat.size - offset);
            if (FS.read(stream, buffer, 0, length, offset) !== length) throw new Error(`Cache changed while saving: ${path}`);
            await transaction(db, 'chunks', 'readwrite', store => store.put(buffer.subarray(0, length), `${info.id}:${index}`));
            offset += length;
          }
        } finally { FS.close(stream); }
        next.files[path] = info;
      }
      // Publish only after every file is complete. Failed writes leave the last
      // committed snapshot readable, even if the tab closes in the middle.
      await transaction(db, 'manifest', 'readwrite', store => store.put(next, 'current'));
      manifest = next;
      // Saved file contents stay on disk. Retain metadata and one shared chunk,
      // while the Rust cache independently owns any tables actively in use.
      for (const [path, info] of Object.entries(next.files)) makeLazy(path, info);
      const live = new Set(Object.values(next.files).map(info => info.id));
      await new Promise((resolve, reject) => {
        const tx = db.transaction('chunks', 'readwrite');
        const cursor = tx.objectStore('chunks').openKeyCursor();
        cursor.onsuccess = () => {
          const entry = cursor.result;
          if (!entry) return;
          if (!live.has(String(entry.key).split(':')[0])) tx.objectStore('chunks').delete(entry.key);
          entry.continue();
        };
        tx.oncomplete = resolve;
        tx.onabort = tx.onerror = () => reject(tx.error);
      });
    },
  };
}
