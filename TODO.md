# sonitus — TODO

## Phase 1: Core pipeline
- [x] Audio capture: open default input device with cpal, write samples to shared ring buffer
- [x] Spectrum rendering: draw frequency bars using ratatui BarChart
- [x] Wire main loop: audio thread -> ring buffer -> FFT -> render at ~30fps
- [x] Handle terminal resize gracefully
- [x] Quit on `q`, Esc, or Ctrl+C with clean terminal restore

## Phase 2: Visuals
- [x] Apply Hann window before FFT
- [x] Logarithmic frequency binning (more bars for bass, fewer for treble)
- [x] Frame smoothing (exponential decay between frames)
- [x] Color gradient on spectrum bars (blue -> cyan -> green -> yellow -> red)
- [x] Waveform mode: plot amplitude across terminal width
- [x] Oscilloscope mode: zero-crossing triggered waveform display
- [ ] Braille character rendering for sub-cell resolution

## Phase 3: macOS system audio
- [x] ScreenCaptureKit integration (macOS 13+) for capturing system audio output
- [x] Device enumeration and selection via `--device` flag / `--list-devices`
- [x] Runtime device switching via `d` keybinding
- [ ] Fallback to virtual audio device (BlackHole) if ScreenCaptureKit unavailable

## Phase 4: Polish
- [ ] Peak hold indicators on spectrum bars
- [ ] Configurable color themes
- [ ] Frequency axis labels (Hz)
- [ ] FPS / latency debug overlay (`--debug` flag)
- [ ] Stereo: side-by-side or overlaid L/R channels
