const COLS = 5;
const ROWS = 3;
const $ = (id) => document.getElementById(id);

let cfg = null;
let widgets = {};
let device = null;
let selectedId = null;
let targetKey = null;
let drag = null;
let saveTimer = null;
let saving = false;

const PRESS = [
  ['none', 'Nothing'],
  ['next', 'Next page'],
  ['prev', 'Previous page'],
  ['page', 'Go to page'],
  ['open', 'Open an app, file or website'],
  ['refresh', 'Refresh weather'],
];

function esc(s) {
  return String(s ?? '').replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
}

const page = () => cfg.pages[cfg.activePage];
const tileById = (id) => page().tiles.find((t) => t.id === id);
const overlaps = (a, b) => a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
const fits = (t) => t.x >= 0 && t.y >= 0 && t.x + t.w <= COLS && t.y + t.h <= ROWS;
const uid = () => Math.random().toString(36).slice(2, 10);

function save() {
  clearTimeout(saveTimer);
  saving = true;
  setSaveState('');
  saveTimer = setTimeout(async () => {
    try {
      const res = await fetch('/api/config', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(cfg),
      });
      if (!res.ok) throw new Error((await res.json()).error || res.statusText);
      setSaveState('Saved');
    } catch (e) {
      setSaveState(`Not saved: ${e.message}`, true);
    } finally {
      saving = false;
    }
  }, 250);
}

let fadeTimer;
function setSaveState(text, error = false) {
  const el = $('save-state');
  el.textContent = text;
  el.classList.toggle('error', error);
  el.classList.remove('fade');
  clearTimeout(fadeTimer);
  if (text && !error) fadeTimer = setTimeout(() => el.classList.add('fade'), 1500);
}

function layout() {
  const stage = $('stage');
  const gap = cfg.gap;
  const unitW = 5 * 72 + 4 * gap + 2 * 0.3 * 72;
  const unitH = 3 * 72 + 2 * gap + 2 * 0.3 * 72;
  const s = Math.max(0.6, Math.min(2.2, (stage.clientWidth - 64) / unitW, (stage.clientHeight - 100) / unitH));
  document.documentElement.style.setProperty('--k', `${72 * s}px`);
  document.documentElement.style.setProperty('--g', `${gap * s}px`);
}

function renderStatus() {
  const text = $('status').querySelector('.status-text');
  if (device?.name) text.textContent = device.name;
  else if (device?.error) text.textContent = 'Stream Deck in use';
  else text.textContent = 'No Stream Deck';
  $('deck').classList.toggle('offline', !device?.name);

  const note = $('note');
  const needsKey = page().tiles.some((t) => t.type === 'weather' || t.type === 'forecast') && !cfg.weather.apiKey;
  if (device?.error) note.textContent = 'Quit the Elgato Stream Deck app so this one can take over the keys.';
  else if (!device?.name) note.textContent = 'Plug in your Stream Deck. The preview keeps working without it.';
  else if (needsKey) note.innerHTML = 'Weather needs an OpenWeatherMap key. <button type="button" data-open-settings>Add it in settings</button>';
  else note.textContent = '';
}

function renderPages() {
  const nav = $('pages');
  nav.innerHTML = '';
  cfg.pages.forEach((p, i) => {
    const tab = document.createElement('div');
    tab.className = 'page-tab' + (i === cfg.activePage ? ' active' : '');
    const b = document.createElement('button');
    b.type = 'button';
    b.textContent = p.name;
    b.title = i === cfg.activePage ? 'Double-click to rename' : `Show ${p.name}`;
    b.setAttribute('aria-current', i === cfg.activePage ? 'page' : 'false');
    b.onclick = () => switchPage(i);
    b.ondblclick = () => renamePage(tab, i);
    tab.append(b);
    if (i === cfg.activePage && cfg.pages.length > 1) {
      const del = document.createElement('button');
      del.type = 'button';
      del.className = 'del';
      del.innerHTML = '&times;';
      del.setAttribute('aria-label', `Delete page ${p.name}`);
      del.onclick = () => {
        if (!confirm(`Delete "${p.name}" and its widgets?`)) return;
        cfg.pages.splice(i, 1);
        cfg.activePage = Math.max(0, i - 1);
        selectedId = null;
        renderAll();
        save();
      };
      tab.append(del);
    }
    nav.append(tab);
  });
  const add = document.createElement('button');
  add.type = 'button';
  add.className = 'page-add';
  add.textContent = '+';
  add.setAttribute('aria-label', 'Add page');
  add.title = 'Add page';
  add.onclick = () => {
    cfg.pages.push({ name: `Page ${cfg.pages.length + 1}`, tiles: [] });
    cfg.activePage = cfg.pages.length - 1;
    selectedId = null;
    renderAll();
    save();
  };
  nav.append(add);
}

function renamePage(tab, i) {
  const input = document.createElement('input');
  input.value = cfg.pages[i].name;
  input.maxLength = 30;
  input.setAttribute('aria-label', 'Page name');
  tab.replaceChildren(input);
  input.focus();
  input.select();
  const done = (keep) => {
    if (keep && input.value.trim()) {
      cfg.pages[i].name = input.value.trim();
      save();
    }
    renderPages();
  };
  input.onkeydown = (e) => {
    if (e.key === 'Enter') done(true);
    if (e.key === 'Escape') done(false);
  };
  input.onblur = () => done(true);
}

function switchPage(i) {
  if (i === cfg.activePage) return;
  cfg.activePage = i;
  selectedId = null;
  targetKey = null;
  renderAll();
  fetch('/api/page', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ index: i }) });
}

function renderKeys() {
  const keys = $('keys');
  if (keys.children.length) {
    [...keys.children].forEach((k, i) => k.classList.toggle('target', i === targetKey));
    return;
  }
  for (let i = 0; i < COLS * ROWS; i++) {
    const k = document.createElement('button');
    k.type = 'button';
    k.className = 'key';
    k.style.setProperty('--c', i % COLS);
    k.style.setProperty('--r', Math.floor(i / COLS));
    k.setAttribute('aria-label', `Empty key, row ${Math.floor(i / COLS) + 1}, column ${(i % COLS) + 1}`);
    k.onclick = () => {
      selectedId = null;
      targetKey = targetKey === i ? null : i;
      renderSelection();
    };
    keys.append(k);
  }
}

function renderOverlays() {
  const box = $('overlays');
  box.innerHTML = '';
  for (const t of page().tiles) {
    const w = widgets[t.type];
    if (!w) continue;
    const el = document.createElement('button');
    el.type = 'button';
    el.className = 'tile' + (t.id === selectedId ? ' selected' : '');
    el.draggable = true;
    el.dataset.id = t.id;
    for (const k of ['x', 'y', 'w', 'h']) el.style.setProperty('--' + k, t[k]);
    el.setAttribute('aria-label', `${w.name}, ${t.w} by ${t.h}`);
    el.onclick = () => {
      selectedId = t.id;
      targetKey = null;
      renderSelection();
    };
    el.ondragstart = (e) => {
      const r = el.getBoundingClientRect();
      const pitch = r.width / t.w;
      drag = {
        kind: 'move',
        id: t.id,
        w: t.w,
        h: t.h,
        dx: Math.min(t.w - 1, Math.floor((e.clientX - r.left) / pitch)),
        dy: Math.min(t.h - 1, Math.floor((e.clientY - r.top) / (r.height / t.h))),
      };
      e.dataTransfer.effectAllowed = 'move';
      e.dataTransfer.setData('text/plain', t.id);
      requestAnimationFrame(() => el.classList.add('dragging'));
    };
    el.ondragend = () => {
      el.classList.remove('dragging');
      endDrag();
    };
    box.append(el);
  }
}

function renderLibrary() {
  const q = $('search').value.trim().toLowerCase();
  const groups = {};
  for (const [type, w] of Object.entries(widgets)) {
    if (q && !w.name.toLowerCase().includes(q) && !w.group.toLowerCase().includes(q)) continue;
    (groups[w.group] ||= []).push([type, w]);
  }
  const lib = $('lib');
  lib.innerHTML = '';
  for (const [group, items] of Object.entries(groups)) {
    const sec = document.createElement('section');
    sec.className = 'group';
    sec.innerHTML = `<h3>${esc(group)}</h3>`;
    for (const [type, w] of items) {
      const b = document.createElement('button');
      b.type = 'button';
      b.className = 'item';
      b.draggable = true;
      b.innerHTML = `<span>${esc(w.name)}</span><span class="size">${w.size[0]}×${w.size[1]}</span>`;
      b.title = `Add ${w.name}`;
      b.onclick = () => addWidget(type);
      b.ondragstart = (e) => {
        drag = { kind: 'new', type, w: w.size[0], h: w.size[1], dx: 0, dy: 0 };
        e.dataTransfer.effectAllowed = 'copy';
        e.dataTransfer.setData('text/plain', type);
      };
      b.ondragend = endDrag;
      sec.append(b);
    }
    lib.append(sec);
  }
  if (!lib.children.length) lib.innerHTML = '<p class="group" style="color:var(--muted);margin:8px">No widgets match that search.</p>';
}

function newTile(type, x, y, w, h) {
  const def = widgets[type];
  return {
    id: uid(),
    type,
    x, y, w, h,
    settings: structuredClone(def.defaults),
    press: { type: def.press || 'none', page: 0, target: '' },
  };
}

function place(tile) {
  const p = page();
  p.tiles = p.tiles.filter((t) => t.id === tile.id || !overlaps(t, tile));
  if (!p.tiles.includes(tile)) p.tiles.push(tile);
  selectedId = tile.id;
  targetKey = null;
  renderAll();
  save();
}

function freeSpot(w, h) {
  for (let y = 0; y + h <= ROWS; y++) {
    for (let x = 0; x + w <= COLS; x++) {
      const t = { x, y, w, h };
      if (!page().tiles.some((o) => overlaps(o, t))) return t;
    }
  }
  return null;
}

function addWidget(type) {
  let [w, h] = widgets[type].size;
  let spot;
  if (targetKey !== null) {
    const x = targetKey % COLS;
    const y = Math.floor(targetKey / COLS);
    spot = { x: Math.min(x, COLS - w), y: Math.min(y, ROWS - h), w, h };
  } else {
    spot = freeSpot(w, h) || freeSpot(1, 1);
  }
  if (!spot) {
    setSaveState('This page is full. Remove a widget or select a key to replace.', true);
    return;
  }
  place(newTile(type, spot.x, spot.y, spot.w, spot.h));
}

function cellFromPoint(e) {
  const r = $('keys').getBoundingClientRect();
  const k = parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--k'));
  const g = parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--g'));
  const col = Math.floor((e.clientX - r.left + g / 2) / (k + g));
  const row = Math.floor((e.clientY - r.top + g / 2) / (k + g));
  return { col: Math.max(0, Math.min(COLS - 1, col)), row: Math.max(0, Math.min(ROWS - 1, row)) };
}

function footprint(e) {
  const { col, row } = cellFromPoint(e);
  return {
    x: Math.max(0, Math.min(COLS - drag.w, col - drag.dx)),
    y: Math.max(0, Math.min(ROWS - drag.h, row - drag.dy)),
    w: drag.w,
    h: drag.h,
  };
}

function endDrag() {
  drag = null;
  $('ghost').hidden = true;
}

function renderSelection() {
  [...$('overlays').children].forEach((el) => el.classList.toggle('selected', el.dataset.id === selectedId));
  renderKeys();
  renderInspector();
}

function renderInspector() {
  const box = $('inspector');
  const t = selectedId && tileById(selectedId);
  if (!t) {
    box.innerHTML = targetKey !== null
      ? `<p class="empty"><strong>Key ${targetKey + 1} selected</strong>Click a widget on the right to place it here.</p>`
      : '<p class="empty"><strong>Nothing selected</strong>Click a widget on the deck to change it, or drag one in from the right.</p>';
    return;
  }
  const w = widgets[t.type];
  box.innerHTML = '';
  const wrap = document.createElement('div');
  wrap.className = 'insp';

  const left = document.createElement('div');
  left.className = 'insp-col';
  left.innerHTML = `<h2 class="insp-head">${esc(w.name)}</h2>`;
  left.append(stepper('Width', 'w', COLS), stepper('Height', 'h', ROWS));
  const blocked = document.createElement('p');
  blocked.className = 'blocked';
  blocked.id = 'blocked';
  left.append(blocked);
  const remove = document.createElement('button');
  remove.type = 'button';
  remove.className = 'btn danger';
  remove.textContent = 'Remove';
  remove.onclick = removeSelected;
  left.append(remove);

  const mid = document.createElement('div');
  mid.className = 'insp-col';
  const s = { ...w.defaults, ...t.settings };
  if (w.schema.length) mid.innerHTML = '<h3>Options</h3>';
  for (const f of w.schema) mid.append(settingField(t, f, s[f.key]));

  const right = document.createElement('div');
  right.className = 'insp-col';
  right.innerHTML = '<h3>When pressed</h3>';
  right.append(pressFields(t));

  wrap.append(left, mid, right);
  box.append(wrap);
}

function stepper(labelText, key, max) {
  const t = tileById(selectedId);
  const row = document.createElement('div');
  row.className = 'stepper';
  row.innerHTML = `<span>${labelText}</span><div class="ctl"><button type="button" aria-label="Decrease ${labelText.toLowerCase()}">−</button><output>${t[key]}</output><button type="button" aria-label="Increase ${labelText.toLowerCase()}">+</button></div>`;
  const [dec, inc] = row.querySelectorAll('button');
  dec.disabled = t[key] <= 1;
  inc.disabled = t[key] >= max;
  dec.onclick = () => resize(key, -1);
  inc.onclick = () => resize(key, 1);
  return row;
}

function resize(key, delta) {
  const t = tileById(selectedId);
  const next = { ...t, [key]: t[key] + delta };
  if (!fits(next)) {
    if (key === 'w') next.x = COLS - next.w;
    else next.y = ROWS - next.h;
  }
  if (!fits(next)) return;
  const hit = page().tiles.find((o) => o.id !== t.id && overlaps(o, next));
  if (hit) {
    $('blocked').textContent = `${widgets[hit.type].name} is in the way.`;
    return;
  }
  Object.assign(t, { x: next.x, y: next.y, w: next.w, h: next.h });
  renderOverlays();
  renderInspector();
  save();
}

function settingField(t, f, value) {
  const set = (v) => {
    t.settings = { ...t.settings, [f.key]: v };
    save();
  };
  if (f.type === 'toggle') {
    const l = document.createElement('label');
    l.className = 'toggle';
    l.innerHTML = `<input type="checkbox"><span>${esc(f.label)}</span>`;
    const input = l.querySelector('input');
    input.checked = !!value;
    input.onchange = () => set(input.checked);
    return l;
  }
  const l = document.createElement('label');
  l.className = 'field';
  l.innerHTML = `<span>${esc(f.label)}</span>`;
  const input = document.createElement('input');
  input.type = f.type === 'color' ? 'color' : 'text';
  input.value = value ?? '';
  if (f.type === 'text') input.maxLength = 40;
  input.oninput = () => set(input.value);
  l.append(input);
  return l;
}

function pressFields(t) {
  const frag = document.createDocumentFragment();
  const w = widgets[t.type];
  const sel = document.createElement('select');
  sel.setAttribute('aria-label', 'When pressed');
  for (const [v, text] of PRESS) {
    if (v === 'refresh' && w.group !== 'Weather') continue;
    sel.add(new Option(text, v, false, t.press.type === v));
  }
  const extra = document.createElement('div');
  const drawExtra = () => {
    extra.innerHTML = '';
    if (t.press.type === 'page') {
      const ps = document.createElement('select');
      ps.setAttribute('aria-label', 'Page');
      cfg.pages.forEach((p, i) => ps.add(new Option(p.name, i, false, t.press.page === i)));
      ps.onchange = () => { t.press.page = Number(ps.value); save(); };
      ps.style.marginTop = '10px';
      extra.append(ps);
    }
    if (t.press.type === 'open') {
      const l = document.createElement('label');
      l.className = 'field';
      l.style.marginTop = '10px';
      l.innerHTML = '<span>Path or URL</span><input type="text" placeholder="https://… or C:\\Program Files\\…\\app.exe" spellcheck="false">';
      const input = l.querySelector('input');
      input.value = t.press.target || '';
      input.oninput = () => { t.press.target = input.value; save(); };
      extra.append(l);
    }
  };
  sel.onchange = () => { t.press.type = sel.value; drawExtra(); save(); };
  drawExtra();
  frag.append(sel, extra);
  return frag;
}

function removeSelected() {
  const p = page();
  p.tiles = p.tiles.filter((t) => t.id !== selectedId);
  selectedId = null;
  renderAll();
  save();
}

function renderAll() {
  layout();
  renderStatus();
  renderPages();
  renderKeys();
  renderOverlays();
  renderInspector();
}

function openSettings() {
  $('set-key').value = cfg.weather.apiKey;
  $('set-city').value = cfg.weather.city;
  document.querySelector(`input[name=units][value=${cfg.weather.units}]`).checked = true;
  $('set-hide-city').checked = !!cfg.weather.hideCity;
  $('set-bright').value = cfg.brightness;
  $('out-bright').textContent = `${cfg.brightness}%`;
  $('set-gap').value = cfg.gap;
  $('out-gap').textContent = `${cfg.gap} px`;
  $('settings').showModal();
}

function bindSettings() {
  $('open-settings').onclick = openSettings;
  $('quit').onclick = async () => {
    await fetch('/api/quit', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: '{}' }).catch(() => {});
    $('settings').close();
    document.body.innerHTML = '<p style="margin:auto;color:#8f8e8a">Deck Display has quit. Start it again from the Start menu.</p>';
  };
  document.addEventListener('click', (e) => {
    if (e.target.closest('[data-open-settings]')) openSettings();
  });
  $('set-key').onchange = (e) => { cfg.weather.apiKey = e.target.value.trim(); save(); renderStatus(); };
  $('set-city').onchange = (e) => { cfg.weather.city = e.target.value.trim(); save(); };
  document.querySelectorAll('input[name=units]').forEach((r) => {
    r.onchange = () => { cfg.weather.units = r.value; save(); };
  });
  $('set-hide-city').onchange = (e) => { cfg.weather.hideCity = e.target.checked; save(); };
  $('set-bright').oninput = (e) => {
    cfg.brightness = Number(e.target.value);
    $('out-bright').textContent = `${cfg.brightness}%`;
    save();
  };
  $('set-gap').oninput = (e) => {
    cfg.gap = Number(e.target.value);
    $('out-gap').textContent = `${cfg.gap} px`;
    layout();
    save();
  };
}

function bindDeckDrop() {
  const deck = $('deck');
  deck.addEventListener('dragover', (e) => {
    if (!drag) return;
    e.preventDefault();
    const f = footprint(e);
    const ghost = $('ghost');
    for (const k of ['x', 'y', 'w', 'h']) ghost.style.setProperty('--' + k, f[k]);
    ghost.hidden = false;
  });
  deck.addEventListener('dragleave', (e) => {
    if (!deck.contains(e.relatedTarget)) $('ghost').hidden = true;
  });
  deck.addEventListener('drop', (e) => {
    if (!drag) return;
    e.preventDefault();
    const f = footprint(e);
    if (drag.kind === 'new') {
      place(newTile(drag.type, f.x, f.y, f.w, f.h));
    } else {
      const t = tileById(drag.id);
      if (t) place(Object.assign(t, { x: f.x, y: f.y }));
    }
    endDrag();
  });
}

document.addEventListener('keydown', (e) => {
  if (e.target.closest('input, select, textarea, dialog')) return;
  if ((e.key === 'Delete' || e.key === 'Backspace') && selectedId) removeSelected();
  if (e.key === 'Escape') {
    selectedId = null;
    targetKey = null;
    renderSelection();
  }
});

$('search').oninput = renderLibrary;
window.addEventListener('resize', () => cfg && layout());
bindSettings();
bindDeckDrop();

const events = new EventSource('/events');
events.addEventListener('state', (e) => {
  const s = JSON.parse(e.data);
  widgets = s.widgets;
  device = s.device;
  const first = !cfg;
  const changed = first || JSON.stringify(s.config) !== JSON.stringify(cfg);
  if (changed && !saving && !drag) {
    cfg = s.config;
    if (selectedId && !tileById(selectedId)) selectedId = null;
    if (first) renderLibrary();
    renderAll();
  } else {
    renderStatus();
  }
});
events.addEventListener('frame', (e) => {
  $('deck').style.setProperty('--panel', `url(${JSON.parse(e.data)})`);
});
events.onerror = () => setSaveState('Lost connection to the app. Is it still running?', true);
events.onopen = () => setSaveState('');
