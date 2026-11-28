use super::*;

fn sym(a11: f64, a12: f64, a22: f64) -> SymMatrix {
    let rows = [[a11, a12], [a12, a22]];
    SymMatrix::from_fn(2, |i, j| rows[i][j])
}

/// Two unequal coupled lines in an inhomogeneous dielectric.
fn unequal() -> CapMatrices {
    CapMatrices {
        c: sym(1.8e-10, -3.0e-11, 1.2e-10),
        c0: sym(5.0e-11, -1.2e-11, 4.0e-11),
        g_over_omega: sym(3.0e-12, -4.0e-13, 1.5e-12),
    }
}

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol * a.abs().max(b.abs())
}

#[test]
fn modes_solve_the_eigenproblem_and_agree_with_the_impedance_matrix() {
    let caps = unequal();
    let zc = impedance_matrix(&caps).unwrap();
    let m = modes(&caps).unwrap();
    assert!(m[0].eps_eff < m[1].eps_eff);
    for mode in &m {
        let (cv, c0v) = (caps.c.mul_vec(&mode.voltages), caps.c0.mul_vec(&mode.voltages));
        for (a, b) in cv.iter().zip(&c0v) {
            assert!((a - mode.eps_eff * b).abs() < 1e-12 * 1.8e-10);
        }
        // v = Zc i
        for (v, back) in mode.voltages.iter().zip(zc.mul_vec(&mode.currents)) {
            assert!((v - back).abs() < 1e-12, "{v} {back}");
        }
    }
    // modes carry power independently: v_j' i_k = 0
    let cross = dot(&m[0].voltages, &m[1].currents);
    assert!(cross.abs() < 1e-12 * dot(&m[0].voltages, &m[0].currents));
    // Zc Y Zc = L C^-1 ... the defining relation: Zc C Zc = L
    let l = caps.inductance().unwrap();
    for j in 0..2 {
        let column: Vec<f64> = (0..2).map(|i| zc.get(i, j)).collect();
        for (i, got) in zc.mul_vec(&caps.c.mul_vec(&column)).iter().enumerate() {
            assert!(close(*got, l.get(i, j), 1e-12), "{got} {}", l.get(i, j));
        }
    }
}

#[test]
fn one_signal_gives_the_familiar_values() {
    let one = |v: f64| SymMatrix::from_fn(1, |_, _| v);
    let caps = CapMatrices { c: one(1.8e-10), c0: one(5.0e-11), g_over_omega: one(2.0e-12) };
    let l = caps.single().unwrap();
    assert_eq!(impedance_matrix(&caps).unwrap().get(0, 0), l.z0());
    let m = &modes(&caps).unwrap()[0];
    assert_eq!(m.voltages, vec![1.0]);
    assert!(close(m.eps_eff, 3.6, 1e-14) && close(m.impedance(), l.z0(), 1e-14));
    // the loss model equals that of the single line: dL/dn = dZ0_air/dn / c
    let dzdn = 1.7e5;
    let walls = WallDerivative { dl_dn: one(dzdn / C_LIGHT), sigma: 5.8e7, rq_mm: 0.0 };
    let loss = mode_loss_model(&caps, m, &walls);
    assert!(close(loss.dzdn, dzdn, 1e-14) && close(loss.g_over_omega, 2.0e-12, 1e-14));
    assert!(unequal().single().is_err());
}

#[test]
fn equal_lines_give_even_and_odd_mode_and_the_pair_impedances() {
    let caps = CapMatrices {
        c: sym(1.5e-10, -2.0e-11, 1.5e-10),
        c0: sym(5.0e-11, -9.0e-12, 5.0e-11),
        g_over_omega: sym(2.0e-12, -1.0e-13, 2.0e-12),
    };
    let [even, odd] = symmetric_pair_modes(&caps).unwrap();
    assert_eq!((even.voltages.clone(), odd.voltages.clone()), (vec![1.0, 1.0], vec![1.0, -1.0]));
    // the textbook values: Z = 1 / (c sqrt((C11 +- C12) (C0_11 +- C0_12)))
    let z = |s: f64| 1.0 / (C_LIGHT * ((1.5e-10 - s * 2.0e-11) * (5.0e-11 - s * 9.0e-12)).sqrt());
    assert!(close(even.impedance(), z(1.0), 1e-14) && close(odd.impedance(), z(-1.0), 1e-14));
    assert!(close(even.eps_eff, 1.3e-10 / 4.1e-11, 1e-14));
    let pair = PairImpedances::new(&impedance_matrix(&caps).unwrap()).unwrap();
    assert!(close(pair.differential, 2.0 * z(-1.0), 1e-13));
    assert!(close(pair.common, z(1.0) / 2.0, 1e-13));
    assert!(close(pair.coupling, (z(1.0) - z(-1.0)) / (z(1.0) + z(-1.0)), 1e-13));
    assert!(pair.conversion.abs() < 1e-12);
    // the general solver finds the same modes
    for m in modes(&caps).unwrap() {
        let want = if m.voltages[1] > 0.0 { &even } else { &odd };
        assert!(close(m.eps_eff, want.eps_eff, 1e-13) && close(m.impedance(), want.impedance(), 1e-13));
    }
}

#[test]
fn degenerate_modes_of_a_homogeneous_dielectric_are_even_and_odd() {
    // C = er C0 up to rounding noise, with the slight asymmetry of a mesh
    for coupling in [-9.0e-12, -9.0e-17] {
        let c0 = sym(5.0e-11, coupling, 5.0e-11 * (1.0 + 1e-9));
        let c = SymMatrix::from_fn(2, |i, j| 3.5 * c0.get(i, j) * (1.0 + 1e-13 * (i + j) as f64));
        let caps = CapMatrices { c, c0, g_over_omega: SymMatrix::zeros(2) };
        if coupling < -1e-13 {
            let m = modes(&caps).unwrap();
            for mode in &m {
                assert!(close(mode.eps_eff, 3.5, 1e-9));
                assert!((mode.voltages[1].abs() - 1.0).abs() < 1e-3, "{:?}", mode.voltages);
            }
            assert!(m[0].voltages[1] * m[1].voltages[1] < 0.0);
        }
        // and with equal lines declared, however weak the coupling
        let [even, odd] = symmetric_pair_modes(&caps).unwrap();
        assert!(even.impedance() >= odd.impedance());
    }
}

#[test]
fn the_mode_losses_follow_the_matrix_formulas() {
    let caps = unequal();
    let zc = impedance_matrix(&caps).unwrap();
    let walls = WallDerivative { dl_dn: sym(4.0e-4, -5.0e-5, 6.0e-4), sigma: 5.8e7, rq_mm: 0.0 };
    for mode in modes(&caps).unwrap() {
        let p = mode_loss_model(&caps, &mode, &walls).at(1e9);
        let (rs, _) = crate::losses::surface_resistance(1e9, 5.8e7, 0.0);
        let (v, i) = (&mode.voltages, &mode.currents);
        let alpha_c = rs / crate::MU0 * walls.dl_dn.form(i, i) / (2.0 * zc.form(i, i));
        let w = 2.0 * std::f64::consts::PI * 1e9;
        let alpha_d = w * caps.g_over_omega.form(v, v) / (2.0 * zc.inverse_spd().unwrap().form(v, v));
        assert!(close(p.alpha_c, alpha_c, 1e-12), "{} {alpha_c}", p.alpha_c);
        assert!(close(p.alpha_d, alpha_d, 1e-12), "{} {alpha_d}", p.alpha_d);
    }
}

#[test]
fn symmetrising_needs_a_swap() {
    assert!(unequal().symmetrised(&[0]).is_err());
    assert!(unequal().symmetrised(&[1, 1]).is_err());
    let s = unequal().symmetrised(&[1, 0]).unwrap();
    assert_eq!(s.c.get(0, 0), s.c.get(1, 1));
}
