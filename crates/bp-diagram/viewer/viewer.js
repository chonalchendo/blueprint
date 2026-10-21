// bp viewer: behaviour for a bp diagram SVG, added by data-id only (28, design change 12). The SVG draws everything;
// this file toggles classes and one transform. The static HTML inlines it; the view imports the same file.
// No dependency, no network, no clock: the same input gives the same page.
(() => {
  const STEPS = [0.25, 0.5, 0.75, 1, 1.5, 2, 3, 4];
  const PARTS = '.node,.edge,.label,g.note[data-id],.group,.life';

  function hash() { return new URLSearchParams(location.hash.slice(1)); }
  function setHash(change) {
    const h = hash();
    for (const [k, v] of Object.entries(change)) (v === null || v === '' ? h.delete(k) : h.set(k, v));
    const s = h.toString();
    history.replaceState(null, '', s ? '#' + s : location.pathname + location.search);
  }
  function el(tag, cls, text) {
    const e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text !== undefined) e.textContent = text;
    return e;
  }

  function attach(svg, opts = {}) {
    const meta = JSON.parse(svg.querySelector('metadata').textContent);
    const ids = meta.ids;
    const view = svg.querySelector('.viewport');
    const [W, H] = meta.size;
    const by = (id) => svg.querySelector('[data-id=' + JSON.stringify(id) + ']');
    const parts = [...svg.querySelectorAll(PARTS)];
    const stage = el('div', 'bp-stage');
    svg.parentNode.insertBefore(stage, svg);
    stage.appendChild(svg);
    stage.tabIndex = 0;
    const bar = el('div', 'bp-controls');
    stage.appendChild(bar);
    let cam = { k: 1, x: 0, y: 0 }, selected = null, step = 0, mode = 'diff';

    // The camera: one transform on the viewport group, in SVG units, kept near the drawing.
    const zoomLabel = el('span', 'bp-zoom');
    function draw(save = true) {
      const k = cam.k, slack = 0.25;
      cam.x = Math.min(Math.max(cam.x, Math.min(0, W - W * k) - W * slack), Math.max(0, W - W * k) + W * slack);
      cam.y = Math.min(Math.max(cam.y, Math.min(0, H - H * k) - H * slack), Math.max(0, H - H * k) + H * slack);
      view.setAttribute('transform', k === 1 && !cam.x && !cam.y ? '' : `translate(${Math.round(cam.x)} ${Math.round(cam.y)}) scale(${k})`);
      zoomLabel.textContent = Math.round(k * 100) + '%';
      if (save) setHash({ z: k === 1 ? null : Math.round(k * 100), x: cam.x ? Math.round(cam.x) : null, y: cam.y ? Math.round(cam.y) : null });
    }
    const unit = () => W / svg.getBoundingClientRect().width;
    function zoomTo(k, cx = W / 2, cy = H / 2) {
      k = Math.min(4, Math.max(0.25, k));
      cam.x = cx - (cx - cam.x) * (k / cam.k);
      cam.y = cy - (cy - cam.y) * (k / cam.k);
      cam.k = k;
      draw();
    }
    const fit = () => { cam = { k: 1, x: 0, y: 0 }; draw(); };
    const stepZoom = (dir) => {
      const next = dir > 0 ? STEPS.find((s) => s > cam.k + 1e-3) : [...STEPS].reverse().find((s) => s < cam.k - 1e-3);
      if (next) zoomTo(next);
    };
    const button = (text, title, fn) => { const b = el('button', '', text); b.title = title; b.onclick = fn; bar.appendChild(b); };
    button('−', 'Zoom out (-)', () => stepZoom(-1));
    bar.appendChild(zoomLabel);
    button('+', 'Zoom in (+)', () => stepZoom(1));
    button('Fit', 'Fit (0)', fit);
    const point = (e) => { const r = svg.getBoundingClientRect(), u = unit(); return [(e.clientX - r.left) * u, (e.clientY - r.top) * u]; };
    svg.addEventListener('wheel', (e) => {
      if (e.ctrlKey || e.metaKey) {
        e.preventDefault();
        const [px, py] = point(e);
        zoomTo(cam.k * Math.exp(-e.deltaY * 0.01), px, py);
      } else if (cam.k > 1) {
        e.preventDefault();
        cam.x -= e.deltaX * unit();
        cam.y -= e.deltaY * unit();
        draw();
      }
    }, { passive: false });
    let drag = null;
    svg.addEventListener('pointerdown', (e) => {
      if (e.button !== 0 || e.target.closest('[data-id]')) return;
      drag = { x: e.clientX, y: e.clientY, cx: cam.x, cy: cam.y, moved: false };
      svg.setPointerCapture(e.pointerId);
    });
    svg.addEventListener('pointermove', (e) => {
      if (!drag) return;
      const dx = e.clientX - drag.x, dy = e.clientY - drag.y;
      if (Math.abs(dx) + Math.abs(dy) > 3) drag.moved = true;
      cam.x = drag.cx + dx * unit();
      cam.y = drag.cy + dy * unit();
      draw(false);
    });
    svg.addEventListener('pointerup', () => {
      if (drag && drag.moved) draw();
      else if (drag) select(null);
      drag = null;
    });

    // Focus: an element, its neighbours and their labels keep full ink; the rest dims (25 % nodes, 20 % edges).
    function keep(id) {
      const out = new Set([id]);
      const item = ids[id];
      if (!item) return out;
      const edge = (eid) => { out.add(eid); for (const l of ids[eid].labels || []) out.add(l); out.add(eid + ':note'); };
      if (item.kind === 'node') {
        for (const [eid, e] of Object.entries(ids)) if (e.kind === 'edge' && (e.from === id || e.to === id)) { edge(eid); out.add(e.from); out.add(e.to); }
      } else if (item.kind === 'edge') { edge(id); out.add(item.from); out.add(item.to); }
      else if (item.kind === 'label' || item.kind === 'note') return keep(item.of);
      else if (item.kind === 'group') for (const m of item.members) out.add(m);
      return out;
    }
    function focus(id) {
      const set = id ? keep(id) : null;
      for (const p of parts) {
        const pid = p.dataset.id || p.dataset.of;
        p.classList.toggle('dim', !!set && !set.has(pid) && !(p.classList.contains('group') && set.size > 1));
      }
    }
    svg.addEventListener('pointerover', (e) => {
      const t = e.target.closest('[data-id]');
      if (t && !step) focus(t.dataset.id);
    });
    svg.addEventListener('pointerleave', () => { if (!step) focus(selected); });

    // Selection and the detail panel.
    const panel = opts.panel === false ? null : el('aside', 'bp-panel');
    if (panel) stage.appendChild(panel);
    function describe(id) {
      const item = ids[id];
      if (!item) return null;
      const name = (x) => (ids[x] && ids[x].label) || x;
      const links = [];
      for (const [eid, e] of Object.entries(ids)) {
        if (e.kind !== 'edge' || (e.from !== id && e.to !== id)) continue;
        const pill = by(eid + ':label');
        const word = pill ? pill.textContent.replace(/^\d+/, '').trim() : '';
        links.push({ id: e.from === id ? e.to : e.from, text: e.from === id ? `→ ${word} ${name(e.to)}` : `← ${name(e.from)} ${word}` });
      }
      return { id, kind: item.kind, label: item.label || id, role: item.role, term: item.term, noun: item.noun, links };
    }
    function select(id, save = true) {
      if (selected) by(selected)?.classList.remove('selected');
      selected = id && ids[id] && ids[id].kind === 'label' ? ids[id].of : id;
      const target = selected && by(selected);
      if (target) target.classList.add('selected');
      if (!step) focus(selected);
      if (save) setHash({ n: selected });
      const info = selected ? describe(selected) : null;
      if (opts.onSelect) opts.onSelect(info);
      if (!panel) return;
      panel.classList.toggle('open', !!info);
      panel.replaceChildren();
      if (!info) return;
      panel.append(el('p', 'bp-kind', [info.kind, info.role].filter(Boolean).join(' · ')), el('h2', '', info.label));
      if (info.term) panel.append(el('p', '', 'Glossary term: '), el('code', '', info.term));
      if (info.noun) panel.append(el('p', '', 'Noun of its home design: '), el('code', '', info.noun));
      const list = el('ul');
      for (const l of info.links) { const li = el('li', '', l.text); li.onclick = () => select(l.id); list.appendChild(li); }
      if (info.links.length) panel.append(list);
    }
    svg.addEventListener('click', (e) => {
      const t = e.target.closest('[data-id]');
      if (t) select(t.dataset.id);
    });

    // Base, head and diff: CSS alone shows each; this only switches the class.
    if (svg.classList.contains('diff')) {
      const seg = el('div', 'bp-modes');
      for (const m of ['base', 'head', 'diff']) {
        const b = el('button', '', m[0].toUpperCase() + m.slice(1));
        b.dataset.mode = m;
        b.onclick = () => setMode(m);
        seg.appendChild(b);
      }
      stage.appendChild(seg);
    }
    function setMode(m, save = true) {
      if (!svg.classList.contains('diff')) return;
      mode = m;
      svg.classList.remove('mode-base', 'mode-head', 'mode-diff');
      svg.classList.add('mode-' + m);
      for (const b of stage.querySelectorAll('.bp-modes button')) b.classList.toggle('on', b.dataset.mode === m);
      if (save) setHash({ m: m === 'diff' ? null : m });
    }

    // Step-through: the active message in the accent, the past at full ink, the future dimmed.
    const card = meta.steps.length ? el('div', 'bp-step') : null;
    if (card) stage.appendChild(card);
    function setStep(s, save = true) {
      if (!card) return;
      step = Math.max(0, Math.min(meta.steps.length, s));
      for (const p of parts) {
        const id = p.dataset.id || p.dataset.of || '';
        const own = ids[id] && ids[id].kind === 'edge' ? id : ids[id] && ids[id].of;
        const at = own && ids[own] ? ids[own].step || (ids[ids[own].of] || {}).step : null;
        p.classList.toggle('active', !!step && at === step);
        p.classList.toggle('dim', !!step && !!at && at > step);
      }
      card.replaceChildren();
      const prev = el('button', '', '‹'), next = el('button', '', '›');
      prev.onclick = () => setStep(step - 1);
      next.onclick = () => setStep(step + 1);
      const word = step ? (by(meta.steps[step - 1] + ':label') || { textContent: '' }).textContent.replace(/^\d+/, '') : 'Step through with ← →';
      card.append(prev, el('span', 'bp-count', step ? `${step} / ${meta.steps.length}` : ''), el('span', '', word), next);
      if (!step) focus(selected);
      if (save) setHash({ s: step || null });
    }

    function onKey(e) {
      if (e.target.closest && e.target.closest('input,textarea')) return;
      if (e.key === '+' || e.key === '=') stepZoom(1);
      else if (e.key === '-') stepZoom(-1);
      else if (e.key === '0' || (e.key === '!' && e.shiftKey)) fit();
      else if (e.key === 'Escape') { select(null); setStep(0); }
      else if (card && e.key === 'ArrowRight') { e.preventDefault(); setStep(step + 1); }
      else if (card && e.key === 'ArrowLeft') { e.preventDefault(); setStep(step - 1); }
      else if (svg.classList.contains('diff') && 'bhd'.includes(e.key)) setMode({ b: 'base', h: 'head', d: 'diff' }[e.key]);
    }
    document.addEventListener('keydown', onKey);

    // Every reader state comes back from the URL.
    const h = hash();
    cam = { k: Math.min(4, Math.max(0.25, (+h.get('z') || 100) / 100)), x: +h.get('x') || 0, y: +h.get('y') || 0 };
    draw(false);
    setMode(h.get('m') || 'diff', false);
    setStep(+h.get('s') || 0, false);
    if (h.get('n')) select(h.get('n'), false);
    return { select, setStep, setMode, fit, zoomTo, detach: () => document.removeEventListener('keydown', onKey) };
  }

  globalThis.bpViewer = { attach };
  if (document.currentScript) for (const svg of document.querySelectorAll('svg.bp')) attach(svg);
})();
