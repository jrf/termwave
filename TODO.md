# termwave — TODO

## Now
- [ ] Peak hold indicators on spectrum bars
- [ ] Braille character rendering for sub-cell resolution

## Next
- [ ] Frequency axis labels (Hz)
- [ ] FPS / latency debug overlay (`--debug` flag)

## Later
- [ ] Linux support (PulseAudio/PipeWire monitor sources for system audio capture)
- [ ] Fallback to virtual audio device (BlackHole) if ScreenCaptureKit unavailable

## Scrapped
- Hann window before FFT — removed because it attenuates samples at buffer edges, adding latency for precision that's invisible in a visualizer. Monstercat smoothing and the equalizer curve handle the artifacts.

## Done
- [x] Dual-resolution FFT (4096 for bass, 2048→4096 zero-padded for mids/highs) #improvement
- [x] Asymmetric auto-sensitivity: fast attack, slow multiplicative recovery #improvement
- [x] Narrower default frequency range (50–10000 Hz) #improvement
- [x] Frequency equalizer curve to compensate for FFT high-frequency roll-off #improvement
- [x] Fix smoothing scale mismatch: moved smoothing after auto-sensitivity normalization #bug
- [x] Monstercat smoothing on by default #improvement
- [x] Settings overlay uses 80% terminal width (min 40 cols) instead of 50% #bug
- [x] Left/right arrows toggle Monstercat in settings overlay #bug
- [x] Faster gravity fall-off (5.0 → 8.0 accel) for snappier visual response #improvement
- [x] Fix mic audio bleeding into system capture after device switch — explicitly pause cpal stream before drop #bug
- [x] Fix orphaned `termwave-tap` process after sleep/wake or unclean shutdown — watchdog timer in tap + signal handler in Rust #bug
- [x] Refactor: extract `VisualizerState`, `AudioState`, `RenderContext` structs; deduplicate DSP pipeline and buffer resets; break up 432-line main() into focused functions #refactor
- [x] Audio capture: open default input device with cpal, write samples to shared ring buffer
- [x] Spectrum rendering: draw frequency bars using ratatui BarChart
- [x] Wire main loop: audio thread -> ring buffer -> FFT -> render at ~30fps
- [x] Handle terminal resize gracefully
- [x] Quit on `q`, Esc, or Ctrl+C with clean terminal restore
- [x] Logarithmic frequency binning (more bars for bass, fewer for treble)
- [x] Frame smoothing (exponential decay between frames)
- [x] Color gradient on spectrum bars
- [x] Waveform mode: plot amplitude across terminal width
- [x] Oscilloscope mode: zero-crossing triggered waveform display
- [x] ScreenCaptureKit integration (macOS 13+) for capturing system audio output
- [x] Device enumeration and selection via `--device` flag / `--list-devices`
- [x] Runtime device switching via `d` keybinding
- [x] Configurable color themes (8 built-in: classic, fire, ocean, purple, matrix, synthwave, tokyo-night-moon, mono)
- [x] Externalize theme colors to TOML files in `~/.config/termwave/themes/`
- [x] Stereo mode: mirrored L/R bars growing up and down from center with half-cell precision
- [x] Now-playing Apple Music track display in status bar
- [x] Bar width and spacing customization (settings menu + CLI flags)
- [x] Sensitivity control with title bar display
- [x] Config persistence to `~/.config/termwave/config.toml`
- [x] Theme-aware UI: all menus, borders, titles, and overlays use the current theme's colors
- [x] Non-blocking settings overlay: settings panel renders at 50% terminal size with live visualizer behind it, changes apply in real time
