// Headless test harness for the AUv3 audio unit. The subclass is
// registered in-process only (AUAudioUnit.registerSubclass): nothing is
// installed or registered with the system, and no window is opened.
//
//   chromagrain-au-harness <media file>
//
// Checks: instantiation through the component system, the parameter tree,
// AVFoundation decoding + hand-off, MIDI render events -> audio, transport,
// parameters (macros + key via the service timer), state save/restore.

import AVFoundation
import AudioToolbox
import Foundation

var failures = 0
func check(_ ok: Bool, _ what: String) {
    print(ok ? "  ok   \(what)" : "  FAIL \(what)")
    if !ok { failures += 1 }
}

func rms(_ x: [Float]) -> Float {
    x.isEmpty ? 0 : (x.reduce(0) { $0 + $1 * $1 } / Float(x.count)).squareRoot()
}

final class Renderer {
    let au: AUAudioUnit
    let frames: AVAudioFrameCount = 512
    var sampleTime = 0.0
    let abl: UnsafeMutableAudioBufferListPointer
    let left: UnsafeMutablePointer<Float>
    let right: UnsafeMutablePointer<Float>

    init(_ au: AUAudioUnit) {
        self.au = au
        abl = AudioBufferList.allocate(maximumBuffers: 2)
        left = .allocate(capacity: Int(frames))
        right = .allocate(capacity: Int(frames))
    }

    /// Render `blocks` blocks; returns the left channel.
    func run(_ blocks: Int) -> [Float] {
        var out: [Float] = []
        for _ in 0..<blocks {
            abl[0] = AudioBuffer(mNumberChannels: 1, mDataByteSize: frames * 4, mData: left)
            abl[1] = AudioBuffer(mNumberChannels: 1, mDataByteSize: frames * 4, mData: right)
            var flags = AudioUnitRenderActionFlags()
            var ts = AudioTimeStamp()
            ts.mSampleTime = sampleTime
            ts.mFlags = .sampleTimeValid
            let status = au.renderBlock(&flags, &ts, frames, 0, abl.unsafeMutablePointer, nil)
            if status != noErr { print("  render error \(status)"); failures += 1; break }
            out.append(contentsOf: UnsafeBufferPointer(start: left, count: Int(frames)))
            if !(0..<Int(frames)).allSatisfy({ left[$0].isFinite && right[$0].isFinite && abs(left[$0]) <= 1 }) {
                print("  FAIL non-finite or clipped samples"); failures += 1
            }
            sampleTime += Double(frames)
        }
        return out
    }

    func midi(_ bytes: [UInt8], offset: Int = 0) {
        au.scheduleMIDIEventBlock?(AUEventSampleTime(sampleTime) + AUEventSampleTime(offset), 0, bytes.count, bytes)
    }
}

func instantiate(_ desc: AudioComponentDescription) async throws -> AUAudioUnit {
    try await withCheckedThrowingContinuation { cont in
        AUAudioUnit.instantiate(with: desc, options: []) { au, err in
            if let au { cont.resume(returning: au) } else { cont.resume(throwing: err ?? NSError(domain: "harness", code: 1)) }
        }
    }
}

func load(_ au: ChromagrainAudioUnit, _ url: URL) async -> Bool {
    await withCheckedContinuation { cont in
        DispatchQueue.main.async { au.load(url: url) { cont.resume(returning: $0) } }
    }
}

func waitForSources(_ au: ChromagrainAudioUnit, _ r: Renderer, _ n: Int) -> Bool {
    for _ in 0..<200 {
        _ = r.run(1) // the next render installs posted media
        if cg_instrument_loaded_source_count(au.engine) >= n { return true }
        Thread.sleep(forTimeInterval: 0.01)
    }
    return false
}

@main
enum Harness {
    static func main() async {
        let args = CommandLine.arguments
        guard args.count > 1 else { print("usage: \(args[0]) <media file>"); exit(2) }
        let media = URL(fileURLWithPath: args[1])

        print("chromagrain AUv3 harness")
        let desc = ChromagrainAudioUnit.componentDescription
        AUAudioUnit.registerSubclass(ChromagrainAudioUnit.self, as: desc, name: "Creative Lab: chromagrain (in-process test)", version: 256)

        let generic: AUAudioUnit
        do { generic = try await instantiate(desc) } catch { print("  FAIL instantiate: \(error)"); exit(1) }
        guard let au = generic as? ChromagrainAudioUnit else { print("  FAIL wrong class"); exit(1) }
        check(true, "instantiated through the component system (aumu chgr CrLb)")
        check(au.outputBusses.count == 1 && au.outputBusses[0].format.channelCount == 2, "one stereo output bus")
        check(au.inputBusses.count == 0, "no input bus (instrument)")
        let params = au.parameterTree?.allParameters ?? []
        check(params.map(\.identifier) == ["grain", "pitch", "smear", "look", "key", "scale", "sync", "gain"], "parameter tree: \(params.map(\.identifier))")

        // Media through AVFoundation.
        let t0 = Date()
        let loaded = await load(au, media)
        check(loaded, "decoded \(media.lastPathComponent) with AVFoundation in \(String(format: "%.2f", Date().timeIntervalSince(t0))) s: \(au.status)")

        au.maximumFramesToRender = 512
        do { try au.allocateRenderResources() } catch { print("  FAIL allocateRenderResources: \(error)"); exit(1) }
        var tempo = 0.0
        au.musicalContextBlock = { bpm, _, _, beat, _, _ in
            bpm?.pointee = 100; beat?.pointee = 0; tempo = 100; return true
        }
        au.transportStateBlock = { flags, _, _, _ in flags?.pointee = .moving; return true }
        try? au.allocateRenderResources() // pick up the host blocks
        let r = Renderer(au)
        check(waitForSources(au, r, 1), "media arrived on the render side (\(cg_instrument_loaded_source_count(au.engine)) source)")

        // Silence until a note.
        check(rms(r.run(10)) == 0, "silent before any note")
        r.midi([0x90, 60, 110], offset: 100)
        let held = r.run(80)
        check(rms(Array(held[2_000...])) > 0.01, "MIDI note-on renders audio (rms \(rms(Array(held[2_000...]))))")
        check(tempo == 100 && abs(cg_instrument_transport(au.engine).bpm - 100) < 1e-9, "host tempo reached the engine")
        r.midi([0x80, 60, 0])
        _ = r.run(100)
        check(rms(r.run(40)) < 1e-4, "note-off rings out to silence")

        // Parameters: macros via the tree, key via the render-safe request + service timer.
        au.parameterTree?.parameter(withAddress: ParamAddress.grain.rawValue)?.value = 0.8
        au.parameterTree?.parameter(withAddress: ParamAddress.key.rawValue)?.value = 10 // A
        au.parameterTree?.parameter(withAddress: ParamAddress.scale.rawValue)?.value = 2 // minor
        try? await Task.sleep(nanoseconds: 150_000_000)
        _ = r.run(2)
        check(abs(cg_instrument_macro_value(au.engine, 0) - 0.8) < 1e-6, "GRAIN parameter reached the engine")
        check(cg_instrument_key_root(au.engine) == 9 && cg_instrument_key_scale(au.engine) == 2, "key A minor reached the engine (root \(cg_instrument_key_root(au.engine)))")
        r.midi([0x90, 64, 100])
        check(rms(Array(r.run(60)[4_000...])) > 0.005, "cloud note sounds")
        r.midi([0xB0, 123, 0]) // all notes off
        _ = r.run(2)
        check(cg_instrument_held_note_count(au.engine) == 0, "CC 123 all-notes-off")

        // State: parameters + media bookmarks restore into a fresh instance.
        let state = au.fullState
        check((state?["chromagrain.sources"] as? [Data])?.count == 1, "state carries the media bookmark")
        if let au2 = try? await instantiate(desc) as? ChromagrainAudioUnit {
            au2.maximumFramesToRender = 512
            try? au2.allocateRenderResources()
            au2.fullState = state
            let r2 = Renderer(au2)
            check(abs((au2.parameterTree?.parameter(withAddress: ParamAddress.grain.rawValue)?.value ?? 0) - 0.8) < 1e-6, "parameters restored")
            check(waitForSources(au2, r2, 1), "media decoded again from the restored state")
            au2.deallocateRenderResources()
        } else {
            check(false, "second instance")
        }
        au.deallocateRenderResources()

        print(failures == 0 ? "PASS" : "\(failures) failure(s)")
        exit(failures == 0 ? 0 : 1)
    }
}
