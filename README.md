# Transverse

Calculates the impedance, losses and fields of a transmission line from its 2D cross-section. Enter the stack-up and the trace geometry, press **Analyze**, and get Z0, ε_eff, L', C', conductor and dielectric loss, the electric field, and where the return current flows. Or give a target impedance and let **Synthesize** find the trace width or gap.

The first supported line is a **coplanar waveguide with ground (CPWG) in a PCB inner layer**, a signal trace with coplanar grounds between two dielectrics, optionally with ground planes above and below and via fences:

```
    =============================================  ground plane 1 (optional)
          upper dielectric   h1, er1, tan d1       | via fence 1 (optional)
    #######  s  [====== w ======]  s  ########      t   signal + coplanar ground
          lower dielectric   h2, er2, tan d2       | via fence 2 (optional)
    =============================================  ground plane 2 (optional)
```

Transverse is a desktop application (Rust, egui) with a command line interface. A Python reference implementation lives in [`python/`](python/); the Rust port reproduces its results (see [Validation](#validation)).

## Features

- **Analysis:** Z0, ε_eff, phase velocity and delay, L', C', R', G', conductor loss (Wheeler's incremental inductance rule with Hammerstad roughness) and dielectric loss at any frequency, loss versus frequency.
- **Synthesis:** solve for the trace width, the gap, or the width at a fixed total width (2s + w) for a target Z0.
- **Return current:** how the electric flux and the high-frequency return current split over ground plane 1, ground plane 2, the coplanar ground (flank, top, bottom, outer edge) and the via fences, with the 90 % widths and current density profiles.
- **Open structures:** missing planes or fences are handled by a grounded outer box that is enlarged automatically until Z0 no longer changes.
- **Accuracy control:** automatic mesh refinement to a target error (default 0.5 %) with Richardson extrapolation, or manual cell sizes (also relative to the wavelength).
- **Units:** all inputs have unit dropdowns (µm to ft, Hz to GHz). A global switch selects metric or imperial lengths on the plots, with the prefix chosen automatically (µm, mm, cm, m or µin, mil, in, ft). Field strength is shown in V/m, or V/mil in imperial mode.
- **Light and dark theme**, saved together with all inputs in a `settings.json` next to the program.

### The application

| Tab | Shows |
| --- | --- |
| Cross-section | Sketch of the structure that follows the plane, fence and gap material settings |
| Results | Full text report of all values |
| Field | \|E\| in dB or linear, potential, permittivity or mesh as a colour map, with field lines, equipotentials, conductors and a cursor readout. Mouse wheel zooms, left drag draws a zoom box, right drag pans |
| Return current | Share table and current density along the ground conductors |
| Loss vs f | Conductor, dielectric and total loss over frequency |

Calculations run on a background thread with progress messages and a Cancel button.

## Download

Builds for Linux (x86_64, glibc 2.28 or newer) and Windows (x86_64) are attached to each release on the [releases page](https://github.com/JPWiedemann/Transverse/releases). Unpack the archive and start `transverse-gui` (`transverse-gui.exe`). Nothing is installed; delete `settings.json` next to the program to reset your inputs.

- Linux needs OpenGL (Mesa is fine) and X11 or Wayland.
- Windows needs nothing besides a normal graphics driver. The binaries are not code signed, so Windows SmartScreen asks for confirmation ("More info", then "Run anyway").

## Build from source

You need a recent stable [Rust](https://rustup.rs) toolchain.

```sh
cargo run --release -p transverse-gui        # the desktop application
cargo test --release                         # unit tests and parity tests against the Python reference
```

Release builds for Linux and Windows from a Linux machine use [zig](https://ziglang.org) through [cargo-zigbuild](https://github.com/rust-cross/cargo-zigbuild):

```sh
cargo install cargo-zigbuild --locked        # and zig, for example: pacman -S zig   or   pip install ziglang
scripts/build-release.sh                     # packages end up in dist/
```

## Command line

`transverse` runs analyses without the GUI. Parameters are a JSON object (lengths in mm, frequency in Hz, conductivity in S/m); anything missing uses the defaults.

```sh
transverse selftest                                  # compare with the Python reference cases
echo '{}' | transverse analyze                       # default stack-up, full report
echo '{"w": 0.3, "s": 0.1, "f": 2e9}' | transverse analyze --json
echo '{}' | transverse synth w 35                    # width for 35 Ohm; also: s, w_total
echo '{"f":2.4e9}' | transverse synth w 35 --phase 90  # ... and the line length for 90 degrees at 2.4 GHz
echo '{}' | transverse analyze --solver fem         # finite elements instead of the grid
echo '{}' | transverse compare                      # both solvers, relative differences
transverse selftest --solver fem                     # FEM against the reference cases
transverse --version
```

The parameters are `w`, `s`, `t`, `g`, `h1`, `er1`, `tand1`, `plane1`, `fence1`, `h2`, `er2`, `tand2`, `plane2`, `fence2`, `gap_mode` (`upper`, `lower`, `custom`), `er_gap`, `tand_gap`, `sigma`, `rq` (RMS roughness), `f`, `length`, `mesh_mode` (`auto`, `manual`), `tol` (target error in %), `dmin`, `dmax`, `richardson`, `max_nodes` and `margin`.

## Method

The default solver is the finite-difference grid described below. A finite-element backend (the Solver field of the form in the desktop app and the web page, `--solver fem` on the command line) meshes the cross-section with a generic triangle mesher, refines it adaptively and solves with quadratic elements; it produces the same outputs (Z0, ε_eff, losses, ground split, field data) and agrees with the grid to better than 0.3 % in Z0 and 1 % in the conductor loss on the reference cases. It is the basis for geometries a grid cannot follow (slanted edges, round conductors).

A quasi-static (quasi-TEM) solution of the Laplace equation ∇·(εr∇V) = 0 on a graded non-uniform tensor grid (finite volume, 5-point stencil), solved with a sparse Cholesky factorisation. The capacitance with and without the dielectrics gives Z0 = 1/(c·√(C·C0)) and ε_eff = C/C0. Two mesh levels are extrapolated (Richardson), and the mesh is refined until the estimated error is below the target.

Limits to keep in mind: the model is quasi-TEM only, εr and tan δ are constant over frequency, and the return-current split assumes a skin depth much smaller than the copper thickness (the report warns when this is not the case).

## Validation

The Rust solver is checked against the Python implementation on 14 analysis and 5 synthesis cases (`tests/reference/cases.json`, generated by `python/export_reference.py`): Z0, ε_eff, C', losses, the ground split, field values and the synthesised dimensions agree to about 1e-9 or better, with identical meshes. For `t = 0` the results are compared with the conformal-mapping solution (agreement within 0.25 %). Examples:

| Case | Z0 |
| --- | --- |
| closed CPWG, t = 0, w = 0.248 mm, s = 0.15 mm, h = 0.25 mm, εr = 3.66 | 49.981 Ω (conformal mapping: 50.018 Ω) |
| default stack-up, w = 0.3605 mm, s = 0.11 mm, t = 18 µm, planes and fences | 35.001 Ω, ε_eff ≈ 3.58 |
| the same without plane 1 and fence 1 | 43.429 Ω |
| synthesis for 35 Ω with s = 0.11 mm | w = 0.3605 mm |

`transverse selftest` repeats these checks on any machine.

## Repository layout

```
crates/
  transverse-core/    numerics: finite-volume solver, mesh levels, extrapolation, losses, synthesis, field post-processing
  transverse-lines/   line types (CPWG), the LineModel interface, results, schematics
  transverse-gui/     desktop application (egui) and its toolkit-independent logic (form, settings, worker)
  transverse-geom/    cross-section geometry for the mesher: shapes, materials, conductor tags, size field
  transverse-mesh/    generic 2D triangle mesher (constrained Delaunay, graded size field, quality refinement)
  transverse-cli/     command line: analyze, synth, compare, selftest
python/               Python reference implementation (uv project, Tk + matplotlib GUI)
tests/reference/      reference cases exported from the Python implementation
scripts/              release build, release version and notes
```

The numerical crates have no file, thread or clock dependencies, so they also build for `wasm32-unknown-unknown`; this is checked in CI.

The Python version runs with [uv](https://docs.astral.sh/uv/):

```sh
cd python
uv run cpwg_solver.py              # GUI
uv run cpwg_solver.py --selftest   # validation against analytic formulas
uv run --group dev pytest
```

## Contributing and releases

Pull requests go to `dev` with a [Conventional Commits](https://www.conventionalcommits.org/) title and are squash merged; `main` only receives the release pull request, and merging it publishes a release. See [CONTRIBUTING.md](CONTRIBUTING.md) and [MAINTAINING.md](MAINTAINING.md).

## License

Transverse is free software, licensed under the [GNU General Public License, version 3 only](LICENSE). Contributions are accepted under the [Contributor License Agreement](CLA.md). Copyright (C) 2026 The Transverse authors.
