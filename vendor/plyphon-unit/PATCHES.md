# Vendored plyphon-unit

Copied from https://github.com/nannou-org/plyphon at rev
`4e3cd234421271724e48b9012835746b0dbb33fc` (`crates/plyphon-unit`,
GPL-3.0-or-later) and wired in with `[patch]` in the workspace
`Cargo.toml`. Changes:

1. `MulAdd` (`src/unit/util.rs`): `mul` and `add` were read at control
   rate even when audio-rate, so `a * b + c` with audio-rate `b` or `c`
   (which sclang's optimiser turns into `MulAdd`) came out stepped every
   64-sample block — an audible buzz. Each input is now read at its own
   rate, and control-rate `mul`/`add` are ramped across the block like
   scsynth does.

Drop this directory and the `[patch]` once the fix is upstream.
2. `BinaryOpUGen` (`src/unit/binary_op.rs`): a control-rate operand
   combined with an audio-rate one (`sig * EnvGen.kr(..)`, the most common
   pattern in any SynthDef) was held flat for the block, so fast envelopes
   zippered at 750 Hz. It is now ramped across the block from the previous
   block's value, like scsynth's `*_ak` / `*_ka` calc functions.
