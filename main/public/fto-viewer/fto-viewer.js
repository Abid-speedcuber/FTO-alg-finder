(function() {
  "use strict";

  var faceColors = [0xffff00, 0x0000ff, 0xff0000, 0x800080, 0xffffff, 0x00a050, 0x808080, 0xff8800];
  var ignoredColor = 8;
  var ignoredColorHex = 0x050505;
  var pieceFacelets = [
    [0, 54, 9, 63],
    [4, 53, 22, 62],
    [8, 67, 35, 49],
    [27, 36, 18, 45],
    [13, 44, 31, 71],
    [26, 40, 17, 58],
    [1, 57],
    [3, 64],
    [6, 51],
    [28, 39],
    [21, 37],
    [15, 42],
    [12, 55],
    [10, 66],
    [33, 69],
    [30, 46],
    [19, 48],
    [24, 60],
  ];
  var polyFaceToFaceletFace = [0, 6, 7, 1, 4, 3, 2, 5];
  var polyStickerToFaceletSlot = [
    [0, 3, 8, 1, 6, 4, 2, 7, 5],
    [0, 1, 4, 2, 5, 7, 3, 6, 8],
    [0, 3, 8, 2, 7, 5, 1, 6, 4],
    [0, 2, 7, 1, 5, 4, 3, 6, 8],
    [4, 6, 8, 7, 5, 2, 3, 1, 0],
    [8, 6, 4, 3, 1, 0, 5, 7, 2],
    [4, 6, 8, 1, 3, 0, 7, 5, 2],
    [8, 3, 0, 6, 1, 4, 5, 7, 2],
  ];
  var ufCenterFacelets = [2, 5, 7, 11, 14, 16, 20, 23, 25, 29, 32, 34];
  var rlCenterFacelets = [38, 41, 43, 47, 50, 52, 65, 68, 70, 56, 59, 61];
  var rlFaceColorToCenterGroup = [0, 1, 3, 2];
  var lastLayerUniqueCenterSlot = [null, 3, 8, 10];
  var lastLayerCenterColors = {
    5: { affected: 0x51e86f, fixed: 0x009245 },
    6: { affected: 0xb8b8b8, fixed: 0x707070 },
    7: { affected: 0xffb34a, fixed: 0xff7800 },
  };
  var targetHighlightHex = 0xf4ff62;
  var ftoKeymap = "I:R K:R' D:L E:L' J:U F:U' H:F G:F' S:D L:D' W:B O:B' 8:BR ,:BR' C:BL 3:BL' U:Rw M:Rw' R:Lw' V:Lw Y:[R] N:[R'] T:[L'] B:[L] ;:[U] A:[U'] P:T Q:T'";

  function createFtoViewer(container, options) {
    options = options || {};
    var scene;
    var camera;
    var renderer;
    var canvas;
    var projector;
    var cubeObject;
    var cubePieces = [];
    var stickerMeshes = [];
    var faceletToSticker = [];
    var faceletColors = [];
    var puzzle;
    var defaultOrbitX = 0.5;
    var defaultOrbitY = -0.0;
    var defaultOrbitZ = 0.01;
    var defaultOrbit = new THREE.Quaternion().multiply(
      new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(1, 0, 0), defaultOrbitX),
      new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(0, 1, 0), defaultOrbitY)
    );
    defaultOrbit.multiplySelf(new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(0, 0, 1), defaultOrbitZ));
    var orbit = new THREE.Quaternion().copy(defaultOrbit);
    var isDragging = false;
    var didDrag = false;
    var lastX = 0;
    var lastY = 0;
    var activeDragMode = "pan";
    var mode = options.mode || "pan";
    var selectedColor = options.color == null ? 0 : options.color;
    var lastLayerMode = !!options.lastLayerMode;
    var centerTargets = {
      uf: [null, null, null, null],
      rl: [null, null, null, null],
      ufSources: [null, null, null, null],
      rlSources: [null, null, null, null],
      rlTopSources: [[], [], [], []],
    };
    var lastLayerCenterMarks = new Array(12).fill(null);
    var targetPick = null;
    var highlightedFacelets = [];
    var moveQueue = [];
    var animating = false;
    var moveHistory = [];
    var disposed = false;

    function initPuzzle() {
      puzzle = poly3d.makePuzzle(8, [-5, 1 / 3, -1 / 3], [], [-5]);
      puzzle.parser = poly3d.makePuzzleParser(puzzle);

      scene = new THREE.Scene();
      cubeObject = new THREE.Object3D();
      scene.addObject(cubeObject);

      var borderMat = new THREE.MeshBasicMaterial({ color: 0x000000 });

      puzzle.enumFacesPolys(function(face, p, poly, idx) {
        if (poly.area < 0.001) {
          return;
        }
        var borderPoly = poly.trim(0.012) || poly;
        var colorPoly = poly.trim(0.055) || borderPoly;
        var borderCords = borderPoly.projection(puzzle.faceUVs[face]);
        var cords = colorPoly.projection(puzzle.faceUVs[face]);
        var logicalFace = polyFaceToFaceletFace[face];
        var faceletIndex = logicalFace * 9 + polyStickerToFaceletSlot[face][p];
        var borderMesh = new THREE.Mesh(new THREE.Ploy(borderCords), [borderMat]);
        var ownMat = new THREE.MeshBasicMaterial({ color: faceColors[logicalFace] });
        var mesh = new THREE.Mesh(new THREE.Ploy(cords), [ownMat]);
        borderMesh.doubleSided = true;
        borderMesh.overdraw = true;
        mesh.doubleSided = true;
        mesh.overdraw = true;
        mesh.position = new THREE.Vector3(0, 0, 0.002);
        mesh.ftoStickerIndex = idx;
        mesh.ftoFaceletIndex = faceletIndex;

        var sticker = new THREE.Object3D();
        sticker.addChild(borderMesh);
        sticker.addChild(mesh);
        var m = twistyjs.axify(puzzle.faceUVs[face][0], puzzle.faceUVs[face][1], puzzle.facePlanes[face].norm)
          .multiplySelf(new THREE.Matrix4().setTranslation(0, 0, 1));
        sticker.matrix.copy(m);
        sticker.matrixAutoUpdate = false;
        sticker.update();

        cubePieces[idx] = [m, sticker, logicalFace, faceletIndex, mesh];
        faceletToSticker[faceletIndex] = idx;
        stickerMeshes.push(mesh);
        faceletColors[faceletIndex] = logicalFace;
        cubeObject.addChild(sticker);
      });

      cubeObject.scale = new THREE.Vector3(0.62, 0.62, 0.62);
      updateOrbit();
    }

    function stickerColorHex(color, faceletIndex) {
      if (color === ignoredColor) {
        return ignoredColorHex;
      }
      if (lastLayerMode && lastLayerCenterColors[color]) {
        var info = centerInfo(faceletIndex);
        if (info && info.orbit === "rl") {
          return lastLayerCenterMarks[info.slot] === "fixed"
            ? lastLayerCenterColors[color].fixed
            : lastLayerCenterMarks[info.slot] === "top"
            ? lastLayerCenterColors[color].affected
            : faceColors[color];
        }
      }
      return faceColors[color];
    }

    function setStickerDisplayColor(stickerIndex, color) {
      var sticker = cubePieces[stickerIndex];
      if (!sticker) {
        return;
      }
      var material = sticker[4].materials[0];
      material.color.setHex(color);
    }

    function refreshStickerDisplayByFacelet(faceletIndex) {
      var stickerIndex = faceletToSticker[faceletIndex];
      if (stickerIndex != null) {
        setStickerDisplayColor(stickerIndex, stickerColorHex(faceletColors[faceletIndex], faceletIndex));
      }
    }

    function clearTargetHighlights() {
      for (var i = 0; i < highlightedFacelets.length; i++) {
        refreshStickerDisplayByFacelet(highlightedFacelets[i]);
      }
      highlightedFacelets = [];
    }

    function showTargetHighlights(facelets) {
      clearTargetHighlights();
      highlightedFacelets = facelets.slice();
      for (var i = 0; i < highlightedFacelets.length; i++) {
        var stickerIndex = faceletToSticker[highlightedFacelets[i]];
        if (stickerIndex != null) {
          setStickerDisplayColor(stickerIndex, targetHighlightHex);
        }
      }
    }

    function setStickerColor(stickerIndex, color) {
      var sticker = cubePieces[stickerIndex];
      if (!sticker) {
        return;
      }
      var info = centerInfo(sticker[3]);
      if (info && info.orbit === "rl") {
        lastLayerCenterMarks[info.slot] = null;
        removeTopSource(info.slot);
        for (var colorGroup = 0; colorGroup < centerTargets.rlSources.length; colorGroup++) {
          if (centerTargets.rlSources[colorGroup] === info.slot) {
            centerTargets.rl[colorGroup] = null;
            centerTargets.rlSources[colorGroup] = null;
          }
        }
      }
      setStickerDisplayColor(stickerIndex, stickerColorHex(color, sticker[3]));
      sticker[2] = color;
      faceletColors[sticker[3]] = color;
    }

    function setLastLayerCenterColor(stickerIndex, color, role) {
      var sticker = cubePieces[stickerIndex];
      if (!sticker) {
        return;
      }
      setStickerColor(stickerIndex, color);
      var info = centerInfo(sticker[3]);
      if (lastLayerMode && info && info.orbit === "rl" && lastLayerCenterColors[color]) {
        lastLayerCenterMarks[info.slot] = role;
        refreshStickerDisplayByFacelet(sticker[3]);
      }
    }

    function notifyState() {
      if (options.onFacelets) {
        options.onFacelets(getFacelets());
      }
    }

    function getCenterTargets() {
      return {
        uf: centerTargets.uf.slice(),
        rl: centerTargets.rl.slice(),
        ufSources: centerTargets.ufSources.slice(),
        rlSources: centerTargets.rlSources.slice(),
        rlTopSources: centerTargets.rlTopSources.map(function(sources) { return sources.slice(); }),
      };
    }

    function notifyCenterTargets() {
      if (options.onCenterTargets) {
        options.onCenterTargets(getCenterTargets());
      }
    }

    function applyMoveInstant(moveName) {
      var idx = puzzle.getTwistyIdx(moveName.axis);
      if (idx == -1) {
        return false;
      }
      var maxPow = puzzle.twistyDetails[idx][1];
      var pow = ((moveName.pow % maxPow) + maxPow) % maxPow;
      var perm = puzzle.moveTable[idx];
      var nextState = [];
      for (var i = 0; i < perm.length; i++) {
        var val = i;
        for (var j = 0; j < pow; j++) {
          val = perm[val] < 0 ? val : perm[val];
        }
        var sticker = cubePieces[val];
        if (!sticker) {
          continue;
        }
        nextState[i] = sticker[2];
      }
      for (var k = 0; k < perm.length; k++) {
        if (!cubePieces[k] || nextState[k] === undefined) {
          continue;
        }
        setStickerColor(k, nextState[k]);
        cubePieces[k][1].matrix.copy(cubePieces[k][0]);
        cubePieces[k][1].update();
      }
      notifyState();
      return true;
    }

    function animateMove(moveName, cb) {
      var idx = puzzle.getTwistyIdx(moveName.axis);
      if (idx == -1) {
        if (cb) cb();
        return;
      }
      var plane = puzzle.twistyPlanes[puzzle.twistyDetails[idx][2]];
      var fullAngle = Math.PI * 2 * moveName.pow / puzzle.twistyDetails[idx][1];
      var speedup = Math.min(3, 1 + moveQueue.length);
      var steps = Math.max(1, Math.round(4 / speedup));
      var step = 0;
      var affected = [];
      puzzle.enumFacesPolys(function(face, p, poly, i) {
        for (var k = 2; k < puzzle.twistyDetails[idx].length; k++) {
          if (puzzle.twistyPlanes[puzzle.twistyDetails[idx][k]].side(poly.center) < 0) {
            return;
          }
        }
        affected.push(i);
      });
      function tick() {
        if (disposed) {
          return;
        }
        step++;
        var rot = new THREE.Matrix4().setRotationAxis(plane.norm, -fullAngle / steps);
        for (var k = 0; k < affected.length; k++) {
          var sticker = cubePieces[affected[k]];
          if (!sticker) {
            continue;
          }
          sticker[1].matrix.multiply(rot, sticker[1].matrix);
          sticker[1].update();
        }
        render();
        if (step < steps) {
          requestAnimationFrame(tick);
        } else {
          applyMoveInstant(moveName);
          if (cb) cb();
        }
      }
      requestAnimationFrame(tick);
    }

    function processQueue() {
      if (animating || moveQueue.length === 0) {
        return;
      }
      animating = true;
      var mv = moveQueue.shift();
      animateMove(mv, function() {
        animating = false;
        processQueue();
      });
    }

    function moveName2str(axis, pow) {
      var m = /^(\d+)([A-Za-z]+)$/.exec(axis);
      if (!m) {
        return "";
      }
      var face = m[2];
      var maxPow = 3;
      var idx = puzzle.getTwistyIdx(axis);
      if (idx != -1) {
        maxPow = puzzle.twistyDetails[idx][1];
      }
      pow = ((pow % maxPow) + maxPow) % maxPow;
      if (pow === 0) {
        return "";
      }
      var suffix = pow === 1 ? "" : (pow === maxPow - 1 ? "'" : pow);
      return face + suffix;
    }

    function queueMove(axis, pow) {
      moveQueue.push({ axis: axis, pow: pow });
      moveHistory.push(moveName2str(axis, pow));
      if (options.onMove) {
        options.onMove(moveHistory.slice());
      }
      processQueue();
    }

    function parseMove(move) {
      var parsed = puzzle.parser.parseScramble(move);
      return parsed.length === 1 ? parsed[0] : null;
    }

    function applyAlgorithm(algorithm) {
      var parsed = parseAlgorithm(algorithm);
      for (var i = 0; i < parsed.length; i++) {
        queueMove(parsed[i][0], parsed[i][1]);
      }
    }

    function applyAlgorithmInstant(algorithm) {
      clearCenterTargetPick();
      clearLastLayerCenterMarks();
      var parsed = parseAlgorithm(algorithm);
      for (var i = 0; i < parsed.length; i++) {
        applyMoveInstant({ axis: parsed[i][0], pow: parsed[i][1] });
      }
      render();
    }

    function getFacelets() {
      return faceletColors.slice();
    }

    function centerInfo(faceletIndex) {
      var ufSlot = ufCenterFacelets.indexOf(faceletIndex);
      if (ufSlot !== -1) {
        return { orbit: "uf", slot: ufSlot, color: Math.floor(ufSlot / 3) };
      }
      var rlSlot = rlCenterFacelets.indexOf(faceletIndex);
      if (rlSlot !== -1) {
        return { orbit: "rl", slot: rlSlot, color: Math.floor(rlSlot / 3) };
      }
      return null;
    }

    function selectedCenterGroup() {
      return selectedCenterGroupForColor(selectedColor);
    }

    function selectedCenterGroupForColor(color) {
      if (color < 4) {
        return { orbit: "uf", color: color };
      }
      return {
        orbit: "rl",
        color: rlFaceColorToCenterGroup[color - 4],
      };
    }

    function targetCandidatesForSelection() {
      var group = selectedCenterGroup();
      var facelets = group.orbit === "uf" ? ufCenterFacelets : rlCenterFacelets;
      var candidates = [];
      for (var slot = 0; slot < facelets.length; slot++) {
        if (Math.floor(slot / 3) === group.color) {
          candidates.push(facelets[slot]);
        }
      }
      return {
        orbit: group.orbit,
        color: group.color,
        sourceSlot: null,
        candidates: candidates,
      };
    }

    function beginCenterTargetPick(faceletIndex) {
      var info = centerInfo(faceletIndex);
      var group = selectedCenterGroup();
      if (!info || info.orbit !== group.orbit) {
        return false;
      }
      targetPick = targetCandidatesForSelection();
      targetPick.sourceSlot = info.slot;
      showTargetHighlights(targetPick.candidates);
      render();
      return true;
    }

    function chooseCenterTarget(faceletIndex) {
      if (!targetPick) {
        return false;
      }
      var info = centerInfo(faceletIndex);
      if (!info || info.orbit !== targetPick.orbit || info.color !== targetPick.color) {
        clearCenterTargetPick();
        return true;
      }
      centerTargets[targetPick.orbit][targetPick.color] = info.slot;
      centerTargets[targetPick.orbit + "Sources"][targetPick.color] = targetPick.sourceSlot;
      clearCenterTargetPick();
      notifyCenterTargets();
      render();
      return true;
    }

    function markLastLayerUniqueCenter(faceletIndex) {
      if (!lastLayerMode || selectedColor < 5 || selectedColor > 7) {
        return false;
      }
      var info = centerInfo(faceletIndex);
      var group = selectedCenterGroup();
      if (!info || !group || info.orbit !== "rl") {
        return false;
      }
      clearLastLayerFixedCenter(group.color);
      setLastLayerCenterColor(faceletToSticker[faceletIndex], selectedColor, "fixed");
      centerTargets.rl[group.color] = lastLayerUniqueCenterSlot[group.color];
      centerTargets.rlSources[group.color] = info.slot;
      removeTopSource(info.slot);
      clearCenterTargetPick();
      notifyState();
      notifyCenterTargets();
      render();
      return true;
    }

    function clearLastLayerFixedCenter(colorGroup) {
      var oldSource = centerTargets.rlSources[colorGroup];
      if (oldSource != null) {
        lastLayerCenterMarks[oldSource] = null;
        refreshStickerDisplayByFacelet(rlCenterFacelets[oldSource]);
      }
      centerTargets.rl[colorGroup] = null;
      centerTargets.rlSources[colorGroup] = null;
    }

    function addTopSource(colorGroup, sourceSlot) {
      removeTopSource(sourceSlot);
      if (centerTargets.rlTopSources[colorGroup].indexOf(sourceSlot) === -1) {
        centerTargets.rlTopSources[colorGroup].push(sourceSlot);
      }
    }

    function removeTopSource(sourceSlot) {
      for (var color = 0; color < centerTargets.rlTopSources.length; color++) {
        var sources = centerTargets.rlTopSources[color];
        var index = sources.indexOf(sourceSlot);
        if (index !== -1) {
          sources.splice(index, 1);
        }
      }
    }

    function clearLastLayerCenterMarks() {
      for (var i = 0; i < lastLayerCenterMarks.length; i++) {
        lastLayerCenterMarks[i] = null;
      }
      for (var color = 1; color <= 3; color++) {
        centerTargets.rl[color] = null;
        centerTargets.rlSources[color] = null;
      }
      centerTargets.rlTopSources = [[], [], [], []];
      for (var slot = 0; slot < rlCenterFacelets.length; slot++) {
        refreshStickerDisplayByFacelet(rlCenterFacelets[slot]);
      }
      notifyCenterTargets();
    }

    function applyLastLayerResetMarks() {
      var fixedSlotByColor = [null, 3, 8, 10];
      var faceletColorByGroup = [4, 5, 7, 6];
      for (var color = 1; color <= 3; color++) {
        var fixedSlot = fixedSlotByColor[color];
        var paintColor = faceletColorByGroup[color];
        setLastLayerCenterColor(faceletToSticker[rlCenterFacelets[fixedSlot]], paintColor, "fixed");
        centerTargets.rl[color] = fixedSlot;
        centerTargets.rlSources[color] = fixedSlot;
        for (var slot = 0; slot < rlCenterFacelets.length; slot++) {
          if (Math.floor(slot / 3) !== color || slot === fixedSlot) {
            continue;
          }
          setLastLayerCenterColor(faceletToSticker[rlCenterFacelets[slot]], paintColor, "top");
          centerTargets.rlTopSources[color].push(slot);
        }
      }
    }

    function canMarkLastLayerCenter(faceletIndex) {
      var info = centerInfo(faceletIndex);
      return lastLayerMode
        && selectedColor >= 5
        && selectedColor <= 7
        && info
        && info.orbit === "rl";
    }

    function clearCenterTargetPick() {
      targetPick = null;
      clearTargetHighlights();
    }

    function parseAlgorithm(algorithm) {
      algorithm = algorithm || "";
      var expanded = algorithm
        .replace(/\bBR\b/g, "Br")
        .replace(/\bBL\b/g, "Bl")
        .replace(/(^|\s)M'(?=\s|$)/g, "$1Rw R'")
        .replace(/(^|\s)Mi(?=\s|$)/g, "$1Rw R'")
        .replace(/(^|\s)M(?=\s|$)/g, "$1Rw' R")
        .replace(/(^|\s)S'(?=\s|$)/g, "$1Fw' F")
        .replace(/(^|\s)Si(?=\s|$)/g, "$1Fw' F")
        .replace(/(^|\s)S(?=\s|$)/g, "$1Fw F'")
        .replace(/(^|\s)E'(?=\s|$)/g, "$1Uw U'")
        .replace(/(^|\s)Ei(?=\s|$)/g, "$1Uw U'")
        .replace(/(^|\s)E(?=\s|$)/g, "$1Uw' U")
        .replace(/\b([A-Z])w\b/g, "2$1");
      return puzzle.parser.parseScramble(expanded);
    }

    function setupCamera() {
      camera = new THREE.Camera(30, 1, 0.1, 1000);
      camera.target = { position: new THREE.Vector3(0, 0, 0) };
      camera.position = new THREE.Vector3(0, 0, 4.2);
    }

    function updateOrbit() {
      if (!cubeObject) {
        return;
      }
      cubeObject.quaternion.copy(orbit);
      cubeObject.useQuaternion = true;
      cubeObject.updateMatrix();
    }

    function setupMouseControls(canvasEl) {
      var cleanup = [];
      function onCleanup(fn) {
        cleanup.push(fn);
      }
      function setCursor(cursor) {
        canvasEl.style.cursor = cursor;
        container.style.cursor = cursor;
      }
      function endDrag() {
        if (!isDragging) {
          return;
        }
        isDragging = false;
        didDrag = false;
        setCursor(mode === "pan" ? "grab" : "crosshair");
      }
      function pointerDown(x, y) {
        var picked = pickSticker(x, y);
        activeDragMode = mode === "paint" && picked != null ? "paint" : "pan";
        isDragging = true;
        didDrag = false;
        lastX = x;
        lastY = y;
        setCursor(activeDragMode === "pan" ? "grabbing" : "crosshair");
      }
      function pointerMove(x, y) {
        if (!isDragging) {
          return;
        }
        var dx = x - lastX;
        var dy = y - lastY;
        if (Math.abs(dx) + Math.abs(dy) > 2) {
          didDrag = true;
        }
        lastX = x;
        lastY = y;
        if (activeDragMode !== "pan") {
          return;
        }
        var len = Math.sqrt(dx * dx + dy * dy);
        if (len < 0.01) {
          return;
        }
        var axis = new THREE.Vector3(dy / len, dx / len, 0);
        var delta = new THREE.Quaternion().setFromAxisAngle(axis, len * 0.008);
        orbit.copy(new THREE.Quaternion().multiply(delta, orbit)).normalize();
        updateOrbit();
        render();
      }
      function rotateZ(amount) {
        var delta = new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(0, 0, 1), amount);
        orbit.copy(new THREE.Quaternion().multiply(delta, orbit)).normalize();
        updateOrbit();
        render();
      }
      function pointerUp(x, y, shiftKey, ctrlKey) {
        if (activeDragMode === "paint" && !didDrag && x != null && y != null) {
          paintAt(x, y, shiftKey, ctrlKey);
        }
        endDrag();
      }

      var onMouseDown = function(e) {
        if (e.button !== 0) {
          return;
        }
        e.preventDefault();
        pointerDown(e.clientX, e.clientY);
      };
      var onMouseMove = function(e) {
        if (e.buttons === 0) {
          endDrag();
          return;
        }
        pointerMove(e.clientX, e.clientY);
      };
      var onMouseUp = function(e) {
        pointerUp(e.clientX, e.clientY, e.shiftKey, e.ctrlKey);
      };
      var onContextMenu = function(e) {
        e.preventDefault();
        ignorePieceAt(e.clientX, e.clientY);
      };
      var onWheel = function(e) {
        if (!isDragging || activeDragMode !== "pan") {
          return;
        }
        e.preventDefault();
        rotateZ(-Math.sign(e.deltaY) * Math.PI / 90);
      };
      var onBlur = function() {
        endDrag();
      };
      var onTouchStart = function(e) {
        if (e.touches.length !== 1) {
          return;
        }
        pointerDown(e.touches[0].clientX, e.touches[0].clientY);
      };
      var onTouchMove = function(e) {
        if (e.touches.length !== 1) {
          return;
        }
        pointerMove(e.touches[0].clientX, e.touches[0].clientY);
      };
      var onTouchEnd = function(e) {
        if (e.touches.length > 0) {
          return;
        }
        var touch = e.changedTouches[0];
        pointerUp(touch && touch.clientX, touch && touch.clientY, false, false);
      };

      container.addEventListener("mousedown", onMouseDown);
      onCleanup(function() { container.removeEventListener("mousedown", onMouseDown); });
      container.addEventListener("contextmenu", onContextMenu);
      onCleanup(function() { container.removeEventListener("contextmenu", onContextMenu); });
      window.addEventListener("mousemove", onMouseMove);
      onCleanup(function() { window.removeEventListener("mousemove", onMouseMove); });
      window.addEventListener("mouseup", onMouseUp);
      onCleanup(function() { window.removeEventListener("mouseup", onMouseUp); });
      window.addEventListener("blur", onBlur);
      onCleanup(function() { window.removeEventListener("blur", onBlur); });
      container.addEventListener("wheel", onWheel, { passive: false });
      onCleanup(function() { container.removeEventListener("wheel", onWheel); });
      container.addEventListener("touchstart", onTouchStart, { passive: true });
      container.addEventListener("touchmove", onTouchMove, { passive: true });
      container.addEventListener("touchend", onTouchEnd);
      onCleanup(function() {
        container.removeEventListener("touchstart", onTouchStart);
        container.removeEventListener("touchmove", onTouchMove);
        container.removeEventListener("touchend", onTouchEnd);
      });

      return cleanup;
    }

    function paintAt(x, y, shiftKey, ctrlKey) {
      var stickerIndex = pickSticker(x, y);
      if (stickerIndex == null) {
        if (targetPick) {
          clearCenterTargetPick();
          render();
        }
        return;
      }
      var faceletIndex = cubePieces[stickerIndex][3];
      if (chooseCenterTarget(faceletIndex)) {
        return;
      }
      if (ctrlKey && markLastLayerUniqueCenter(faceletIndex)) {
        return;
      }
      if (canMarkLastLayerCenter(faceletIndex)) {
        var info = centerInfo(faceletIndex);
        var group = selectedCenterGroup();
        if (info && group && centerTargets.rlSources[group.color] === info.slot) {
          clearLastLayerFixedCenter(group.color);
        }
        setLastLayerCenterColor(stickerIndex, selectedColor, "top");
        addTopSource(group.color, info.slot);
        notifyCenterTargets();
      } else {
        setStickerColor(stickerIndex, selectedColor);
      }
      if (shiftKey) {
        beginCenterTargetPick(faceletIndex);
      }
      render();
      notifyState();
    }

    function ignorePieceAt(x, y) {
      var stickerIndex = pickSticker(x, y);
      if (stickerIndex == null || !cubePieces[stickerIndex]) {
        return;
      }
      clearCenterTargetPick();
      var faceletIndex = cubePieces[stickerIndex][3];
      var group = pieceFacelets.find(function(facelets) {
        return facelets.indexOf(faceletIndex) !== -1;
      });
      if (!group) {
        group = [faceletIndex];
      }
      for (var i = 0; i < group.length; i++) {
        var groupedSticker = faceletToSticker[group[i]];
        if (groupedSticker != null) {
          setStickerColor(groupedSticker, ignoredColor);
        }
      }
      render();
      notifyState();
    }

    function pickSticker(x, y) {
      if (!canvas || !projector) {
        return null;
      }
      var rect = canvas.getBoundingClientRect();
      var vector = new THREE.Vector3(
        ((x - rect.left) / rect.width) * 2 - 1,
        -((y - rect.top) / rect.height) * 2 + 1,
        0.5
      );
      projector.unprojectVector(vector, camera);
      var direction = vector.subSelf(camera.position).normalize();
      var ray = new THREE.Ray(camera.position, direction);
      var hits = ray.intersectObjects(stickerMeshes);
      return hits.length ? hits[0].object.ftoStickerIndex : null;
    }

    function bindKeyboard() {
      var actions = {};
      var pairs = ftoKeymap.split(" ");
      var char2code = { 59: 186, 61: 187 };
      pairs.forEach(function(pair) {
        var parts = pair.split(":");
        if (parts.length !== 2) {
          return;
        }
        var key = parts[0];
        var moveStr = parts[1];
        var keyCode = key.length === 1 ? key.toUpperCase().charCodeAt(0) : (char2code[key] || 0);
        if (key === ";") {
          keyCode = 186;
        }
        var parsed = parseMove(moveStr);
        if (parsed) {
          actions[keyCode] = parsed;
        }
      });
      var onKeyDown = function(e) {
        if (isTypingTarget(e.target) || isTypingTarget(document.activeElement)) {
          return;
        }
        if (e.altKey || e.ctrlKey || e.metaKey) {
          return;
        }
        var action = actions[e.keyCode];
        if (action) {
          queueMove(action[0], action[1]);
          e.preventDefault();
        }
      };
      document.addEventListener("keydown", onKeyDown);
      return function() {
        document.removeEventListener("keydown", onKeyDown);
      };
    }

    function isTypingTarget(target) {
      if (!target) {
        return false;
      }
      var tag = target.tagName ? target.tagName.toLowerCase() : "";
      return tag === "input" || tag === "textarea" || tag === "select" || target.isContentEditable;
    }

    function render() {
      renderer.render(scene, camera);
    }

    function resize() {
      if (!canvas || !canvas.parentElement) {
        return;
      }
      var rect = canvas.parentElement.getBoundingClientRect();
      var size = Math.max(220, Math.min(rect.width, rect.height || rect.width));
      renderer.setSize(size, size);
      render();
    }

    initPuzzle();
    setupCamera();
    renderer = new THREE.CanvasRenderer();
    projector = new THREE.Projector();
    canvas = renderer.domElement;
    canvas.style.cursor = mode === "pan" ? "grab" : "crosshair";
    canvas.style.touchAction = "none";
    canvas.style.userSelect = "none";
    canvas.draggable = false;
    container.innerHTML = "";
    container.style.cursor = mode === "pan" ? "grab" : "crosshair";
    container.style.touchAction = "none";
    container.style.userSelect = "none";
    container.appendChild(canvas);
    var mouseCleanup = setupMouseControls(canvas);
    var unbindKeyboard = options.keyboard === false ? function() {} : bindKeyboard();
    window.addEventListener("resize", resize);
    resize();
    notifyState();
    notifyCenterTargets();

    return {
      applyAlgorithm: applyAlgorithm,
      applyAlgorithmInstant: applyAlgorithmInstant,
      queueMove: queueMove,
      getFacelets: getFacelets,
      getCenterTargets: getCenterTargets,
      setMode: function(nextMode) {
        mode = nextMode;
        if (canvas) {
          canvas.style.cursor = mode === "pan" ? "grab" : "crosshair";
          container.style.cursor = mode === "pan" ? "grab" : "crosshair";
        }
      },
      setColor: function(color) {
        selectedColor = color;
      },
      setLastLayerMode: function(enabled) {
        lastLayerMode = !!enabled;
        clearCenterTargetPick();
        if (!lastLayerMode) {
          clearLastLayerCenterMarks();
        }
        for (var i = 0; i < cubePieces.length; i++) {
          if (cubePieces[i]) {
            refreshStickerDisplayByFacelet(cubePieces[i][3]);
          }
        }
        render();
      },
      resetPuzzle: function() {
        clearCenterTargetPick();
        moveQueue = [];
        animating = false;
        moveHistory = [];
        centerTargets = {
          uf: [null, null, null, null],
          rl: [null, null, null, null],
          ufSources: [null, null, null, null],
          rlSources: [null, null, null, null],
          rlTopSources: [[], [], [], []],
        };
        lastLayerCenterMarks = new Array(12).fill(null);
        for (var i = 0; i < cubePieces.length; i++) {
          if (cubePieces[i]) {
            setStickerColor(i, Math.floor(cubePieces[i][3] / 9));
            cubePieces[i][1].matrix.copy(cubePieces[i][0]);
            cubePieces[i][1].update();
          }
        }
        if (lastLayerMode) {
          applyLastLayerResetMarks();
        }
        if (options.onMove) {
          options.onMove([]);
        }
        render();
        notifyState();
        notifyCenterTargets();
      },
      resetView: function() {
        orbit.copy(defaultOrbit);
        updateOrbit();
        render();
      },
      dispose: function() {
        disposed = true;
        isDragging = false;
        didDrag = false;
        for (var i = 0; i < mouseCleanup.length; i++) {
          mouseCleanup[i]();
        }
        unbindKeyboard();
        window.removeEventListener("resize", resize);
        if (canvas && canvas.parentElement) {
          canvas.parentElement.removeChild(canvas);
        }
      },
    };
  }

  window.createFtoViewer = createFtoViewer;
})();
