//! DSP: FFT spectrum analysis, waveform extraction, and smoothing.

use rustfft::{num_complex::Complex, Fft, FftPlanner};

/// Large FFT window for bass frequencies (below ~200 Hz). 4096 at 48 kHz gives
/// ~11.7 Hz bin spacing — sufficient for resolving bars down to 50 Hz (our default
/// low_freq). Halved from 8192 to cut bass latency from ~170ms to ~85ms.
pub const FFT_SIZE: usize = 4096;

/// FFT output size for mids/highs. We take only `FFT_WINDOW_SMALL` real samples
/// and zero-pad to this size, giving ~42ms time resolution with the frequency
/// bin spacing of a 4096-point FFT.
pub const FFT_SIZE_SMALL: usize = 4096;

/// Number of real samples to use for the small FFT before zero-padding.
const FFT_WINDOW_SMALL: usize = 2048;

/// Frequency below which we use the large FFT for better bass resolution.
const BASS_CROSSOVER_HZ: f32 = 200.0;

/// Cached FFT plans for reuse across frames. Holds both a large plan (for bass)
/// and a small plan (for mids/highs) to balance frequency vs time resolution.
pub struct SpectrumAnalyzer {
    fft_large: std::sync::Arc<dyn Fft<f32>>,
    fft_small: std::sync::Arc<dyn Fft<f32>>,
}

impl SpectrumAnalyzer {
    pub fn new() -> Self {
        let mut planner = FftPlanner::new();
        let fft_large = planner.plan_fft_forward(FFT_SIZE);
        let fft_small = planner.plan_fft_forward(FFT_SIZE_SMALL);
        Self {
            fft_large,
            fft_small,
        }
    }

    /// Run an FFT and return positive-frequency magnitudes.
    ///
    /// `window` controls how many real samples are used (the rest is zero-padded
    /// to `fft_size`). This lets a short analysis window produce the same number
    /// of frequency bins as a larger FFT.
    fn run_fft(
        fft: &dyn Fft<f32>,
        samples: &[f32],
        window: usize,
        fft_size: usize,
    ) -> Vec<f32> {
        let mut buffer: Vec<Complex<f32>> = samples
            .iter()
            .take(window)
            .map(|&s| Complex { re: s, im: 0.0 })
            .collect();
        buffer.resize(fft_size, Complex { re: 0.0, im: 0.0 });
        fft.process(&mut buffer);
        buffer[..fft_size / 2]
            .iter()
            .map(|c| c.norm() / fft_size as f32)
            .collect()
    }

    /// Compute dual-resolution magnitude spectra: a large FFT for bass and a
    /// smaller FFT (short window, zero-padded) for mids/highs.
    /// Returns `(bass_magnitudes, main_magnitudes)`.
    pub fn spectrum_dual(&self, samples: &[f32]) -> (Vec<f32>, Vec<f32>) {
        let bass = Self::run_fft(&*self.fft_large, samples, FFT_SIZE, FFT_SIZE);
        // Use only the most recent FFT_WINDOW_SMALL samples, zero-padded to FFT_SIZE_SMALL
        let offset = samples.len().saturating_sub(FFT_WINDOW_SMALL);
        let main = Self::run_fft(&*self.fft_small, &samples[offset..], FFT_WINDOW_SMALL, FFT_SIZE_SMALL);
        (bass, main)
    }
}

/// Bin a dual-resolution spectrum into `n` bars using logarithmic frequency scaling.
///
/// `bass_magnitudes` comes from the large FFT (better frequency resolution for bass),
/// `main_magnitudes` comes from the small FFT (better time resolution for mids/highs).
/// Bars whose center frequency is below `BASS_CROSSOVER_HZ` use the bass magnitudes;
/// the rest use the main magnitudes.
///
/// `low_freq` and `high_freq` control the visible frequency range.
pub fn bin_spectrum(
    bass_magnitudes: &[f32],
    main_magnitudes: &[f32],
    n: usize,
    sample_rate: u32,
    low_freq: f32,
    high_freq: f32,
) -> Vec<f32> {
    if (bass_magnitudes.is_empty() && main_magnitudes.is_empty()) || n == 0 {
        return vec![0.0; n];
    }

    let nyquist = sample_rate as f32 / 2.0;
    let bass_freq_per_bin = nyquist / bass_magnitudes.len().max(1) as f32;
    let main_freq_per_bin = nyquist / main_magnitudes.len().max(1) as f32;

    let min_freq = low_freq.max(1.0);
    let max_freq = high_freq.min(nyquist);
    let log_min = min_freq.ln();
    let log_max = max_freq.ln();

    let mut bars = vec![0.0f32; n];

    #[allow(clippy::needless_range_loop)]
    for i in 0..n {
        let freq_lo = (log_min + (log_max - log_min) * i as f32 / n as f32).exp();
        let freq_hi = (log_min + (log_max - log_min) * (i + 1) as f32 / n as f32).exp();
        let center_freq = (freq_lo + freq_hi) * 0.5;

        // Pick the appropriate magnitude source based on center frequency
        let (magnitudes, freq_per_bin) = if center_freq < BASS_CROSSOVER_HZ
            && !bass_magnitudes.is_empty()
        {
            (bass_magnitudes, bass_freq_per_bin)
        } else if !main_magnitudes.is_empty() {
            (main_magnitudes, main_freq_per_bin)
        } else {
            (bass_magnitudes, bass_freq_per_bin)
        };

        let f_lo = freq_lo / freq_per_bin;
        let f_hi = freq_hi / freq_per_bin;

        let lo = (f_lo as usize).min(magnitudes.len() - 1);
        let hi = (f_hi as usize).max(lo + 1).min(magnitudes.len());

        let sum: f32 = magnitudes[lo..hi].iter().sum();
        let avg = sum / (hi - lo) as f32;

        // Equalizer: boost higher frequencies to compensate for the natural
        // ~1/f roll-off in FFT magnitudes. Without this, highs look dead
        // compared to bass even at the same perceived loudness.
        let eq = (center_freq / min_freq).ln().max(0.0) + 1.0;
        bars[i] = avg * eq;
    }

    bars
}

/// Smooth between consecutive frames to reduce flickering.
/// Frame-rate independent: `factor` (0.0–0.99) is interpreted as the per-frame
/// retention at 60fps, then converted to a time constant so behavior is
/// consistent at any frame rate.
///
/// `dt` is the time since the last frame in seconds.
pub fn smooth(prev: &[f32], current: &[f32], factor: f32, dt: f32) -> Vec<f32> {
    if prev.len() != current.len() {
        return current.to_vec();
    }
    // Convert user-facing factor (assumed at 60fps) to a time constant:
    //   at 60fps, factor = e^(-dt_ref / τ)  →  τ = -dt_ref / ln(factor)
    // Then compute the actual per-frame factor: e^(-dt / τ)
    let alpha = if factor <= 0.0 {
        0.0
    } else if factor >= 1.0 {
        1.0
    } else {
        let dt_ref = 1.0 / 60.0;
        let tau = -dt_ref / factor.ln();
        (-dt / tau).exp()
    };
    prev.iter()
        .zip(current.iter())
        .map(|(p, c)| p * alpha + c * (1.0 - alpha))
        .collect()
}

/// Gravity-based bar fall-off. Bars rise instantly but fall with acceleration,
/// creating a natural-looking decay. Frame-rate independent via dt scaling.
pub struct Gravity {
    heights: Vec<f32>,
    velocities: Vec<f32>,
    /// Downward acceleration in units/s² (typical: 3.0–8.0).
    accel: f32,
}

impl Gravity {
    pub fn new(accel: f32) -> Self {
        Self {
            heights: Vec::new(),
            velocities: Vec::new(),
            accel,
        }
    }

    /// Apply gravity to bars in-place. `dt` is the time since the last frame
    /// in seconds. Bars that are rising jump to the new value; bars that are
    /// falling decelerate smoothly.
    pub fn apply(&mut self, bars: &mut [f32], dt: f32) {
        // Resize on bar count change
        if self.heights.len() != bars.len() {
            self.heights = vec![0.0; bars.len()];
            self.velocities = vec![0.0; bars.len()];
        }

        for (i, bar) in bars.iter_mut().enumerate() {
            if *bar >= self.heights[i] {
                // Rising: snap to new value, reset velocity
                self.heights[i] = *bar;
                self.velocities[i] = 0.0;
            } else {
                // Falling: v += a*dt, h -= v*dt (symplectic Euler)
                self.velocities[i] += self.accel * dt;
                self.heights[i] -= self.velocities[i] * dt;
                if self.heights[i] < 0.0 {
                    self.heights[i] = 0.0;
                    self.velocities[i] = 0.0;
                }
            }
            *bar = self.heights[i];
        }
    }
}

/// Monstercat-style smoothing: each peak spreads influence to its neighbors
/// with exponential falloff, creating a smooth connected envelope.
///
/// `strength` controls the falloff rate (0.5–0.9 typical, higher = wider spread).
pub fn monstercat(bars: &mut [f32], strength: f32) {
    let n = bars.len();
    if n < 2 {
        return;
    }

    // Forward pass: each bar pulls up its right neighbor
    for i in 1..n {
        let prev = bars[i - 1] * strength;
        if prev > bars[i] {
            bars[i] = prev;
        }
    }

    // Backward pass: each bar pulls up its left neighbor
    for i in (0..n - 1).rev() {
        let next = bars[i + 1] * strength;
        if next > bars[i] {
            bars[i] = next;
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

/// Automatic sensitivity: asymmetric gain control.
///
/// Fast attack (immediate jump when signal exceeds peak) and slow
/// multiplicative recovery, so quiet sections gradually fill the display
/// without the jittery "pumping" of a fast exponential decay.
///
/// Frame-rate independent: both attack and recovery factors are scaled by dt.
pub struct AutoSensitivity {
    peak: f32,
    /// Minimum peak to prevent division by tiny numbers during silence.
    min_peak: f32,
}

/// Per-frame recovery multiplier at 60 fps. The peak shrinks by this factor
/// each frame when the signal is quieter, so it takes many frames to recover.
/// At 60 fps: 0.99^60 ≈ 0.55 per second — recovers to half in ~1.1s.
const SENSITIVITY_RECOVERY_60: f32 = 0.99;

/// Per-frame overshoot multiplier at 60 fps. When a bar overshoots (> 1.0
/// after normalization), the peak is nudged up by this factor so future
/// frames clip less. 0.98^60 ≈ 0.30 — fairly aggressive correction.
const SENSITIVITY_OVERSHOOT_60: f32 = 0.98;

impl AutoSensitivity {
    pub fn new() -> Self {
        Self {
            peak: 0.001,
            min_peak: 0.005,
        }
    }

    /// Normalize bars in-place based on tracked peak.
    /// `dt` is the time since the last frame in seconds.
    /// Returns the current sensitivity peak for diagnostics.
    pub fn apply(&mut self, bars: &mut [f32], dt: f32) -> f32 {
        let current_max = bars.iter().cloned().fold(0.0f32, f32::max);

        // Scale the per-frame factors by dt for frame-rate independence.
        // factor_at_fps^(fps) = factor_at_fps^(1/dt_ref) should equal
        // factor^(1/dt), so: factor = factor_at_fps^(dt / dt_ref)
        let dt_ref = 1.0 / 60.0;
        let ratio = dt / dt_ref;

        if current_max > self.peak {
            // Fast attack: jump to current level immediately
            self.peak = current_max;
        } else {
            // Slow multiplicative recovery: peak shrinks gradually
            self.peak *= SENSITIVITY_RECOVERY_60.powf(ratio);
            // Don't let peak drop below the current signal
            if self.peak < current_max {
                self.peak = current_max;
            }
        }

        let peak = self.peak.max(self.min_peak);

        // Normalize and handle overshoot
        let mut overshot = false;
        for bar in bars.iter_mut() {
            *bar /= peak;
            if *bar > 1.0 {
                overshot = true;
                *bar = 1.0;
            }
        }

        // If bars clipped, nudge peak upward so it adapts
        if overshot {
            self.peak /= SENSITIVITY_OVERSHOOT_60.powf(ratio);
        }

        peak
    }
}
