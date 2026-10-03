// Minimal line-art gears in the header (Fortress of Meropide feel): a small gear train drawn at a fixed
// size and pinned to the left corner so resizing never crops or zooms it.

/** Closed gear outline with trapezoid teeth. */
function gearPath(r, teeth, depth) {
  const pitch = (Math.PI * 2) / teeth;
  const ri = r - depth;
  const pt = (a, rad) => `${(Math.cos(a) * rad).toFixed(2)},${(Math.sin(a) * rad).toFixed(2)}`;
  let d = '';
  for (let i = 0; i < teeth; i++) {
    const a = i * pitch;
    d += `${i === 0 ? 'M' : 'L'}${pt(a, ri)} L${pt(a + pitch * 0.2, r)} L${pt(a + pitch * 0.5, r)} L${pt(a + pitch * 0.7, ri)} `;
  }
  return `${d}Z`;
}

const polar = (a, rad) => [(Math.cos(a) * rad).toFixed(2), (Math.sin(a) * rad).toFixed(2)];

/**
 * A gear centred at cx,cy turning once per `seconds` (clockwise unless `ccw`).
 * `face` is 'spokes' (wheel) or 'plain' (hub only).
 */
function gear({ cx, cy, r, teeth, seconds, ccw = false, face = 'plain', count = 0 }) {
  const depth = Math.max(3, r * 0.12);
  const hub = Math.max(2.5, r * 0.16);
  const rim = (r - depth) * 0.74;
  let inner = `<path d="${gearPath(r, teeth, depth)}"/><circle r="${hub}"/>`;
  if (face === 'spokes') {
    inner += `<circle r="${rim}"/>`;
    for (let i = 0; i < count; i++) {
      const a = (i / count) * Math.PI * 2;
      const [x1, y1] = polar(a, hub);
      const [x2, y2] = polar(a, rim);
      inner += `<line x1="${x1}" y1="${y1}" x2="${x2}" y2="${y2}"/>`;
    }
  }
  const style = `animation-duration:${seconds}s;animation-direction:${ccw ? 'reverse' : 'normal'}`;
  return `<g transform="translate(${cx} ${cy})"><g class="mech-spin" style="${style}">${inner}</g></g>`;
}

/** Centre of a gear of radius r2 meshing with (cx, cy, r1) at `deg` degrees. */
function meshAt(cx, cy, r1, r2, deg) {
  const dist = r1 + r2 - Math.max(3, r1 * 0.12) * 0.8;
  const a = (deg * Math.PI) / 180;
  return { cx: cx + Math.cos(a) * dist, cy: cy + Math.sin(a) * dist };
}

function buildHeroArt() {
  const left = document.getElementById('hero-mech-left');
  if (!left) return;

  // Meshing gears turn opposite ways, with periods proportional to their tooth counts.
  // Left: a big spoked wheel rising from the bottom-left corner, driving a small gear.
  const wheel = { cx: 30, cy: 104, r: 62, teeth: 22, seconds: 44, face: 'spokes', count: 6 };
  left.innerHTML =
    gear(wheel) +
    gear({ ...meshAt(30, 104, 62, 20, -38), r: 20, teeth: 7, seconds: (44 * 7) / 22, ccw: true });
}

document.addEventListener('DOMContentLoaded', buildHeroArt);
