// The route preview: the map's roads as the game's own city map draws them (read by the
// game program, `route_map`), the trip's route in the map's red on the very lanes the game
// drives, its stops with their times, and the entry points - one clicked is where the bus
// starts. Dragged to move, the wheel to zoom where the mouse is.

import { html, useEffect, useRef, useState } from '../vendor/preact-htm.js';
import { call } from './api.js';
import { t, plain } from './i18n.js';
import { Spinner } from './ui.js';

const cache = new Map();

/** The stop objects stand beside the road; their dots go onto the route itself: the nearest
 * point of the route's lanes (within 40 m - a stop the route misses stays where it is). */
function onRoute(d) {
  for (const s of d.stops) {
    let best = null;
    let bd = 40 * 40;
    for (const p of d.route) {
      for (let i = 0; i + 3 < p.length; i += 2) {
        const [ax, ay, bx, by] = [p[i], p[i + 1], p[i + 2], p[i + 3]];
        const vx = bx - ax;
        const vy = by - ay;
        const l = vx * vx + vy * vy || 1;
        const k = Math.max(0, Math.min(1, ((s.at[0] - ax) * vx + (s.at[1] - ay) * vy) / l));
        const x = ax + vx * k;
        const y = ay + vy * k;
        const dd = (x - s.at[0]) ** 2 + (y - s.at[1]) ** 2;
        if (dd < bd) (bd = dd), (best = [x, y]);
      }
    }
    if (best) s.at = best;
  }
  return d;
}


// The map package's surface (`d.package`, see docs/MAP_PACKAGE.md): its triangles and lines as
// Path2D objects in map metres, made once and drawn under a transform that follows the view.
const FILL_KEYS = ['water', 'green', 'built', 'grass', 'forest', 'sport', 'farm']; // by class
const FILL_ORDER = [2, 6, 3, 1, 4, 5, 0]; // under everything: built-up land first, water last

function surfacePaths(pkg) {
  if (pkg.paths) return pkg.paths;
  const fills = pkg.fills.map((f) => {
    const path = new Path2D();
    for (let i = 0; i + 5 < f.length; i += 6) {
      path.moveTo(f[i], f[i + 1]);
      path.lineTo(f[i + 2], f[i + 3]);
      path.lineTo(f[i + 4], f[i + 5]);
      path.closePath();
    }
    return path;
  });
  // a river's width tells river from stream; the railway and the borders are one path each
  const lines = { river: new Path2D(), stream: new Path2D(), rail: new Path2D(), border: new Path2D() };
  for (const l of pkg.lines) {
    const path = l.c === 0 ? (l.w >= 5 ? lines.river : lines.stream) : l.c === 1 ? lines.rail : l.c === 2 ? lines.border : null;
    if (!path) continue;
    for (let i = 0; i + 1 < l.p.length; i += 2) (i ? path.lineTo : path.moveTo).call(path, l.p[i], l.p[i + 1]);
  }
  // the terrain's contour lines: the ordinary ones fainter than the index ones (every fifth)
  const contours = { thin: new Path2D(), index: new Path2D() };
  for (const l of pkg.contours || []) {
    const path = l.i ? contours.index : contours.thin;
    for (let i = 0; i + 1 < l.p.length; i += 2) (i ? path.lineTo : path.moveTo).call(path, l.p[i], l.p[i + 1]);
  }
  const images = (pkg.icons || []).map((src) => Object.assign(new Image(), { src }));
  return (pkg.paths = { fills, lines, contours, images });
}

const CONTOUR = '#be9664';

/** The regions' names as big, faint watermarks, each as large as its box allows (a box of width 0 has no room: no name). */
function drawRegions(g, pkg, X, Y, v) {
  g.save();
  g.textAlign = 'center';
  g.textBaseline = 'middle';
  g.fillStyle = '#ffffff';
  g.globalAlpha = 0.16;
  for (const r of pkg.regions || []) {
    if (!(r.bw > 0)) continue;
    g.font = '700 100px Inter, sans-serif';
    const px = Math.min((r.bw / v.mpp / Math.max(g.measureText(r.name).width, 1)) * 100, (r.bh / v.mpp) * 0.9);
    if (px < 9) continue;
    g.font = `700 ${px}px Inter, sans-serif`;
    g.fillText(r.name, X(r.at[0]), Y(r.at[1]));
  }
  g.restore();
}

/** The package's icons (stations and stops), upright at their place; the width is in pixels. */
function drawMarkers(g, pkg, X, Y) {
  const { images } = surfacePaths(pkg);
  for (const m of pkg.markers || []) {
    const img = images[m.icon];
    if (!img || !img.complete || !img.naturalWidth) continue;
    const w = m.w;
    const h = (w * img.naturalHeight) / img.naturalWidth;
    g.drawImage(img, X(m.at[0]) - w / 2, Y(m.at[1]) - h / 2, w, h);
  }
}

/** Where real north lies, as a small compass in the corner: the needle turned `north` degrees clockwise from the map's own up. */
function drawCompass(g, north) {
  const [x, y, r] = [28, 28, 14];
  g.save();
  g.translate(x, y);
  g.fillStyle = 'rgba(14,14,14,0.8)';
  g.beginPath();
  g.arc(0, 0, r + 6, 0, Math.PI * 2);
  g.fill();
  g.rotate((north * Math.PI) / 180);
  g.fillStyle = '#d63028';
  g.beginPath();
  g.moveTo(0, -r);
  g.lineTo(5, 3);
  g.lineTo(-5, 3);
  g.closePath();
  g.fill();
  g.fillStyle = '#9a9a9a';
  g.beginPath();
  g.moveTo(0, r);
  g.lineTo(5, -1);
  g.lineTo(-5, -1);
  g.closePath();
  g.fill();
  g.restore();
}

/** The surface under the roads: ground, areas, then rivers, rail and borders. */
function drawSurface(g, pkg, v, W, H, relief) {
  const c = pkg.colors;
  const { fills, lines, contours } = surfacePaths(pkg);
  g.fillStyle = c.land;
  g.fillRect(0, 0, W, H);
  g.save();
  // map metres (y up) onto the canvas: widths below are pixels, so they are scaled by `mpp`
  g.translate(W / 2 - v.cx / v.mpp, H / 2 + v.cy / v.mpp);
  g.scale(1 / v.mpp, -1 / v.mpp);
  g.lineCap = g.lineJoin = 'round';
  for (const k of FILL_ORDER) {
    g.fillStyle = g.strokeStyle = c[FILL_KEYS[k]];
    g.fill(fills[k]);
    g.lineWidth = 0.8 * v.mpp; // (no seams between neighbouring triangles)
    g.stroke(fills[k]);
  }
  const stroke = (path, color, px) => {
    g.strokeStyle = color;
    g.lineWidth = px * v.mpp;
    g.stroke(path);
  };
  if (relief) {
    // far away only the index lines (every fifth) are drawn: all of them would be a net over the whole map
    if (v.mpp < 7) {
      g.globalAlpha = 0.45;
      stroke(contours.thin, CONTOUR, 0.9);
    }
    g.globalAlpha = 0.85;
    stroke(contours.index, CONTOUR, 1.6);
    g.globalAlpha = 1;
  }
  stroke(lines.river, c.river, 6);
  stroke(lines.stream, c.river, 2.4);
  stroke(lines.rail, c.rail_edge, 3.4);
  stroke(lines.rail, c.rail, 1.6);
  g.globalAlpha = 0.7;
  stroke(lines.border, c.border, 1.2);
  g.restore();
}

function load(map, date, trip) {
  const key = `${map}|${date}|${trip}`;
  if (!cache.has(key)) cache.set(key, call('route_map', { map, date, trip }).then(onRoute, (e) => ({ error: e.message })));
  return cache.get(key);
}

export function RouteMap({ map, date, trip, stops = [], entry = -1, onEntry, height = 340 }) {
  const box = useRef();
  const cv = useRef();
  const [data, setData] = useState(null);
  const view = useRef(null);
  const [hover, setHover] = useState(-1);
  // the terrain's contour lines, when the map's package has them (off at first: the roads are the point)
  const [relief, setRelief] = useState(false);

  useEffect(() => {
    if (!map) return;
    let alive = true;
    load(map, date, trip).then((d) => {
      if (!alive) return;
      setData(d);
      view.current = null;
    });
    return () => (alive = false);
  }, [map, date, trip]);

  // the picture, drawn again whenever anything it shows changes
  const draw = () => {
    const c = cv.current;
    const d = data;
    if (!c || !d || d.error) return;
    const dpr = window.devicePixelRatio || 1;
    const W = box.current.clientWidth;
    const H = box.current.clientHeight;
    if (c.width !== Math.round(W * dpr) || c.height !== Math.round(H * dpr)) Object.assign(c, { width: Math.round(W * dpr), height: Math.round(H * dpr) });
    if (!view.current) {
      // the trip's own route when there is one, else the map
      let lo = d.lo;
      let hi = d.hi;
      if (d.route.length) {
        lo = [Infinity, Infinity];
        hi = [-Infinity, -Infinity];
        for (const p of d.route) for (let i = 0; i < p.length; i += 2) {
          lo = [Math.min(lo[0], p[i]), Math.min(lo[1], p[i + 1])];
          hi = [Math.max(hi[0], p[i]), Math.max(hi[1], p[i + 1])];
        }
      }
      const span = Math.max((hi[0] - lo[0]) / (W - 80), (hi[1] - lo[1]) / (H - 60), 0.2);
      view.current = { cx: (lo[0] + hi[0]) / 2, cy: (lo[1] + hi[1]) / 2, mpp: span };
    }
    const v = view.current;
    const g = c.getContext('2d');
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.clearRect(0, 0, W, H);
    const X = (x) => W / 2 + (x - v.cx) / v.mpp;
    const Y = (y) => H / 2 - (y - v.cy) / v.mpp;
    const line = (p) => {
      g.beginPath();
      for (let i = 0; i < p.length; i += 2) (i ? g.lineTo : g.moveTo).call(g, X(p[i]), Y(p[i + 1]));
    };
    if (d.package) {
      drawSurface(g, d.package, v, W, H, relief);
      drawRegions(g, d.package, X, Y, v);
    }
    g.lineCap = g.lineJoin = 'round';
    const [casing, road, main, red] = d.colors;
    for (let pass = 0; pass < 2; pass++) {
      for (const r of d.roads) {
        line(r.p);
        g.strokeStyle = pass ? (r.main ? main : road) : casing;
        g.lineWidth = Math.max((r.w + (pass ? 0 : 2)) / v.mpp, pass ? 1.4 : 2.4);
        g.stroke();
      }
    }
    g.strokeStyle = red;
    g.lineWidth = 4.5;
    for (const p of d.route) {
      line(p);
      g.stroke();
    }
    if (d.package) {
      drawMarkers(g, d.package, X, Y);
      drawCompass(g, d.package.north || 0);
    }
    // stops, with their times and names
    g.font = '600 11.5px Inter, sans-serif';
    // (a label that would cover one already written goes to the left of its stop, else away)
    const taken = [];
    const free = (x, y, w) => !taken.some((r) => x < r[0] + r[2] && x + w > r[0] && y < r[1] + 20 && y + 20 > r[1]);
    for (const s of d.stops) {
      const x = X(s.at[0]);
      const y = Y(s.at[1]);
      g.fillStyle = '#f0f0f0';
      g.beginPath();
      g.arc(x, y, 3.2, 0, Math.PI * 2);
      g.fill();
      const st = stops[s.k];
      if (st) {
        const label = `${st.time}  ${st.name}`;
        const w = g.measureText(label).width + 10;
        const lx = [x + 8, x - 8 - w].find((lx) => free(lx, y - 10, w));
        if (lx === undefined) continue;
        taken.push([lx, y - 10, w]);
        g.fillStyle = 'rgba(14,14,14,0.86)';
        g.beginPath();
        g.roundRect(lx, y - 10, w, 20, 4);
        g.fill();
        g.fillStyle = '#e8a030';
        g.fillText(plain(st.time), lx + 5, y + 4);
        g.fillStyle = '#ececec';
        g.fillText(plain(st.name), lx + 5 + g.measureText(`${st.time}  `).width, y + 4);
      }
    }
    // entry points: amber, the hovered one ringed grey, the chosen one white
    d.entries.forEach((e, i) => {
      const x = X(e.at[0]);
      const y = Y(e.at[1]);
      g.fillStyle = '#e8a030';
      g.beginPath();
      g.arc(x, y, 4, 0, Math.PI * 2);
      g.fill();
      if (i === hover || e.index === entry) {
        g.strokeStyle = e.index === entry ? '#ffffff' : '#a0a0a0';
        g.lineWidth = 2;
        g.beginPath();
        g.arc(x, y, e.index === entry ? 10 : 7.5, 0, Math.PI * 2);
        g.stroke();
      }
    });
    if (hover >= 0 && d.entries[hover]) {
      const e = d.entries[hover];
      g.font = '500 12px Inter, sans-serif';
      const w = g.measureText(e.name).width + 12;
      g.fillStyle = 'rgba(14,14,14,0.9)';
      g.beginPath();
      g.roundRect(X(e.at[0]) - w / 2, Y(e.at[1]) - 34, w, 22, 4);
      g.fill();
      g.fillStyle = '#ffffff';
      g.fillText(plain(e.name), X(e.at[0]) - w / 2 + 6, Y(e.at[1]) - 19);
    }
  };
  useEffect(draw);
  useEffect(() => {
    const ro = new ResizeObserver(() => draw());
    ro.observe(box.current);
    return () => ro.disconnect();
  });

  const at = (e) => {
    const r = cv.current.getBoundingClientRect();
    return [e.clientX - r.left, e.clientY - r.top];
  };
  const entryAt = ([px, py]) => {
    const d = data;
    const v = view.current;
    if (!d?.entries || !v) return -1;
    const W = box.current.clientWidth;
    const H = box.current.clientHeight;
    let best = -1;
    let bd = 12 * 12;
    d.entries.forEach((e, i) => {
      const dx = W / 2 + (e.at[0] - v.cx) / v.mpp - px;
      const dy = H / 2 - (e.at[1] - v.cy) / v.mpp - py;
      if (dx * dx + dy * dy < bd) (bd = dx * dx + dy * dy), (best = i);
    });
    return best;
  };
  const drag = useRef(null);
  return html`<div class="routemap" ref=${box} style=${height ? `height:${height}px` : ''}
      onPointerDown=${(e) => {
        drag.current = { p: at(e), moved: 0 };
        e.currentTarget.setPointerCapture(e.pointerId);
      }}
      onPointerMove=${(e) => {
        const p = at(e);
        const d = drag.current;
        if (d && view.current) {
          const dx = p[0] - d.p[0];
          const dy = p[1] - d.p[1];
          d.moved += Math.abs(dx) + Math.abs(dy);
          d.p = p;
          view.current.cx -= dx * view.current.mpp;
          view.current.cy += dy * view.current.mpp;
          draw();
        } else {
          const h = entryAt(p);
          if (h !== hover) setHover(h);
        }
      }}
      onPointerUp=${(e) => {
        const d = drag.current;
        drag.current = null;
        if (d && d.moved < 5) {
          const i = entryAt(at(e));
          if (i >= 0) onEntry?.(data.entries[i].index);
        }
      }}
      onPointerLeave=${() => setHover(-1)}
      onWheel=${(e) => {
        e.preventDefault();
        const v = view.current;
        if (!v) return;
        const [px, py] = at(e);
        const W = box.current.clientWidth;
        const H = box.current.clientHeight;
        const wx = v.cx + (px - W / 2) * v.mpp;
        const wy = v.cy - (py - H / 2) * v.mpp;
        v.mpp = Math.min(Math.max(v.mpp * (e.deltaY > 0 ? 1.15 : 1 / 1.15), 0.15), 200);
        v.cx = wx - (px - W / 2) * v.mpp;
        v.cy = wy + (py - H / 2) * v.mpp;
        draw();
      }}>
    <canvas ref=${cv}></canvas>
    ${!data && html`<div class="state"><${Spinner} /> ${t('Reading the map…')}</div>`}
    ${data?.error && html`<div class="state">${data.error}</div>`}
    ${data?.package?.contours?.length > 0 && html`<button class=${'tool' + (relief ? ' on' : '')} onPointerDown=${(e) => e.stopPropagation()} onClick=${() => setRelief(!relief)}>${t('Relief')}</button>`}
    ${data?.package?.credit && html`<div class="credit">${data.package.credit}</div>`}
    ${data && !data.error && html`<div class="legend">${t('{roads} roads | {stops} stops | {entries} entry points', { roads: data.roads.length, stops: data.stops.length, entries: data.entries.length })}</div>`}
  </div>`;
}
