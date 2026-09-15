export function pose(x, y, { tilt = 6, shift = 12 } = {}) {
  const clamp = v => Math.max(-1, Math.min(1, Number.isFinite(v) ? v : 0));
  x = clamp(x); y = clamp(y);
  return { rx: -y * tilt, ry: x * tilt, x: x * shift, y: y * shift };
}

export function mountMotion(element) {
  const reduced = matchMedia('(prefers-reduced-motion: reduce)');
  const abort = new AbortController();
  const options = { signal: abort.signal };
  let frame = 0, target = [0, 0], enabled = true, visible = true;
  const render = () => {
    frame = 0;
    const p = pose(...target, { tilt: Number(element.dataset.tilt ?? 6), shift: Number(element.dataset.shift ?? 12) });
    for (const [key, value] of Object.entries(p)) element.style.setProperty(`--${key}`, `${value}${key.startsWith('r') ? 'deg' : 'px'}`);
  };
  const update = (x = 0, y = 0) => {
    target = enabled && visible && !reduced.matches && !document.hidden ? [x, y] : [0, 0];
    if (!frame) frame = requestAnimationFrame(render);
  };
  const reset = () => update();
  element.addEventListener('pointermove', e => {
    if (e.pointerType === 'touch') return; // Leave native vertical scrolling intact.
    const box = element.getBoundingClientRect();
    update((e.clientX - box.left) / box.width * 2 - 1, (e.clientY - box.top) / box.height * 2 - 1);
  }, options);
  element.addEventListener('pointerleave', reset, options);
  element.addEventListener('focusout', reset, options);
  element.addEventListener('keydown', e => {
    const directions = { ArrowLeft: [-.7,0], ArrowRight: [.7,0], ArrowUp: [0,-.7], ArrowDown: [0,.7], Escape: [0,0] };
    if (e.target === element && directions[e.key]) { e.preventDefault(); update(...directions[e.key]); }
  }, options);
  reduced.addEventListener('change', reset, options);
  document.addEventListener('visibilitychange', reset, options);
  const observer = new IntersectionObserver(entries => { visible = entries[0].isIntersecting; if (!visible) reset(); });
  observer.observe(element);
  return { preview: update, setEnabled(value) { enabled = value; reset(); }, destroy() { abort.abort(); observer.disconnect(); cancelAnimationFrame(frame); target = [0,0]; render(); } };
}
