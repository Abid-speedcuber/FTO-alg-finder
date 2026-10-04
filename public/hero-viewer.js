window.addEventListener("DOMContentLoaded", function() {
  if (!window.createFtoViewer) {
    return;
  }

  function flickViewer(host, dx, dy) {
    var rect = host.getBoundingClientRect();
    var startX = rect.left + rect.width * 0.52;
    var startY = rect.top + rect.height * 0.48;

    host.dispatchEvent(new MouseEvent("mousedown", {
      bubbles: true,
      button: 0,
      clientX: startX,
      clientY: startY,
    }));
    window.dispatchEvent(new MouseEvent("mousemove", {
      bubbles: true,
      buttons: 1,
      clientX: startX + dx,
      clientY: startY + dy,
    }));
    window.dispatchEvent(new MouseEvent("mouseup", {
      bubbles: true,
      button: 0,
      clientX: startX + dx,
      clientY: startY + dy,
    }));
  }

  function orbitObject(quaternion) {
    return { x: quaternion.x, y: quaternion.y, z: quaternion.z, w: quaternion.w };
  }

  function orbitBeforeDrag(targetOrbit, dx, dy) {
    var len = Math.sqrt(dx * dx + dy * dy);
    if (len < 0.01) {
      return targetOrbit;
    }
    var axis = new THREE.Vector3(dy / len, dx / len, 0);
    var dragDelta = new THREE.Quaternion().setFromAxisAngle(axis, len * 0.008);
    var inverseDelta = new THREE.Quaternion().copy(dragDelta).inverse();
    var target = new THREE.Quaternion(targetOrbit.x, targetOrbit.y, targetOrbit.z, targetOrbit.w);
    return orbitObject(new THREE.Quaternion().multiply(inverseDelta, target).normalize());
  }

  function randomIndex(length) {
    if (window.crypto && window.crypto.getRandomValues) {
      var value = new Uint32Array(1);
      window.crypto.getRandomValues(value);
      return value[0] % length;
    }
    return Math.floor(Math.random() * length);
  }

  function randomHeroScramble(length) {
    var faces = ["U", "F", "R", "L", "D", "B", "BR", "BL", "Rw", "Lw", "Fw", "Uw"];
    var suffixes = ["", "'", "2"];
    var moves = [];
    var previousFace = "";

    while (moves.length < length) {
      var face = faces[randomIndex(faces.length)];
      if (face === previousFace) {
        continue;
      }
      previousFace = face;
      moves.push(face + suffixes[randomIndex(suffixes.length)]);
    }

    return moves.join(" ");
  }

  var faceColors = ["#ffd95a", "#2b62d3", "#c83c43", "#a935a9", "#f5f0df", "#2bb673", "#86919c", "#ff8426"];
  var brandTargetOrbit = {
    x: -0.2018569335660823,
    y: 0.8119895911098634,
    z: 0.42947911817847,
    w: 0.339815198820151,
  };
  var brandHost = document.getElementById("brand-fto-viewer");
  var heroHost = document.getElementById("hero-fto-viewer");

  if (brandHost) {
    var brandViewer = window.createFtoViewer(brandHost, {
      keyboard: false,
      minSize: 34,
      faceColors: faceColors,
    });
    window.brandFtoViewer = brandViewer;
    requestAnimationFrame(function() {
      brandViewer.setOrbit(orbitBeforeDrag(brandTargetOrbit, 220, -145));
      brandViewer.animateOrbitTo(brandTargetOrbit, 1180);
    });
  }

  if (heroHost) {
    var heroScramble = randomHeroScramble(16);

    var viewer = window.createFtoViewer(heroHost, {
      keyboard: false,
      faceColors: faceColors,
    });

    viewer.applyAlgorithmInstant(heroScramble);
    viewer.setKeyboardEnabled(false);

    requestAnimationFrame(function() {
      flickViewer(heroHost, 82, -54);
    });
  }
  });
