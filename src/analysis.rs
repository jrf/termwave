//! DSP: FFT spectrum analysis, waveform extraction, and smoothing.

use rustfft::{num_complex::Complex, Fft, FftPlanner};

/// FFT size. 4096 at 48 kHz gives ~11.7 Hz bin spacing — sufficient for
/// resolving bars down to 50 Hz (our default low_freq). ~85ms latency.
pub const FFT_SIZE: usize = 4096;

/// Cached FFT plan for reuse across frames.
pub struct SpectrumAnalyzer {
    fft: std::sync::Arc<dyn Fft<f32>>,
}

impl SpectrumAnalyzer {
    pub fn new() -> Self {
        let mut planner = FftPlanner::new();
        Self {
            fft: planner.plan_fft_forward(FFT_SIZE),
        }
    }

    /// Compute positive-frequency magnitudes (raw, unnormalized).
    pub fn spectrum(&self, samples: &[f32]) -> Vec<f32> {
        let mut buffer: Vec<Complex<f32>> = samples
            .iter()
            .take(FFT_SIZE)
            .enumerate()
            .map(|(i, &s)| {
                // Hann window: tapers edges to zero, reducing spectral leakage
                let w = 0.5
                    * (1.0
                        - (2.0 * std::f32::consts::PI * i as f32 / (FFT_SIZE - 1) as f32)
                            .cos());
                Complex { re: s * w, im: 0.0 }
            })
            .collect();
        buffer.resize(FFT_SIZE, Complex { re: 0.0, im: 0.0 });
        self.fft.process(&mut buffer);
        buffer[..FFT_SIZE / 2]
            .iter()
            .map(|c| c.norm())
            .collect()
    }
}

/// Pre-computed bin layout for mapping FFT output to spectrum bars.
///
/// Computed once when bar count, sample rate, or frequency range changes.
/// Each frame, call `apply()` with new magnitudes to produce bars without
/// recomputing boundaries.
pub struct BinLayout {
    /// Per-bar: (bin_lo, bin_hi, eq_gain)
    ranges: Vec<(usize, usize, f32)>,
}

impl BinLayout {
    /// Compute bin layout for `n` bars over the given frequency range.
    pub fn new(n: usize, sample_rate: u32, low_freq: f32, high_freq: f32) -> Self {
        if n == 0 {
            return Self { ranges: Vec::new() };
        }

        let num_bins = FFT_SIZE / 2;
        let nyquist = sample_rate as f32 / 2.0;
        let freq_per_bin = nyquist / num_bins as f32;

        let min_freq = low_freq.max(1.0);
        let max_freq = high_freq.min(nyquist);
        let log_min = min_freq.ln();
        let log_max = max_freq.ln();

        // --- Pass 1: frequency boundaries with minimum-bandwidth separation ---
        let mut freqs: Vec<(f32, f32)> = (0..n)
            .map(|i| {
                let freq_lo = (log_min + (log_max - log_min) * i as f32 / n as f32).exp();
                let freq_hi =
                    (log_min + (log_max - log_min) * (i + 1) as f32 / n as f32).exp();
                (freq_lo, freq_hi)
            })
            .collect();

        for i in 1..n {
            if freqs[i].0 <= freqs[i - 1].0 {
                freqs[i].0 = freqs[i - 1].0 + freq_per_bin;
            }
        }

        // --- Pass 2: assign non-overlapping FFT bin ranges ---
        let mut prev_lo: usize = 0;
        let mut ranges: Vec<(usize, usize, f32)> = Vec::with_capacity(n);

        for (i, &(freq_lo, freq_hi)) in freqs.iter().enumerate() {

            let f_lo = freq_lo / freq_per_bin;
            let f_hi = freq_hi / freq_per_bin;

            let mut lo = (f_lo as usize).min(num_bins.saturating_sub(1));
            if i > 0 && lo <= prev_lo && prev_lo + 1 < num_bins {
                lo = prev_lo + 1;
                // Adjust previous bar's upper bound to prevent overlap.
                if let Some(prev) = ranges.last_mut() {
                    if prev.1 > lo {
                        prev.1 = lo;
                    }
                }
            }
            let hi = (f_hi as usize).max(lo + 1).min(num_bins);
            prev_lo = lo;

            // Equalizer: base scale brings raw f32 magnitudes into a range where
            // auto-sensitivity can converge quickly. freq^0.85 compensates for
            // the natural roll-off. log2(fft_size) normalizes for FFT size.
            const BASE_SCALE: f32 = 1.0 / 8192.0; // 1/2^13 for f32 [-1,1] input
            let eq = BASE_SCALE * freq_hi.powf(0.85) / (FFT_SIZE as f32).log2();

            ranges.push((lo, hi, eq));
        }

        Self { ranges }
    }

    /// Apply the cached layout to FFT magnitudes, producing spectrum bars.
    pub fn apply(&self, magnitudes: &[f32]) -> Vec<f32> {
        let n = self.ranges.len();
        let mut bars = vec![0.0f32; n];

        for (i, &(lo, hi, eq)) in self.ranges.iter().enumerate() {
            // Guard against degenerate ranges from aggressive anti-clumping
            let hi = hi.max(lo + 1).min(magnitudes.len());
            let lo = lo.min(magnitudes.len().saturating_sub(1));
            if lo >= hi {
                continue;
            }
            let sum: f32 = magnitudes[lo..hi].iter().sum();
            let avg = sum / (hi - lo) as f32;
            bars[i] = avg * eq;
        }

        bars
    }
}

/// Apply per-band EQ gains to spectrum bars via linear interpolation.
///
/// `eq_gains` defines N gain multipliers evenly distributed across the bar count.
/// Each bar's gain is linearly interpolated between the two nearest EQ bands.
/// Values > 1.0 boost, < 1.0 cut, 1.0 = no change.
pub fn apply_eq(bars: &mut [f32], eq_gains: &[f32]) {
    if eq_gains.is_empty() || bars.is_empty() {
        return;
    }
    if eq_gains.len() == 1 {
        if eq_gains[0] != 1.0 {
            for bar in bars.iter_mut() {
                *bar *= eq_gains[0];
            }
        }
        return;
    }

    let n = bars.len();
    let bands = eq_gains.len();

    for (i, bar) in bars.iter_mut().enumerate() {
        // Map bar index to a position in the EQ band array
        let pos = i as f32 * (bands - 1) as f32 / (n - 1).max(1) as f32;
        let lo = (pos as usize).min(bands - 2);
        let frac = pos - lo as f32;
        let gain = eq_gains[lo] * (1.0 - frac) + eq_gains[lo + 1] * frac;
        *bar *= gain;
    }
}

/// Integral smoothing: additive memory accumulation.
///
/// Each frame: `out = mem * alpha + current`, then `mem = out`.
/// `noise_reduction` is 0.0–1.0. Higher = more memory = smoother.
/// `alpha` is `noise_reduction^(60/fps)` for framerate-independent decay.
///
/// Energy accumulates via the integral, and auto-sensitivity normalizes it
/// back by adjusting the gain multiplier.
pub fn smooth(mem: &mut Vec<f32>, bars: &mut [f32], noise_reduction: f32, framerate: f32) {
    if mem.len() != bars.len() {
        *mem = bars.to_vec();
        return;
    }
    let alpha = noise_reduction.powf(60.0 / framerate);

    for (i, bar) in bars.iter_mut().enumerate() {
        *bar += mem[i] * alpha;
        mem[i] = *bar;
    }
}

/// Parabolic bar fall-off. Bars rise instantly to new peaks, then
/// fall along a parabolic curve: `peak * (1 - fall² * gravity_mod)`.
///
/// `fall` increments proportionally to elapsed time each frame, so the
/// decay takes the same wall-clock time regardless of framerate.
pub struct Gravity {
    peaks: Vec<f32>,
    falls: Vec<f32>,
    prev: Vec<f32>,
}

/// Per-frame fall increment at 60fps.
const FALL_STEP_60: f32 = 0.028;

impl Gravity {
    pub fn new() -> Self {
        Self {
            peaks: Vec::new(),
            falls: Vec::new(),
            prev: Vec::new(),
        }
    }

    /// Apply parabolic falloff to bars in-place.
    /// `framerate` is the target FPS (used for framerate-independent scaling).
    /// `noise_reduction` controls how quickly bars fall (higher = slower).
    pub fn apply(&mut self, bars: &mut [f32], framerate: f32, noise_reduction: f32) {
        if self.peaks.len() != bars.len() {
            self.peaks = vec![0.0; bars.len()];
            self.falls = vec![0.0; bars.len()];
            self.prev = vec![0.0; bars.len()];
        }

        // Scale fall step so that fall accumulates at the same rate per second
        // regardless of FPS: at 60fps, step = FALL_STEP_60; at 30fps, step = 2×.
        let fall_step = FALL_STEP_60 * 60.0 / framerate;
        // gravity_mod no longer needs framerate compensation since fall itself
        // is now time-proportional.
        let gravity_mod = 2.0 / noise_reduction.max(0.1);

        for (i, bar) in bars.iter_mut().enumerate() {
            if *bar < self.prev[i] && noise_reduction > 0.1 {
                // Falling: parabolic decay from peak
                *bar = self.peaks[i] * (1.0 - self.falls[i] * self.falls[i] * gravity_mod);
                if *bar < 0.0 {
                    *bar = 0.0;
                }
                self.falls[i] += fall_step;
            } else {
                // Rising or equal: snap to new peak
                self.peaks[i] = *bar;
                self.falls[i] = 0.0;
            }
            self.prev[i] = *bar;
        }
    }
}

/// Monstercat smoothing: each bar spreads influence to ALL other
/// bars with exponential distance-based falloff.
///
/// `monstercat` is the falloff parameter (typical: 1.0–2.0). The divisor
/// base is `monstercat * 1.5`, so higher values = steeper falloff = less spread.
/// O(n²) but n is typically < 200 bars so this is fine at audio framerates.
pub fn monstercat(bars: &mut [f32], monstercat: f32) {
    let n = bars.len();
    if n < 2 {
        return;
    }
    let base = monstercat * 1.5;

    for z in 0..n {
        // Spread left
        for m_y in (0..z).rev() {
            let de = (z - m_y) as f32;
            let spread = bars[z] / base.powf(de);
            if spread > bars[m_y] {
                bars[m_y] = spread;
            }
        }
        // Spread right
        for m_y in (z + 1)..n {
            let de = (m_y - z) as f32;
            let spread = bars[z] / base.powf(de);
            if spread > bars[m_y] {
                bars[m_y] = spread;
            }
        }
    }
}

/// Apply a noise floor — zero out any bar below the threshold.
pub fn noise_gate(bars: &mut [f32], floor: f32) {
    for bar in bars.iter_mut() {
        if *bar < floor {
            *bar = 0.0;
        }
    }
}

/// Automatic sensitivity: multiplicative gain adjustment.
///
/// Sensitivity (`sens`) scales all bars. When any bar exceeds 1.0 (overshoot),
/// sens is reduced by 2% per frame. When no overshoot occurs, sens grows by
/// 1% per frame (with a 10% boost during initial ramp-up). All rates are
/// scaled for framerate independence.
///
/// Split into two phases:
///   1. `scale()` — multiply bars by current sens (before gravity/integral)
///   2. `adjust()` — clamp overshoots and update sens (after gravity/integral)
pub struct AutoSensitivity {
    pub sens: f32,
    sens_init: bool,
}

impl AutoSensitivity {
    pub fn new() -> Self {
        Self {
            sens: 1.0,
            sens_init: true,
        }
    }

    /// Phase 1: scale bars by current sensitivity. Call before gravity/integral.
    pub fn scale(&self, bars: &mut [f32]) {
        for bar in bars.iter_mut() {
            *bar *= self.sens;
        }
    }

    /// Phase 2: clamp overshoots and adjust sens for next frame.
    /// Call after gravity and integral smoothing.
    ///
    /// Per-frame factors use `base^(60/fps)` so the per-second rate is
    /// constant regardless of framerate.
    pub fn adjust(&mut self, bars: &mut [f32], framerate: f32, silence: bool) {
        let time_scale = 60.0 / framerate;

        let mut max_bar: f32 = 0.0;
        for bar in bars.iter() {
            if *bar > max_bar {
                max_bar = *bar;
            }
        }

        if max_bar > 1.0 {
            // Proportional attack: if bars overshoot by 5×, cut sens by 5×.
            // Blend with a minimum 2% cut so mild overshoots still converge.
            let reduction = (1.0 / max_bar).max(0.98_f32.powf(time_scale));
            self.sens *= reduction;
            self.sens_init = false;

            for bar in bars.iter_mut() {
                if *bar > 1.0 {
                    *bar = 1.0;
                }
            }
        } else if !silence {
            self.sens *= 1.01_f32.powf(time_scale);
            if self.sens_init {
                self.sens *= 1.1_f32.powf(time_scale);
            }
        }

        // Cap sensitivity to prevent runaway gain during long silences.
        const MAX_SENS: f32 = 100.0;
        if self.sens > MAX_SENS {
            self.sens = MAX_SENS;
        }
    }
}
