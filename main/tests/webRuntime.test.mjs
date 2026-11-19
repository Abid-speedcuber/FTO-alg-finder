import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { readFile, stat } from 'node:fs/promises';
import { resolve, extname } from 'node:path';
import { test } from 'node:test';
import { chromium } from 'playwright';

// Build public/web first with ./build.sh --web. Exercise the deployed subpath,
// real shared-memory WASM and IndexedDB, rather than mocking the Rust bridge.
const root = resolve(import.meta.dirname, '../../public');
const mime = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.wasm': 'application/wasm', '.svg': 'image/svg+xml' };
async function server(headers = true) {
  const app = createServer(async (req, res) => {
    try {
      let path = resolve(root, '.' + new URL(req.url, 'http://localhost').pathname);
      if (!path.startsWith(root + '/') && path !== root) throw new Error('outside root');
      if ((await stat(path)).isDirectory()) path += '/index.html';
      if (headers) {
        res.setHeader('Cross-Origin-Opener-Policy', 'same-origin');
        res.setHeader('Cross-Origin-Embedder-Policy', 'require-corp');
      }
      res.setHeader('Content-Type', mime[extname(path)] ?? 'application/octet-stream');
      res.end(await readFile(path));
    } catch { res.writeHead(404).end(); }
  });
  await new Promise(done => app.listen(0, '127.0.0.1', done));
  return { url: `http://127.0.0.1:${app.address().port}`, close: () => new Promise(done => app.close(done)) };
}
// Kept as one sequential test because there is one cache owner per origin.
test('WASM worker: validation, pthread cancellation, Bloom persistence and deletion', { timeout: 600000 }, async () => {
  await stat(root + '/web/wasm/fto_web.wasm');
  const host = await server();
  const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || '/usr/bin/google-chrome', headless: true, args: ['--no-sandbox'] });
  try {
    const page = await browser.newPage();
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.goto(host.url + '/');
    // A same-origin blank document keeps app initialization out of this worker test.
    await page.evaluate(() => { document.body.innerHTML = ''; });
    await page.evaluate(() => {
      window.events = [];
      window.replies = new Map(); window.nextId = 0;
      window.worker = new Worker('./web/solver-worker.js', { type: 'module' });
      window.worker.onerror = error => { window.failure = error.message || 'Worker failed'; };
      window.worker.onmessage = ({ data }) => {
        if (data.fatal) { window.failure = data.fatal; return; }
        if (data.id !== undefined) { const [ok, no] = window.replies.get(data.id); window.replies.delete(data.id); data.ok ? ok(data.value) : no(new Error(data.error)); }
        else window.events.push(data);
      };
      window.command = (command, args = {}) => new Promise((ok, no) => {
        const id = ++window.nextId; window.replies.set(id, [ok, no]); window.worker.postMessage({ id, command, args });
      });
    });
    const command = (name, args) => page.evaluate(([name, args]) => window.command(name, args), [name, args]);
    await page.waitForFunction(() => window.failure || window.events.some(e => e.ready), null, { timeout: 30000 });
    assert.equal(await page.evaluate(() => window.failure), undefined);
    const valid = await command('validate_facelets', { facelets: Array.from({ length: 72 }, (_, i) => Math.floor(i / 9)) });
    assert.deepEqual(valid.errors, []);
    assert.equal((await command('get_gpu_profile')).eligible, false);
    await assert.rejects(command('unknown_command'), /Unknown browser command/);
    await command('generate_bloom_table', { request: { moves: ['U', "U'"], depth: 1, lastLayerMode: false, maxMemoryMib: 1 } });
    await page.waitForFunction(() => window.failure || window.events.some(e => ['bloom-generated', 'solve-error'].includes(e.event)), null, { timeout: 360000 });
    assert.equal(await page.evaluate(() => window.failure), undefined);
    assert.equal(await page.evaluate(() => window.events.find(e => e.event === 'solve-error')?.payload), undefined);
    await page.waitForFunction(() => window.events.some(e => e.payload?.text === 'Browser tables saved.'), null, { timeout: 60000 });
    const diskOnly = await command('browser_storage_status');
    assert.equal(diskOnly.residentFileBytes, 0);
    assert.ok(diskOnly.lazyFiles >= 2);
    const tables = await command('list_pruning_tables');
    assert.ok(tables.some(t => t.kind === 'bloom'));
    // Recreate the worker on the same origin to restore the persisted filesystem.
    await page.evaluate(() => window.worker.terminate());
    await page.waitForTimeout(200);
    await page.evaluate(() => {
      window.events = [];
      window.worker = new Worker('./web/solver-worker.js', { type: 'module' });
      window.worker.onerror = error => { window.failure = error.message || 'Worker failed'; };
      window.worker.onmessage = ({ data }) => {
        if (data.fatal) window.failure = data.fatal;
        else if (data.id !== undefined) { const [ok, no] = window.replies.get(data.id); window.replies.delete(data.id); data.ok ? ok(data.value) : no(new Error(data.error)); }
        else window.events.push(data);
      };
    });
    await page.waitForFunction(() => window.failure || window.events.some(e => e.ready), null, { timeout: 30000 });
    assert.equal(await page.evaluate(() => window.failure), undefined);
    assert.equal((await command('browser_storage_status')).residentFileBytes, 0);
    assert.deepEqual((await command('list_pruning_tables')).map(t => t.id), tables.map(t => t.id));
    // A real partial goal whose bottom corner is unaffected by either U turn.
    const partial = Array(72).fill(8);
    for (const slot of [27, 36, 18, 45]) partial[slot] = Math.floor(slot / 9);
    await command('solve_fto', { request: {
      facelets: partial, allowedMoves: ['U', "U'"], instanceMoves: ['U', "U'"],
      depths: [1], findAll: true, restrictedPruning: true, miniPruning: false,
      lastLayerMode: false, threads: 8,
    } });
    await page.waitForFunction(() => window.failure || window.events.some(e => ['solve-result', 'solve-error'].includes(e.event)), null, { timeout: 30000 });
    assert.equal(await page.evaluate(() => window.events.find(e => e.event === 'solve-error')?.payload), undefined);
    const solutions = await page.evaluate(() => window.events.find(e => e.event === 'solve-result').payload.solutions);
    assert.deepEqual([...solutions].sort(), ['U', "U'"].sort());
    await page.evaluate(() => { window.events = []; });
    await command('start_batch', { request: {
      runId: 'wasm-batch', cases: Array.from({ length: 8 }, (_, i) => ({ id: `case-${i}`, setup: '', facelets: partial })),
      allowedMoves: ['U', "U'"], instanceMoves: ['U', "U'"], depths: [1], findAll: true,
      temporaryTables: false, tableMemoryMib: 0, lastLayerMode: false, threads: 8, slowCaseSeconds: 0,
    } });
    await page.waitForFunction(() => window.failure || window.events.some(e => e.event === 'batch-event' && ['done', 'error'].includes(e.payload.kind)), null, { timeout: 30000 });
    assert.equal(await page.evaluate(() => window.failure), undefined);
    assert.equal(await page.evaluate(() => window.events.find(e => e.event === 'batch-event' && e.payload.kind === 'error')?.payload.message), undefined);
    const finished = await page.evaluate(() => window.events.filter(e => e.event === 'batch-event' && e.payload.case?.firstDepth === 1).map(e => e.payload.caseId));
    assert.equal(new Set(finished).size, 8);
    await page.evaluate(() => { window.events = []; });
    // A large requested count is accepted without creating idle workers for a
    // tiny search whose finite branches are exhausted during task splitting.
    await command('start_batch', { request: {
      runId: 'large-thread-count', cases: [{ id: 'tiny-case', setup: '', facelets: partial }],
      allowedMoves: ['U', "U'"], instanceMoves: ['U', "U'"], depths: [1], findAll: true,
      temporaryTables: false, tableMemoryMib: 0, lastLayerMode: false, threads: 2048, slowCaseSeconds: 0,
    } });
    await page.waitForFunction(() => window.failure || window.events.some(e => e.event === 'batch-event' && ['done', 'error'].includes(e.payload.kind)), null, { timeout: 30000 });
    assert.equal(await page.evaluate(() => window.events.find(e => e.event === 'batch-event' && e.payload.kind === 'error')?.payload.message), undefined);
    await command('unload_pruning_table');
    assert.equal((await command('pruning_cache_status')).hasTables, false);
    assert.equal((await command('browser_storage_status')).residentFileBytes, 0);
    await page.evaluate(() => { window.events = []; });
    // Start a deep build and cancel while pthread work is active.
    await command('generate_bloom_table', { request: { moves: ['U', "U'", 'R', "R'", 'F', "F'"], depth: 16, lastLayerMode: false, maxMemoryMib: 32 } });
    await command('stop_solve');
    await page.waitForFunction(() => window.failure || window.events.some(e => ['solve-cancelled', 'solve-error'].includes(e.event)), null, { timeout: 30000 });
    assert.equal(await page.evaluate(() => window.failure), undefined);
    assert.equal(await page.evaluate(() => window.events.find(e => e.event === 'solve-error')?.payload), undefined);
    assert.equal((await command('get_solver_status')).running, false);
    await page.evaluate(() => { window.events = []; });
    await command('delete_pruning_table', { id: tables.find(t => t.kind === 'bloom').id });
    await page.waitForFunction(() => window.events.some(e => e.payload?.text === 'Browser tables saved.'), null, { timeout: 60000 });
    assert.equal((await command('list_pruning_tables')).some(t => t.kind === 'bloom'), false);
    const savedPaths = await page.evaluate(() => new Promise((resolve, reject) => {
      const request = indexedDB.open('fto-wasm-files-v1', 1);
      request.onsuccess = () => {
        const db = request.result;
        const get = db.transaction('manifest').objectStore('manifest').get('current');
        get.onsuccess = () => { resolve(Object.keys(get.result.files)); db.close(); };
        get.onerror = () => { reject(get.error); db.close(); };
      };
      request.onerror = () => reject(request.error);
    }));
    assert.equal(savedPaths.some(path => path.endsWith('.bloom')), false);
    assert.deepEqual(errors, []);
  } finally { await browser.close(); await host.close(); }
});

test('static host without headers establishes isolation through the service worker', { timeout: 45000 }, async () => {
  const host = await server(false);
  const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || '/usr/bin/google-chrome', headless: true, args: ['--no-sandbox'] });
  try {
    const page = await browser.newPage();
    await page.goto(host.url + '/web/');
    await page.waitForFunction(() => window.crossOriginIsolated, null, { timeout: 30000 });
    assert.equal(await page.evaluate(() => typeof SharedArrayBuffer), 'function');
    await page.getByRole('heading', { name: 'FTO Alg Finder', exact: true }).waitFor();
    if (await page.getByRole('heading', { name: 'Set up your default instance', exact: true }).isVisible()) {
      await page.getByRole('button', { name: 'Save', exact: true }).click();
    }
    await page.getByRole('button', { name: 'Open menu', exact: true }).click();
    await page.getByRole('button', { name: 'Settings', exact: true }).click();
    await page.getByText('This browser version uses CPU table generation. GPU acceleration is not available yet.', { exact: true }).waitFor();
    assert.equal(await page.getByLabel('Use GPU acceleration by default for table generation').isDisabled(), true);
  } finally { await browser.close(); await host.close(); }
});
