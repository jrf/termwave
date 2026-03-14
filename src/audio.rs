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

/// Stereo sample buffers (left, right).
pub type StereoPair = (SampleBuffer, SampleBuffer);

/// Create a new shared sample buffer.
pub fn new_buffer(capacity: usize) -> SampleBuffer {
    Arc::new(Mutex::new(vec![0.0; capacity]))
}

/// Create a stereo pair of buffers.
pub fn new_stereo_buffers(capacity: usize) -> StereoPair {
    (new_buffer(capacity), new_buffer(capacity))
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

/// Start capturing audio into the shared buffers from a device.
///
/// Writes mono-mixed data to `mono_buf`, and separate L/R to `stereo`.
pub fn start_capture(
    mono_buf: SampleBuffer,
    stereo: StereoPair,
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

    clear_buffers(&mono_buf, &stereo);

    let config = device.default_input_config()?;
    let sample_rate = config.sample_rate().0;
    let channels = config.channels() as usize;

    let err_fn = |err: cpal::StreamError| {
        eprintln!("audio stream error: {}", err);
    };

    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => {
            let mb = mono_buf.clone();
            let st = (stereo.0.clone(), stereo.1.clone());
            device.build_input_stream(
                &config.into(),
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    write_samples(&mb, &st, data, channels);
                },
                err_fn,
                None,
            )?
        }
        cpal::SampleFormat::I16 => {
            let mb = mono_buf.clone();
            let st = (stereo.0.clone(), stereo.1.clone());
            device.build_input_stream(
                &config.into(),
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    let floats: Vec<f32> = data
                        .iter()
                        .map(|&s| s as f32 / i16::MAX as f32)
                        .collect();
                    write_samples(&mb, &st, &floats, channels);
                },
                err_fn,
                None,
            )?
        }
        cpal::SampleFormat::U16 => {
            let mb = mono_buf.clone();
            let st = (stereo.0.clone(), stereo.1.clone());
            device.build_input_stream(
                &config.into(),
                move |data: &[u16], _: &cpal::InputCallbackInfo| {
                    let floats: Vec<f32> = data
                        .iter()
                        .map(|&s| (s as f32 / u16::MAX as f32) * 2.0 - 1.0)
                        .collect();
                    write_samples(&mb, &st, &floats, channels);
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
pub fn start_tap(
    mono_buf: SampleBuffer,
    stereo: StereoPair,
    sample_rate: u32,
) -> Result<(u32, CaptureHandle)> {
    let tap_bin = find_tap_binary();

    clear_buffers(&mono_buf, &stereo);

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

    // The tap outputs interleaved stereo (L, R, L, R...).
    // Split into L/R channels and mix to mono.
    thread::spawn(move || {
        let mut read_buf = [0u8; 4096];
        loop {
            match stdout.read(&mut read_buf) {
                Ok(0) => break,
                Ok(n) => {
                    // Align to 8 bytes (one stereo frame = 2 × f32 = 8 bytes)
                    let sample_bytes = &read_buf[..n - (n % 8)];
                    let samples: Vec<f32> = sample_bytes
                        .chunks_exact(4)
                        .map(|chunk| f32::from_ne_bytes(chunk.try_into().unwrap()))
                        .collect();

                    if samples.len() >= 2 {
                        let mut mono = Vec::with_capacity(samples.len() / 2);
                        let mut left = Vec::with_capacity(samples.len() / 2);
                        let mut right = Vec::with_capacity(samples.len() / 2);

                        for frame in samples.chunks_exact(2) {
                            let l = frame[0];
                            let r = frame[1];
                            mono.push((l + r) / 2.0);
                            left.push(l);
                            right.push(r);
                        }

                        write_to_buffer(&mono_buf, &mono);
                        write_to_buffer(&stereo.0, &left);
                        write_to_buffer(&stereo.1, &right);
                    }
                }
                Err(_) => break,
            }
        }
    });

    Ok((sample_rate, CaptureHandle::Tap(child)))
}

fn clear_buffers(mono: &SampleBuffer, stereo: &StereoPair) {
    mono.lock().unwrap().fill(0.0);
    stereo.0.lock().unwrap().fill(0.0);
    stereo.1.lock().unwrap().fill(0.0);
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

/// Write samples into a single ring buffer.
fn write_to_buffer(buffer: &SampleBuffer, samples: &[f32]) {
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

/// Write incoming interleaved samples: mono-mix to mono_buf, split L/R to stereo.
fn write_samples(
    mono_buf: &SampleBuffer,
    stereo: &StereoPair,
    data: &[f32],
    channels: usize,
) {
    let mut mono = Vec::with_capacity(data.len() / channels);
    let mut left = Vec::with_capacity(data.len() / channels);
    let mut right = Vec::with_capacity(data.len() / channels);

    for frame in data.chunks(channels) {
        let l = frame[0];
        let r = if channels > 1 { frame[1] } else { l };
        mono.push((l + r) / 2.0);
        left.push(l);
        right.push(r);
    }

    write_to_buffer(mono_buf, &mono);
    write_to_buffer(&stereo.0, &left);
    write_to_buffer(&stereo.1, &right);
}

/// List available input device names.
pub fn list_devices() -> Result<Vec<String>> {
    let host = cpal::default_host();
    let mut devices: Vec<String> = host
        .input_devices()?
        .filter_map(|d| d.name().ok())
        .collect();

    let tap_bin = find_tap_binary();
    if std::path::Path::new(&tap_bin).exists() || which_exists(&tap_bin) {
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
