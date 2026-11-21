"""Reference cases. They also serve as the parity targets for the Rust port.

All tolerances are relative.
"""

from dataclasses import replace

import pytest

from cpwg_solver import Params, analytic, ground_split, solve, synthesize


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
