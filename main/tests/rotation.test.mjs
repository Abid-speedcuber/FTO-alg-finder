import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import vm from 'node:vm';
import ts from 'typescript';

const source = readFileSync(new URL('../src/rotation.ts', import.meta.url), 'utf8');
const compiled = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2020, module: ts.ModuleKind.ESNext } }).outputText;
const { FACES, DEFAULT_HOLD, parseExecutionHold, rotateAlgorithm } = await import(`data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`);
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
  const alg = "U R' (F U F') Uw Rw' M E' S";
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
      assert.equal(rotateAlgorithm(`${face}w'`, hold), `${hold.faces[face]}w'`);
      assert.equal(rotateAlgorithm(`${face}2`, hold), `${hold.faces[face]}2`);
    }
    assert.equal(rotateAlgorithm("(R U R')", hold), `(${hold.faces.R} ${hold.faces.U} ${hold.faces.R}')`);
    assert.equal(rotateAlgorithm('unknown RUR Rwrong', hold), 'unknown RUR Rwrong');
  }
});

test('slice translation respects the core M/E/S definitions and inverses', () => {
  for (const hold of holds.filter(h => h.input !== 'U,F')) {
    for (const [slice, face, inverse] of [['M', 'R', true], ['E', 'U', true], ['S', 'F', false]]) {
      const mapped = hold.faces[face];
      assert.equal(rotateAlgorithm(slice, hold), `${mapped}w${inverse ? "'" : ''} ${mapped}${inverse ? '' : "'"}`);
      assert.equal(rotateAlgorithm(`${slice}'`, hold), `${mapped}w${inverse ? '' : "'"} ${mapped}${inverse ? "'" : ''}`);
    }
  }
});
