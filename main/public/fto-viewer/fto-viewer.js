(function() {
  "use strict";

  var faceColors = [0xffffff, 0xff8800, 0xffff00, 0x00ff00, 0x0000ff, 0xff0000, 0x800080, 0x00ffff];
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
    var faceletColors = [];
    var puzzle;
    var defaultOrbit = new THREE.Quaternion().multiply(
      new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(1, 0, 0), -0.25),
      new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(0, 1, 0), 0.2)
    );
    var orbit = new THREE.Quaternion().copy(defaultOrbit);
    var isDragging = false;
    var didDrag = false;
    var lastX = 0;
    var lastY = 0;
    var mode = options.mode || "pan";
    var selectedColor = options.color == null ? 0 : options.color;
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

      var borderMat = new THREE.MeshBasicMaterial({ color: 0x000000, wireframe: true, wireframeLinewidth: 1 });

      puzzle.enumFacesPolys(function(face, p, poly, idx) {
        if (poly.area < 0.001) {
          return;
        }
        var trimmed = poly.trim(0.03);
        if (trimmed) {
          poly = trimmed;
        }
        var cords = poly.projection(puzzle.faceUVs[face]);
        var logicalFace = polyFaceToFaceletFace[face];
        var faceletIndex = logicalFace * 9 + polyStickerToFaceletSlot[face][p];
        var ownMat = new THREE.MeshBasicMaterial({ color: faceColors[logicalFace] });
        var mesh = new THREE.Mesh(new THREE.Ploy(cords), [ownMat, borderMat]);
        mesh.doubleSided = true;
        mesh.overdraw = true;
        mesh.ftoStickerIndex = idx;
        mesh.ftoFaceletIndex = faceletIndex;

        var sticker = new THREE.Object3D();
        sticker.addChild(mesh);
        var m = twistyjs.axify(puzzle.faceUVs[face][0], puzzle.faceUVs[face][1], puzzle.facePlanes[face].norm)
          .multiplySelf(new THREE.Matrix4().setTranslation(0, 0, 1));
        sticker.matrix.copy(m);
        sticker.matrixAutoUpdate = false;
        sticker.update();

        cubePieces[idx] = [m, sticker, logicalFace, faceletIndex];
        stickerMeshes.push(mesh);
        faceletColors[faceletIndex] = logicalFace;
        cubeObject.addChild(sticker);
      });

      cubeObject.scale = new THREE.Vector3(0.62, 0.62, 0.62);
      updateOrbit();
    }

    function setStickerColor(stickerIndex, color) {
      var sticker = cubePieces[stickerIndex];
      if (!sticker) {
        return;
      }
      var material = sticker[1].children[0].materials[0];
      material.color.setHex(faceColors[color]);
      sticker[2] = color;
      faceletColors[sticker[3]] = color;
    }

    function notifyState() {
      if (options.onFacelets) {
        options.onFacelets(getFacelets());
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
      var speedup = Math.min(6, 1 + moveQueue.length);
      var steps = Math.max(2, Math.round(12 / speedup));
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
      var parsed = parseAlgorithm(algorithm);
      for (var i = 0; i < parsed.length; i++) {
        applyMoveInstant({ axis: parsed[i][0], pow: parsed[i][1] });
      }
      render();
    }

    function getFacelets() {
      return faceletColors.slice();
    }

    function parseAlgorithm(algorithm) {
      algorithm = algorithm || "";
      var expanded = algorithm
        .replace(/\bM'\b/g, "Rw R'")
        .replace(/\bMi\b/g, "Rw R'")
        .replace(/\bM\b/g, "Rw' R");
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
      function endDrag() {
        if (!isDragging) {
          return;
        }
        isDragging = false;
        didDrag = false;
        canvasEl.style.cursor = mode === "pan" ? "grab" : "crosshair";
      }
      function pointerDown(x, y) {
        isDragging = true;
        didDrag = false;
        lastX = x;
        lastY = y;
        canvasEl.style.cursor = mode === "pan" ? "grabbing" : "crosshair";
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
        if (mode !== "pan") {
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
      function pointerUp(x, y) {
        if (mode !== "pan" && !didDrag && x != null && y != null) {
          paintAt(x, y);
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
        pointerUp(e.clientX, e.clientY);
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
        pointerUp(touch && touch.clientX, touch && touch.clientY);
      };

      canvasEl.addEventListener("mousedown", onMouseDown);
      onCleanup(function() { canvasEl.removeEventListener("mousedown", onMouseDown); });
      window.addEventListener("mousemove", onMouseMove);
      onCleanup(function() { window.removeEventListener("mousemove", onMouseMove); });
      window.addEventListener("mouseup", onMouseUp);
      onCleanup(function() { window.removeEventListener("mouseup", onMouseUp); });
      window.addEventListener("blur", onBlur);
      onCleanup(function() { window.removeEventListener("blur", onBlur); });
      canvasEl.addEventListener("touchstart", onTouchStart, { passive: true });
      canvasEl.addEventListener("touchmove", onTouchMove, { passive: true });
      canvasEl.addEventListener("touchend", onTouchEnd);
      onCleanup(function() {
        canvasEl.removeEventListener("touchstart", onTouchStart);
        canvasEl.removeEventListener("touchmove", onTouchMove);
        canvasEl.removeEventListener("touchend", onTouchEnd);
      });

      return cleanup;
    }

    function paintAt(x, y) {
      var stickerIndex = pickSticker(x, y);
      if (stickerIndex == null) {
        return;
      }
      setStickerColor(stickerIndex, selectedColor);
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
    container.appendChild(canvas);
    var mouseCleanup = setupMouseControls(canvas);
    var unbindKeyboard = options.keyboard === false ? function() {} : bindKeyboard();
    window.addEventListener("resize", resize);
    resize();
    notifyState();

    return {
      applyAlgorithm: applyAlgorithm,
      applyAlgorithmInstant: applyAlgorithmInstant,
      queueMove: queueMove,
      getFacelets: getFacelets,
      setMode: function(nextMode) {
        mode = nextMode;
        if (canvas) {
          canvas.style.cursor = mode === "pan" ? "grab" : "crosshair";
        }
      },
      setColor: function(color) {
        selectedColor = color;
      },
      resetPuzzle: function() {
        moveQueue = [];
        animating = false;
        moveHistory = [];
        for (var i = 0; i < cubePieces.length; i++) {
          if (cubePieces[i]) {
            setStickerColor(i, Math.floor(cubePieces[i][3] / 9));
            cubePieces[i][1].matrix.copy(cubePieces[i][0]);
            cubePieces[i][1].update();
          }
        }
        if (options.onMove) {
          options.onMove([]);
        }
        render();
        notifyState();
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
