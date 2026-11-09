import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import ts from 'typescript';
const source = readFileSync(new URL('../src/depthSelection.ts', import.meta.url), 'utf8');
const compiled = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2020, module: ts.ModuleKind.ESNext } }).outputText;
const { parseDepthSelection: parse } = await import(`data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`);

test('empty and explicit auto retain automatic search', () => {
  for (const value of ['', ' ', 'auto', ' AUTO ']) assert.deepEqual(parse(value), { depths: null, error: null });
});
test('print-style inclusive ranges and arbitrary depths are sorted and deduplicated', () => {
  assert.deepEqual(parse('17, 12-15, 20, 13, 19-20').depths, [12,13,14,15,17,19,20]);
  assert.deepEqual(parse('0, 255, 007, 9 - 11, 7-7').depths, [0,7,9,10,11,255]);
  assert.deepEqual(parse('17').depths, [17]);
  assert.equal(parse('0-255').depths.length, 256);
});
test('rejects malformed, descending, fractional and out-of-bounds selections', () => {
  for (const input of ['12-', '-5', '17-12', 'auto,12', '1,,2', '2,', '1 2', '1.5', '1e2', '256', '0-256', '99999999999999999999', '1-2-3']) {
    assert.ok(parse(input).error, input);
    assert.equal(parse(input).depths, null, input);
  }
});
