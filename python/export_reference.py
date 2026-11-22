"""Export reference cases (parameters + results of the Python FD solver) as JSON.

The Rust port reads tests/reference/cases.json and must reproduce the results.
Usage: uv run export_reference.py > ../tests/reference/cases.json
"""

import json
import sys
from dataclasses import asdict, replace

from cpwg_solver import Params, analytic, solve

BASE = Params()
CASES = {
    "closed_t0_w0.248_s0.15": Params(w=0.248, s=0.15, t=0.0, h1=0.25, h2=0.25, er1=3.66, er2=3.66),
    "closed_t0_w0.2_s0.1": Params(w=0.2, s=0.1, t=0.0, h1=0.25, h2=0.25, er1=3.66, er2=3.66),
    "cbcpw_air_above": Params(
        w=0.3, s=0.1, t=0.0, h1=0.2, er1=1.0, h2=0.254, er2=3.66, g=2.0, plane1=False, fence1=False
    ),
    "final_stackup": BASE,
    "open_no_plane1_no_fence1": replace(BASE, plane1=False, fence1=False),
    "open_no_planes": replace(BASE, plane1=False, plane2=False, fence1=False, fence2=False),
    "no_fences_planes_kept": replace(BASE, fence1=False, fence2=False),
    "gap_custom_air": replace(BASE, gap_mode="custom", er_gap=1.0, tand_gap=0.0),
    "gap_lower": replace(BASE, gap_mode="lower"),
    "manual_mesh_richardson": replace(BASE, mesh_mode="manual", dmin=0.004, dmax=0.03),
    "manual_mesh_single_level": replace(
        BASE, mesh_mode="manual", dmin=0.004, dmax=0.03, richardson=False
    ),
    "tight_tolerance": replace(BASE, tol=0.1),
    "explicit_margin": replace(BASE, plane1=False, fence1=False, margin=3.0),
}


def main() -> None:
    out = []
    for name, p in CASES.items():
        sol = solve(p)
        entry = {
            "name": name,
            "params": asdict(p),
            "z0": sol.z0,
            "eeff": sol.eeff,
            "c": sol.C,
            "c0": sol.C0,
            "l": sol.L,
            "g_over_omega": sol.G_over_w,
            "nodes": sol.info["nodes"],
            "margin": sol.info["margin"],
            "est_err": sol.info.get("est_err"),
        }
        if p.t == 0 and p.closed():
            z0, eeff, _ = analytic(p)
            entry["analytic_z0"] = z0
            entry["analytic_eeff"] = eeff
        elif p.t == 0:
            z0, eeff, exact = analytic(p)
            if exact:
                entry["analytic_z0"] = z0
                entry["analytic_eeff"] = eeff
        out.append(entry)
        print(f"{name}: z0={sol.z0:.4f} nodes={sol.info['nodes']}", file=sys.stderr)
    json.dump({"cases": out}, sys.stdout, indent=1)
    print()


if __name__ == "__main__":
    main()
