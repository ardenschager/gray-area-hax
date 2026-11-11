// Principal class of the AUv3 app extension: creates the audio unit and
// shows a placeholder SwiftUI editor (load a video, the macros, a canvas).
// The real play-first UI arrives later; keep this thin.

import CoreAudioKit
import SwiftUI

@objc(ChromagrainAudioUnitFactory)
public final class AudioUnitFactory: AUViewController, AUAudioUnitFactory {
    private var audioUnit: ChromagrainAudioUnit?

    public func createAudioUnit(with componentDescription: AudioComponentDescription) throws -> AUAudioUnit {
        let au = try ChromagrainAudioUnit(componentDescription: componentDescription, options: [])
        audioUnit = au
        DispatchQueue.main.async { [weak self] in self?.attachEditor() }
        return au
    }

    #if os(macOS)
    public override func loadView() {
        view = NSView(frame: NSRect(x: 0, y: 0, width: 560, height: 380))
        preferredContentSize = view.frame.size
        attachEditor()
    }

    private func attachEditor() {
        guard isViewLoaded, let au = audioUnit else { return }
        let host = NSHostingView(rootView: EditorView(model: EditorModel(au)))
        host.frame = view.bounds
        host.autoresizingMask = [.width, .height]
        view.subviews.forEach { $0.removeFromSuperview() }
        view.addSubview(host)
    }
    #else
    private func attachEditor() {
        guard isViewLoaded, let au = audioUnit else { return }
        let host = UIHostingController(rootView: EditorView(model: EditorModel(au)))
        addChild(host)
        host.view.frame = view.bounds
        host.view.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        view.addSubview(host.view)
        host.didMove(toParent: self)
    }
    #endif
}

final class EditorModel: ObservableObject {
    let au: ChromagrainAudioUnit
    @Published var status: String
    @Published var values: [AUParameterAddress: AUValue] = [:]
    private var token: AUParameterObserverToken?

    init(_ au: ChromagrainAudioUnit) {
        self.au = au
        status = au.status
        for p in au.parameterTree?.allParameters ?? [] { values[p.address] = p.value }
        token = au.parameterTree?.token(byAddingParameterObserver: { [weak self] address, value in
            DispatchQueue.main.async { self?.values[address] = value }
        })
        au.onStatus = { [weak self] s in DispatchQueue.main.async { self?.status = s } }
    }

    func binding(_ address: ParamAddress) -> Binding<Double> {
        Binding(
            get: { Double(self.values[address.rawValue] ?? 0) },
            set: { v in
                self.values[address.rawValue] = AUValue(v)
                self.au.parameterTree?.parameter(withAddress: address.rawValue)?.setValue(AUValue(v), originator: self.token)
            })
    }

    func loadVideo() {
        #if os(macOS)
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.movie, .audio]
        panel.allowsMultipleSelection = false
        if panel.runModal() == .OK, let url = panel.url { au.load(url: url) }
        #endif
    }
}

struct EditorView: View {
    @ObservedObject var model: EditorModel

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Text("chromagrain").font(.headline)
                Button("Load video…") { model.loadVideo() }
                Text(model.status).font(.caption).foregroundColor(.secondary).lineLimit(1)
            }
            ForEach([(ParamAddress.grain, "GRAIN"), (.pitch, "PITCH"), (.smear, "SMEAR"), (.look, "LOOK")], id: \.0) { addr, label in
                HStack {
                    Text(label).frame(width: 60, alignment: .leading)
                    Slider(value: model.binding(addr), in: 0...1)
                }
            }
            ZStack { // placeholder canvas: the picture lands here with the real UI
                RoundedRectangle(cornerRadius: 4).fill(Color(white: 0.1))
                Text("canvas · \(cg_instrument_loaded_source_count(model.au.engine)) source(s)")
                    .foregroundColor(.gray)
            }
        }
        .padding(12)
    }
}
