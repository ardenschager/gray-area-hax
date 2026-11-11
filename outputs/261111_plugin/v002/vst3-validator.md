# Steinberg VST3 validator on target/bundled/chromagrain.vst3

VST3 SDK 3.8.1 `validator`, built headless with the Command Line Tools
(`tools/vst3-validate.sh`, no Xcode, no GUI).

- `tools/vst3-validate.sh`: **47 tests passed, 0 tests failed**
- `tools/vst3-validate.sh -e` (extensive): **537 tests passed, 0 tests failed**

Notes the validator printed (informational, not failures):
- Info:      Mono Output-SpeakerArrangement is not supported. Plug-in suggests: Stereo.
- Info:  64bit Audio Processing not supported.

| test | result |
|---|---|
| Scan Editor Classes | Succeeded |
| Scan Buses | Succeeded |
| Scan Parameters | Succeeded |
| MIDI Mapping | Succeeded |
| MIDI Learn | Succeeded |
| Scan Units | Succeeded |
| Scan Programs | Succeeded |
| Terminate/Initialize | Succeeded |
| Check Unit Structure | Succeeded |
| Valid State Transition 32bits | Succeeded |
| Valid State Transition 64bits | Succeeded |
| Bus Consistency | Succeeded |
| Bus Activation | Succeeded |
| Check Audio Bus Arrangement | Succeeded |
| SideChain Arrangement | Succeeded |
| Suspend/Resume | Succeeded |
| Note Expression | Succeeded |
| Keyswitch | Succeeded |
| ProcessContext Requirements | Succeeded |
| Process Test | Succeeded |
| Process function running in another thread | Succeeded |
| Silence Flags | Succeeded |
| Silence Processing | Succeeded |
| Parameters Flush (no Buffer) | Succeeded |
| Parameters Flush 2 (only numChannel==0) | Succeeded |
| Parameters Flush 2 (no Buffer, no parameter change) | Succeeded |
| Variable Block Size | Succeeded |
| Process Format | Succeeded |
| Parameter Bypass persistence | Succeeded |
| In: Stereo: 2 Channels, Out: Stereo: 2 Channels | Succeeded |
| In: Mono: 1 Channels, Out: Mono: 1 Channels | Succeeded |
| Accuracy: Block, 1 Parameters, Change every100 Samples | Succeeded |
| Accuracy: Sample, 1 Parameters, Change every100 Samples | Succeeded |
| Process Test | Succeeded |
| Process function running in another thread | Succeeded |
| Silence Flags | Succeeded |
| Silence Processing | Succeeded |
| Parameters Flush (no Buffer) | Succeeded |
| Parameters Flush 2 (only numChannel==0) | Succeeded |
| Parameters Flush 2 (no Buffer, no parameter change) | Succeeded |
| Variable Block Size | Succeeded |
| Process Format | Succeeded |
| Parameter Bypass persistence | Succeeded |
| In: Stereo: 2 Channels, Out: Stereo: 2 Channels | Succeeded |
| In: Mono: 1 Channels, Out: Mono: 1 Channels | Succeeded |
| Accuracy: Block, 1 Parameters, Change every100 Samples | Succeeded |
| Accuracy: Sample, 1 Parameters, Change every100 Samples | Succeeded |
