// Apple Vision sidecar for chromagrain: on-device video segmentation on
// macOS, no Python / torch / model download. Drop-in for tools/segment.py.
//
// Build (chromagrain does this automatically, caching the binary):
//     swiftc -O tools/segment_vision.swift -o segment_vision
//
// Usage:
//     segment_vision <video.mp4> "<prompt>" <out_dir>
//     segment_vision --check
//
// Contract (same as tools/segment.py):
//   * one grayscale PGM (P5, maxval 255) per decoded video frame, named
//     mask_00000.pgm, mask_00001.pgm, ... in <out_dir>, at the frame's
//     resolution; 255 = keep. Edges are soft (0..255) — chromagrain bakes
//     the mask straight into alpha, so soft values become soft edges.
//   * exit 0 with "ok <n_frames>" as the last stdout line on success;
//     nonzero with a message on stderr on failure.
//   * --check exits 0 iff Vision segmentation is usable.
//
// Prompt -> mode:
//   * empty, or mentions a person word (person, people, man, dancer, face,
//     crowd, ...) -> VNGeneratePersonSegmentationRequest (all people).
//   * anything else ("subject", "cat", "car") -> VNGenerateForegroundInstance
//     MaskRequest (macOS 14+), union of every salient foreground instance.
//     Vision can't filter by class, so the prompt only picks the mode.

import AVFoundation
import CoreVideo
import Foundation
import Vision

func fail(_ msg: String, _ code: Int32 = 1) -> Never {
    FileHandle.standardError.write((msg + "\n").data(using: .utf8)!)
    exit(code)
}

func log(_ msg: String) {
    FileHandle.standardError.write((msg + "\n").data(using: .utf8)!)
}

let personWords: Set<String> = [
    "person", "persons", "people", "human", "humans", "man", "men", "woman",
    "women", "boy", "boys", "girl", "girls", "kid", "kids", "child",
    "children", "dancer", "dancers", "face", "faces", "body", "bodies",
    "everyone", "everybody", "crowd", "guy", "guys", "lady", "ladies",
    "figure", "figures", "someone", "performer", "performers", "singer",
]

func wantsPerson(_ prompt: String) -> Bool {
    let words = prompt.lowercased()
        .components(separatedBy: CharacterSet.letters.inverted)
        .filter { !$0.isEmpty }
    if words.isEmpty { return true }
    return words.contains { personWords.contains($0) }
}

// ------------------------------------------------------------ mask access

/// Sample a Vision mask buffer (OneComponent8 or OneComponent32Float) into
/// a w*h float [0,1] grid with bilinear filtering.
func sampleMask(_ buf: CVPixelBuffer, width w: Int, height h: Int) -> [Float] {
    CVPixelBufferLockBaseAddress(buf, .readOnly)
    defer { CVPixelBufferUnlockBaseAddress(buf, .readOnly) }
    let mw = CVPixelBufferGetWidth(buf)
    let mh = CVPixelBufferGetHeight(buf)
    let stride = CVPixelBufferGetBytesPerRow(buf)
    let fmt = CVPixelBufferGetPixelFormatType(buf)
    guard let base = CVPixelBufferGetBaseAddress(buf) else {
        return [Float](repeating: 0, count: w * h)
    }
    let isFloat = fmt == kCVPixelFormatType_OneComponent32Float
    @inline(__always) func at(_ x: Int, _ y: Int) -> Float {
        let row = base + y * stride
        if isFloat {
            return row.assumingMemoryBound(to: Float.self)[x]
        }
        return Float(row.assumingMemoryBound(to: UInt8.self)[x]) / 255
    }
    var out = [Float](repeating: 0, count: w * h)
    let sx = Float(mw) / Float(w)
    let sy = Float(mh) / Float(h)
    for y in 0..<h {
        let fy = max(0, (Float(y) + 0.5) * sy - 0.5)
        let y0 = min(Int(fy), mh - 1)
        let y1 = min(y0 + 1, mh - 1)
        let ty = fy - Float(y0)
        for x in 0..<w {
            let fx = max(0, (Float(x) + 0.5) * sx - 0.5)
            let x0 = min(Int(fx), mw - 1)
            let x1 = min(x0 + 1, mw - 1)
            let tx = fx - Float(x0)
            let top = at(x0, y0) * (1 - tx) + at(x1, y0) * tx
            let bot = at(x0, y1) * (1 - tx) + at(x1, y1) * tx
            out[y * w + x] = top * (1 - ty) + bot * ty
        }
    }
    return out
}

func writePGM(_ path: String, _ data: [UInt8], _ w: Int, _ h: Int) throws {
    var bytes = Data("P5\n\(w) \(h)\n255\n".utf8)
    bytes.append(contentsOf: data)
    try bytes.write(to: URL(fileURLWithPath: path))
}

// ------------------------------------------------------------ segmenters

protocol Segmenter {
    func mask(_ frame: CVPixelBuffer, _ w: Int, _ h: Int) throws -> [Float]
}

/// Nearest-sample a UInt8 label buffer (VNInstanceMaskObservation
/// .instanceMask: 0 = background, k = instance k) to w*h.
func sampleLabels(_ buf: CVPixelBuffer, width w: Int, height h: Int) -> [UInt8] {
    CVPixelBufferLockBaseAddress(buf, .readOnly)
    defer { CVPixelBufferUnlockBaseAddress(buf, .readOnly) }
    let mw = CVPixelBufferGetWidth(buf)
    let mh = CVPixelBufferGetHeight(buf)
    let stride = CVPixelBufferGetBytesPerRow(buf)
    var out = [UInt8](repeating: 0, count: w * h)
    guard let base = CVPixelBufferGetBaseAddress(buf) else { return out }
    for y in 0..<h {
        let row = (base + min(y * mh / h, mh - 1) * stride).assumingMemoryBound(to: UInt8.self)
        for x in 0..<w {
            out[y * w + x] = row[min(x * mw / w, mw - 1)]
        }
    }
    return out
}

/// People. Vision's person matte alone is great on modern footage but can
/// drop out frame-to-frame on grainy/archival film (profiles, B&W), so on
/// macOS 14+ we also run foreground-instance segmentation and keep every
/// instance that overlaps *recent* person evidence (the matte, held with a
/// decay). Output = max(person matte, kept instances): stable silhouettes
/// that still only include people.
final class PersonSegmenter: Segmenter {
    let seq = VNSequenceRequestHandler()
    let req: VNGeneratePersonSegmentationRequest = {
        let r = VNGeneratePersonSegmentationRequest()
        r.qualityLevel = .accurate
        r.outputPixelFormat = kCVPixelFormatType_OneComponent8
        return r
    }()
    let useInstances: Bool
    /// Person evidence, held across frames: max(matte, evidence * decay).
    var evidence: [Float] = []
    let decay: Float = 0.9
    /// Minimum mean evidence over an instance's pixels to keep it.
    let keepThreshold: Float = 0.15
    /// Weight of kept instances fed back into evidence (> keepThreshold
    /// so a tracked person persists while Vision keeps segmenting it).
    let reinforce: Float = 0.5

    init() {
        if #available(macOS 14.0, *) { useInstances = true } else { useInstances = false }
    }

    func mask(_ frame: CVPixelBuffer, _ w: Int, _ h: Int) throws -> [Float] {
        try seq.perform([req], on: frame)
        var person = [Float](repeating: 0, count: w * h)
        if let obs = req.results?.first {
            person = sampleMask(obs.pixelBuffer, width: w, height: h)
        }
        if evidence.count != person.count {
            evidence = person
        } else {
            for i in 0..<person.count {
                evidence[i] = max(person[i], evidence[i] * decay)
            }
        }
        guard useInstances, #available(macOS 14.0, *) else { return person }
        let personPixels = person.reduce(0) { $0 + ($1 >= 0.35 ? 1 : 0) }
        guard let inst = try instancesOverlappingEvidence(frame, w, h, personPixels: personPixels) else {
            return person
        }
        var out = person
        for i in 0..<out.count {
            out[i] = max(out[i], inst[i])
            // A kept instance is (weaker) person evidence itself, so a
            // confirmed person stays tracked through long matte dropouts.
            evidence[i] = max(evidence[i], inst[i] * reinforce)
        }
        return out
    }

    @available(macOS 14.0, *)
    func instancesOverlappingEvidence(_ frame: CVPixelBuffer, _ w: Int, _ h: Int, personPixels: Int) throws -> [Float]? {
        let fg = VNGenerateForegroundInstanceMaskRequest()
        let handler = VNImageRequestHandler(cvPixelBuffer: frame, options: [:])
        try handler.perform([fg])
        guard let obs = fg.results?.first, !obs.allInstances.isEmpty else { return nil }
        let labels = sampleLabels(obs.instanceMask, width: w, height: h)
        var sum = [Float](repeating: 0, count: 256)
        var cnt = [Int](repeating: 0, count: 256)
        for i in 0..<labels.count where labels[i] != 0 {
            sum[Int(labels[i])] += evidence[i]
            cnt[Int(labels[i])] += 1
        }
        let maxArea = (w * h * 35) / 100
        let maxVsPerson = personPixels > 0 ? Int(Float(personPixels) * 1.40) + 64 : maxArea
        let minPrecision: Float = personPixels > 0 ? 0.55 : keepThreshold
        var keep = IndexSet()
        for k in obs.allInstances where cnt[k] > 0
            && cnt[k] <= min(maxArea, maxVsPerson)
            && sum[k] / Float(cnt[k]) >= minPrecision {
            keep.insert(k)
        }
        if keep.isEmpty { return nil }
        let buf = try obs.generateScaledMaskForImage(forInstances: keep, from: handler)
        return sampleMask(buf, width: w, height: h)
    }
}

@available(macOS 14.0, *)
final class SubjectSegmenter: Segmenter {
    let req = VNGenerateForegroundInstanceMaskRequest()
    func mask(_ frame: CVPixelBuffer, _ w: Int, _ h: Int) throws -> [Float] {
        let handler = VNImageRequestHandler(cvPixelBuffer: frame, options: [:])
        try handler.perform([req])
        guard let obs = req.results?.first, !obs.allInstances.isEmpty else {
            return [Float](repeating: 0, count: w * h)
        }
        let buf = try obs.generateScaledMaskForImage(
            forInstances: obs.allInstances, from: handler)
        return sampleMask(buf, width: w, height: h)
    }
}

// ------------------------------------------------------------ main

let args = CommandLine.arguments
if args.count == 2 && args[1] == "--check" {
    if #available(macOS 14.0, *) {
        print("vision ok (person+subject)")
    } else {
        print("vision ok (person)")
    }
    exit(0)
}
guard args.count == 4 else {
    fail("usage: segment_vision <video.mp4> \"<prompt>\" <out_dir> | --check", 2)
}
let videoPath = args[1]
let prompt = args[2]
let outDir = args[3]

let segmenter: Segmenter
if wantsPerson(prompt) {
    log("vision mode: person (prompt \"\(prompt)\")")
    segmenter = PersonSegmenter()
} else if #available(macOS 14.0, *) {
    log("vision mode: subject / foreground instances (prompt \"\(prompt)\")")
    segmenter = SubjectSegmenter()
} else {
    log("vision mode: person (subject masks need macOS 14+)")
    segmenter = PersonSegmenter()
}

do {
    try FileManager.default.createDirectory(
        atPath: outDir, withIntermediateDirectories: true)
} catch {
    fail("cannot create \(outDir): \(error)")
}

let asset = AVURLAsset(url: URL(fileURLWithPath: videoPath))
// Track lookup via the async API, bridged with a semaphore (CLI context).
final class TrackBox: @unchecked Sendable { var track: AVAssetTrack?; var err: Error? }
let box = TrackBox()
let sem = DispatchSemaphore(value: 0)
asset.loadTracks(withMediaType: .video) { tracks, err in
    box.track = tracks?.first
    box.err = err
    sem.signal()
}
sem.wait()
guard let track = box.track else {
    fail("no video track in \(videoPath)\(box.err.map { ": \($0)" } ?? "")")
}
let reader: AVAssetReader
do {
    reader = try AVAssetReader(asset: asset)
} catch {
    fail("cannot open \(videoPath): \(error)")
}
let output = AVAssetReaderTrackOutput(track: track, outputSettings: [
    kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA,
])
output.alwaysCopiesSampleData = false
reader.add(output)
guard reader.startReading() else {
    fail("cannot decode \(videoPath): \(reader.error?.localizedDescription ?? "?")")
}

// Temporal smoothing: EMA of the soft mask against the previous frame to
// damp flicker, then a gentle contrast curve that keeps soft edges.
let emaAlpha: Float = 0.6
var prev: [Float]? = nil
var n = 0
let t0 = Date()

/// smoothstep(0.25, 0.75) -> byte: crisper interior, soft boundary.
func toByte(_ v: Float) -> UInt8 {
    let t: Float = min(max((v - 0.25) / 0.5, 0), 1)
    let s: Float = t * t * (3 - 2 * t)
    return UInt8((s * 255).rounded())
}

/// Segment + write one frame. Returns false when the stream is exhausted.
func processNextFrame() -> Bool {
    guard let sample = output.copyNextSampleBuffer() else { return false }
    guard let pb = CMSampleBufferGetImageBuffer(sample) else { return true }
    let w = CVPixelBufferGetWidth(pb)
    let h = CVPixelBufferGetHeight(pb)
    var m: [Float]
    do {
        m = try segmenter.mask(pb, w, h)
    } catch {
        fail("vision failed on frame \(n): \(error)")
    }
    if let p = prev, p.count == m.count {
        for i in 0..<m.count {
            // Motion-adaptive: small changes (flicker) get alpha ~0.6,
            // large changes (real motion) pass through, so no ghost trail.
            let d: Float = abs(m[i] - p[i])
            let a: Float = min(1, emaAlpha + 0.8 * d)
            m[i] = a * m[i] + (1 - a) * p[i]
        }
    }
    prev = m
    let bytes: [UInt8] = m.map(toByte)
    let path = (outDir as NSString).appendingPathComponent(
        String(format: "mask_%05d.pgm", n))
    do {
        try writePGM(path, bytes, w, h)
    } catch {
        fail("cannot write \(path): \(error)")
    }
    n += 1
    return true
}

while autoreleasepool(invoking: { processNextFrame() }) {}

if reader.status == .failed {
    fail("decode failed after \(n) frames: \(reader.error?.localizedDescription ?? "?")")
}
if n == 0 {
    fail("no frames decoded from \(videoPath)")
}
let secs = Date().timeIntervalSince(t0)
log(String(format: "vision: %d frames in %.2fs (%.1f ms/frame)", n, secs, secs * 1000 / Double(n)))
print("ok \(n)")
