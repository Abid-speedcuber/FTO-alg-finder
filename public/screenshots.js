(() => {
  const dialog = document.querySelector('.screenshot-dialog');
  if (!dialog || typeof dialog.showModal !== 'function') return;

  const viewport = dialog.querySelector('.screenshot-viewport');
  const image = dialog.querySelector('.screenshot-full');
  const pointers = new Map();
  let scale = 1;
  let width = 0;
  let height = 0;
  let x = 0;
  let y = 0;
  let opener;

  function render() {
    const limitX = Math.max(0, (width * scale - viewport.clientWidth) / 2);
    const limitY = Math.max(0, (height * scale - viewport.clientHeight) / 2);
    x = Math.max(-limitX, Math.min(limitX, x));
    y = Math.max(-limitY, Math.min(limitY, y));
    image.style.transform = `translate(-50%, -50%) translate(${x}px, ${y}px) scale(${scale})`;
    viewport.classList.toggle('is-zoomed', scale > 1);
  }

  function zoom(nextScale, clientX, clientY) {
    if (!width) return;
    const bounds = viewport.getBoundingClientRect();
    const px = clientX - bounds.left - viewport.clientWidth / 2;
    const py = clientY - bounds.top - viewport.clientHeight / 2;
    nextScale = Math.max(1, Math.min(8, nextScale));
    const ratio = nextScale / scale;
    x = px - (px - x) * ratio;
    y = py - (py - y) * ratio;
    scale = nextScale;
    render();
  }

  function fit() {
    if (!dialog.open || !image.naturalWidth) return;
    const ratio = Math.min(viewport.clientWidth / image.naturalWidth,
      viewport.clientHeight / image.naturalHeight);
    width = image.naturalWidth * ratio;
    height = image.naturalHeight * ratio;
    image.style.width = `${width}px`;
    image.style.height = `${height}px`;
    render();
  }

  image.addEventListener('load', fit);
  document.querySelectorAll('.screenshot-link').forEach(link => {
    const thumbnail = link.querySelector('img');
    link.setAttribute('aria-label', `Enlarge: ${thumbnail.alt}`);
    link.setAttribute('aria-haspopup', 'dialog');
    link.addEventListener('click', event => {
      if (event.ctrlKey || event.metaKey || event.shiftKey || event.altKey) return;
      event.preventDefault();
      opener = link;
      scale = 1;
      x = y = width = height = 0;
      image.style.width = '0px';
      image.style.height = '0px';
      image.alt = thumbnail.alt;
      image.src = link.href;
      dialog.showModal();
      document.body.classList.add('preview-open');
      if (image.complete) fit();
    });
  });

  dialog.querySelector('.screenshot-close').addEventListener('click', () => dialog.close());
  dialog.addEventListener('close', () => {
    document.body.classList.remove('preview-open');
    pointers.clear();
    viewport.classList.remove('is-dragging');
    opener?.focus({ preventScroll: true });
  });
  viewport.addEventListener('wheel', event => {
    event.preventDefault();
    const delta = event.deltaY * (event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? viewport.clientHeight : 1);
    zoom(scale * Math.exp(-delta * 0.002), event.clientX, event.clientY);
  }, { passive: false });
  viewport.addEventListener('dblclick', event => {
    zoom(scale > 1 ? 1 : 2, event.clientX, event.clientY);
  });
  viewport.addEventListener('pointerdown', event => {
    if (event.button !== 0) return;
    pointers.set(event.pointerId, {
      x: event.clientX, y: event.clientY,
      startX: event.clientX, startY: event.clientY,
      outside: event.target !== image, moved: false,
    });
    if (pointers.size > 1) pointers.forEach(point => { point.moved = true; });
    viewport.setPointerCapture(event.pointerId);
    viewport.classList.toggle('is-dragging', scale > 1);
  });
  viewport.addEventListener('pointermove', event => {
    const previous = pointers.get(event.pointerId);
    if (!previous) return;
    const dx = event.clientX - previous.x;
    const dy = event.clientY - previous.y;
    if (Math.hypot(event.clientX - previous.startX, event.clientY - previous.startY) > 4) previous.moved = true;
    if (pointers.size === 2) {
      const other = [...pointers.entries()].find(([id]) => id !== event.pointerId)[1];
      const before = Math.hypot(previous.x - other.x, previous.y - other.y);
      const after = Math.hypot(event.clientX - other.x, event.clientY - other.y);
      if (before > 0) zoom(scale * after / before,
        (previous.x + other.x) / 2, (previous.y + other.y) / 2);
      x += dx / 2;
      y += dy / 2;
    } else if (pointers.size === 1) {
      x += dx;
      y += dy;
    }
    previous.x = event.clientX;
    previous.y = event.clientY;
    render();
  });
  viewport.addEventListener('pointerup', event => {
    const point = pointers.get(event.pointerId);
    if (point?.outside && !point.moved) dialog.close();
  });
  function release(event) {
    pointers.delete(event.pointerId);
    if (!pointers.size) viewport.classList.remove('is-dragging');
  }
  viewport.addEventListener('pointerup', release);
  viewport.addEventListener('pointercancel', release);
  viewport.addEventListener('lostpointercapture', release);
  window.addEventListener('resize', fit);
})();
