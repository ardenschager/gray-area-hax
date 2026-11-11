// Parameters of the chromagrain AUv3: the four macros, key + scale, sync
// grid and gain. Every change goes to the Rust engine through realtime-safe
// C calls (atomics / requests), so this is safe from the render thread.

import AudioToolbox
import Foundation

enum ParamAddress: AUParameterAddress, CaseIterable {
    case grain = 0, pitch, smear, look, key, scale, sync, gain
}

enum ChromagrainParameters {
    static let keyNames = ["off (chromatic)", "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"]
    /// Same order as `CgScale` in chromagrain.h.
    static let scaleNames = ["chromatic", "major", "minor", "harmonic minor", "major pentatonic", "minor pentatonic",
                             "blues", "dorian", "phrygian", "lydian", "mixolydian", "whole tone"]
    static let syncNames = ["free", "1 bar", "1/2", "1/4", "1/8", "1/16", "1/32"]
    static let syncBeats: [Float] = [0, 4, 2, 1, 0.5, 0.25, 0.125]

    static func makeTree() -> AUParameterTree {
        let rw: AudioUnitParameterOptions = [.flag_IsReadable, .flag_IsWritable]
        func knob(_ id: String, _ name: String, _ addr: ParamAddress, _ value: AUValue) -> AUParameter {
            let p = AUParameterTree.createParameter(withIdentifier: id, name: name, address: addr.rawValue, min: 0, max: 1,
                                                    unit: .generic, unitName: nil, flags: rw.union(.flag_CanRamp),
                                                    valueStrings: nil, dependentParameters: nil)
            p.value = value
            return p
        }
        func menu(_ id: String, _ name: String, _ addr: ParamAddress, _ names: [String], _ value: AUValue) -> AUParameter {
            let p = AUParameterTree.createParameter(withIdentifier: id, name: name, address: addr.rawValue, min: 0,
                                                    max: AUValue(names.count - 1), unit: .indexed, unitName: nil,
                                                    flags: rw, valueStrings: names, dependentParameters: nil)
            p.value = value
            return p
        }
        let gain = AUParameterTree.createParameter(withIdentifier: "gain", name: "Gain", address: ParamAddress.gain.rawValue,
                                                   min: -36, max: 12, unit: .decibels, unitName: nil,
                                                   flags: rw.union(.flag_CanRamp), valueStrings: nil, dependentParameters: nil)
        gain.value = 0
        return AUParameterTree.createTree(withChildren: [
            knob("grain", "Grain", .grain, 0),
            knob("pitch", "Pitch", .pitch, 0.5),
            knob("smear", "Smear", .smear, 0),
            knob("look", "Look", .look, 0),
            menu("key", "Key", .key, keyNames, 0),
            menu("scale", "Scale", .scale, scaleNames, 2),
            menu("sync", "Sync", .sync, syncNames, 0),
            gain,
        ])
    }

    /// Route one parameter value to the engine. Realtime-safe.
    static func apply(address: AUParameterAddress, value: AUValue, engine: OpaquePointer) {
        guard let addr = ParamAddress(rawValue: address) else { return }
        switch addr {
        case .grain, .pitch, .smear, .look:
            cg_instrument_post_macro(engine, UInt32(address), value)
        case .key:
            cg_instrument_request_key_root(engine, Int32(value.rounded()) - 1)
        case .scale:
            cg_instrument_request_key_scale(engine, UInt32(max(0, value.rounded())))
        case .sync:
            let i = min(max(Int(value.rounded()), 0), syncBeats.count - 1)
            cg_instrument_post_sync(engine, syncBeats[i])
        case .gain:
            cg_instrument_post_gain(engine, powf(10, value / 20))
        }
    }
}
