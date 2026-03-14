//! Audio capture from system input devices via cpal, or from sonitus-tap subprocess.

use std::io::{BufRead, BufReader, Read as _};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::Stream;

/// Shared sample buffer between audio capture thread and render thread.
pub type SampleBuffer = Arc<Mutex<Vec<f32>>>;

/// Create a new shared sample buffer.
pub fn new_buffer(capacity: usize) -> SampleBuffer {
    Arc::new(Mutex::new(vec![0.0; capacity]))
}

/// Handle for an active audio capture — either a cpal stream or a tap subprocess.
/// The Stream/Child are held to keep them alive; they're used via Drop, not read.
#[allow(dead_code)]
pub enum CaptureHandle {
    Device(Stream),
    Tap(Child),
}

impl Drop for CaptureHandle {
    fn drop(&mut self) {
        if let CaptureHandle::Tap(ref mut child) = self {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Start capturing audio into the shared buffer from a device.
///
/// Returns the sample rate and a handle that must be kept alive.
pub fn start_capture(
    buffer: SampleBuffer,
    device_name: Option<&str>,
) -> Result<(u32, CaptureHandle)> {
    let host = cpal::default_host();

    let device = match device_name {
        Some(name) => host
            .input_devices()?
            .find(|d| d.name().map(|n| n == name).unwrap_or(false))
            .context(format!("audio device '{}' not found", name))?,
        None => host
            .default_input_device()
            .context("no default input device available")?,
    };

    // Clear the buffer before starting a new source
    {
        let mut buf = buffer.lock().unwrap();
        buf.fill(0.0);
    }

    let config = device.default_input_config()?;
    let sample_rate = config.sample_rate().0;
    let channels = config.channels() as usize;

    let buf = buffer.clone();
    let err_fn = |err: cpal::StreamError| {
        eprintln!("audio stream error: {}", err);
    };

    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &config.into(),
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                write_samples(&buf, data, channels);
            },
            err_fn,
            None,
        )?,
        cpal::SampleFormat::I16 => {
            let buf = buffer.clone();
            device.build_input_stream(
                &config.into(),
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    let floats: Vec<f32> = data
                        .iter()
                        .map(|&s| s as f32 / i16::MAX as f32)
                        .collect();
                    write_samples(&buf, &floats, channels);
                },
                err_fn,
                None,
            )?
        }
        cpal::SampleFormat::U16 => {
            let buf = buffer.clone();
            device.build_input_stream(
                &config.into(),
                move |data: &[u16], _: &cpal::InputCallbackInfo| {
                    let floats: Vec<f32> = data
                        .iter()
                        .map(|&s| (s as f32 / u16::MAX as f32) * 2.0 - 1.0)
                        .collect();
                    write_samples(&buf, &floats, channels);
                },
                err_fn,
                None,
            )?
        }
        _ => anyhow::bail!("unsupported sample format"),
    };

    stream.play()?;
    Ok((sample_rate, CaptureHandle::Device(stream)))
}

/// Start capturing system audio via sonitus-tap subprocess.
///
/// Looks for `sonitus-tap` in the same directory as the current executable,
/// then falls back to PATH.
pub fn start_tap(buffer: SampleBuffer, sample_rate: u32) -> Result<(u32, CaptureHandle)> {
    let tap_bin = find_tap_binary();

    // Clear the buffer before starting a new source
    {
        let mut buf = buffer.lock().unwrap();
        buf.fill(0.0);
    }

    let mut child = Command::new(&tap_bin)
        .args(["--sample-rate", &sample_rate.to_string()])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context(format!(
            "failed to start sonitus-tap (looked for '{}'). \
             Build it with: cd tap && swift build -c release",
            tap_bin
        ))?;

    let mut stdout = child.stdout.take().context("failed to get tap stdout")?;
    let stderr = child.stderr.take();

    // Drain stderr in a background thread so the subprocess doesn't block.
    // Discard output — we're in a TUI and can't write to stderr without corrupting it.
    if let Some(stderr) = stderr {
        thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines() {
                if line.is_err() {
                    break;
                }
            }
        });
    }

    // Read thread: consume raw f32 samples from the subprocess stdout
    thread::spawn(move || {
        let mut read_buf = [0u8; 4096];
        loop {
            match stdout.read(&mut read_buf) {
                Ok(0) => break,
                Ok(n) => {
                    let sample_bytes = &read_buf[..n - (n % 4)];
                    let samples: Vec<f32> = sample_bytes
                        .chunks_exact(4)
                        .map(|chunk| f32::from_ne_bytes(chunk.try_into().unwrap()))
                        .collect();

                    if !samples.is_empty() {
                        write_samples_mono(&buffer, &samples);
                    }
                }
                Err(_) => break,
            }
        }
    });

    Ok((sample_rate, CaptureHandle::Tap(child)))
}

/// Find the sonitus-tap binary: check next to our own executable first, then PATH.
fn find_tap_binary() -> String {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("sonitus-tap");
            if candidate.exists() {
                return candidate.to_string_lossy().into_owned();
            }
        }
    }
    "sonitus-tap".to_string()
}

/// Write pre-mono-mixed samples into the ring buffer.
fn write_samples_mono(buffer: &SampleBuffer, samples: &[f32]) {
    let mut buf = buffer.lock().unwrap();
    let capacity = buf.len();
    let new_len = samples.len();

    if new_len >= capacity {
        buf.copy_from_slice(&samples[new_len - capacity..]);
    } else {
        buf.rotate_left(new_len);
        buf[capacity - new_len..].copy_from_slice(samples);
    }
}

/// Write incoming samples into the ring buffer (mono-mix if stereo).
fn write_samples(buffer: &SampleBuffer, data: &[f32], channels: usize) {
    let mono: Vec<f32> = data
        .chunks(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect();

    write_samples_mono(buffer, &mono);
}

/// List available input device names.
pub fn list_devices() -> Result<Vec<String>> {
    let host = cpal::default_host();
    let mut devices: Vec<String> = host
        .input_devices()?
        .filter_map(|d| d.name().ok())
        .collect();

    // Add system audio option if sonitus-tap binary exists
    let tap_bin = find_tap_binary();
    if std::path::Path::new(&tap_bin).exists()
        || which_exists(&tap_bin)
    {
        devices.insert(0, SYSTEM_AUDIO_LABEL.to_string());
    }

    Ok(devices)
}

/// Check if a binary name exists on PATH.
fn which_exists(name: &str) -> bool {
    Command::new("which")
        .arg(name)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Label used in the device menu for system audio capture.
pub const SYSTEM_AUDIO_LABEL: &str = "System Audio (ScreenCaptureKit)";
