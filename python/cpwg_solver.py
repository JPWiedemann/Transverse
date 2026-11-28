#!/usr/bin/env python3
"""
CPWG / stripline-type coplanar waveguide in a PCB inner layer
2D quasi-static finite-difference (finite-volume) field solver with GUI.

Cross-section (not to scale), y = 0 at the bottom of the signal layer:

    =============================================  ground plane 1 (optional), y = t + h1
          upper dielectric   h1, er1, tand1        | via fence 1 (optional)
    #######  s  [====== w ======]  s  ########      t   (signal + coplanar ground, width g)
          lower dielectric   h2, er2, tand2        | via fence 2 (optional)
    =============================================  ground plane 2 (optional), y = -h2

h1 / h2 are measured from the copper surface to the respective plane (or to
the board surface if the plane is omitted; air above / below).  Gaps between
signal and coplanar ground (height t) are filled with a selectable dielectric.
The coplanar ground extends to the edge of the computation domain; the via
fences are vertical walls at |x| = w/2 + s + g, from the copper layer to
plane 1 / plane 2.

Features
  * Analysis : Z0, eps_eff, delay, L', C', R', G', conductor / dielectric loss
  * Synthesis: solve for w (s fixed), s (w fixed) or w with 2s+w fixed
  * Split of electric flux and HF return current over ground plane 1, ground
    plane 2, coplanar ground (flank / top / bottom / outer edge), via fences
  * Open structures (missing planes / fences): the domain is extended
    automatically until Z0 changes less than a quarter of the target error
  * Mesh: automatic (refined until the estimated error < target) or manual
    cell sizes (length or fraction of the wavelength)
  * Cancel button and progress messages in the GUI; mesh overlay in the field tab
  * Inputs are remembered between runs in settings.json next to
    this script (or the .exe) (written on Analyze / Synthesize and when the window closes)

Method
  * Laplace equation div(eps grad V) = 0 on a non-uniform tensor grid whose
    lines coincide with every conductor edge; cells grow geometrically into
    open (air) regions so the node count stays small.
  * C  = eps0 * sum(coef * dV^2) (with dielectric), C0 the same in air
  * Z0 = 1 / (c * sqrt(C * C0)),  eps_eff = C / C0
  * Two mesh levels + Richardson extrapolation.
  * Dielectric loss: G = w*eps0*sum(eps*tand-weighted coef * dV^2), alpha_d = G*Z0/2
  * Conductor loss : Wheeler incremental-inductance rule,
                     alpha_c = Rs/(2*eta0*Z0) * dZ0_air/dn  (Hammerstad roughness)
  * Return current : surface charge of the air solution (J_s = I * rho_s / Q),
                     valid when the skin depth << t.

Requirements: Python 3.8+, numpy, scipy  (matplotlib for the plot tabs)
Usage:  python cpwg_solver.py            -> GUI
        python cpwg_solver.py --selftest -> validation against analytic formulas
"""

import contextlib
import itertools
import json
import math
import os
import queue
import sys
import threading
from dataclasses import dataclass, replace

import numpy as np
import scipy.sparse as sp
import scipy.sparse.linalg as sla
from scipy.optimize import brentq
from scipy.special import ellipk

EPS0 = 8.8541878128e-12
MU0 = 4e-7 * math.pi
C_LIGHT = 299792458.0
NP2DB = 20.0 / math.log(10.0)  # 8.686
ETA0 = MU0 * C_LIGHT
TOL_GEO = 1e-9  # geometric tolerance [mm]


def _app_dir():
    """Folder of the script, or of the .exe when frozen (PyInstaller / Nuitka)."""
    if getattr(sys, "frozen", False) or "__compiled__" in globals():
        return os.path.dirname(os.path.abspath(sys.executable))
    return os.path.dirname(os.path.abspath(__file__))


SETTINGS_FILE = os.path.join(_app_dir(), "settings.json")


# --------------------------------------------------------------------------
# Control (cancel / progress)
# --------------------------------------------------------------------------
class Cancelled(Exception):
    pass


class MeshTooLarge(Exception):
    pass


class Ctl:
    """Passed through all solver calls: cancel flag and progress callback."""

    def __init__(self, progress=None):
        self.event = threading.Event()
        self.progress_cb = progress
        self.n_solves = 0

    def cancel(self):
        self.event.set()

    def check(self):
        if self.event.is_set():
            raise Cancelled("Cancelled by user")

    def msg(self, text):
        if self.progress_cb:
            self.progress_cb(text)


# --------------------------------------------------------------------------
# Parameters (all lengths in mm, frequency in Hz)
# --------------------------------------------------------------------------
@dataclass
class Params:
    # geometry
    w: float = 0.3605
    s: float = 0.11
    t: float = 0.018
    g: float = 1.0  # distance gap edge -> via fence (coplanar ground continues beyond)
    # upper dielectric / plane 1 / fence 1
    h1: float = 0.204
    er1: float = 3.52
    tand1: float = 0.02
    plane1: bool = True
    fence1: bool = True
    # lower dielectric / plane 2 / fence 2
    h2: float = 0.254
    er2: float = 3.66
    tand2: float = 0.02
    plane2: bool = True
    fence2: bool = True
    # gap material
    gap_mode: str = "upper"  # "upper", "lower", "custom"
    er_gap: float = 3.52
    tand_gap: float = 0.02
    # conductor
    sigma: float = 5.8e7  # S/m
    rq: float = 0.0  # RMS roughness [mm]
    # frequency / length
    f: float = 1e9  # Hz
    length: float = 100.0  # mm
    # mesh
    mesh_mode: str = "auto"  # "auto" or "manual"
    tol: float = 0.5  # target relative error [%] (auto mesh, domain size)
    dmin: float = 0.002  # manual: smallest cell (at conductor edges) [mm]
    dmax: float = 0.02  # manual: largest cell in the core region [mm]
    richardson: bool = True  # manual: second level with half cell size + extrapolation
    max_nodes: int = 600000
    margin: float = 0.0  # domain extension for open structures [mm], 0 = auto

    def gap_props(self):
        if self.gap_mode == "upper":
            return self.er1, self.tand1
        if self.gap_mode == "lower":
            return self.er2, self.tand2
        return self.er_gap, self.tand_gap

    def closed(self):
        return self.plane1 and self.plane2 and self.fence1 and self.fence2

    def wavelength(self):
        """Wavelength [mm] in the densest dielectric at f."""
        er = max(self.er1, self.er2, self.gap_props()[0])
        return C_LIGHT / (self.f * math.sqrt(er)) * 1e3

    def validate(self):
        for name in ("w", "s", "h1", "h2", "g", "er1", "er2", "sigma", "f", "tol"):
            if getattr(self, name) <= 0:
                raise ValueError(f"{name} must be > 0")
        if self.t < 0:
            raise ValueError("t must be >= 0")
        if self.gap_props()[0] <= 0:
            raise ValueError("er_gap must be > 0")
        if self.mesh_mode == "manual" and (self.dmin <= 0 or self.dmax < self.dmin):
            raise ValueError("manual mesh: need 0 < min cell <= max cell")


# --------------------------------------------------------------------------
# Analytic estimate (conformal mapping, t = 0)
# --------------------------------------------------------------------------
def _kk(k):
    """K(k)/K(k')."""
    return ellipk(k * k) / ellipk(1.0 - k * k)


def analytic(p: Params):
    """Partial-capacitance conformal mapping, t = 0, infinite coplanar ground.
    A missing plane is treated as a half-space filled with that layer's er
    (exact only if that er = 1). Returns (Z0, eps_eff, exact_flag)."""
    k0 = p.w / (p.w + 2 * p.s)

    def q(h, plane):
        if not plane:
            return _kk(k0)
        k = math.tanh(math.pi * p.w / (4 * h)) / math.tanh(math.pi * (p.w + 2 * p.s) / (4 * h))
        return _kk(k)

    q1, q2 = q(p.h1, p.plane1), q(p.h2, p.plane2)
    eeff = (p.er1 * q1 + p.er2 * q2) / (q1 + q2)
    z0 = 60 * math.pi / (math.sqrt(eeff) * (q1 + q2))
    exact = (p.plane1 or p.er1 == 1) and (p.plane2 or p.er2 == 1)
    return z0, eeff, exact


# --------------------------------------------------------------------------
# Mesh
# --------------------------------------------------------------------------
def _graded(a, b, dmin, dmax, r=1.2):
    """Points from a to b, spacing dmin at both ends growing by r up to dmax."""
    L = b - a
    if L <= 0:
        return np.array([a])
    if 2 * dmin >= L:
        n = max(1, math.ceil(L / dmin))
        return np.linspace(a, b, n + 1)
    steps, st, tot = [], dmin, 0.0
    while tot + st < L / 2:
        steps.append(st)
        tot += st
        st = min(st * r, dmax)
    cs = np.cumsum([0.0, *steps])
    left = a + cs
    right = b - cs
    mid_gap = right[-1] - left[-1]
    pts = list(left) + list(right[::-1])
    if mid_gap > dmax:
        n = math.ceil(mid_gap / dmax)
        pts += list(np.linspace(left[-1], right[-1], n + 1)[1:-1])
    pts = np.unique(np.array(pts))
    keep = [pts[0]]
    for x in pts[1:-1]:
        if x - keep[-1] > 0.3 * dmin:
            keep.append(x)
    if pts[-1] - keep[-1] < 0.3 * dmin and len(keep) > 1:
        keep.pop()
    keep.append(pts[-1])
    return np.array(keep)


def _graded_out(a, b, d0, dcap, r=1.25):
    """Points from a towards b (either direction), first step d0 growing by r up to dcap."""
    sgn = 1.0 if b > a else -1.0
    L = abs(b - a)
    pts, pos, st = [a], 0.0, d0
    while pos + st < L:
        pos += st
        pts.append(a + sgn * pos)
        st = min(st * r, dcap)
    if L - pos < 0.3 * st and len(pts) > 1:
        pts.pop()
    pts.append(b)
    return np.array(pts)


def _axis(breaks, dmin, dmax):
    breaks = np.unique(np.round(np.array(breaks, float), 12))
    out = [np.array([breaks[0]])]
    for a, b in itertools.pairwise(breaks):
        out.append(_graded(a, b, dmin, dmax)[1:])
    return np.concatenate(out)


def base_sizes(p: Params):
    """Default cell sizes (auto mesh, level 1)."""
    hmin = min(p.h1, p.h2)
    feat = [p.s / 30, p.w / 60, hmin / 60]
    if p.t > 0:
        feat.append(p.t / 6)
    dmin = max(min(feat), 1e-3 * min(p.w, p.s, hmin))  # relative floor
    dmax = max(min(hmin / 20, (p.w + 2 * p.s) / 20), 2 * dmin)
    return dmin, dmax


def geometry(p: Params, M):
    """Domain extents [mm]. M = extension for open regions."""
    xe = p.w / 2 + p.s + p.g
    xd = xe if p.closed() else xe + M
    ytop = p.t + p.h1
    ybot = -p.h2
    ymax = ytop if p.plane1 else ytop + M
    ymin = ybot if p.plane2 else ybot - M
    return {"xe": xe, "xd": xd, "ytop": ytop, "ybot": ybot, "ymin": ymin, "ymax": ymax, "M": M}


def build_mesh(p: Params, dmin, dmax, M):
    G = geometry(p, M)
    xe, xd = G["xe"], G["xd"]
    far = max(M / 6, dmax)
    X = _axis([-xe, -(p.w / 2 + p.s), -p.w / 2, p.w / 2, p.w / 2 + p.s, xe], dmin, dmax)
    if xd > xe + TOL_GEO:
        right = _graded_out(xe, xd, dmin, far)
        X = np.concatenate([(-right[::-1])[:-1], X, right[1:]])
    ybr = [G["ybot"], 0.0, p.t, G["ytop"]] if p.t > 0 else [G["ybot"], 0.0, G["ytop"]]
    Y = _axis(ybr, dmin, dmax)
    if not p.plane2:
        below = _graded_out(G["ybot"], G["ymin"], dmin, far)
        Y = np.concatenate([below[::-1][:-1], Y])
    if not p.plane1:
        above = _graded_out(G["ytop"], G["ymax"], dmin, far)
        Y = np.concatenate([Y, above[1:]])
    return X, Y, G


# --------------------------------------------------------------------------
# Field solver
# --------------------------------------------------------------------------
# node categories
C_FREE, C_SIG, C_PL1, C_PL2, C_COP, C_F1, C_F2, C_BND = range(8)


class Solution:
    """Result of a quasi-static solve. Frequency-dependent values via methods."""

    cat: np.ndarray  # node categories (C_*)
    fixed: np.ndarray  # conductor nodes
    er_cell: np.ndarray  # permittivity per cell
    edges: tuple[np.ndarray, np.ndarray, np.ndarray, np.ndarray]
    _dzdn: float | None
    _gs: dict | None

    def __init__(self, p, X, Y, V, V0, C, C0, G_over_w, info, geom):
        self.p = p
        self.X, self.Y, self.V, self.V0 = X, Y, V, V0
        self.C, self.C0 = C, C0
        self.G_over_w = G_over_w  # G / omega  [F/m]
        self.info = info
        self.geom = geom
        self.eeff = C / C0
        self.z0 = 1.0 / (C_LIGHT * math.sqrt(C * C0))
        self.L = 1.0 / (C_LIGHT**2 * C0)
        self._dzdn = None
        self._gs = None

    def skin_depth(self, f):
        return 1.0 / math.sqrt(math.pi * f * MU0 * self.p.sigma)

    def rs(self, f):
        rs = math.sqrt(math.pi * f * MU0 / self.p.sigma)
        d = self.skin_depth(f)
        rq = self.p.rq * 1e-3
        k = 1.0 + 2.0 / math.pi * math.atan(1.4 * (rq / d) ** 2) if rq > 0 else 1.0
        return rs * k, k

    def dzdn(self, ctl: Ctl | None = None) -> float:
        if self._dzdn is None:
            self._dzdn = wheeler_dzdn(self, ctl)
        return self._dzdn

    def losses(self, f):
        w = 2 * math.pi * f
        rs, krough = self.rs(f)
        alpha_c = rs / (2.0 * ETA0 * self.z0) * self.dzdn()  # Np/m
        G = w * self.G_over_w  # S/m
        alpha_d = G * self.z0 / 2.0  # Np/m
        R = 2.0 * self.z0 * alpha_c  # Ohm/m
        return {
            "alpha_c": alpha_c,
            "alpha_d": alpha_d,
            "R": R,
            "G": G,
            "rs": rs,
            "krough": krough,
            "delta": self.skin_depth(f),
        }


def _solve_level(p: Params, dmin, dmax, M, ctl: Ctl) -> Solution:
    ctl.check()
    X, Y, G = build_mesh(p, dmin, dmax, M)
    nx, ny = len(X), len(Y)
    N = nx * ny
    if p.max_nodes < N:
        raise MeshTooLarge(
            f"Mesh would have {N} nodes (> max {p.max_nodes}). "
            f"Increase cell sizes / target error or max nodes."
        )
    dx, dy = np.diff(X), np.diff(Y)
    tol = TOL_GEO

    # --- cell properties (air by default)
    yc = 0.5 * (Y[:-1] + Y[1:])
    er_g, td_g = p.gap_props()
    er_row = np.ones(ny - 1)
    td_row = np.zeros(ny - 1)
    lower = (yc > G["ybot"]) & (yc < 0)
    gapr = (yc > 0) & (yc < p.t)
    upper = (yc > p.t) & (yc < G["ytop"])
    er_row[lower], td_row[lower] = p.er2, p.tand2
    er_row[gapr], td_row[gapr] = er_g, td_g
    er_row[upper], td_row[upper] = p.er1, p.tand1
    er_cell = np.repeat(er_row[:, None], nx - 1, axis=1)
    td_cell = np.repeat(td_row[:, None], nx - 1, axis=1)

    # --- node categories
    xx, yy = np.meshgrid(X, Y)
    ax_ = np.abs(xx)
    xe = G["xe"]
    cat = np.zeros((ny, nx), np.int8)
    cat[0, :] = cat[-1, :] = C_BND
    cat[:, 0] = cat[:, -1] = C_BND
    on_fence = np.abs(ax_ - xe) <= tol
    if p.fence1:
        cat[on_fence & (yy >= p.t - tol) & (yy <= G["ytop"] + tol)] = C_F1
    if p.fence2:
        cat[on_fence & (yy >= G["ybot"] - tol) & (yy <= tol)] = C_F2
    if p.plane1:
        cat[np.abs(yy - G["ytop"]) <= tol] = C_PL1
    if p.plane2:
        cat[np.abs(yy - G["ybot"]) <= tol] = C_PL2
    band = (yy >= -tol) & (yy <= p.t + tol)
    cat[band & (ax_ >= p.w / 2 + p.s - tol)] = C_COP  # coplanar ground up to the grid edge
    cat[band & (ax_ <= p.w / 2 + tol)] = C_SIG

    fixed = cat > 0
    vfix = (cat == C_SIG).astype(float)
    idx = np.arange(N).reshape(ny, nx)

    def coef_vec(cell):
        E = cell * dy[:, None] / 2
        hc = np.zeros((ny, nx - 1))
        hc[1:] += E
        hc[:-1] += E
        hc /= dx[None, :]
        F = cell * dx[None, :] / 2
        vc = np.zeros((ny - 1, nx))
        vc[:, 1:] += F
        vc[:, :-1] += F
        vc /= dy[:, None]
        return np.concatenate([hc.ravel(), vc.ravel()])

    ea = np.concatenate([idx[:, :-1].ravel(), idx[:-1, :].ravel()])
    eb = np.concatenate([idx[:, 1:].ravel(), idx[1:, :].ravel()])
    fflat = fixed.ravel()
    free = ~fflat

    def laplace(coef):
        ctl.check()
        ctl.n_solves += 1
        A = sp.coo_matrix(
            (
                np.concatenate([coef, coef, -coef, -coef]),
                (np.concatenate([ea, eb, ea, eb]), np.concatenate([ea, eb, eb, ea])),
            ),
            shape=(N, N),
        ).tocsr()
        x = vfix.ravel().copy()
        Aff = A[free][:, free]
        rhs = -(A[free][:, fflat] @ x[fflat])
        x[free] = sla.spsolve(Aff.tocsc(), rhs)
        dV = x[ea] - x[eb]
        return x, dV, float(np.sum(coef * dV * dV))

    coef_d = coef_vec(er_cell)
    coef_0 = coef_vec(np.ones((ny - 1, nx - 1)))
    coef_t = coef_vec(er_cell * td_cell)

    x, dV, W = laplace(coef_d)
    x0, _dV0, W0 = laplace(coef_0)

    C = EPS0 * W
    C0 = EPS0 * W0
    G_over_w = EPS0 * float(np.sum(coef_t * dV * dV))
    info = {
        "nodes": N,
        "nx": nx,
        "ny": ny,
        "dmin": float(min(dx.min(), dy.min())),
        "dmin_set": dmin,
        "dmax_set": dmax,
    }
    sol = Solution(p, X, Y, x.reshape(ny, nx), x0.reshape(ny, nx), C, C0, G_over_w, info, G)
    sol.cat = cat
    sol.fixed = fixed
    sol.er_cell = er_cell
    sol.edges = (ea, eb, coef_d, coef_0)
    return sol


def _combine(a: Solution, b: Solution) -> Solution:
    """Richardson extrapolation (1st order) of two levels, fields from b."""
    ex = lambda u, v: 2 * v - u
    sol = Solution(
        b.p,
        b.X,
        b.Y,
        b.V,
        b.V0,
        ex(a.C, b.C),
        ex(a.C0, b.C0),
        ex(a.G_over_w, b.G_over_w),
        dict(b.info),
        b.geom,
    )
    for k in ("cat", "fixed", "er_cell", "edges"):
        setattr(sol, k, getattr(b, k))
    sol.info["levels"] = (a.z0, b.z0)
    sol.info["est_err"] = abs(b.z0 - a.z0) / b.z0  # error of the finer level
    return sol


def auto_margin(p: Params, ctl: Ctl):
    """Domain extension for open structures: doubled until Z0 changes < tol/4."""
    if p.closed():
        return 0.0
    dmin, dmax = base_sizes(p)
    dmin, dmax = 2 * dmin, 2 * dmax  # coarse mesh is enough for this check
    M = max(4 * max(p.h1, p.h2), p.w + 2 * p.s)
    if not (p.plane1 and p.plane2):
        M = max(M, p.w / 2 + p.s + p.g)
    z_prev = _solve_level(p, dmin, dmax, M, ctl).z0
    for _ in range(8):
        M2 = 2 * M
        ctl.msg(f"Domain size check: margin {M2:.3g} mm ...")
        z = _solve_level(p, dmin, dmax, M2, ctl).z0
        if abs(z - z_prev) / z < p.tol / 100 / 4:
            return M2
        M, z_prev = M2, z
    ctl.msg("Domain size: not fully converged, using largest margin")
    return M


def solve(p: Params, ctl: Ctl | None = None) -> Solution:
    """Analysis of one geometry (auto or manual mesh, Richardson extrapolation)."""
    p.validate()
    ctl = ctl or Ctl()
    M = p.margin if p.margin > 0 else auto_margin(p, ctl)
    if p.mesh_mode == "manual":
        ctl.msg(f"Solving (manual mesh, min cell {p.dmin * 1e3:.3g} um) ...")
        a = _solve_level(p, p.dmin, p.dmax, M, ctl)
        if not p.richardson:
            a.info["est_err"] = None
            a.info["mesh_used"] = (p.dmin, p.dmax, False)
            a.info["margin"] = M
            return a
        b = _solve_level(p, p.dmin / 2, p.dmax / 2, M, ctl)
        sol = _combine(a, b)
        sol.info["mesh_used"] = (p.dmin, p.dmax, True)
    else:
        d0, d1 = base_sizes(p)
        q = 1.0
        ctl.msg("Solving mesh level 1 ...")
        a = _solve_level(p, d0 / q, d1 / q, M, ctl)
        ctl.msg(f"Solving mesh level 2 ({a.info['nodes']} nodes on level 1) ...")
        b = _solve_level(p, d0 / (2 * q), d1 / (2 * q), M, ctl)
        lvl = 2
        while abs(b.z0 - a.z0) / b.z0 > p.tol / 100:
            q *= 2
            lvl += 1
            ctl.msg(
                f"Refining: mesh level {lvl} (est. error "
                f"{abs(b.z0 - a.z0) / b.z0 * 100:.2f} % > {p.tol:g} %) ..."
            )
            try:
                c = _solve_level(p, d0 / (2 * q), d1 / (2 * q), M, ctl)
            except MeshTooLarge:
                ctl.msg("Node limit reached - using current level")
                break
            a, b = b, c
        sol = _combine(a, b)
        sol.info["mesh_used"] = (d0 / q, d1 / q, True)
    sol.info["margin"] = M
    return sol


def recede_walls(p: Params, d: float) -> Params:
    """The structure with every metal surface moved by d into its metal (d < 0: out of it), as
    new dimensions. The heights h1/h2 are measured from the copper surfaces, which move as well, so
    a plane that recedes gets 2d further from a thick copper layer (d from a zero-thickness one,
    whose faces cannot move). The via fences keep their distance g: they recede with the
    coplanar-ground edge."""
    thick = p.t > 0
    plane = 2 * d if thick else d
    return replace(
        p,
        w=p.w - 2 * d,
        s=p.s + 2 * d,
        t=max(p.t - 2 * d, 0.0) if thick else 0.0,
        h1=p.h1 + plane if p.plane1 else p.h1,
        h2=p.h2 + plane if p.plane2 else p.h2,
    )


def wheeler_dzdn(sol: Solution, ctl: Ctl | None = None) -> float:
    """dZ0_air/dn [Ohm/m]: all metal walls receded by +dn / advanced by -dn,
    same mesh settings and domain as the given solution."""
    p = sol.p
    ctl = ctl or Ctl()
    dn = min(p.s / 20, p.w / 20)  # relative to the geometry
    if p.t > 0:
        dn = min(dn, p.t / 8)
    dmin, dmax, rich = sol.info["mesh_used"]
    pa = replace(
        p,
        er1=1.0,
        er2=1.0,
        gap_mode="custom",
        er_gap=1.0,
        mesh_mode="manual",
        dmin=dmin,
        dmax=dmax,
        richardson=rich,
        margin=sol.info["margin"] if sol.info["margin"] > 0 else 1.0,
    )

    def z(d):
        ctl.msg(f"Conductor loss (Wheeler): wall shift {d * 1e3:+.2f} um ...")
        return solve(recede_walls(pa, d), ctl).z0

    return (z(dn) - z(-dn)) / (2 * dn * 1e-3)


# --------------------------------------------------------------------------
# Synthesis
# --------------------------------------------------------------------------
def synthesize(p: Params, target, var, ctl: Ctl | None = None, rtol=1e-4):
    """var: 'w' (s fixed), 's' (w fixed), 'w_total' (2s+w fixed). Returns (Params, Solution).

    rtol: relative stopping tolerance of the varied dimension (default 0.01 %)."""
    ctl = ctl or Ctl()
    p.validate()
    if p.margin <= 0 and not p.closed():
        ctl.msg("Synthesis: determining domain size ...")
        p = replace(p, margin=auto_margin(p, ctl))
    total = 2 * p.s + p.w

    def mk(x):
        if var == "w":
            return replace(p, w=x)
        if var == "s":
            return replace(p, s=x)
        if var == "w_total":
            return replace(p, w=x, s=(total - x) / 2)
        raise ValueError(var)

    cache = {}
    it = [0]

    def f(x):
        if x not in cache:
            it[0] += 1
            sol = solve(mk(x), ctl)
            cache[x] = sol
            ctl.msg(f"Synthesis step {it[0]}: {var} = {x * 1e3:.2f} um -> Z0 = {sol.z0:.3f} Ohm")
        return cache[x].z0 - target

    x0 = {"w": p.w, "s": p.s, "w_total": p.w}[var]
    scale = p.w + 2 * p.s  # all limits relative to the geometry
    if var == "w_total":
        lo_lim, hi_lim = 1e-3 * total, total * (1 - 2e-3)
    elif var == "w":
        lo_lim, hi_lim = 1e-3 * scale, 20 * max(p.h1, p.h2) + 10 * p.w
    else:
        lo_lim, hi_lim = 1e-3 * scale, 20 * max(p.h1, p.h2) + 10 * p.s
    x0 = min(max(x0, lo_lim * 2), hi_lim / 2)

    f0 = f(x0)
    if abs(f0) < 1e-6 * target:
        return mk(x0), cache[x0]
    increasing = var == "s"  # Z0 grows with s, falls with w
    go_up = (f0 < 0) == increasing
    a, fa = x0, f0
    for _ in range(40):
        b = a * 1.25 if go_up else a / 1.25
        b = min(max(b, lo_lim), hi_lim)
        fb = f(b)
        if fa * fb <= 0:
            break
        if b in (lo_lim, hi_lim):
            raise ValueError(
                f"Target {target} Ohm not reachable by varying {var} "
                f"(range {lo_lim * 1e3:.0f}..{hi_lim * 1e3:.0f} um, "
                f"Z0 there = {fb + target:.2f} Ohm)."
            )
        a, fa = b, fb
    else:
        raise ValueError("No bracket found.")
    lo, hi = sorted((a, b))
    x = brentq(f, lo, hi, xtol=1e-12 * scale, rtol=np.float64(rtol))
    pp = mk(x)
    return pp, (cache[x] if x in cache else solve(pp, ctl))


# --------------------------------------------------------------------------
# Ground split (field / return current)
# --------------------------------------------------------------------------
GROUND_CATS = (
    "plane1",
    "plane2",
    "cop_flank",
    "cop_top",
    "cop_bottom",
    "cop_outer",
    "fence1",
    "fence2",
    "boundary",
)
COP_CATS = ("cop_flank", "cop_top", "cop_bottom", "cop_outer")
GROUND_LABELS = {
    "plane1": "Ground plane 1 (top)",
    "plane2": "Ground plane 2 (bottom)",
    "cop_flank": "gap flank",
    "cop_top": "top face",
    "cop_bottom": "bottom face",
    "cop_outer": "outer edge",
    "fence1": "Via fence 1 (to top)",
    "fence2": "Via fence 2 (to bottom)",
    "boundary": "Domain boundary (far field)",
}


def ground_split(sol: Solution):
    """Share of the electric flux (solution with dielectrics: where the field
    lines end) and of the HF return current (air solution: J_s ~ rho_s) per
    ground conductor. Returns dict with 'field', 'current' (category -> %),
    'x90_planes', 'x90_cop' [mm] and 'profiles' {cat: (x, J_s/I [1/mm])}."""
    if sol._gs is not None:
        return sol._gs
    p = sol.p
    ea, eb, coef_d, coef_0 = sol.edges
    ny, nx = sol.V.shape
    X, Y = sol.X, sol.Y
    catn = sol.cat.ravel()
    fixed = sol.fixed.ravel()
    jj, ii = np.divmod(np.arange(nx * ny), nx)

    m = fixed[ea] ^ fixed[eb]
    a, b = ea[m], eb[m]
    cond = np.where(fixed[a], a, b)
    free = np.where(fixed[a], b, a)
    cj, ci, fj, fi = jj[cond], ii[cond], jj[free], ii[free]
    cc = catn[cond]
    gnd = cc != C_SIG

    lab = np.full(cond.shape, "", dtype=object)
    lab[cc == C_PL1] = "plane1"
    lab[cc == C_PL2] = "plane2"
    lab[cc == C_F1] = "fence1"
    lab[cc == C_F2] = "fence2"
    lab[cc == C_BND] = "boundary"
    cop = cc == C_COP
    lab[cop & (fj > cj)] = "cop_top"
    lab[cop & (fj < cj)] = "cop_bottom"
    same = cop & (fj == cj)
    inner = np.abs(X[fi]) < np.abs(X[ci])
    lab[same & inner] = "cop_flank"
    lab[same & ~inner] = "cop_outer"

    dxn = np.zeros(nx)
    dxn[1:] += np.diff(X) / 2
    dxn[:-1] += np.diff(X) / 2
    dyn = np.zeros(ny)
    dyn[1:] += np.diff(Y) / 2
    dyn[:-1] += np.diff(Y) / 2
    dual = np.where(fi == ci, dxn[ci], dyn[cj])

    out = {}
    for key, V, coef in (
        ("field", sol.V.ravel(), coef_d[m]),
        ("current", sol.V0.ravel(), coef_0[m]),
    ):
        flux = coef * (V[free] - V[cond])
        tot = flux[gnd].sum()
        d = {c: 100.0 * flux[gnd & (lab == c)].sum() / tot for c in GROUND_CATS}
        d["coplanar"] = sum(d[c] for c in COP_CATS)
        out[key] = d
        if key == "current":
            pl = gnd & np.isin(lab, ["plane1", "plane2"])
            out["x90_planes"] = None
            if pl.any():
                xs, f_ = np.abs(X[ci[pl]]), flux[pl]
                o = np.argsort(xs)
                k = min(np.searchsorted(np.cumsum(f_[o]) / f_.sum(), 0.9), len(xs) - 1)
                out["x90_planes"] = float(xs[o][k])
            cp = gnd & np.isin(lab, list(COP_CATS))
            xs, f_ = np.abs(X[ci[cp]]), flux[cp]
            o = np.argsort(xs)
            k = min(np.searchsorted(np.cumsum(f_[o]) / f_.sum(), 0.9), len(xs) - 1)
            out["x90_cop"] = float(xs[o][k] - (p.w / 2 + p.s))
            prof = {}
            for c in ("plane1", "plane2", "cop_top", "cop_bottom"):
                sel = gnd & (lab == c)
                if not sel.any():
                    continue
                xs = X[ci[sel]]
                js = flux[sel] / dual[sel] / tot
                ux, inv = np.unique(xs, return_inverse=True)
                prof[c] = (ux, np.bincount(inv, weights=js))
            out["profiles"] = prof
    sol._gs = out
    return out


# --------------------------------------------------------------------------
# Field post-processing
# --------------------------------------------------------------------------
def e_field(sol: Solution, air=False):
    """Ex, Ey, |E| in V/mm for 1 V on the signal trace (NaN inside conductors)."""
    V = sol.V0 if air else sol.V
    Ey, Ex = np.gradient(V, sol.Y, sol.X)
    Ex, Ey = -Ex, -Ey
    E = np.hypot(Ex, Ey)
    for a in (Ex, Ey, E):
        a[sol.fixed] = np.nan
    return Ex, Ey, E


# --------------------------------------------------------------------------
# Report text
# --------------------------------------------------------------------------
def report(sol: Solution, title="Analysis", ctl: Ctl | None = None):
    p = sol.p
    sol.dzdn(ctl)
    lo = sol.losses(p.f)
    to_db_mm = NP2DB / 1000.0
    ac, ad = lo["alpha_c"] * to_db_mm, lo["alpha_d"] * to_db_mm
    tot = ac + ad
    vp = C_LIGHT / math.sqrt(sol.eeff)
    struct = []
    struct.append("plane 1" if p.plane1 else "NO plane 1")
    struct.append("plane 2" if p.plane2 else "NO plane 2")
    struct.append("fence 1" if p.fence1 else "no fence 1")
    struct.append("fence 2" if p.fence2 else "no fence 2")
    lines = [
        f"=== {title} ===",
        f"Geometry : w = {p.w * 1e3:.2f} um, s = {p.s * 1e3:.2f} um, t = {p.t * 1e3:.2f} um, "
        f"g = {p.g:.3f} mm",
        f"           2s + w = {(2 * p.s + p.w) * 1e3:.2f} um   [{', '.join(struct)}]",
        "",
        f"Z0              = {sol.z0:8.3f} Ohm",
        f"eps_eff         = {sol.eeff:8.4f}",
        f"v_p             = {vp / C_LIGHT:8.4f} c",
        f"Delay           = {1e12 / vp * 1e-3:8.3f} ps/mm   ({1e12 / vp * 0.0254:.2f} ps/in)",
        f"L'              = {sol.L * 1e6:8.4f} nH/mm",
        f"C'              = {sol.C * 1e9:8.4f} pF/mm",
        "",
        f"@ f = {p.f / 1e9:g} GHz",
        f"Skin depth      = {lo['delta'] * 1e6:8.3f} um"
        + (
            "   (!) t < 3*delta: Rs model inaccurate"
            if p.t > 0 and p.t * 1e-3 < 3 * lo["delta"]
            else ""
        )
        + ("   (!) t = 0: conductor loss only indicative" if p.t == 0 else ""),
        f"Rs              = {lo['rs'] * 1e3:8.3f} mOhm/sq  (roughness factor {lo['krough']:.3f})",
        f"R'              = {lo['R'] * 1e-3:8.4f} Ohm/mm",
        f"G'              = {lo['G'] * 1e3 * 1e-3:8.5f} mS/mm",
        f"Conductor loss  = {ac:8.5f} dB/mm  ({ac * 25.4:.4f} dB/in)",
        f"Dielectric loss = {ad:8.5f} dB/mm  ({ad * 25.4:.4f} dB/in)",
        f"Total loss      = {tot:8.5f} dB/mm  ({tot * 25.4:.4f} dB/in)",
        f"Length {p.length:g} mm : {tot * p.length:.3f} dB, "
        f"delay {1e12 / vp * p.length * 1e-3:.1f} ps",
    ]
    gs = ground_split(sol)
    fdl, cur = gs["field"], gs["current"]
    lines += ["", "Ground split                     E-field   Return current (HF)"]
    rows = [("plane1", p.plane1), ("plane2", p.plane2)]
    for c, on in rows:
        if on:
            lines.append(f"  {GROUND_LABELS[c]:31s}{fdl[c]:6.1f} %   {cur[c]:6.1f} %")
    lines.append(
        f"  {'Coplanar ground (total)':31s}{fdl['coplanar']:6.1f} %   {cur['coplanar']:6.1f} %"
    )
    for c in COP_CATS:
        if c == "cop_outer" and fdl[c] < 0.05 and cur[c] < 0.05:
            continue
        lines.append(f"    {GROUND_LABELS[c]:29s}{fdl[c]:6.1f} %   {cur[c]:6.1f} %")
    for c, on in (("fence1", p.fence1), ("fence2", p.fence2), ("boundary", not p.closed())):
        if on:
            lines.append(f"  {GROUND_LABELS[c]:31s}{fdl[c]:6.1f} %   {cur[c]:6.1f} %")
    if gs["x90_planes"] is not None:
        lines.append(f"  90 % of plane current within |x| <= {gs['x90_planes']:.3f} mm")
    lines.append(f"  90 % of coplanar current within {gs['x90_cop']:.3f} mm of the gap edge")

    za, ea_, exact = analytic(p)
    lines += [
        "",
        f"Analytic (t=0)  : Z0 = {za:.2f} Ohm, eps_eff = {ea_:.3f}"
        + ("" if exact else "  (rough: open side treated as filled half-space)"),
    ]
    lines.append(
        f"Mesh            : {sol.info['nx']} x {sol.info['ny']} = {sol.info['nodes']} nodes, "
        f"min. cell {sol.info['dmin'] * 1e3:.2f} um"
    )
    if sol.info.get("est_err") is not None:
        za_, zb_ = sol.info["levels"]
        lines.append(
            f"Accuracy        : levels {za_:.3f} / {zb_:.3f} Ohm -> extrapolated "
            f"{sol.z0:.3f} Ohm (fine-level error {sol.info['est_err'] * 100:.2f} %, "
            f"extrapolated value better)"
        )
    if sol.info.get("margin", 0) > 0:
        lines.append(
            f"Domain          : open structure, extension {sol.info['margin']:.3g} mm "
            f"(converged to < {p.tol / 4:g} %)"
        )
    return "\n".join(lines)


def selftest():
    print("Self-test 1: closed structure, t = 0 -> exact conformal mapping")
    for w, s in ((0.248, 0.15), (0.2, 0.1), (0.4, 0.2)):
        p = Params(w=w, s=s, t=0.0, h1=0.25, h2=0.25, er1=3.66, er2=3.66)
        sol = solve(p)
        za = analytic(p)[0]
        print(
            f"  w={w} s={s}:  FD {sol.z0:.3f}  analytic {za:.3f}  "
            f"dev {100 * (sol.z0 / za - 1):+.2f} %   nodes {sol.info['nodes']}"
        )
    print("\nSelf-test 2: conductor-backed CPW, no plane 1, air above (er1 = 1), t = 0")
    p = Params(
        w=0.3, s=0.1, t=0.0, h1=0.2, er1=1.0, h2=0.254, er2=3.66, g=2.0, plane1=False, fence1=False
    )
    sol = solve(p)
    za = analytic(p)[0]
    print(
        f"  FD {sol.z0:.3f}  analytic {za:.3f}  dev {100 * (sol.z0 / za - 1):+.2f} %   "
        f"margin {sol.info['margin']:.3g} mm, nodes {sol.info['nodes']}"
    )
    print("\nDefault (final stack-up):")
    print(report(solve(Params())))


# --------------------------------------------------------------------------
# Plotting (matplotlib, also usable without GUI)
# --------------------------------------------------------------------------
CU = "#c87533"


def _view(sol, zoom):
    """x / y limits of the displayed region."""
    p, G = sol.p, sol.geom
    hmax = max(p.h1, p.h2)
    if zoom:
        xr = min(p.w / 2 + p.s + 1.2 * (p.w / 2 + p.s), G["xd"])
        y0 = G["ybot"] - (0.5 * hmax if not p.plane2 else hmax / 30)
        y1 = G["ytop"] + (0.5 * hmax if not p.plane1 else hmax / 30)
        return (-xr, xr), (y0, y1)
    pad = hmax / 30
    return (-G["xd"], G["xd"]), (G["ymin"] - pad, G["ymax"] + pad)


def draw_conductors(ax, sol):
    from matplotlib.patches import Rectangle

    p, G = sol.p, sol.geom
    xe, xd = G["xe"], G["xd"]
    th = max(p.t, (p.h1 + p.h2) / 300)  # visible even for t = 0
    y0c = 0.0 if p.t > 0 else -th / 2
    kw = {"fc": CU, "ec": "k", "lw": 0.5, "zorder": 5}
    ax.add_patch(Rectangle((-p.w / 2, y0c), p.w, th, **kw))
    ax.add_patch(Rectangle((-xd, y0c), xd - p.w / 2 - p.s, th, **kw))
    ax.add_patch(Rectangle((p.w / 2 + p.s, y0c), xd - p.w / 2 - p.s, th, **kw))
    pl = (p.h1 + p.h2) / 60
    if p.plane1:
        ax.add_patch(Rectangle((-xd, G["ytop"]), 2 * xd, pl, **kw))
    else:
        ax.axhline(G["ytop"], color="w", lw=0.8, ls="--", zorder=4)
    if p.plane2:
        ax.add_patch(Rectangle((-xd, G["ybot"] - pl), 2 * xd, pl, **kw))
    else:
        ax.axhline(G["ybot"], color="w", lw=0.8, ls="--", zorder=4)
    fw = (p.h1 + p.h2) / 150
    for sx in (-1, 1):
        if p.fence1:
            ax.add_patch(Rectangle((sx * xe - fw / 2, p.t), fw, p.h1, **kw))
        if p.fence2:
            ax.add_patch(Rectangle((sx * xe - fw / 2, G["ybot"]), fw, p.h2, **kw))
    ax.axhline(0.0, color="w", lw=0.3, ls=":", zorder=4)
    ax.axhline(p.t, color="w", lw=0.3, ls=":", zorder=4)


FIELD_MODES = [
    "|E| [dB] + field lines",
    "|E| [dB]",
    "|E| (linear)",
    "Potential",
    "Permittivity er",
    "Mesh",
]


def plot_field(
    fig, sol, mode=FIELD_MODES[0], show_eq=True, zoom=True, db_range=40.0, show_mesh=False
):
    """Field view of a solution (1 V on the signal) into a matplotlib Figure."""
    from mpl_toolkits.axes_grid1 import make_axes_locatable

    fig.clear()
    ax = fig.add_subplot(111)

    def cbar(im, label):
        cax = make_axes_locatable(ax).append_axes("right", size="2.5%", pad=0.1)
        fig.colorbar(im, cax=cax, label=label)

    X, Y, V = sol.X, sol.Y, sol.V
    Xg, Yg = np.meshgrid(X, Y)
    Ex, Ey, E = e_field(sol)
    (xa, xb), (ya, yb) = _view(sol, zoom)
    inview = (Xg >= xa) & (Xg <= xb) & (Yg >= ya) & (Yg <= yb) & np.isfinite(E)
    Ev = E[inview] if inview.any() else E[np.isfinite(E)]
    hi = np.percentile(Ev, 99.7)
    if mode == "Mesh":
        show_mesh = True
        ax.pcolormesh(
            X, Y, sol.er_cell, shading="flat", cmap="Greys", vmin=0, vmax=3 * sol.er_cell.max()
        )
    elif mode == "Potential":
        im = ax.pcolormesh(Xg, Yg, V, shading="gouraud", cmap="viridis")
        cbar(im, "V [V]")
    elif mode == "Permittivity er":
        im = ax.pcolormesh(X, Y, sol.er_cell, shading="flat", cmap="viridis")
        cbar(im, "er")
    elif mode == "|E| (linear)":
        im = ax.pcolormesh(
            Xg,
            Yg,
            np.ma.masked_invalid(np.clip(E, 0, hi)),
            shading="gouraud",
            cmap="inferno",
            vmin=0,
            vmax=hi,
        )
        cbar(im, "|E| [V/mm]")
    else:
        top = 20 * np.log10(hi)
        Edb = 20 * np.log10(np.maximum(E, 1e-30))
        im = ax.pcolormesh(
            Xg,
            Yg,
            np.ma.masked_invalid(np.clip(Edb, top - db_range, top)),
            shading="gouraud",
            cmap="inferno",
            vmin=top - db_range,
            vmax=top,
        )
        cbar(im, "|E| [dB re 1 V/mm]")
    if mode.endswith("field lines"):
        from scipy.interpolate import RegularGridInterpolator

        nxu = 320
        nyu = int(np.clip(nxu * (yb - ya) / (xb - xa), 40, 400))
        xu = np.linspace(max(xa, X[0]), min(xb, X[-1]), nxu)
        yu = np.linspace(max(ya, Y[0]), min(yb, Y[-1]), nyu)
        Yu, Xu = np.meshgrid(yu, xu, indexing="ij")
        pts = np.column_stack([Yu.ravel(), Xu.ravel()])
        exi = RegularGridInterpolator((Y, X), np.nan_to_num(Ex))(pts).reshape(Yu.shape)
        eyi = RegularGridInterpolator((Y, X), np.nan_to_num(Ey))(pts).reshape(Yu.shape)
        msk = (
            RegularGridInterpolator((Y, X), sol.fixed.astype(float), method="nearest")(pts).reshape(
                Yu.shape
            )
            > 0.5
        )
        exi[msk] = np.nan
        eyi[msk] = np.nan
        ax.streamplot(
            xu,
            yu,
            exi,
            eyi,
            density=(2.2, 1.4),
            color="#9fd3ff",
            linewidth=0.6,
            arrowsize=0.6,
            zorder=3,
        )
    if show_eq and mode not in ("Permittivity er", "Mesh"):
        ax.contour(
            Xg,
            Yg,
            V,
            levels=np.linspace(0.1, 0.9, 9),
            colors="w",
            linewidths=0.5,
            alpha=0.8,
            zorder=3,
        )
    draw_conductors(ax, sol)
    if show_mesh:
        xs = X[(xa <= X) & (xb >= X)]
        ys = Y[(ya <= Y) & (yb >= Y)]
        mk = {
            "colors": "k" if mode == "Mesh" else "white",
            "linewidths": 0.3,
            "alpha": 0.6 if mode == "Mesh" else 0.5,
            "zorder": 6,
        }
        ax.vlines(xs, max(ya, Y[0]), min(yb, Y[-1]), **mk)
        ax.hlines(ys, max(xa, X[0]), min(xb, X[-1]), **mk)
    ax.set_xlim(xa, xb)
    ax.set_ylim(ya, yb)
    ax.set_aspect("equal")
    ax.set_xlabel("x [mm]")
    ax.set_ylabel("y [mm]  (0 = signal layer)")
    ttl = f"{mode}  -  1 V on signal, Z0 = {sol.z0:.2f} Ohm"
    if show_mesh:
        ttl += (
            f"\nmesh {sol.info['nx']} x {sol.info['ny']} = {sol.info['nodes']} nodes "
            f"(finest level), min. cell {sol.info['dmin'] * 1e3:.2f} um"
        )
    ax.set_title(ttl, fontsize=10)
    fig.tight_layout()
    return ax


def plot_loss(fig, sol, fmax_hz=20e9):
    """Loss vs frequency into a matplotlib Figure."""
    fig.clear()
    ax = fig.add_subplot(111)
    fs = np.linspace(fmax_hz / 400, fmax_hz, 400)
    ls = [sol.losses(f) for f in fs]
    ac = np.array([x["alpha_c"] for x in ls]) * NP2DB / 1000
    ad = np.array([x["alpha_d"] for x in ls]) * NP2DB / 1000
    ax.plot(fs / 1e9, ac, label="Conductor")
    ax.plot(fs / 1e9, ad, label="Dielectric")
    ax.plot(fs / 1e9, ac + ad, "k", lw=2, label="Total")
    if sol.p.f <= fmax_hz:
        ax.axvline(sol.p.f / 1e9, color="0.6", ls="--", lw=0.8)
    ax.set_xlabel("Frequency [GHz]")
    ax.set_ylabel("Loss [dB/mm]")
    sec = ax.secondary_yaxis("right", functions=(lambda v: v * 25.4, lambda v: v / 25.4))
    sec.set_ylabel("Loss [dB/in]")
    ax.set_title(f"Loss vs frequency (Z0 = {sol.z0:.2f} Ohm; er, tan d constant)", fontsize=10)
    ax.grid(alpha=0.3)
    ax.legend()
    fig.tight_layout()
    return ax


def plot_return(fig, sol, gs=None):
    """HF return-current density J_s/I along x on the ground conductors."""
    if gs is None:
        gs = ground_split(sol)
    p = sol.p
    fig.clear()
    ax = fig.add_subplot(111)
    cur = gs["current"]
    styles = {
        "plane1": ("C0", "-", "Ground plane 1 (top)"),
        "plane2": ("C1", "-", "Ground plane 2 (bottom)"),
        "cop_top": ("C2", "-", "Coplanar top face"),
        "cop_bottom": ("C2", "--", "Coplanar bottom face"),
    }
    xr = min(p.w / 2 + p.s + min(p.g, 1.5 * (p.w / 2 + p.s)), sol.geom["xd"])
    ymax = 0.0
    for c, (col, ls, lab) in styles.items():
        if c not in gs["profiles"]:
            continue
        x, j = gs["profiles"][c]
        vis = np.abs(x) <= xr
        if vis.any():
            ymax = max(ymax, float(np.nanmax(j[vis])))
        if c.startswith("cop"):  # break the line across signal and gaps
            neg, pos = x < 0, x > 0
            x = np.concatenate([x[neg], [np.nan], x[pos]])
            j = np.concatenate([j[neg], [np.nan], j[pos]])
        ax.plot(x, j, color=col, ls=ls, lw=1.4, label=f"{lab}, {cur[c]:.1f} %")
    ymax *= 1.15
    ax.axvspan(-p.w / 2, p.w / 2, color="C0", alpha=0.08, lw=0)
    for xv in (-p.w / 2 - p.s, -p.w / 2, p.w / 2, p.w / 2 + p.s):
        ax.axvline(xv, color="0.5", lw=0.6, ls=":")
    ax.text(0, ymax * 0.97, "signal", ha="center", va="top", fontsize=9, color="0.3")
    for sx in (-1, 1):
        ax.text(
            sx * (p.w / 2 + p.s / 2),
            ymax * 0.97,
            "s",
            ha="center",
            va="top",
            fontsize=9,
            color="0.3",
        )
    ax.set_xlim(-xr, xr)
    ax.set_ylim(0, ymax)
    ax.set_xlabel("x [mm]")
    ax.set_ylabel("J_s / I  [1/mm]")
    ax.set_title(
        f"HF return-current density (coplanar gap flank {cur['cop_flank']:.1f} %, "
        f"not shown as a curve)",
        fontsize=10,
    )
    ax.grid(alpha=0.3)
    ax.legend(fontsize=9, loc="upper right", bbox_to_anchor=(1.0, 0.9))
    fig.tight_layout()
    return ax


# --------------------------------------------------------------------------
# GUI
# --------------------------------------------------------------------------
LEN_UNITS = {"µm": 1e-3, "mm": 1.0, "cm": 10.0, "m": 1000.0, "mil": 0.0254, "in": 25.4, "ft": 304.8}
FREQ_UNITS = {"Hz": 1.0, "kHz": 1e3, "MHz": 1e6, "GHz": 1e9}


def run_gui():
    import time
    import tkinter as tk
    from tkinter import messagebox, ttk

    from matplotlib.backends.backend_tkagg import (
        FigureCanvasTkAgg,
        NavigationToolbar2Tk,  # pyright: ignore[reportPrivateImportUsage]
    )
    from matplotlib.figure import Figure

    root = tk.Tk()
    root.title("CPWG Inner-Layer Impedance Solver (2D FD)")
    defaults = Params()

    def num(text):
        return float(str(text).strip().replace(",", "."))

    class QField:
        """Label + entry + unit dropdown. get()/set() in internal units (mm or Hz)."""

        def __init__(self, parent, row, label, value, units, unit, lam=None):
            self.units = dict(units)
            self.lam = lam
            names = [*self.units, "λ"] if lam is not None else list(self.units)
            ttk.Label(parent, text=label).grid(row=row, column=0, sticky="w", padx=(0, 6))
            self.var = tk.StringVar()
            self.entry = ttk.Entry(parent, textvariable=self.var, width=10)
            self.entry.grid(row=row, column=1, sticky="ew")
            self.unit = tk.StringVar(value=unit)
            self.combo = ttk.Combobox(
                parent, textvariable=self.unit, values=names, width=5, state="readonly"
            )
            self.combo.grid(row=row, column=2, sticky="w", padx=(4, 0))
            self.combo.bind("<<ComboboxSelected>>", self._unit_changed)
            self._last = unit
            self.set(value)

        def factor(self, u):
            if u == "λ":
                if self.lam is None:
                    raise ValueError("no wavelength reference")
                return self.lam()
            return self.units[u]

        def get(self):
            return num(self.var.get()) * self.factor(self.unit.get())

        def set(self, v):
            self.var.set(f"{v / self.factor(self.unit.get()):.6g}")

        def _unit_changed(self, _e=None):
            new = self.unit.get()
            try:
                v = num(self.var.get()) * self.factor(self._last)
                self._last = new
                self.set(v)
            except Exception:
                self._last = new

        def dump(self):
            return {"value": self.var.get(), "unit": self.unit.get()}

        def load(self, d):
            u = d.get("unit")
            if u in self.units or (u == "λ" and self.lam is not None):
                self.unit.set(u)
                self._last = u
            if "value" in d:
                self.var.set(str(d["value"]))

        def enable(self, on):
            st = ["!disabled"] if on else ["disabled"]
            self.entry.state(st)
            self.combo.state(st + (["readonly"] if on else []))

    class PField:
        """Label + entry + fixed unit label."""

        def __init__(self, parent, row, label, value, unit="", scale=1.0):
            self.scale = scale
            ttk.Label(parent, text=label).grid(row=row, column=0, sticky="w", padx=(0, 6))
            self.var = tk.StringVar(value=f"{value / scale:g}")
            self.entry = ttk.Entry(parent, textvariable=self.var, width=10)
            self.entry.grid(row=row, column=1, sticky="ew")
            ttk.Label(parent, text=unit).grid(row=row, column=2, sticky="w", padx=(4, 0))

        def get(self):
            return num(self.var.get()) * self.scale

        def set(self, v):
            self.var.set(f"{v / self.scale:.6g}")

        def dump(self):
            return {"value": self.var.get()}

        def load(self, d):
            if "value" in d:
                self.var.set(str(d["value"]))

        def enable(self, on):
            self.entry.state(["!disabled"] if on else ["disabled"])

    # ------------------------------------------------------------ layout
    left = ttk.Frame(root, padding=6)
    left.grid(row=0, column=0, sticky="nsw")
    colA = ttk.Frame(left)
    colA.grid(row=0, column=0, sticky="n", padx=(0, 6))
    colB = ttk.Frame(left)
    colB.grid(row=0, column=1, sticky="n")
    right = ttk.Frame(root, padding=6)
    right.grid(row=0, column=1, sticky="nsew")
    root.columnconfigure(1, weight=1)
    root.rowconfigure(0, weight=1)

    F = {}

    def group(parent, title):
        lf = ttk.LabelFrame(parent, text=title, padding=4)
        lf.pack(fill="x", pady=2)
        lf.columnconfigure(1, weight=1)
        return lf

    def lam():
        try:
            er = max(
                F["er1"].get(),
                F["er2"].get(),
                F["er_gap"].get() if gap_mode.get() == "custom" else 0,
            )
            return C_LIGHT / (F["f"].get() * math.sqrt(er)) * 1e3
        except Exception:
            return float("nan")

    gap_mode = tk.StringVar(value=defaults.gap_mode)
    plane1 = tk.BooleanVar(value=defaults.plane1)
    plane2 = tk.BooleanVar(value=defaults.plane2)
    fence1 = tk.BooleanVar(value=defaults.fence1)
    fence2 = tk.BooleanVar(value=defaults.fence2)

    g = group(colA, "Geometry")
    F["w"] = QField(g, 0, "Trace width w", defaults.w, LEN_UNITS, "mm")
    F["s"] = QField(g, 1, "Gap s", defaults.s, LEN_UNITS, "mm")
    F["t"] = QField(g, 2, "Copper thickness t", defaults.t, LEN_UNITS, "µm")
    F["g"] = QField(g, 3, "Via fence distance g", defaults.g, LEN_UNITS, "mm")

    g = group(colA, "Upper dielectric / plane 1")
    F["h1"] = QField(g, 0, "Height h1", defaults.h1, LEN_UNITS, "mm")
    F["er1"] = PField(g, 1, "er1", defaults.er1)
    F["tand1"] = PField(g, 2, "tan d1", defaults.tand1)
    ttk.Checkbutton(g, text="Ground plane 1 (top)", variable=plane1).grid(
        row=3, column=0, columnspan=3, sticky="w"
    )
    ttk.Checkbutton(g, text="Via fence 1 (coplanar to top)", variable=fence1).grid(
        row=4, column=0, columnspan=3, sticky="w"
    )

    g = group(colA, "Lower dielectric / plane 2")
    F["h2"] = QField(g, 0, "Height h2", defaults.h2, LEN_UNITS, "mm")
    F["er2"] = PField(g, 1, "er2", defaults.er2)
    F["tand2"] = PField(g, 2, "tan d2", defaults.tand2)
    ttk.Checkbutton(g, text="Ground plane 2 (bottom)", variable=plane2).grid(
        row=3, column=0, columnspan=3, sticky="w"
    )
    ttk.Checkbutton(g, text="Via fence 2 (coplanar to bottom)", variable=fence2).grid(
        row=4, column=0, columnspan=3, sticky="w"
    )

    g = group(colA, "Gap material (signal - coplanar ground)")
    ttk.Label(g, text="Material").grid(row=0, column=0, sticky="w")
    ttk.Combobox(
        g, textvariable=gap_mode, values=["upper", "lower", "custom"], width=8, state="readonly"
    ).grid(row=0, column=1, sticky="w")
    F["er_gap"] = PField(g, 1, "er gap (custom)", defaults.er_gap)
    F["tand_gap"] = PField(g, 2, "tan d gap (custom)", defaults.tand_gap)

    g = group(colB, "Conductor")
    F["sigma"] = PField(g, 0, "Conductivity", defaults.sigma, "MS/m", 1e6)
    F["rq"] = QField(g, 1, "Roughness Rq", defaults.rq, LEN_UNITS, "µm")

    g = group(colB, "Frequency / length")
    F["f"] = QField(g, 0, "Frequency", defaults.f, FREQ_UNITS, "GHz")
    F["length"] = QField(g, 1, "Line length", defaults.length, LEN_UNITS, "mm")

    g = group(colB, "Mesh")
    mesh_mode = tk.StringVar(value="Auto (target error)")
    ttk.Label(g, text="Mode").grid(row=0, column=0, sticky="w")
    ttk.Combobox(
        g,
        textvariable=mesh_mode,
        values=["Auto (target error)", "Manual"],
        width=17,
        state="readonly",
    ).grid(row=0, column=1, columnspan=2, sticky="w")
    F["tol"] = PField(g, 1, "Target error", defaults.tol, "%")
    F["dmin"] = QField(g, 2, "Min. cell (edges)", defaults.dmin, LEN_UNITS, "µm", lam=lam)
    F["dmax"] = QField(g, 3, "Max. cell (core)", defaults.dmax, LEN_UNITS, "µm", lam=lam)
    richardson = tk.BooleanVar(value=True)
    rich_cb = ttk.Checkbutton(g, text="Richardson (2nd level, half size)", variable=richardson)
    rich_cb.grid(row=4, column=0, columnspan=3, sticky="w")
    F["max_nodes"] = PField(g, 5, "Max. nodes", defaults.max_nodes)
    F["margin"] = QField(g, 6, "Domain margin (0=auto)", defaults.margin, LEN_UNITS, "mm")

    def mesh_mode_changed(*_):
        manual = mesh_mode.get() == "Manual"
        F["dmin"].enable(manual)
        F["dmax"].enable(manual)
        rich_cb.state(["!disabled"] if manual else ["disabled"])

    mesh_mode.trace_add("write", mesh_mode_changed)
    mesh_mode_changed()

    g = group(colB, "Synthesis")
    F["z_target"] = PField(g, 0, "Target Z0", 35.0, "Ohm")
    ttk.Label(g, text="Vary").grid(row=1, column=0, sticky="w")
    var_map = {"w (s fixed)": "w", "s (w fixed)": "s", "w (2s+w fixed)": "w_total"}
    syn_var = tk.StringVar(value="w (s fixed)")
    ttk.Combobox(g, textvariable=syn_var, values=list(var_map), width=14, state="readonly").grid(
        row=1, column=1, columnspan=2, sticky="w"
    )

    bf = ttk.Frame(colB)
    bf.pack(fill="x", pady=6)
    b_an = ttk.Button(bf, text="Analyze")
    b_sy = ttk.Button(bf, text="Synthesize")
    b_ca = ttk.Button(bf, text="Cancel")
    for i, b in enumerate((b_an, b_sy, b_ca)):
        b.grid(row=0, column=i, sticky="ew", padx=2)
        bf.columnconfigure(i, weight=1)
    b_ca.state(["disabled"])

    # ------------------------------------------------------------ notebook
    nb = ttk.Notebook(right)
    nb.grid(row=0, column=0, sticky="nsew")
    right.columnconfigure(0, weight=1)
    right.rowconfigure(0, weight=1)

    tab_geo = ttk.Frame(nb, padding=4)
    nb.add(tab_geo, text="Cross-section")
    cv = tk.Canvas(
        tab_geo, width=700, height=340, bg="white", highlightthickness=1, highlightbackground="#bbb"
    )
    cv.pack(fill="both", expand=True)

    COL_UP, COL_LO, COL_CUSTOM = "#e6c97a", "#7fb0e6", "#93d17f"
    COL_CU, COL_SIG = "#d9824f", "#1f5fbf"

    def draw_schematic(*_):
        cv.delete("all")
        x0, x1 = 40, 610
        xf0, xf1 = 95, 555  # fences / coplanar outer ends
        yt, y1, yc, y2, yb = 62, 72, 162, 176, 266
        cx, hw, s = 325, 70, 45
        gm = gap_mode.get()
        gap_col = {"upper": COL_UP, "lower": COL_LO}.get(gm, COL_CUSTOM)
        cv.create_text(
            330,
            16,
            text="CPWG in PCB inner layer (not to scale)",
            font=("TkDefaultFont", 11, "bold"),
        )
        cv.create_rectangle(x0, y1, x1, yc, fill=COL_UP, outline="")
        cv.create_rectangle(x0, y2, x1, yb, fill=COL_LO, outline="")
        # copper layer: gaps + area beyond the coplanar ground filled with gap material
        cv.create_rectangle(x0, yc, x1, y2, fill=gap_col, outline="")
        if plane1.get():
            cv.create_rectangle(x0, yt, x1, y1, fill=COL_CU, outline="")
            cv.create_text(x1, yt - 8, anchor="e", text="Ground plane 1", fill="#444")
        else:
            cv.create_line(x0, y1, x1, y1, dash=(4, 3), fill="#666")
            cv.create_text(330, y1 - 22, text="no ground plane 1  (air)", fill="#666")
        if plane2.get():
            cv.create_rectangle(x0, yb, x1, yb + 10, fill=COL_CU, outline="")
            cv.create_text(x1, yb + 22, anchor="e", text="Ground plane 2", fill="#444")
        else:
            cv.create_line(x0, yb, x1, yb, dash=(4, 3), fill="#666")
            cv.create_text(330, yb + 22, text="no ground plane 2  (air)", fill="#666")
        cv.create_rectangle(x0, yc, cx - hw - s, y2, fill=COL_CU, outline="")
        cv.create_rectangle(cx + hw + s, yc, x1, y2, fill=COL_CU, outline="")
        cv.create_rectangle(cx - hw, yc, cx + hw, y2, fill=COL_SIG, outline="")
        for xv in (xf0, xf1):
            if fence1.get():
                cv.create_rectangle(xv - 4, y1, xv + 4, yc, fill=COL_CU, outline="#8a4a25")
            if fence2.get():
                cv.create_rectangle(xv - 4, y2, xv + 4, yb, fill=COL_CU, outline="#8a4a25")
        if fence1.get():
            cv.create_text(xf1 - 8, y1 + 14, anchor="e", text="Via fence 1", fill="#444")
        if fence2.get():
            cv.create_text(xf1 - 8, yb - 14, anchor="e", text="Via fence 2", fill="#444")
        cv.create_text(x0 + 70, y1 + 20, anchor="w", text="Upper dielectric:  h1, er1, tan d1")
        cv.create_text(x0 + 70, yb - 20, anchor="w", text="Lower dielectric:  h2, er2, tan d2")
        cv.create_text(cx, y2 + 16, text="Signal", font=("TkDefaultFont", 10, "bold"))
        cv.create_text((xf0 + cx - hw - s) / 2, y2 + 14, text="Coplanar ground", fill="#333")
        # dimensions
        ya = yc - 14
        for xa, xb, lab in (
            (cx - hw - s, cx - hw, "s"),
            (cx - hw, cx + hw, "w"),
            (cx + hw, cx + hw + s, "s"),
        ):
            cv.create_line(xa, ya, xb, ya, arrow="both", arrowshape=(6, 7, 3))
            cv.create_text((xa + xb) / 2, ya - 9, text=lab, font=("TkDefaultFont", 11))
        for xv in (cx - hw - s, cx - hw, cx + hw, cx + hw + s):
            cv.create_line(xv, ya - 6, xv, yc, dash=(2, 2), fill="#777")
        yg = y2 + 34
        cv.create_line(cx + hw + s, y2, cx + hw + s, yg + 4, dash=(2, 2), fill="#555")
        cv.create_line(xf1, y2, xf1, yg + 4, dash=(2, 2), fill="#555")
        cv.create_line(cx + hw + s, yg, xf1, yg, arrow="both", arrowshape=(6, 7, 3))
        cv.create_text((cx + hw + s + xf1) / 2, yg - 9, text="g", font=("TkDefaultFont", 11))
        xd = x1 + 28
        cv.create_line(xd, y1, xd, yc, arrow="both", arrowshape=(6, 7, 3))
        cv.create_text(xd + 14, (y1 + yc) / 2, text="h1", font=("TkDefaultFont", 11))
        cv.create_line(xd, y2, xd, yb, arrow="both", arrowshape=(6, 7, 3))
        cv.create_text(xd + 14, (y2 + yb) / 2, text="h2", font=("TkDefaultFont", 11))
        cv.create_line(x1 + 10, yc, x1 + 10, y2, fill="#333")
        cv.create_text(x1 + 14, (yc + y2) / 2, anchor="w", text="t")
        cv.create_line(x0 - 6, y2, x0, y2, fill="#666")
        cv.create_text(x0 - 8, y2, anchor="e", text="y=0", fill="#666", font=("TkDefaultFont", 8))
        # legend
        ly = 316
        for i, (col, txt) in enumerate(
            (
                (COL_UP, "upper dielectric"),
                (COL_LO, "lower dielectric"),
                (COL_CUSTOM, "custom gap material"),
                (COL_CU, "copper"),
            )
        ):
            lx = 60 + i * 150
            cv.create_rectangle(lx, ly - 6, lx + 14, ly + 6, fill=col, outline="#666")
            cv.create_text(lx + 20, ly, anchor="w", text=txt, fill="#333")

    for v in (gap_mode, plane1, plane2, fence1, fence2):
        v.trace_add("write", draw_schematic)
    draw_schematic()

    tab_res = ttk.Frame(nb, padding=4)
    nb.add(tab_res, text="Results")
    out = tk.Text(tab_res, width=90, height=34, font=("Courier", 10))
    out.pack(fill="both", expand=True)
    out.insert(
        "end",
        "Set parameters and press 'Analyze' or 'Synthesize'.\n"
        "Heights h1/h2 are measured from the copper surface to the plane "
        "(or board surface).\n",
    )

    tab_field = ttk.Frame(nb, padding=4)
    nb.add(tab_field, text="Field")
    tab_ret = ttk.Frame(nb, padding=4)
    nb.add(tab_ret, text="Return current")
    tab_loss = ttk.Frame(nb, padding=4)
    nb.add(tab_loss, text="Loss vs f")

    field_q = tk.StringVar(value=FIELD_MODES[0])
    show_eq = tk.BooleanVar(value=True)
    zoom = tk.BooleanVar(value=True)
    show_mesh = tk.BooleanVar(value=False)
    db_range = tk.StringVar(value="40")
    readout = tk.StringVar(value="")

    ret_top = ttk.Frame(tab_ret)
    ret_top.pack(fill="x")
    tree = ttk.Treeview(ret_top, columns=("field", "current"), height=10)
    tree.heading("#0", text="Ground conductor")
    tree.heading("field", text="E-field share")
    tree.heading("current", text="Return current (HF)")
    tree.column("#0", width=280)
    tree.column("field", width=120, anchor="e")
    tree.column("current", width=150, anchor="e")
    tree.pack(side="left", fill="x", expand=True)
    ret_info = tk.StringVar(value="Run an analysis first.")
    ttk.Label(tab_ret, textvariable=ret_info, anchor="w", justify="left", wraplength=780).pack(
        fill="x", pady=4
    )

    fctl = ttk.Frame(tab_field)
    fctl.pack(fill="x")
    ttk.Label(fctl, text="Show").pack(side="left")
    ttk.Combobox(fctl, textvariable=field_q, width=22, state="readonly", values=FIELD_MODES).pack(
        side="left", padx=4
    )
    ttk.Checkbutton(fctl, text="Equipotentials", variable=show_eq).pack(side="left", padx=4)
    ttk.Checkbutton(fctl, text="Zoom to trace", variable=zoom).pack(side="left", padx=4)
    ttk.Checkbutton(fctl, text="Show mesh", variable=show_mesh).pack(side="left", padx=4)
    ttk.Label(fctl, text="dB range").pack(side="left", padx=(8, 2))
    e_db = ttk.Entry(fctl, textvariable=db_range, width=5)
    e_db.pack(side="left")
    e_db.bind("<Return>", lambda e: draw_field())
    ffig = Figure(figsize=(8, 4.6), dpi=100)
    fcan = FigureCanvasTkAgg(ffig, master=tab_field)
    NavigationToolbar2Tk(fcan, tab_field).update()
    fcan.get_tk_widget().pack(fill="both", expand=True)
    ttk.Label(tab_field, textvariable=readout, anchor="w", font=("Courier", 10)).pack(fill="x")

    rfig = Figure(figsize=(8, 3.8), dpi=100)
    rcan = FigureCanvasTkAgg(rfig, master=tab_ret)
    NavigationToolbar2Tk(rcan, tab_ret).update()
    rcan.get_tk_widget().pack(fill="both", expand=True)

    lctl = ttk.Frame(tab_loss)
    lctl.pack(fill="x")
    fmax = QField(lctl, 0, "f max", 20e9, FREQ_UNITS, "GHz")
    ttk.Button(lctl, text="Update", command=lambda: draw_loss()).grid(row=0, column=3, padx=6)
    lfig = Figure(figsize=(8, 4.6), dpi=100)
    lcan = FigureCanvasTkAgg(lfig, master=tab_loss)
    NavigationToolbar2Tk(lcan, tab_loss).update()
    lcan.get_tk_widget().pack(fill="both", expand=True)

    # status bar
    sbar = ttk.Frame(root)
    sbar.grid(row=1, column=0, columnspan=2, sticky="ew")
    prog = ttk.Progressbar(sbar, mode="indeterminate", length=160)
    prog.pack(side="left", padx=4, pady=2)
    status = tk.StringVar(value="Ready")
    ttk.Label(sbar, textvariable=status, anchor="w").pack(side="left", fill="x", expand=True)

    state = {"sol": None, "busy": False, "ctl": None, "t0": 0.0}
    q = queue.Queue()

    def placeholder(fig, canvas):
        fig.clear()
        ax = fig.add_subplot(111)
        ax.text(0.5, 0.5, "Run an analysis first", ha="center", va="center", transform=ax.transAxes)
        ax.set_axis_off()
        canvas.draw()

    def draw_field():
        sol = state["sol"]
        if sol is None:
            placeholder(ffig, fcan)
            return
        try:
            dbr = max(num(db_range.get()), 1.0)
        except ValueError:
            dbr = 40.0
        plot_field(ffig, sol, field_q.get(), show_eq.get(), zoom.get(), dbr, show_mesh.get())
        fcan.draw()

    def on_motion(ev):
        sol = state["sol"]
        if sol is None or ev.inaxes is None or ev.xdata is None:
            return
        X, Y = sol.X, sol.Y
        i = int(np.clip(np.searchsorted(X, ev.xdata), 1, len(X) - 1))
        j = int(np.clip(np.searchsorted(Y, ev.ydata), 1, len(Y) - 1))
        i -= int(ev.xdata - X[i - 1] < X[i] - ev.xdata)
        j -= int(ev.ydata - Y[j - 1] < Y[j] - ev.ydata)
        if state.get("E_for") is not sol:
            state["E"] = e_field(sol)[2]
            state["E_for"] = sol
        e = state["E"][j, i]
        ci, cj = min(i, len(X) - 2), min(j, len(Y) - 2)
        readout.set(
            f"x = {ev.xdata:+.4f} mm  y = {ev.ydata:+.4f} mm   V = {sol.V[j, i]:.4f} V   "
            + (
                f"|E| = {e:.3f} V/mm ({20 * math.log10(max(e, 1e-30)):.1f} dB)"
                if np.isfinite(e)
                else "conductor"
            )
            + f"   er = {sol.er_cell[cj, ci]:.2f}"
        )

    def draw_loss():
        sol = state["sol"]
        if sol is None:
            placeholder(lfig, lcan)
            return
        try:
            fm = fmax.get()
        except Exception:
            fm = 20e9
        plot_loss(lfig, sol, fm)
        lcan.draw()

    def draw_return():
        tree.delete(*tree.get_children())
        sol = state["sol"]
        if sol is None:
            placeholder(rfig, rcan)
            return
        p = sol.p
        gs = ground_split(sol)
        fdl, cur = gs["field"], gs["current"]
        fmt = lambda v: f"{v:6.1f} %"
        for c, on in (("plane1", p.plane1), ("plane2", p.plane2)):
            if on:
                tree.insert("", "end", text=GROUND_LABELS[c], values=(fmt(fdl[c]), fmt(cur[c])))
        cop = tree.insert(
            "",
            "end",
            text="Coplanar ground (total)",
            values=(fmt(fdl["coplanar"]), fmt(cur["coplanar"])),
            open=True,
        )
        for c in COP_CATS:
            if c == "cop_outer" and fdl[c] < 0.05 and cur[c] < 0.05:
                continue
            tree.insert(cop, "end", text=GROUND_LABELS[c], values=(fmt(fdl[c]), fmt(cur[c])))
        for c, on in (("fence1", p.fence1), ("fence2", p.fence2), ("boundary", not p.closed())):
            if on:
                tree.insert("", "end", text=GROUND_LABELS[c], values=(fmt(fdl[c]), fmt(cur[c])))
        d = sol.skin_depth(p.f) * 1e3
        ok = p.t > 0 and d < p.t / 3
        txt = ""
        if gs["x90_planes"] is not None:
            txt += (
                f"90 % of the plane return current flows within |x| <= {gs['x90_planes']:.3f} mm; "
            )
        txt += (
            f"90 % of the coplanar return current within {gs['x90_cop']:.3f} mm of the gap edge.\n"
            f"E-field share = where the field lines end (solution with dielectrics). Return "
            f"current = HF distribution (skin depth << t). Skin depth at {p.f / 1e9:g} GHz: "
            f"{d * 1e3:.2f} um -> "
            + ("valid." if ok else "NOT well below t: the real current spreads more widely.")
        )
        if not p.closed():
            txt += (
                "\nDomain boundary = flux / current reaching the outer edge of the computation "
                "domain (remote ground)."
            )
        ret_info.set(txt)
        plot_return(rfig, sol, gs)
        rcan.draw()

    fcan.mpl_connect("motion_notify_event", on_motion)
    for v in (field_q, show_eq, zoom, show_mesh):
        v.trace_add("write", lambda *a: draw_field())

    # ------------------------------------------------------------ logic
    def read_params():
        kw = {
            k: F[k].get()
            for k in (
                "w",
                "s",
                "t",
                "g",
                "h1",
                "er1",
                "tand1",
                "h2",
                "er2",
                "tand2",
                "er_gap",
                "tand_gap",
                "sigma",
                "rq",
                "f",
                "length",
                "tol",
                "margin",
            )
        }
        kw.update(
            plane1=plane1.get(),
            plane2=plane2.get(),
            fence1=fence1.get(),
            fence2=fence2.get(),
            gap_mode=gap_mode.get(),
            mesh_mode="manual" if mesh_mode.get() == "Manual" else "auto",
            richardson=richardson.get(),
            max_nodes=int(F["max_nodes"].get()),
        )
        if kw["mesh_mode"] == "manual":
            kw["dmin"] = F["dmin"].get()
            kw["dmax"] = F["dmax"].get()
        p = Params(**kw)
        p.validate()
        return p

    def set_busy(b):
        state["busy"] = b
        for btn in (b_an, b_sy):
            btn.state(["disabled"] if b else ["!disabled"])
        b_ca.state(["!disabled"] if b else ["disabled"])
        if b:
            state["t0"] = time.time()
            prog.start(12)
        else:
            prog.stop()

    def worker(fn):
        ctl = Ctl(progress=lambda m: q.put(("progress", m)))
        state["ctl"] = ctl

        def run():
            try:
                q.put(("done", fn(ctl)))
            except Cancelled as e:
                q.put(("cancelled", e))
            except Exception as e:
                q.put(("error", e))

        set_busy(True)
        threading.Thread(target=run, daemon=True).start()

    def poll():
        try:
            while True:
                kind, val = q.get_nowait()
                el = time.time() - state["t0"]
                if kind == "progress":
                    status.set(f"[{el:5.1f} s]  {val}")
                elif kind == "cancelled":
                    set_busy(False)
                    status.set(f"Cancelled after {el:.1f} s")
                elif kind == "error":
                    set_busy(False)
                    status.set("Error")
                    messagebox.showerror("Error", str(val))
                elif kind == "done":
                    set_busy(False)
                    text, sol, newp = val
                    state["sol"] = sol
                    if newp is not None:
                        F["w"].set(newp.w)
                        F["s"].set(newp.s)
                        save_settings()
                    out.delete("1.0", "end")
                    out.insert("end", text)
                    status.set(
                        f"Done in {el:.1f} s: Z0 = {sol.z0:.3f} Ohm, {sol.info['nodes']} nodes"
                    )
                    if nb.index(nb.select()) == 0:
                        nb.select(tab_res)
                    draw_field()
                    draw_loss()
                    draw_return()
        except queue.Empty:
            pass
        root.after(100, poll)

    def on_analyze():
        try:
            p = read_params()
        except ValueError as e:
            messagebox.showerror("Input", str(e))
            return

        save_settings()

        def job(ctl):
            sol = solve(p, ctl)
            return report(sol, ctl=ctl), sol, None

        worker(job)

    def on_synth():
        try:
            p = read_params()
            zt = F["z_target"].get()
        except ValueError as e:
            messagebox.showerror("Input", str(e))
            return
        var = var_map[syn_var.get()]
        save_settings()

        def job(ctl):
            pp, sol = synthesize(p, zt, var, ctl)
            return (
                report(sol, title=f"Synthesis: Z0 = {zt:g} Ohm, vary {syn_var.get()}", ctl=ctl),
                sol,
                pp,
            )

        worker(job)

    def on_cancel():
        if state["ctl"] is not None:
            state["ctl"].cancel()
            status.set("Cancelling (after the current linear solve) ...")

    # ------------------------------------------------------------ settings file
    plain_vars = {
        "gap_mode": gap_mode,
        "plane1": plane1,
        "plane2": plane2,
        "fence1": fence1,
        "fence2": fence2,
        "mesh_mode": mesh_mode,
        "richardson": richardson,
        "syn_var": syn_var,
        "field_q": field_q,
        "show_eq": show_eq,
        "zoom": zoom,
        "show_mesh": show_mesh,
        "db_range": db_range,
    }

    def save_settings():
        data = {
            "version": 1,
            "fields": {k: f.dump() for k, f in F.items()},
            "vars": {k: v.get() for k, v in plain_vars.items()},
        }
        data["fields"]["fmax"] = fmax.dump()
        try:
            with open(SETTINGS_FILE, "w", encoding="utf-8") as fh:
                json.dump(data, fh, indent=2, ensure_ascii=False)
        except OSError as e:
            status.set(f"Could not save settings: {e}")

    def load_settings():
        try:
            with open(SETTINGS_FILE, encoding="utf-8") as fh:
                data = json.load(fh)
        except (OSError, ValueError):
            return
        fields = dict(F)
        fields["fmax"] = fmax
        for k, d in data.get("fields", {}).items():
            if k in fields and isinstance(d, dict):
                with contextlib.suppress(Exception):
                    fields[k].load(d)
        for k, v in data.get("vars", {}).items():
            if k in plain_vars:
                with contextlib.suppress(Exception):
                    plain_vars[k].set(v)
        status.set(f"Settings loaded from {SETTINGS_FILE}")

    def on_close():
        save_settings()
        root.destroy()

    load_settings()
    root.protocol("WM_DELETE_WINDOW", on_close)

    b_an.configure(command=on_analyze)
    b_sy.configure(command=on_synth)
    b_ca.configure(command=on_cancel)
    draw_field()
    draw_loss()
    draw_return()
    root.after(100, poll)
    root.mainloop()


def main():
    if "--selftest" in sys.argv:
        selftest()
    else:
        run_gui()


if __name__ == "__main__":
    main()
