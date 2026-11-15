import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import vm from 'node:vm';
import test from 'node:test';

for (const path of ['../public/fto-viewer/fto-viewer.js', '../../public/fto-viewer/fto-viewer.js']) {
  test(`${path}: wide and rear-face shortcuts queue real moves and respect text focus`, () => {
    const source = readFileSync(new URL(path, import.meta.url), 'utf8');
    const moves = [], handlers = {};
    const document = {activeElement: null, addEventListener(name, handler) {handlers[name] = handler;}, removeEventListener() {}};
    const context = vm.createContext({$: {UDPOLY_RE: ''}, DEBUG: false, document, queueMove(axis, power) {moves.push([axis, power]);}});
    vm.runInContext(readFileSync(new URL('../public/fto-viewer/poly3dlib.js', import.meta.url), 'utf8'), context);
    vm.runInContext(`var puzzle = poly3d.makePuzzle(8, [-5, 1/3, -1/3], [], [-5]); puzzle.parser = poly3d.makePuzzleParser(puzzle);`, context);
    const parseStart = source.indexOf('    function parseMove(move) {');
    const keyboardStart = source.indexOf('    function bindKeyboard() {');
    vm.runInContext(source.match(/var ftoKeymap = "[^"]+";/)[0] +
      source.slice(parseStart, source.indexOf('    function applyAlgorithm(', parseStart)) +
      source.slice(keyboardStart, source.indexOf('    function render()', keyboardStart)) + 'bindKeyboard();', context);
    for (const [key, axis, power] of [['U','2R',1],['M','2R',-1],['R','2L',-1],['V','2L',1],['8','1Br',1],[',','1Br',-1],['C','1Bl',1],['3','1Bl',-1]]) {
      let prevented = false;
      handlers.keydown({keyCode: key.charCodeAt(0), target: {}, preventDefault() {prevented = true;}});
      assert.deepEqual(moves.at(-1), [axis, power], `shortcut ${key}`);
      assert.ok(prevented);
      assert.notEqual(context.puzzle.getTwistyIdx(axis), -1);
    }
    const count = moves.length;
    for (const tagName of ['INPUT', 'TEXTAREA', 'SELECT']) {
      handlers.keydown({keyCode: 85, target: {tagName}});
      document.activeElement = {tagName};
      handlers.keydown({keyCode: 85, target: {}});
      document.activeElement = null;
    }
    for (const modifier of ['ctrlKey', 'metaKey', 'altKey']) handlers.keydown({keyCode: 85, target: {}, [modifier]: true});
    assert.equal(moves.length, count, 'typing and modified keys must not turn the puzzle');
  });
}
