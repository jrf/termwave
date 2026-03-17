import Foundation
import ScreenCaptureKit
import CoreMedia
import AVFoundation

// termwave-tap: Captures system audio via ScreenCaptureKit and writes raw f32
// PCM samples to stdout (native endian). Sends interleaved stereo (L, R, L, R...)
// by default, or mono with --mono. Requires macOS 13+.

func log(_ msg: String) {
    FileHandle.standardError.write("\(msg)\n".data(using: .utf8)!)
}

// Keep strong references so they aren't deallocated
var globalStream: SCStream?
var globalTap: AudioTap?
var globalSigSrc: DispatchSourceSignal?
var globalTermSrc: DispatchSourceSignal?

@available(macOS 13.0, *)
class AudioTap: NSObject, SCStreamOutput, SCStreamDelegate {
    let outputHandle = FileHandle.standardOutput
    let monoMode: Bool
    var samplesReceived: UInt64 = 0

    init(mono: Bool = false) {
        self.monoMode = mono
        super.init()
    }

    func stream(_ stream: SCStream, didOutputSampleBuffer sampleBuffer: CMSampleBuffer, of type: SCStreamOutputType) {
        guard type == .audio else { return }

        guard CMSampleBufferDataIsReady(sampleBuffer) else { return }
        guard let blockBuffer = CMSampleBufferGetDataBuffer(sampleBuffer) else { return }

        let length = CMBlockBufferGetDataLength(blockBuffer)
        guard length > 0 else { return }

        var data = Data(count: length)
        data.withUnsafeMutableBytes { rawBuf in
            guard let ptr = rawBuf.baseAddress else { return }
            CMBlockBufferCopyDataBytes(blockBuffer, atOffset: 0, dataLength: length, destination: ptr)
        }

        let floatCount = length / MemoryLayout<Float>.size
        let floats = data.withUnsafeBytes { buf in
            Array(buf.bindMemory(to: Float.self).prefix(floatCount))
        }

        let channels = channelCount(from: sampleBuffer)

        let output: [Float]
        if monoMode {
            // Mix down to mono
            if channels > 1 {
                output = stride(from: 0, to: floats.count - (channels - 1), by: channels).map { i in
                    var sum: Float = 0
                    for ch in 0..<channels {
                        sum += floats[i + ch]
                    }
                    return sum / Float(channels)
                }
            } else {
                output = floats
            }
        } else if channels >= 2 {
            // Send interleaved stereo (first 2 channels only)
            if channels == 2 {
                output = floats
            } else {
                // Extract just L/R from >2 channel input
                var stereo: [Float] = []
                stereo.reserveCapacity(floats.count / channels * 2)
                for i in stride(from: 0, to: floats.count - (channels - 1), by: channels) {
                    stereo.append(floats[i])
                    stereo.append(floats[i + 1])
                }
                output = stereo
            }
        } else {
            // Mono source: duplicate to stereo
            var stereo: [Float] = []
            stereo.reserveCapacity(floats.count * 2)
            for s in floats {
                stereo.append(s)
                stereo.append(s)
            }
            output = stereo
        }

        // Write raw f32 bytes to stdout
        output.withUnsafeBufferPointer { buf in
            let bytes = UnsafeRawBufferPointer(buf)
            let outData = Data(bytes)
            outputHandle.write(outData)
        }

        if samplesReceived == 0 {
            log("receiving audio (\(channels)ch → \(monoMode ? "mono" : "stereo"), \(floatCount) samples in first buffer)")
        }
        samplesReceived += UInt64(output.count)
    }

    func stream(_ stream: SCStream, didStopWithError error: Error) {
        log("stream stopped with error: \(error.localizedDescription)")
        exit(1)
    }

    private func channelCount(from sampleBuffer: CMSampleBuffer) -> Int {
        guard let formatDesc = CMSampleBufferGetFormatDescription(sampleBuffer) else {
            return 2
        }
        guard let asbd = CMAudioFormatDescriptionGetStreamBasicDescription(formatDesc) else {
            return 2
        }
        return Int(asbd.pointee.mChannelsPerFrame)
    }
}

@available(macOS 13.0, *)
func setup() async throws {
    let args = CommandLine.arguments
    let sampleRate: Double = {
        if let idx = args.firstIndex(of: "--sample-rate"), idx + 1 < args.count,
           let rate = Double(args[idx + 1]) {
            return rate
        }
        return 48000.0
    }()

    log("starting capture (sample rate: \(Int(sampleRate)))")

    let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: false)

    guard let display = content.displays.first else {
        log("error: no displays found")
        exit(1)
    }

    log("display: \(display.width)x\(display.height)")

    let filter = SCContentFilter(display: display, excludingApplications: [], exceptingWindows: [])

    let config = SCStreamConfiguration()
    config.capturesAudio = true
    config.excludesCurrentProcessAudio = true
    config.showsCursor = false
    config.width = 2
    config.height = 2
    config.minimumFrameInterval = CMTime(value: 1, timescale: 4)
    config.sampleRate = Int(sampleRate)
    config.channelCount = 2

    let monoMode = args.contains("--mono")
    let tap = AudioTap(mono: monoMode)
    let stream = SCStream(filter: filter, configuration: config, delegate: tap)

    try stream.addStreamOutput(tap, type: .screen, sampleHandlerQueue: .global(qos: .utility))
    try stream.addStreamOutput(tap, type: .audio, sampleHandlerQueue: .global(qos: .userInteractive))

    // Store in globals so they aren't deallocated
    globalTap = tap
    globalStream = stream

    log("starting stream...")
    try await stream.startCapture()
    log("stream started, waiting for audio...")

    // Signal handlers on main queue
    let sigSrc = DispatchSource.makeSignalSource(signal: SIGINT, queue: .main)
    signal(SIGINT, SIG_IGN)
    sigSrc.setEventHandler {
        log("received SIGINT, stopping...")
        Task {
            try? await globalStream?.stopCapture()
            exit(0)
        }
    }
    sigSrc.resume()
    globalSigSrc = sigSrc

    let termSrc = DispatchSource.makeSignalSource(signal: SIGTERM, queue: .main)
    signal(SIGTERM, SIG_IGN)
    termSrc.setEventHandler {
        log("received SIGTERM, stopping...")
        Task {
            try? await globalStream?.stopCapture()
            exit(0)
        }
    }
    termSrc.resume()
    globalTermSrc = termSrc

    // setup() returns — dispatchMain() in the entry point keeps the process alive
}

if #available(macOS 13.0, *) {
    Task {
        do {
            try await setup()
        } catch {
            log("error: \(error.localizedDescription)")
            exit(1)
        }
    }
    // Single dispatchMain() on the actual main thread — keeps the process alive
    // and services signal handler dispatch sources
    dispatchMain()
} else {
    log("error: requires macOS 13.0 or later")
    exit(1)
}
