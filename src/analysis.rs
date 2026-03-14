//! DSP: FFT spectrum analysis, waveform extraction, and smoothing.

use std::f32::consts::PI;

use rustfft::{num_complex::Complex, FftPlanner};

/// FFT window size. Must be a power of two.
pub const FFT_SIZE: usize = 2048;

/// Apply a Hann window to samples in-place.
fn hann_window(samples: &mut [Complex<f32>]) {
    let n = samples.len() as f32;
    for (i, s) in samples.iter_mut().enumerate() {
        let w = 0.5 * (1.0 - (2.0 * PI * i as f32 / n).cos());
        s.re *= w;
    }
}

/// Compute the magnitude spectrum from a buffer of time-domain samples.
///
/// Returns `FFT_SIZE / 2` magnitude values (positive frequencies only).
pub fn spectrum(samples: &[f32]) -> Vec<f32> {
    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(FFT_SIZE);

    let mut buffer: Vec<Complex<f32>> = samples
        .iter()
        .take(FFT_SIZE)
        .map(|&s| Complex { re: s, im: 0.0 })
        .collect();

    // Zero-pad if we have fewer samples than FFT_SIZE
    buffer.resize(FFT_SIZE, Complex { re: 0.0, im: 0.0 });

    hann_window(&mut buffer);
    fft.process(&mut buffer);

    // Positive frequencies only, convert to magnitude
    buffer[..FFT_SIZE / 2]
        .iter()
        .map(|c| c.norm() / FFT_SIZE as f32)
        .collect()
}

/// Bin a full spectrum into `n` bars using logarithmic frequency scaling.
///
/// Low frequencies get more bars, high frequencies are compressed —
/// matching human pitch perception.
pub fn bin_spectrum(magnitudes: &[f32], n: usize, sample_rate: u32) -> Vec<f32> {
    if magnitudes.is_empty() || n == 0 {
        return vec![0.0; n];
    }

    let nyquist = sample_rate as f32 / 2.0;
    let freq_per_bin = nyquist / magnitudes.len() as f32;

    // Logarithmic frequency range: 20 Hz to nyquist
    let min_freq: f32 = 20.0;
    let max_freq: f32 = nyquist.min(20_000.0);
    let log_min = min_freq.ln();
    let log_max = max_freq.ln();

    let mut bars = vec![0.0f32; n];

    for i in 0..n {
        let f_lo = ((log_min + (log_max - log_min) * i as f32 / n as f32).exp()) / freq_per_bin;
        let f_hi =
            ((log_min + (log_max - log_min) * (i + 1) as f32 / n as f32).exp()) / freq_per_bin;

        let lo = (f_lo as usize).max(0).min(magnitudes.len() - 1);
        let hi = (f_hi as usize).max(lo + 1).min(magnitudes.len());

        let sum: f32 = magnitudes[lo..hi].iter().sum();
        bars[i] = sum / (hi - lo) as f32;
    }

    bars
}

/// Smooth between consecutive frames to reduce flickering.
///
/// `factor` controls decay: 0.0 = no smoothing, 0.9 = heavy smoothing.
pub fn smooth(prev: &[f32], current: &[f32], factor: f32) -> Vec<f32> {
    prev.iter()
        .zip(current.iter())
        .map(|(p, c)| p * factor + c * (1.0 - factor))
        .collect()
}
