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
    fills, borders, performance,
    THREE: { Quaternion, Vector3: class {} },
    window: { addEventListener: (name, handler) => { events[name] = handler; }, removeEventListener() {} },
  });
  const init = `
    for (var f = 0; f < 72; f++) {
      (function(slot) {
        faceletToSticker[slot] = slot;
        faceletColors[slot] = Math.floor(slot / 9);
        cubePieces[slot] = [null, { matrix: { copy: function() {} }, update: function() {} },
          faceletColors[slot], slot,
          { materials: [{ color: { setHex: function(hex) { fills[slot] = hex; } } }] },
          { color: { setHex: function(hex) { borders[slot] = hex; } } }];
      })(f);
    }
    render = function() {};
    pickSticker = function(x) { return x >= 0 && x < 72 ? x : null; };
    canvas = { style: {} };
    var mouseCleanup = setupMouseControls(canvas);
    var unbindKeyboard = function() {};
    function setKeyboardEnabled() {}
  `;
  vm.runInContext(source.slice(0, start) + init + source.slice(end), context);
  const container = { style: {}, addEventListener: (name, handler) => { events[name] = handler; }, removeEventListener() {} };
  const api = context.window.createFtoViewer(container, {});
  return { api, borders, click(facelet, shiftKey = true) {
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
