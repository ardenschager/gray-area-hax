// The chromagrain AUv3 instrument (type 'aumu'). Rendering calls the Rust
// engine through the C ABI in crates/ffi/include/chromagrain.h:
//   * MIDI note on/off render events -> cg_instrument_note_on/off, applied
//     at their sample offsets (the block is split at each event);
//   * host tempo/transport -> cg_instrument_set_transport;
//   * parameters -> realtime-safe cg_instrument_post_* / request_* calls;
//   * media is decoded with AVFoundation (MediaDecoder) off the render
//     thread and handed over with cg_instrument_post_source_pcm_rgba.
// The render block never allocates, locks or calls into Objective-C.

import AVFoundation
import AudioToolbox
import Foundation

func fourCC(_ s: String) -> FourCharCode {
    s.utf8.prefix(4).reduce(0) { ($0 << 8) | FourCharCode($1) }
}

/// Everything the render block needs, owned by the audio unit.
final class RenderState {
    let engine: OpaquePointer
    var scratch: UnsafeMutablePointer<Float>
    var scratchFrames: Int
    var musicalContext: AUHostMusicalContextBlock?
    var transportState: AUHostTransportStateBlock?

    init(engine: OpaquePointer, frames: Int) {
        self.engine = engine
        scratchFrames = frames
        scratch = .allocate(capacity: 2 * frames)
        scratch.initialize(repeating: 0, count: 2 * frames)
    }

    deinit { scratch.deallocate() }

    func ensureScratch(_ frames: Int) {
        guard frames > scratchFrames else { return }
        scratch.deallocate()
        scratch = .allocate(capacity: 2 * frames)
        scratch.initialize(repeating: 0, count: 2 * frames)
        scratchFrames = frames
    }

    @inline(__always)
    private func handle(_ event: UnsafePointer<AURenderEvent>) {
        switch event.pointee.head.eventType {
        case .MIDI:
            let m = event.pointee.MIDI
            let status = m.data.0 & 0xF0
            let note = m.data.1, velocity = m.data.2
            if status == 0x90 && velocity > 0 {
                cg_instrument_note_on(engine, note, Float(velocity) / 127)
            } else if status == 0x80 || status == 0x90 {
                cg_instrument_note_off(engine, note)
            } else if status == 0xB0 && (note == 120 || note == 123) {
                cg_instrument_all_notes_off(engine)
            }
        case .parameter, .parameterRamp:
            let p = event.pointee.parameter
            ChromagrainParameters.apply(address: p.parameterAddress, value: p.value, engine: engine)
        default:
            break
        }
    }

    func render(timestamp: UnsafePointer<AudioTimeStamp>, frames: Int,
                output: UnsafeMutablePointer<AudioBufferList>, events: UnsafePointer<AURenderEvent>?) -> OSStatus {
        guard frames <= scratchFrames else { return kAudioUnitErr_TooManyFramesToProcess }
        let abl = UnsafeMutableAudioBufferListPointer(output)
        var left = scratch, right = scratch + scratchFrames
        if abl.count >= 2 {
            for (i, base) in [scratch, scratch + scratchFrames].enumerated() where abl[i].mData == nil {
                abl[i].mData = UnsafeMutableRawPointer(base)
            }
            left = abl[0].mData!.assumingMemoryBound(to: Float.self)
            right = abl[1].mData!.assumingMemoryBound(to: Float.self)
        }

        if let mc = musicalContext {
            var tempo = 120.0, beat = 0.0
            if mc(&tempo, nil, nil, &beat, nil, nil) {
                var flags = AUHostTransportStateFlags()
                var playing = false
                if let ts = transportState, ts(&flags, nil, nil, nil) {
                    playing = flags.contains(.moving)
                }
                cg_instrument_set_transport(engine, CgTransport(bpm: tempo, playing: playing, beat: beat))
            }
        }

        let start = AUEventSampleTime(timestamp.pointee.mSampleTime)
        var event = events
        var pos = 0
        while pos < frames {
            while let e = event, Int(max(0, e.pointee.head.eventSampleTime - start)) <= pos {
                handle(e)
                event = UnsafePointer(e.pointee.head.next)
            }
            var end = frames
            if let e = event {
                end = min(frames, Int(e.pointee.head.eventSampleTime - start))
            }
            end = max(end, pos + 1)
            cg_instrument_process(engine, left + pos, right + pos, end - pos)
            pos = end
        }
        while let e = event { // stamped past this buffer
            handle(e)
            event = UnsafePointer(e.pointee.head.next)
        }

        if abl.count == 1 { // mono host bus: fold down
            let out = (abl[0].mData ?? UnsafeMutableRawPointer(scratch)).assumingMemoryBound(to: Float.self)
            for i in 0..<frames { out[i] = 0.5 * (left[i] + right[i]) }
            abl[0].mData = UnsafeMutableRawPointer(out)
            abl[0].mDataByteSize = UInt32(frames * MemoryLayout<Float>.size)
        } else {
            for i in 0..<2 { abl[i].mDataByteSize = UInt32(frames * MemoryLayout<Float>.size) }
        }
        return noErr
    }
}

public final class ChromagrainAudioUnit: AUAudioUnit {
    public static let componentDescription = AudioComponentDescription(
        componentType: kAudioUnitType_MusicDevice,
        componentSubType: fourCC("chgr"),
        componentManufacturer: fourCC("CrLb"),
        componentFlags: 0,
        componentFlagsMask: 0)

    /// The Rust engine (`CgInstrument*`).
    public let engine: OpaquePointer
    let state: RenderState
    private let outputBus: AUAudioUnitBus
    private var busArray: AUAudioUnitBusArray!
    private let tree: AUParameterTree
    private let serviceQueue = DispatchQueue(label: "chromagrain.service")
    private var serviceTimer: DispatchSourceTimer?

    /// Loaded media, for the editor and the saved state.
    public private(set) var sources: [URL] = []
    private var bookmarks: [Data] = []
    /// One line of status for the editor ("loading x…", errors).
    public private(set) var status = "no video loaded" { didSet { onStatus?(status) } }
    public var onStatus: ((String) -> Void)?

    public override init(componentDescription: AudioComponentDescription,
                         options: AudioComponentInstantiationOptions = []) throws {
        engine = cg_instrument_new(48_000)
        state = RenderState(engine: engine, frames: 4096)
        outputBus = try AUAudioUnitBus(format: AVAudioFormat(standardFormatWithSampleRate: 48_000, channels: 2)!)
        tree = ChromagrainParameters.makeTree()
        try super.init(componentDescription: componentDescription, options: options)
        outputBus.maximumChannelCount = 2
        busArray = AUAudioUnitBusArray(audioUnit: self, busType: .output, busses: [outputBus])
        maximumFramesToRender = 4096
        let engine = self.engine
        tree.implementorValueObserver = { param, value in
            ChromagrainParameters.apply(address: param.address, value: value, engine: engine)
        }
        for p in tree.allParameters {
            ChromagrainParameters.apply(address: p.address, value: p.value, engine: engine)
        }
        _ = cg_instrument_service(engine)
        startService()
    }

    deinit {
        serviceTimer?.cancel()
        serviceQueue.sync {} // let a running tick finish before the engine goes
        cg_instrument_free(engine)
    }

    /// Key changes requested from the render thread are built here, off it;
    /// replaced media is freed here too.
    private func startService() {
        let t = DispatchSource.makeTimerSource(queue: serviceQueue)
        let engine = self.engine
        t.schedule(deadline: .now() + .milliseconds(30), repeating: .milliseconds(30))
        t.setEventHandler { _ = cg_instrument_service(engine) }
        t.resume()
        serviceTimer = t
    }

    public override var outputBusses: AUAudioUnitBusArray { busArray }
    public override var parameterTree: AUParameterTree? {
        get { tree }
        set {}
    }
    public override var channelCapabilities: [NSNumber]? { [0, 2] }
    public override var latency: TimeInterval {
        Double(cg_instrument_latency_samples(engine)) / max(1, outputBus.format.sampleRate)
    }

    public override func allocateRenderResources() throws {
        try super.allocateRenderResources()
        state.ensureScratch(Int(maximumFramesToRender))
        state.musicalContext = musicalContextBlock
        state.transportState = transportStateBlock
        cg_instrument_reset(engine, UInt32(outputBus.format.sampleRate))
        _ = cg_instrument_repost_key(engine) // key state is rate-specific
    }

    public override var internalRenderBlock: AUInternalRenderBlock {
        let state = self.state
        return { [unowned(unsafe) state] _, timestamp, frameCount, _, outputData, events, _ in
            state.render(timestamp: timestamp, frames: Int(frameCount), output: outputData, events: events)
        }
    }

    // ------------------------------------------------------------ media

    /// Decode `url` with AVFoundation off the main and render threads and
    /// hand it to the engine.
    public func load(url: URL, completion: ((Bool) -> Void)? = nil) {
        status = "loading \(url.lastPathComponent)…"
        let engine = self.engine
        Task.detached(priority: .userInitiated) { [self] in
            let ok: Bool
            let message: String
            do {
                let media = try await MediaDecoder.decode(url: url)
                ok = media.post(to: engine)
                message = ok ? "\(media.name) (\(String(format: "%.1f", media.seconds)) s)" : "\(media.name): engine queue full"
            } catch {
                ok = false
                message = "\(url.lastPathComponent): \(error.localizedDescription)"
            }
            await MainActor.run {
                if ok {
                    self.sources.append(url)
                    if let b = try? url.bookmarkData(options: Self.bookmarkOptions, includingResourceValuesForKeys: nil, relativeTo: nil) {
                        self.bookmarks.append(b)
                    }
                }
                self.status = message
                completion?(ok)
            }
        }
    }

    #if os(macOS)
    static let bookmarkOptions: URL.BookmarkCreationOptions = [.withSecurityScope]
    static let resolveOptions: URL.BookmarkResolutionOptions = [.withSecurityScope]
    #else
    static let bookmarkOptions: URL.BookmarkCreationOptions = []
    static let resolveOptions: URL.BookmarkResolutionOptions = []
    #endif

    /// Parameter values (from the base class) plus the loaded media as
    /// security-scoped bookmarks, decoded again on restore.
    public override var fullState: [String: Any]? {
        get {
            var s = super.fullState ?? [:]
            s["chromagrain.sources"] = bookmarks
            return s
        }
        set {
            super.fullState = newValue
            guard let marks = newValue?["chromagrain.sources"] as? [Data] else { return }
            for mark in marks {
                var stale = false
                if let url = try? URL(resolvingBookmarkData: mark, options: Self.resolveOptions, relativeTo: nil, bookmarkDataIsStale: &stale) {
                    _ = url.startAccessingSecurityScopedResource()
                    load(url: url)
                }
            }
        }
    }
}

// AUAudioUnit instances are used from the host's main, render and loader
// threads by design; the mutable state here is main-thread only (sources,
// status) or handed to the engine through its thread-safe C calls.
extension ChromagrainAudioUnit: @unchecked Sendable {}
