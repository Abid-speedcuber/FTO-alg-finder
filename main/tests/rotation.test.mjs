import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import vm from 'node:vm';
import ts from 'typescript';

const source = readFileSync(new URL('../src/rotation.ts', import.meta.url), 'utf8');
const compiled = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2020, module: ts.ModuleKind.ESNext } }).outputText;
const { FACES, DEFAULT_HOLD, parseExecutionHold, rotateAlgorithm, normalizeNotation, normalizeMoveSelection, resolveInputAlgorithm, invertInputAlgorithm } = await import(`data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`);
const context = vm.createContext({ $: { UDPOLY_RE: '' }, DEBUG: false });
vm.runInContext(readFileSync(new URL('../public/fto-viewer/poly3dlib.js', import.meta.url), 'utf8'), context);
const puzzle = context.poly3d.makePuzzle(8, [-5, 1 / 3, -1 / 3], [], [-5]);
const normals = Object.fromEntries(Array.from(puzzle.faceNames, (name, i) => [name.toUpperCase(), puzzle.facePlanes[i].norm]));
const dot = (a, b) => a.inprod(b);
const determinant = (a, b, c) => a.inprod(b.outprod(c));
// In EIF the side names are shifted one position around U relative to CIF.
const eifToOriginal = { U: 'U', D: 'D', F: 'R', L: 'F', BL: 'L', B: 'BL', BR: 'B', R: 'BR' };
const opposite = { U: 'D', D: 'U', F: 'B', B: 'F', R: 'BL', BL: 'R', L: 'BR', BR: 'L' };
const holds = FACES.flatMap(up => FACES.map(front => parseExecutionHold(`${up},${front}`)).filter(Boolean));

test('all 48 holds match the viewer geometry and preserve handedness', () => {
  assert.equal(holds.length, 48);
  assert.equal(holds.filter(hold => hold.notation === 'CIF').length, 24);
  for (const hold of holds) {
    const [up, front] = hold.input.split(',');
    assert.equal(hold.faces[up], 'U');
    assert.equal(hold.faces[front], 'F');
    assert.equal(new Set(Object.values(hold.faces)).size, 8);
    const target = face => normals[hold.notation === 'CIF' ? face : eifToOriginal[face]];
    for (const a of FACES) {
      assert.equal(hold.faces[opposite[a]], opposite[hold.faces[a]]);
      for (const b of FACES) {
        assert.ok(Math.abs(dot(normals[a], normals[b]) - dot(target(hold.faces[a]), target(hold.faces[b]))) < 1e-8, `${hold.input}: ${a},${b}`);
      }
    }
    assert.ok(determinant(normals.U, normals.F, normals.R) * determinant(target(hold.faces.U), target(hold.faces.F), target(hold.faces.R)) > 0, `${hold.input} mirrored the alg`);
  }
});

test('CIF rotations conjugate the actual viewer face and wide-move permutations', () => {
  const centers = [];
  puzzle.enumFacesPolys((face, p, poly, i) => { centers[i] = poly.center; });
  for (const hold of holds.filter(h => h.notation === 'CIF')) {
    // Solve a linear map from three independent original face normals to their
    // targets, then match the rotated sticker centers independently of the mapper.
    const a = normals.U, b = normals.F, c = normals.R;
    const det = determinant(a, b, c);
    const transform = v => normals[hold.faces.U].scalar(v.inprod(b.outprod(c)) / det)
      .add(normals[hold.faces.F], v.inprod(c.outprod(a)) / det)
      .add(normals[hold.faces.R], v.inprod(a.outprod(b)) / det);
    const rotated = centers.map(v => {
      const point = transform(v);
      const index = centers.findIndex(other => point.abs(other) < 1e-6);
      assert.notEqual(index, -1, `${hold.input}: sticker has no rotated counterpart`);
      return index;
    });
    for (const face of FACES) for (const wide of [false, true]) {
      const axis = f => `${wide ? 2 : 1}${f[0]}${f.slice(1).toLowerCase()}`;
      const original = puzzle.moveTable[puzzle.getTwistyIdx(axis(face))];
      const mapped = puzzle.moveTable[puzzle.getTwistyIdx(axis(hold.faces[face]))];
      assert.ok(original && mapped);
      for (let i = 0; i < centers.length; i++) {
        assert.equal(rotated[original[i] < 0 ? i : original[i]], mapped[rotated[i]] < 0 ? rotated[i] : mapped[rotated[i]], `${hold.input}: ${face}${wide ? 'w' : ''}`);
      }
    }
  }
});

test('identity, requested R,F hold, normalization, and invalid inputs', () => {
  const alg = "U R' (F U F') u r' Rs Us' Fs";
  assert.equal(rotateAlgorithm(alg, DEFAULT_HOLD), alg);
  assert.equal(parseExecutionHold(' r, f ').input, 'R,F');
  assert.equal(parseExecutionHold('R,F').notation, 'EIF');
  assert.equal(rotateAlgorithm("R F R'", parseExecutionHold('R,F')), "U F U'");
  for (const input of ['U', 'U,D', 'R,BL', 'F,B', 'U,U', 'X,F', 'U,F,R']) assert.equal(parseExecutionHold(input), null);
  assert.equal(parseExecutionHold('').input, 'U,F');
  assert.equal(parseExecutionHold('u,br').input, 'U,BR');
});

test('wide moves and triggers translate atomically without replacing partial face names', () => {
  for (const hold of holds) {
    for (const face of FACES) {
      assert.equal(rotateAlgorithm(`${face.toLowerCase()}'`, hold), `${hold.faces[face].toLowerCase()}'`);
      assert.equal(rotateAlgorithm(`${face}2`, hold), `${hold.faces[face]}2`);
    }
    assert.equal(rotateAlgorithm("(R U R')", hold), `(${hold.faces.R} ${hold.faces.U} ${hold.faces.R}')`);
    assert.equal(rotateAlgorithm('unknown RUR Rwrong', hold), 'unknown RUR Rwrong');
  }
});

test('slice names follow their face for all execution holds', () => {
  for (const hold of holds) for (const face of FACES) for (const suffix of ["", "'", "2"]) {
    const mapped = hold.faces[face];
    const reverse = ['D', 'B', 'BR', 'BL'].includes(mapped);
    assert.equal(rotateAlgorithm(`${face}s${suffix}`, hold), `${reverse ? opposite[mapped] : mapped}s${reverse ? (suffix ? '' : "'") : suffix}`);
  }
  assert.equal(normalizeNotation("Uw Fw' Rw Lw' M M' E E' S S'"), "u f' r l' Rs' Rs Us' Us Fs Fs'");
});

test('viewer parses standard wide and slice notation on every face', () => {
  const viewer = readFileSync(new URL('../public/fto-viewer/fto-viewer.js', import.meta.url), 'utf8');
  context.puzzle = puzzle;
  context.options = {};
  puzzle.parser = context.poly3d.makePuzzleParser(puzzle);
  vm.runInContext(viewer.slice(viewer.indexOf('    function parseAlgorithm(algorithm)'), viewer.indexOf('    function setupCamera()')), context);
  const parse = alg => JSON.parse(JSON.stringify(context.parseAlgorithm(alg)));
  for (const face of FACES) {
    for (const suffix of ['', "'", '2']) {
      assert.equal(parse(`${face.toLowerCase()}${suffix}`).length, 1);
      assert.deepEqual(parse(`${face.toLowerCase()}${suffix}`), parse(`2${face[0]}${face.slice(1).toLowerCase()}${suffix}`));
    }
    assert.deepEqual(parse(`${face}s`), parse(`${face.toLowerCase()} ${face}'`));
    assert.deepEqual(parse(`${face}s'`), parse(`${face.toLowerCase()}' ${face}`));
  }
  context.options.resolveAlgorithm = resolveInputAlgorithm;
  assert.deepEqual(parse("(Rs Ls' Us Fs')"), parse("r R' l' L u U' f' F"));
  assert.deepEqual(parse("M E' S N"), parse("Rs' Us Fs Ls"));
  assert.deepEqual(parse("{U,R} R U R' U'"), parse("BR U BR' U'"));
  assert.deepEqual(parse("R' {U,L} R U R' U'"), parse("R' F U F' U'"));
  assert.deepEqual(parse("Uw Fw' Rw Lw' M E S N"), parse("u f' r l' Rs' Us' Fs Ls"));
});

test('selectable moves include every wide face and all four slice axes', () => {
  const app = readFileSync(new URL('../src/App.tsx', import.meta.url), 'utf8');
  const selected = vm.runInNewContext(app.match(/const moves = (\[[\s\S]*?\]);/)[1]);
  assert.equal(selected.length, 40);
  assert.equal(new Set(selected).size, 40);
  assert.ok(selected.every(move => !move.includes("(")));
  for (const face of FACES) for (const suffix of ['', "'"]) assert.ok(selected.includes(face.toLowerCase() + suffix));
  for (const face of ['R', 'L', 'U', 'F']) for (const suffix of ['', "'"]) assert.ok(selected.includes(face + 's' + suffix));
});

test('saved move selections drop three-turn triggers and retain standard moves', () => {
  assert.deepEqual(normalizeMoveSelection(["R", "Uw", "M", "(R U R')", "(R U' R')", "(R' U R)", "(R' U' R)", "(F U F')", "(F U' F')", "(F' U F)", "(F' U' F)", "Ls"]), ["R", "u", "Rs'", "Ls"]);
});

test('input orientations apply to following moves and resolve before inversion', () => {
  assert.equal(resolveInputAlgorithm("{U,R} R U R' U'"), "BR U BR' U'");
  assert.equal(resolveInputAlgorithm("R' {U,L} R U R' U'"), "R' F U F' U'");
  assert.equal(resolveInputAlgorithm("{U,R}R { U, F }R"), "BR R");
  assert.equal(resolveInputAlgorithm("{U,R} r Rs N'"), "br Ls' Fs'");
  assert.equal(resolveInputAlgorithm("Uw Fwi M E' S N Ni Lsi Ri R2"), "u f' Rs' Us Fs Ls Ls' Ls' R' R2");
  assert.equal(invertInputAlgorithm("R' {U,L} R U R' U'"), "U F U' F' R");
  assert.equal(resolveInputAlgorithm('R'), 'R');
  for (const input of ['{} R', '{U,D} R', '{U,U} R', '{X,R} R', '{U,R', 'R } U']) assert.throws(() => resolveInputAlgorithm(input));
  for (const hold of holds) for (const face of FACES) {
    assert.equal(resolveInputAlgorithm(`{${hold.input}} ${hold.faces[face]}`), face);
    assert.equal(resolveInputAlgorithm(`{${hold.input}} ${hold.faces[face].toLowerCase()}`), face.toLowerCase());
  }
});

test('algorithm combiner resolves aliases and orientations before cancellation', async () => {
  const combineSource = readFileSync(new URL('../src/combine.ts', import.meta.url), 'utf8');
  const rotationUrl = `data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`;
  const combineJs = ts.transpileModule(combineSource, { compilerOptions: { target: ts.ScriptTarget.ES2020, module: ts.ModuleKind.ESNext } }).outputText.replace('"./rotation"', JSON.stringify(rotationUrl));
  const { tokenizeAlg, cancelTokens } = await import(`data:text/javascript;base64,${Buffer.from(combineJs).toString('base64')}`);
  assert.deepEqual(tokenizeAlg("R' {U,L} (R U R' U') N"), ["R'", "F", "U", "F'", "U'", "Rs'"]);
  assert.deepEqual(cancelTokens(tokenizeAlg("Rw r' N Ls' M Rs")), []);
});
