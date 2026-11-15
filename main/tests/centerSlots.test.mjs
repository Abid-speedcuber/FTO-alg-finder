import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import test from 'node:test';

// Exercise the viewer's real event handlers and state API without a renderer.
// Only the scene construction and screen hit-test are replaced.
function viewer(path) {
  const source = readFileSync(new URL(path, import.meta.url), 'utf8');
  const start = source.indexOf('    applyFaceColors(options.faceColors, false);');
  const end = source.indexOf('    return {\n      applyAlgorithm:', start);
  const fills = [], borders = [], events = {};
  class Quaternion {
    multiply() { return this; } multiplySelf() { return this; }
    copy() { return this; } setFromAxisAngle() { return this; }
  }
  const context = vm.createContext({
    fills, borders, performance, $: { UDPOLY_RE: "" }, DEBUG: false,
    THREE: { Quaternion, Vector3: class {} },
    window: { addEventListener: (name, handler) => { events[name] = handler; }, removeEventListener() {} },
  });
  const init = `
    puzzle = poly3d.makePuzzle(8, [-5, 1 / 3, -1 / 3], [], [-5]);
    puzzle.parser = poly3d.makePuzzleParser(puzzle);
    puzzle.enumFacesPolys(function(face, p, poly, index) {
      var slot = polyFaceToFaceletFace[face] * 9 + polyStickerToFaceletSlot[face][p];
      faceletToSticker[slot] = index;
      faceletColors[slot] = Math.floor(slot / 9);
      cubePieces[index] = [null, { matrix: { copy: function() {} }, update: function() {} },
        faceletColors[slot], slot,
        { materials: [{ color: { setHex: function(hex) { fills[slot] = hex; } } }] },
        { color: { setHex: function(hex) { borders[slot] = hex; } } }];
    });
    render = function() {};
    pickSticker = function(x) { return x >= 0 && x < 72 ? faceletToSticker[x] : null; };
    canvas = { style: {} };
    var mouseCleanup = setupMouseControls(canvas);
    var unbindKeyboard = function() {};
    function setKeyboardEnabled() {}
  `;
  vm.runInContext(readFileSync(new URL("../public/fto-viewer/poly3dlib.js", import.meta.url), "utf8"), context);
  vm.runInContext(source.slice(0, start) + init + source.slice(end), context);
  const container = { style: {}, addEventListener: (name, handler) => { events[name] = handler; }, removeEventListener() {} };
  const api = context.window.createFtoViewer(container, {});
  return { api, borders, rightClick(facelet) {
    events.contextmenu({clientX:facelet, clientY:0, preventDefault() {}});
  }, click(facelet, shiftKey = true) {
    const event = { button: 0, clientX: facelet, clientY: 0, shiftKey, ctrlKey: false, preventDefault() {} };
    events.mousedown(event); events.mouseup(event);
  } };
}

for (const path of ['../public/fto-viewer/fto-viewer.js', '../../public/fto-viewer/fto-viewer.js']) {
  test(`${path}: Shift-click toggles home slots in every mode without painting`, () => {
    for (const mode of ['pan', 'paint', 'swap']) {
      const { api, click, borders } = viewer(path);
      api.setMode(mode); api.setColor(0);
      const before = Array.from(api.getFacelets());
      click(20); click(23); // Red home slots, independent of selected paint color.
      assert.deepEqual(Array.from(api.getCenterTargets().ufSlots[2]), [6, 7]);
      assert.equal(borders[20], 0xffd700); assert.equal(borders[23], 0xffd700);
      assert.deepEqual(Array.from(api.getFacelets()), before);
      click(20);
      assert.deepEqual(Array.from(api.getCenterTargets().ufSlots[2]), [7]);
      assert.equal(borders[20], 0x000000);
      click(0); // Corner: no slot toggle and no paint.
      assert.deepEqual(Array.from(api.getFacelets()), before);
    }
  });
  test(`${path}: golden outlines survive swap selection, clear on reset, and restore when supported`, () => {
    const { api, click, borders } = viewer(path);
    click(20); click(65);
    const saved = api.getCenterTargets();
    api.setMode('swap'); click(20, false);
    assert.equal(borders[20], 0xff80d4);
    api.setMode('pan'); assert.equal(borders[20], 0xffd700);
    api.resetPuzzle(); assert.equal(borders[20], 0x000000); assert.equal(borders[65], 0x000000);
    if (!api.setCenterTargets) return;
    api.setCenterTargets(saved);
    assert.equal(borders[20], 0xffd700); assert.equal(borders[65], 0xffd700);
    saved.ufSlots[2].push(7);
    assert.deepEqual(Array.from(api.getCenterTargets().ufSlots[2]), [6]);
    const legacy = api.getCenterTargets(); delete legacy.ufSlots; delete legacy.rlSlots;
    api.setCenterTargets(legacy);
    assert.equal(borders[20], 0x000000);
  });
}


for (const path of ['../public/fto-viewer/fto-viewer.js', '../../public/fto-viewer/fto-viewer.js']) {
  test(`${path}: right-click toggles corners, edges, and centers back to their previous colors`, () => {
    const {api, rightClick, click, borders} = viewer(path);
    api.applyAlgorithmInstant("R U F");
    const before = Array.from(api.getFacelets());
    for (const slots of [[0,54,9,63], [1,57], [20]]) {
      rightClick(slots[0]);
      for (const slot of slots) assert.equal(api.getFacelets()[slot], 8);
      rightClick(slots[slots.length-1]);
      assert.deepEqual(Array.from(api.getFacelets()), before);
    }
    click(20); rightClick(20); rightClick(20);
    assert.equal(borders[20], 0xffd700);
    assert.deepEqual(Array.from(api.getFacelets()), before);
  });
  test(`${path}: remembered colors follow ignored pieces through turns, swaps, and twists`, () => {
    for (const action of ['turns', 'swap', 'twist']) {
      const actual = viewer(path), expected = viewer(path);
      actual.api.applyAlgorithmInstant('R F'); expected.api.applyAlgorithmInstant('R F');
      actual.rightClick(0);
      for (const current of [actual, expected]) {
        if (action === 'turns') current.api.applyAlgorithmInstant("U L R'");
        else {
          current.api.setMode('swap');
          current.click(0, false); current.click(action === 'swap' ? 4 : 0, false);
        }
      }
      const ignored = actual.api.getFacelets().findIndex(color => color === 8);
      assert.notEqual(ignored, -1);
      actual.rightClick(ignored);
      assert.deepEqual(Array.from(actual.api.getFacelets()), Array.from(expected.api.getFacelets()), action);
    }
  });
  test(`${path}: loaded black pieces use home colors and stale remembered colors are cleared`, () => {
    const {api, rightClick} = viewer(path);
    const solved = Array.from(api.getFacelets());
    api.applyAlgorithmInstant('R F'); rightClick(0);
    api.resetPuzzle();
    const loaded = solved.slice(); for (const slot of [0,54,9,63]) loaded[slot] = 8;
    api.setFacelets(loaded); rightClick(0);
    assert.deepEqual(Array.from(api.getFacelets()), solved);
  });
  test(`${path}: loading a mask preserves the underlying colors, including repeated loads`, () => {
    const {api, rightClick} = viewer(path);
    api.applyAlgorithmInstant("R F U");
    const before = Array.from(api.getFacelets());
    const mask = before.slice();
    for (const slot of [0,54,9,63]) mask[slot] = 8;
    api.setFacelets(mask);
    api.setFacelets(mask);
    rightClick(0);
    assert.deepEqual(Array.from(api.getFacelets()), before);
  });
  test(`${path}: saved black pieces retain their colors through a fresh viewer and later moves`, () => {
    const original = viewer(path), expected = viewer(path), restored = viewer(path);
    for (const current of [original, expected]) current.api.applyAlgorithmInstant("R F U");
    for (const slot of [0,1,20]) original.rightClick(slot);
    const saved = JSON.parse(JSON.stringify({facelets: original.api.getFacelets(), remembered: original.api.getIgnoredStickerState()}));
    restored.api.setFacelets(saved.facelets, saved.remembered);
    for (const current of [restored, expected]) current.api.applyAlgorithmInstant("L U R'");
    while (restored.api.getFacelets().includes(8)) restored.rightClick(restored.api.getFacelets().findIndex(color => color === 8));
    assert.deepEqual(Array.from(restored.api.getFacelets()), Array.from(expected.api.getFacelets()));
  });
  test(`${path}: saved black last-layer centers retain their distinguished identity`, () => {
    const original = viewer(path), restored = viewer(path);
    original.api.setLastLayerMode(true);
    const before = JSON.parse(JSON.stringify(original.api.getCenterTargets()));
    original.rightClick(47);
    restored.api.setLastLayerMode(true);
    restored.api.setFacelets(original.api.getFacelets(), JSON.parse(JSON.stringify(original.api.getIgnoredStickerState())));
    restored.api.setCenterTargets?.(original.api.getCenterTargets());
    restored.rightClick(47);
    assert.equal(restored.api.getFacelets()[47], 5);
    assert.deepEqual(JSON.parse(JSON.stringify(restored.api.getCenterTargets())), before);
  });
  test(`${path}: toggling the odd last-layer center restores its role`, () => {
    const {api, rightClick} = viewer(path);
    api.setLastLayerMode(true);
    const before = api.getCenterTargets();
    rightClick(47); assert.equal(api.getFacelets()[47], 8);
    rightClick(47);
    assert.equal(api.getFacelets()[47], 5);
    assert.deepEqual(JSON.parse(JSON.stringify(api.getCenterTargets())), JSON.parse(JSON.stringify(before)));
  });
}
