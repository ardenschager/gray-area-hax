// Container app for the chromagrain AUv3 extension. Its only job is to
// carry the extension; the instrument runs inside a host (Logic,
// GarageBand, AUM, ...).

import SwiftUI

@main
struct ChromagrainApp: App {
    var body: some Scene {
        WindowGroup {
            VStack(spacing: 8) {
                Text("chromagrain").font(.title)
                Text("The chromagrain Audio Unit is installed with this app.\nOpen it from an AU host as an instrument (Creative Lab ▸ chromagrain).")
                    .multilineTextAlignment(.center)
                    .foregroundColor(.secondary)
            }
            .padding(32)
        }
    }
}
