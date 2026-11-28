"""Reference cases. They also serve as the parity targets for the Rust port.

All tolerances are relative.
"""

from dataclasses import replace

import pytest

from cpwg_solver import Params, analytic, ground_split, recede_walls, solve, synthesize


def rel(a, b):
    return abs(a / b - 1.0)


@pytest.mark.parametrize(("w", "s"), [(0.248, 0.15), (0.2, 0.1), (0.4, 0.2)])
def test_closed_t0_matches_conformal_mapping(w, s):
    p = Params(w=w, s=s, t=0.0, h1=0.25, h2=0.25, er1=3.66, er2=3.66)
    assert rel(solve(p).z0, analytic(p)[0]) < 1e-3


def test_cbcpw_air_above_matches_conformal_mapping():
    p = Params(
        w=0.3, s=0.1, t=0.0, h1=0.2, er1=1.0, h2=0.254, er2=3.66, g=2.0, plane1=False, fence1=False
    )
    assert rel(solve(p).z0, analytic(p)[0]) < 2.5e-3


def test_final_stackup_regression():
    sol = solve(Params())
    assert rel(sol.z0, 35.001) < 5e-4
    split = ground_split(sol)["current"]
    assert rel(split["coplanar"], 29.2) < 0.03


def test_open_variant_regression():
    sol = solve(replace(Params(), plane1=False, fence1=False))
    assert rel(sol.z0, 43.429) < 5e-4


def test_synthesis_relative_accuracy():
    p, sol = synthesize(Params(), 35.0, "w")
    assert rel(sol.z0, 35.0) < 5e-4
    assert rel(p.w, 0.3605) < 5e-4


def metal_surfaces(p, y0):
    """Positions of: signal side face, coplanar face at the gap, via fence, copper bottom, copper
    top, plane 1, plane 2, with y0 the height of the bottom of the signal layer."""
    gap_face = p.w / 2 + p.s
    return [p.w / 2, gap_face, gap_face + p.g, y0, y0 + p.t, y0 + p.t + p.h1, y0 - p.h2]


@pytest.mark.parametrize("t", [0.018, 0.035, 0.0])
def test_wheeler_moves_every_metal_surface_into_its_metal(t):
    d = 1e-3
    faces = 1 if t > 0 else 0  # a zero-thickness conductor has no top and bottom face to move
    p = replace(Params(), t=t)
    before = metal_surfaces(p, 0.0)
    # the bottom face of thick copper recedes upwards, and the new dimensions are measured from it
    after = metal_surfaces(recede_walls(p, d), faces * d)
    direction = [-1, 1, 1, faces, -faces, 1, -1]
    for old, new, sign in zip(before, after, direction, strict=True):
        assert new - old == pytest.approx(sign * d, abs=1e-12)
