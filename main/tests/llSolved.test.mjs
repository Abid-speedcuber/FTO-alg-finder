import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import test from 'node:test';
import ts from 'typescript';

const compile = name => ts.transpileModule(readFileSync(new URL(`../src/${name}.ts`, import.meta.url), 'utf8'), {
  compilerOptions: {target: ts.ScriptTarget.ES2020, module: ts.ModuleKind.ESNext},
}).outputText;
const moduleUrl = code => `data:text/javascript;base64,${Buffer.from(code).toString('base64')}`;
const setupUrl = moduleUrl(compile('llSetup'));
const {SOLVED_FACELETS, EMPTY_LL_SELECTION, planLlOption, applyU} = await import(setupUrl);
const {EP_OPTIONS, CP_OPTIONS} = await import(moduleUrl(compile('llOptions').replace('"./llSetup"', JSON.stringify(setupUrl))));

test('Solved L6X and L3C reset only their own group and preserve AUF', () => {
  const ep = EP_OPTIONS.find(option => option.id === 'lunge');
  const cp = CP_OPTIONS.find(option => option.id === 'cw');
  const epSolved = EP_OPTIONS.find(option => option.id === 'solved');
  const cpSolved = CP_OPTIONS.find(option => option.id === 'solved');
  assert.ok(epSolved && cpSolved);
  const epCase = planLlOption(SOLVED_FACELETS, EMPTY_LL_SELECTION, 'EP', ep);
  const both = planLlOption(epCase.next, epCase.selection, 'CP', cp);
  const aligned = applyU(both.next);
  const resetEp = planLlOption(aligned, both.selection, 'EP', epSolved);
  const resetCp = planLlOption(aligned, both.selection, 'CP', cpSolved);
  const justCp = planLlOption(SOLVED_FACELETS, EMPTY_LL_SELECTION, 'CP', cp);
  assert.deepEqual(resetEp.next, applyU(justCp.next));
  assert.deepEqual(resetCp.next, applyU(epCase.next));
  assert.equal(resetEp.selection.CP, 'cw');
  assert.equal(resetCp.selection.EP, 'lunge');
  assert.deepEqual(resetEp.resets, []);
  assert.deepEqual(resetCp.resets, []);
  assert.deepEqual(planLlOption(resetEp.next, resetEp.selection, 'CP', cpSolved).next, applyU(SOLVED_FACELETS));
});
