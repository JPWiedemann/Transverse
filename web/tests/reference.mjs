// Runs the reference cases of tests/reference/cases.json (made by the Python tool with its finite-difference
// grid) through the WebAssembly module, which solves with finite elements, and compares them. The tolerances
// are those of the native finite-element parity tests (crates/transverse-lines/tests/fem_reference.rs);
// they are relative, except for the ground-split shares (percentage points).
//
//   scripts/build-web.sh && node web/tests/reference.mjs
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { initSync, analyze, synthesize, line_types, param_specs, defaults, colormap } from '../pkg/transverse_wasm.js';

const here = dirname(fileURLToPath(import.meta.url));
initSync({ module: readFileSync(join(here, '../pkg/transverse_wasm_bg.wasm')) });
const cases = JSON.parse(readFileSync(join(here, '../../tests/reference/cases.json'), 'utf8'));

const LINE = 'planar';
const TOL_Z0 = 1e-3;
const TOL_EEFF = 1e-3;
const TOL_LOSS = 1.2e-3;
const TOL_SYNTH = 3e-3;

let checks = 0;
let failures = 0;

function close(what, got, want, tol) {
  checks += 1;
  const err = Math.abs(got - want) / Math.max(Math.abs(want), 1e-300);
  if (!(err <= tol)) {
    failures += 1;
    console.error(`FAIL ${what}: got ${got}, want ${want} (rel. error ${err.toExponential(2)})`);
  }
}

/** Percentage-point difference of a ground-split share. */
function points(what, got, want, limit) {
  checks += 1;
  if (!(Math.abs(got - want) < limit)) {
    failures += 1;
    console.error(`FAIL ${what}: got ${got} %, want ${want} % (limit ${limit} points)`);
  }
}

function same(what, got, want) {
  checks += 1;
  if (got !== want) {
    failures += 1;
    console.error(`FAIL ${what}: got ${got}, want ${want}`);
  }
}

// the reference stores choices by name, the form contract by index
const GAP = { upper: 0, lower: 1, custom: 2 };
const MESH = { auto: 0, manual: 1 };
function toSet(p) {
  return { ...p, gap_mode: GAP[p.gap_mode], mesh_mode: MESH[p.mesh_mode] };
}

// the contract itself
const types = JSON.parse(line_types());
same('synthesis targets', types[0].synth_targets.map((t) => t.id).join(), 'w,s,w_total,pair_w,d,pair_s,pair_wd');
same('line types', types.map((t) => t.id).join(), LINE);
const specs = JSON.parse(param_specs(LINE));
same('colour map size', colormap('Inferno').length, 256 * 3);
same('form size', specs.length, Object.keys(JSON.parse(defaults(LINE))).length);

// the two manual-mesh cases hold the value of a deliberately coarse grid, which is not what the finite elements
// converge to
for (const c of cases.cases.filter((k) => !k.name.startsWith('manual_mesh'))) {
  // the domain extension of an open structure is part of the reference value: the same one here
  const set = toSet({ ...c.params, margin: c.margin > 0 ? c.margin : c.params.margin });
  const a = analyze(LINE, JSON.stringify(set), undefined);
  const s = JSON.parse(a.summary());
  const m = s.modes[0];
  close(`${c.name} z0`, m.z0, c.z0, TOL_Z0);
  close(`${c.name} eps_eff`, m.eps_eff, c.eeff, TOL_EEFF);
  close(`${c.name} C`, m.c, c.c, TOL_Z0);
  same(`${c.name} triangle mesh`, s.accuracy.nx === 0 && s.accuracy.nodes > 1000, true);
  close(`${c.name} alpha_c`, m.at_f.alpha_c, c.loss.alpha_c, TOL_LOSS);
  close(`${c.name} alpha_d`, m.at_f.alpha_d, c.loss.alpha_d, TOL_LOSS);
  // loss over frequency through the typed-array path: the same model as the mode result
  const row = a.sample(new Float64Array([c.params.f]));
  close(`${c.name} sample alpha_c`, row[2], m.at_f.alpha_c, 1e-12);
  // measured: up to 0.4 points, 1.5 for t = 0, and one exception: the field share of the gap flank with an air
  // gap, where the reference grid is not converged (3.86 %; 3.45 % at a target error of 0.03 % and still
  // falling; the finite elements give 3.33 % here and 3.18 % at 0.01 %)
  const limit = c.params.t === 0 ? 1.7 : 0.4;
  const fieldLimit = (key) => (c.name === 'gap_custom_air' && key === 'cop_flank' ? 0.6 : limit);
  for (const r of m.split.rows) {
    if (r.key in c.split.field) {
      points(`${c.name} field share ${r.key}`, r.field_pct, c.split.field[r.key], fieldLimit(r.key));
      points(`${c.name} current share ${r.key}`, r.current_pct, c.split.current[r.key], limit);
    }
  }
  if (s.has_field) {
    const info = JSON.parse(a.field_info());
    same(`${c.name} field is a mesh`, info.mesh, true);
    // the field image of the signal region: right size, potential scale 0..1 V, a probe between the conductors
    const r = a.field_raster('potential', info.view_zoom, info.view_zoom, false, 40, 300);
    same(`${c.name} raster bytes`, r.rgba().length, r.width() * r.height() * 4);
    const leg = JSON.parse(r.legend());
    close(`${c.name} potential scale top`, leg.vmax, 1, 1e-9);
    const [x0, x1, y0, y1] = info.view_zoom;
    const p = JSON.parse(a.field_probe((x0 + x1) / 2, (y0 + y1) / 2));
    same(`${c.name} probe finds the field`, p !== null && typeof p.v === 'number', true);
    r.free();
  }
  a.free();
}

for (const c of cases.synthesis) {
  const a = synthesize(LINE, JSON.stringify(toSet(c.params)), c.var, c.target, undefined, undefined, undefined);
  const p = JSON.parse(a.params());
  close(`${c.name} w`, p.w, c.result.w, TOL_SYNTH);
  // Z0 depends only weakly on the gap, so a Z0 error of 5e-4 moves a synthesised gap by several times that
  close(`${c.name} s`, p.s, c.result.s, 5e-3);
  close(`${c.name} z0`, JSON.parse(a.summary()).modes[0].z0, c.z0, TOL_SYNTH);
  a.free();
}

// a synthesis with a phase also sets the line length: a quarter wavelength of the guided wave for
// 90 degrees, with the eps_eff of the synthesised geometry (the 35 ohm synthesis case)
{
  const c = cases.synthesis[0];
  const a = synthesize(LINE, JSON.stringify(toSet(c.params)), c.var, c.target, undefined, 90, undefined);
  const m = JSON.parse(a.summary()).modes[0];
  const lambdaMm = (299_792_458 / (c.params.f * Math.sqrt(m.eps_eff))) * 1e3;
  close('phase synthesis: length', JSON.parse(a.params()).length, lambdaMm / 4, 1e-9);
  close('phase synthesis: wavelength', m.wavelength_mm, lambdaMm, 1e-9);
  close('phase synthesis: phase', m.phase_deg, 90, 1e-9);
  close('phase synthesis: z0 still reached', m.z0, c.z0, TOL_SYNTH);
  a.free();
}

// the mesh of a result and the questions the field view asks about it
{
  const c = cases.cases.find((k) => k.name === 'final_stackup');
  const a = analyze(LINE, JSON.stringify(toSet(c.params)), undefined);
  const info = JSON.parse(a.field_info());
  const edges = a.field_mesh_edges(new Float64Array(info.view_full));
  same('mesh edges are segments', edges.length > 4000 && edges.length % 4 === 0, true);
  same('mesh edges of a small window', a.field_mesh_edges(new Float64Array([-0.1, 0.1, -0.05, 0.05])).length < edges.length / 4, true);
  const r = a.field_raster('e_db_lines', info.view_zoom, info.view_zoom, false, 40, 300);
  same('raster bytes', r.rgba().length, r.width() * r.height() * 4);
  r.free();
  same('field lines', JSON.parse(a.field_lines(new Float64Array(info.view_zoom), 1, 1)).length > 20, true);
  a.free();
}

console.log(`${checks} checks, ${failures} failed`);
process.exit(failures === 0 ? 0 : 1);
