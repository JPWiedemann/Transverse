// Runs the reference cases of tests/reference/cases.json (made by the Python tool) through the
// WebAssembly module and compares them. All tolerances are relative.
//
//   scripts/build-web.sh && node web/tests/reference.mjs
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { initSync, analyze, synthesize, line_types, param_specs, defaults } from '../pkg/transverse_wasm.js';

const here = dirname(fileURLToPath(import.meta.url));
initSync({ module: readFileSync(join(here, '../pkg/transverse_wasm_bg.wasm')) });
const cases = JSON.parse(readFileSync(join(here, '../../tests/reference/cases.json'), 'utf8'));

const LINE = 'cpwg_inner';
const TOL = 1e-6; // native and wasm agree to ~1e-12 in practice; same algorithm as the Python tool
const TOL_LOSS = 1e-6;

let checks = 0;
let failures = 0;
let worst = 0;

function close(what, got, want, tol = TOL) {
  checks += 1;
  const err = Math.abs(got - want) / Math.max(Math.abs(want), 1e-300);
  worst = Math.max(worst, err);
  if (!(err <= tol)) {
    failures += 1;
    console.error(`FAIL ${what}: got ${got}, want ${want} (rel. error ${err.toExponential(2)})`);
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
same('line types', types.map((t) => t.id).join(), LINE);
const specs = JSON.parse(param_specs(LINE));
same('form size', specs.length, Object.keys(JSON.parse(defaults(LINE))).length);

for (const c of cases.cases) {
  const a = analyze(LINE, JSON.stringify(toSet(c.params)), undefined);
  const s = JSON.parse(a.summary());
  const m = s.modes[0];
  close(`${c.name} z0`, m.z0, c.z0);
  close(`${c.name} eps_eff`, m.eps_eff, c.eeff);
  close(`${c.name} C`, m.c, c.c);
  same(`${c.name} nodes`, s.accuracy.nodes, c.nodes);
  close(`${c.name} alpha_c`, m.at_f.alpha_c, c.loss.alpha_c, TOL_LOSS);
  close(`${c.name} alpha_d`, m.at_f.alpha_d, c.loss.alpha_d, TOL_LOSS);
  // loss over frequency through the typed-array path
  const row = a.sample(new Float64Array([c.params.f]));
  close(`${c.name} sample alpha_c`, row[2], c.loss.alpha_c, TOL_LOSS);
  for (const r of m.split.rows) {
    if (r.key in c.split.field) {
      close(`${c.name} field share ${r.key}`, r.field_pct, c.split.field[r.key], TOL_LOSS);
      close(`${c.name} current share ${r.key}`, r.current_pct, c.split.current[r.key], TOL_LOSS);
    }
  }
  if (s.has_field) {
    const info = JSON.parse(a.field_info());
    same(`${c.name} potential size`, a.field_v().length, info.nx * info.ny);
  }
  a.free();
}

for (const c of cases.synthesis) {
  const a = synthesize(LINE, JSON.stringify(toSet(c.params)), c.var, c.target, undefined);
  const p = JSON.parse(a.params());
  close(`${c.name} w`, p.w, c.result.w);
  close(`${c.name} s`, p.s, c.result.s);
  close(`${c.name} z0`, JSON.parse(a.summary()).modes[0].z0, c.z0);
  a.free();
}

console.log(`${checks} checks, ${failures} failed, worst relative deviation ${worst.toExponential(2)}`);
process.exit(failures === 0 ? 0 : 1);
