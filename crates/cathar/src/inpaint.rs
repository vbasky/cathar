//! Audio inpainting — reconstruct known missing spans (dropouts, tape splices,
//! digital mutes) via autoregressive **Janssen / Godsill–Rayner** interpolation.
//!
//! An AR model is estimated from the samples around the gap (Levinson–Durbin on
//! the autocovariance); the missing samples are then chosen to minimise the AR
//! prediction error given the fixed neighbours — a symmetric banded linear
//! system solved by banded Cholesky. The estimate/solve is iterated a few times
//! (the classic Janssen refinement). Pure Rust, deterministic.

const AR_ORDER: usize = 32;
/// Gaps longer than this (samples) fall back to linear fill — the dense solve
/// would be too large.
const MAX_SOLVE: usize = 2048;

/// Reconstruct the span `[start, start + len)` of `signal` by AR interpolation,
/// refined over `iterations` passes (3 is a good default). Returns a new buffer;
/// out-of-range or empty spans return an unchanged copy.
pub fn inpaint_gap(signal: &[f32], start: usize, len: usize, iterations: u32) -> Vec<f32> {
    let n = signal.len();
    if len == 0 || start >= n || start + len > n {
        return signal.to_vec();
    }
    let mut out = signal.to_vec();

    // Linear pre-fill across the gap (also the fallback for very long gaps).
    let left = if start > 0 { out[start - 1] } else { 0.0 };
    let right = if start + len < n { out[start + len] } else { 0.0 };
    for (i, s) in out[start..start + len].iter_mut().enumerate() {
        let t = (i + 1) as f32 / (len + 1) as f32;
        *s = left * (1.0 - t) + right * t;
    }
    if len > MAX_SOLVE {
        return out;
    }

    // AR reach scales with model order, so grow the order with the gap (capped).
    let p = len.clamp(AR_ORDER, 128);
    // Context must dominate the gap so the AR estimate reflects the signal, not
    // the hole; also keep a floor for tiny gaps.
    let ctx = (len * 4).max(p * 8).max(1024);
    let w0 = start.saturating_sub(ctx);
    let w1 = (start + len + ctx).min(n);
    let mut seg: Vec<f64> = out[w0..w1].iter().map(|&s| s as f64).collect();
    let gstart = start - w0;

    for _ in 0..iterations.max(1) {
        // Estimate AR from the known samples only (the gap would bias it).
        let a = estimate_ar_known(&seg, gstart, len, p);
        let b = coef_autocorr(&a, p);
        solve_gap(&mut seg, gstart, len, &b, p);
    }

    for (i, s) in out[start..start + len].iter_mut().enumerate() {
        *s = seg[gstart + i] as f32;
    }
    out
}

/// Detect interior runs of exact-zero or NaN samples (digital dropouts/mutes)
/// up to `max_gap_ms` long and reconstruct each with [`inpaint_gap`].
pub fn inpaint_auto(signal: &[f32], sample_rate: u32, max_gap_ms: f32) -> Vec<f32> {
    let n = signal.len();
    let max_gap = ((max_gap_ms / 1000.0) * sample_rate as f32).max(1.0) as usize;
    let mut out = signal.to_vec();
    let mut i = 0;
    while i < n {
        let dropout = |v: f32| v == 0.0 || v.is_nan();
        if dropout(out[i]) {
            let start = i;
            while i < n && dropout(out[i]) {
                i += 1;
            }
            let len = i - start;
            // Interior gaps only, within the size bound.
            if len >= 2 && len <= max_gap && start > 0 && start + len < n {
                out = inpaint_gap(&out, start, len, 3);
            }
        } else {
            i += 1;
        }
    }
    out
}

/// AR coefficients `[1, a1, …, ap]` (Levinson) estimated from the samples
/// *outside* the gap `[gstart, gstart+glen)`, so the hole doesn't bias the fit.
fn estimate_ar_known(seg: &[f64], gstart: usize, glen: usize, p: usize) -> Vec<f64> {
    let n = seg.len();
    let known = |i: usize| i < gstart || i >= gstart + glen;
    let mut r = vec![0.0f64; p + 1];
    for (lag, rl) in r.iter_mut().enumerate() {
        let mut s = 0.0;
        for i in lag..n {
            if known(i) && known(i - lag) {
                s += seg[i] * seg[i - lag];
            }
        }
        *rl = s;
    }
    if r[0] <= 0.0 {
        let mut a = vec![0.0; p + 1];
        a[0] = 1.0;
        return a;
    }
    r[0] *= 1.0 + 1e-6; // white-noise regularisation for stability
    levinson(&r, p)
}

/// Levinson–Durbin recursion: solve the Yule–Walker equations for AR order `p`.
fn levinson(r: &[f64], p: usize) -> Vec<f64> {
    let mut a = vec![0.0f64; p + 1];
    a[0] = 1.0;
    let mut e = r[0];
    for i in 1..=p {
        let mut acc = r[i];
        for j in 1..i {
            acc += a[j] * r[i - j];
        }
        if e.abs() < 1e-12 {
            break;
        }
        let k = -acc / e;
        let prev: Vec<f64> = a[1..i].to_vec();
        for j in 1..i {
            a[j] = prev[j - 1] + k * prev[i - 1 - j];
        }
        a[i] = k;
        e *= 1.0 - k * k;
        if e <= 0.0 {
            break;
        }
    }
    a
}

/// Autocorrelation of the coefficient vector: `b[j] = Σ_k a[k] a[k+j]`.
fn coef_autocorr(a: &[f64], p: usize) -> Vec<f64> {
    let mut b = vec![0.0f64; p + 1];
    for (j, bj) in b.iter_mut().enumerate() {
        let mut s = 0.0;
        for k in 0..=(p - j) {
            s += a[k] * a[k + j];
        }
        *bj = s;
    }
    b
}

/// Solve the banded normal equations for the missing samples in `seg`.
fn solve_gap(seg: &mut [f64], gstart: usize, len: usize, b: &[f64], p: usize) {
    let n = seg.len();
    let mut mat = vec![vec![0.0f64; len]; len];
    let mut rhs = vec![0.0f64; len];
    // A well-fitting AR model makes the Gram matrix near-singular (its filter has
    // roots on the unit circle); a small ridge on the diagonal keeps the solve
    // well-conditioned without noticeably biasing the reconstruction.
    let ridge = 1e-6 * b[0];
    for i in 0..len {
        let mi = (gstart + i) as isize;
        for (j, row) in mat.iter_mut().enumerate() {
            let d = i.abs_diff(j);
            if d <= p {
                row[i] = b[d];
            }
        }
        mat[i][i] += ridge;
        // Known-neighbour contributions move to the right-hand side.
        let mut s = 0.0;
        for d in -(p as isize)..=(p as isize) {
            let idx = mi + d;
            if idx < 0 || idx as usize >= n {
                continue;
            }
            let gi = idx - gstart as isize;
            if gi >= 0 && (gi as usize) < len {
                continue; // unknown → stays in the matrix
            }
            s += b[d.unsigned_abs()] * seg[idx as usize];
        }
        rhs[i] = -s;
    }
    let x = solve_spd_banded(&mut mat, &rhs, p);
    for (i, xi) in x.iter().enumerate() {
        seg[gstart + i] = *xi;
    }
}

/// Banded symmetric-positive-definite solve via Cholesky (half-bandwidth `p`).
// Textbook index-based linear algebra: the `[i][k]`/`[k][i]` (row vs column)
// access pattern doesn't map cleanly onto iterators, so keep the range loops.
#[allow(clippy::needless_range_loop)]
fn solve_spd_banded(mat: &mut [Vec<f64>], rhs: &[f64], p: usize) -> Vec<f64> {
    let n = rhs.len();
    // Lower Cholesky in place, restricted to the band.
    for i in 0..n {
        for j in i.saturating_sub(p)..=i {
            let klo = i.saturating_sub(p).max(j.saturating_sub(p));
            let mut sum = mat[i][j];
            for k in klo..j {
                sum -= mat[i][k] * mat[j][k];
            }
            if i == j {
                mat[i][i] = sum.max(1e-12).sqrt();
            } else {
                mat[i][j] = sum / mat[j][j];
            }
        }
    }
    // Forward solve L y = rhs.
    let mut y = vec![0.0f64; n];
    for i in 0..n {
        let mut s = rhs[i];
        for k in i.saturating_sub(p)..i {
            s -= mat[i][k] * y[k];
        }
        y[i] = s / mat[i][i];
    }
    // Back solve Lᵀ x = y.
    let mut x = vec![0.0f64; n];
    for i in (0..n).rev() {
        let khi = (i + p).min(n - 1);
        let mut s = y[i];
        for k in (i + 1)..=khi {
            s -= mat[k][i] * x[k];
        }
        x[i] = s / mat[i][i];
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chord(sr: u32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| {
                let t = i as f32 / sr as f32;
                0.4 * (2.0 * std::f32::consts::PI * 220.0 * t).sin()
                    + 0.3 * (2.0 * std::f32::consts::PI * 440.0 * t).sin()
            })
            .collect()
    }

    #[test]
    fn reconstructs_gap_close_to_truth() {
        let sr = 48_000;
        let truth = chord(sr, 12_000);
        let (start, len) = (6_000, 480); // 10 ms gap
        let mut gapped = truth.clone();
        for s in &mut gapped[start..start + len] {
            *s = 0.0;
        }
        let filled = inpaint_gap(&gapped, start, len, 4);
        // Interior of the gap should track the original far better than the hole.
        let err = |x: &[f32]| -> f32 {
            (start..start + len).map(|i| (x[i] - truth[i]).powi(2)).sum::<f32>() / len as f32
        };
        assert!(
            err(&filled) < err(&gapped) * 0.2,
            "gap not reconstructed: {}",
            err(&filled).sqrt()
        );
        assert!(err(&filled).sqrt() < 0.15, "residual too high: {}", err(&filled).sqrt());
    }

    #[test]
    fn zero_length_is_bypass() {
        let sr = 48_000;
        let x = chord(sr, 4096);
        assert_eq!(inpaint_gap(&x, 1000, 0, 3), x);
    }

    #[test]
    fn auto_fills_detected_mute() {
        let sr = 48_000;
        let truth = chord(sr, 12_000);
        let mut gapped = truth.clone();
        for s in &mut gapped[6_000..6_300] {
            *s = 0.0;
        }
        let filled = inpaint_auto(&gapped, sr, 50.0);
        let energy: f32 = filled[6_000..6_300].iter().map(|s| s * s).sum();
        assert!(energy > 1e-3, "auto-inpaint left the mute empty");
    }
}
