// AVFoundation media decoding: the Apple MediaBackend. Audio comes out as
// interleaved stereo float PCM at 48 kHz, video as RGBA8 frames downscaled
// to <= 480 px wide at 15 fps, both capped at 90 s (the same limits as the
// desktop ffmpeg backend in crates/core/src/media.rs), then handed to the
// engine with cg_instrument_post_source_pcm_rgba.

import AVFoundation
import Accelerate
import CoreMedia
import Foundation

struct DecodedMedia {
    var name: String
    var pcm: [Float] = [] // interleaved stereo
    var sampleRate: Double = MediaDecoder.sampleRate
    var rgba: [UInt8] = []
    var width = 0
    var height = 0
    var fps: Float = Float(MediaDecoder.fps)

    var frames: Int { width * height > 0 ? rgba.count / (width * height * 4) : 0 }
    var seconds: Double {
        max(Double(pcm.count / 2) / sampleRate, Double(frames) / Double(fps))
    }

    /// Queue on the engine (thread-safe). False if its queue is full.
    func post(to engine: OpaquePointer) -> Bool {
        pcm.withUnsafeBufferPointer { p in
            rgba.withUnsafeBufferPointer { v in
                name.withCString { n in
                    cg_instrument_post_source_pcm_rgba(engine, n, p.baseAddress, p.count, 2, UInt32(sampleRate),
                                                       v.baseAddress, v.count, UInt32(width), UInt32(height), fps, 0)
                }
            }
        }
    }
}

enum MediaDecoderError: LocalizedError {
    case nothingDecodable
    case reader(String)

    var errorDescription: String? {
        switch self {
        case .nothingDecodable: return "no decodable audio or video"
        case .reader(let s): return s
        }
    }
}

enum MediaDecoder {
    static let sampleRate: Double = 48_000
    static let maxSeconds: Double = 90
    static let fps: Double = 15
    static let maxWidth: Double = 480

    static func decode(url: URL) async throws -> DecodedMedia {
        let asset = AVURLAsset(url: url)
        var media = DecodedMedia(name: url.lastPathComponent)
        if let track = try await asset.loadTracks(withMediaType: .audio).first {
            media.pcm = try readAudio(asset: asset, track: track)
        }
        if let track = try await asset.loadTracks(withMediaType: .video).first {
            let size = try await track.load(.naturalSize)
            (media.rgba, media.width, media.height) = try readVideo(asset: asset, track: track, size: size)
        }
        if media.pcm.isEmpty && media.rgba.isEmpty {
            throw MediaDecoderError.nothingDecodable
        }
        return media
    }

    private static var range: CMTimeRange {
        CMTimeRange(start: .zero, duration: CMTime(seconds: maxSeconds, preferredTimescale: 600))
    }

    static func readAudio(asset: AVAsset, track: AVAssetTrack) throws -> [Float] {
        let reader = try AVAssetReader(asset: asset)
        reader.timeRange = range
        let output = AVAssetReaderTrackOutput(track: track, outputSettings: [
            AVFormatIDKey: kAudioFormatLinearPCM,
            AVLinearPCMBitDepthKey: 32,
            AVLinearPCMIsFloatKey: true,
            AVLinearPCMIsNonInterleaved: false,
            AVLinearPCMIsBigEndianKey: false,
            AVSampleRateKey: sampleRate,
            AVNumberOfChannelsKey: 2,
        ])
        reader.add(output)
        guard reader.startReading() else { throw MediaDecoderError.reader(reader.error?.localizedDescription ?? "audio reader failed") }
        var pcm: [Float] = []
        while let sample = output.copyNextSampleBuffer() {
            guard let block = CMSampleBufferGetDataBuffer(sample) else { continue }
            let bytes = CMBlockBufferGetDataLength(block)
            let start = pcm.count
            pcm.append(contentsOf: repeatElement(0, count: bytes / 4))
            _ = pcm.withUnsafeMutableBytes { raw in
                CMBlockBufferCopyDataBytes(block, atOffset: 0, dataLength: bytes, destination: raw.baseAddress! + start * 4)
            }
        }
        if reader.status == .failed { throw MediaDecoderError.reader(reader.error?.localizedDescription ?? "audio decode failed") }
        return pcm
    }

    static func readVideo(asset: AVAsset, track: AVAssetTrack, size: CGSize) throws -> ([UInt8], Int, Int) {
        let scale = min(1, maxWidth / max(1, Double(size.width)))
        let w = max(2, Int(Double(size.width) * scale)) & ~1
        let h = max(2, Int(Double(size.height) * scale)) & ~1
        let reader = try AVAssetReader(asset: asset)
        reader.timeRange = range
        let output = AVAssetReaderTrackOutput(track: track, outputSettings: [
            kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA,
        ])
        output.alwaysCopiesSampleData = false
        reader.add(output)
        guard reader.startReading() else { throw MediaDecoderError.reader(reader.error?.localizedDescription ?? "video reader failed") }

        var rgba: [UInt8] = []
        var frame = [UInt8](repeating: 0, count: w * h * 4)
        var next = 0.0
        let step = 1 / fps
        while let sample = output.copyNextSampleBuffer() {
            let t = CMSampleBufferGetPresentationTimeStamp(sample).seconds
            guard t + 1e-6 >= next, let pixels = CMSampleBufferGetImageBuffer(sample) else { continue }
            next += step
            while next <= t { next += step } // dropped source frames: stay on the grid
            CVPixelBufferLockBaseAddress(pixels, .readOnly)
            defer { CVPixelBufferUnlockBaseAddress(pixels, .readOnly) }
            guard let base = CVPixelBufferGetBaseAddress(pixels) else { continue }
            var src = vImage_Buffer(data: base, height: vImagePixelCount(CVPixelBufferGetHeight(pixels)),
                                    width: vImagePixelCount(CVPixelBufferGetWidth(pixels)),
                                    rowBytes: CVPixelBufferGetBytesPerRow(pixels))
            frame.withUnsafeMutableBytes { raw in
                var dst = vImage_Buffer(data: raw.baseAddress, height: vImagePixelCount(h), width: vImagePixelCount(w), rowBytes: w * 4)
                vImageScale_ARGB8888(&src, &dst, nil, vImage_Flags(kvImageHighQualityResampling))
                let bgraToRgba: [UInt8] = [2, 1, 0, 3]
                vImagePermuteChannels_ARGB8888(&dst, &dst, bgraToRgba, vImage_Flags(kvImageNoFlags))
            }
            rgba.append(contentsOf: frame)
        }
        if reader.status == .failed { throw MediaDecoderError.reader(reader.error?.localizedDescription ?? "video decode failed") }
        return (rgba, rgba.isEmpty ? 0 : w, rgba.isEmpty ? 0 : h)
    }
}
