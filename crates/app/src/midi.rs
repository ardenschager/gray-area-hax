//! MIDI input: note pads + CC control, via midir. Ports are polled and
//! connected from the UI; messages queue through a channel and the app
//! drains them each frame.

use std::sync::mpsc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MidiMsg {
    NoteOn { note: u8, velocity: u8 },
    NoteOff { note: u8 },
    Cc { cc: u8, value: u8 },
}

/// Parse a raw MIDI message (pure, unit-testable).
pub fn parse(bytes: &[u8]) -> Option<MidiMsg> {
    if bytes.len() < 2 {
        return None;
    }
    let status = bytes[0] & 0xF0;
    match status {
        0x90 => {
            let velocity = *bytes.get(2)?;
            if velocity == 0 {
                Some(MidiMsg::NoteOff { note: bytes[1] })
            } else {
                Some(MidiMsg::NoteOn { note: bytes[1], velocity })
            }
        }
        0x80 => Some(MidiMsg::NoteOff { note: bytes[1] }),
        0xB0 => Some(MidiMsg::Cc { cc: bytes[1], value: *bytes.get(2)? }),
        _ => None,
    }
}

pub struct Midi {
    rx: Option<mpsc::Receiver<MidiMsg>>,
    conn: Option<midir::MidiInputConnection<()>>,
    pub ports: Vec<String>,
    pub connected: Option<String>,
    pub last_error: Option<String>,
}

impl Midi {
    pub fn new() -> Midi {
        let mut m = Midi {
            rx: None,
            conn: None,
            ports: Vec::new(),
            connected: None,
            last_error: None,
        };
        m.refresh();
        m
    }

    /// Re-enumerate input ports.
    pub fn refresh(&mut self) {
        self.ports.clear();
        match midir::MidiInput::new("chromagrain") {
            Ok(input) => {
                for port in input.ports() {
                    self.ports
                        .push(input.port_name(&port).unwrap_or_else(|_| "?".into()));
                }
            }
            Err(e) => self.last_error = Some(e.to_string()),
        }
    }

    /// Connect to the input port at `index` (from `ports`).
    pub fn connect(&mut self, index: usize) -> Result<(), String> {
        self.conn = None;
        self.connected = None;
        let mut input = midir::MidiInput::new("chromagrain").map_err(|e| e.to_string())?;
        input.ignore(midir::Ignore::None);
        let ports = input.ports();
        let port = ports.get(index).ok_or("port disappeared")?;
        let name = input.port_name(port).unwrap_or_else(|_| "midi".into());
        let (tx, rx) = mpsc::channel();
        let conn = input
            .connect(
                port,
                "chromagrain-in",
                move |_, bytes, _| {
                    if let Some(msg) = parse(bytes) {
                        let _ = tx.send(msg);
                    }
                },
                (),
            )
            .map_err(|e| e.to_string())?;
        self.conn = Some(conn);
        self.rx = Some(rx);
        self.connected = Some(name);
        Ok(())
    }

    /// Drain pending messages.
    pub fn poll(&mut self) -> Vec<MidiMsg> {
        let mut out = Vec::new();
        if let Some(rx) = &self.rx {
            while let Ok(m) = rx.try_recv() {
                out.push(m);
            }
        }
        out
    }
}

/// What a mapped CC controls.
#[derive(Debug, Clone, PartialEq)]
pub enum CcTarget {
    /// A grain parameter on a specific clip.
    ClipParam { track: usize, clip: usize, param: String },
    /// A track's level fader (audio gain + video opacity).
    TrackLevel { track: usize },
}

impl CcTarget {
    pub fn label(&self) -> String {
        match self {
            CcTarget::ClipParam { track, clip, param } => {
                format!("{param} (t{track} c{clip})")
            }
            CcTarget::TrackLevel { track } => format!("level (t{track})"),
        }
    }
}

/// One CC-to-parameter mapping. `cc == None` means "learning": the next
/// incoming CC claims it.
#[derive(Debug, Clone, PartialEq)]
pub struct CcMapping {
    pub cc: Option<u8>,
    pub target: CcTarget,
}

/// Apply a CC value (0..127) through a mapping onto the project.
/// Returns true when something changed.
pub fn apply_cc(
    project: &mut chromagrain_core::timeline::Project,
    target: &CcTarget,
    value: u8,
) -> bool {
    let f = value as f64 / 127.0;
    match target {
        CcTarget::ClipParam { track, clip, param } => {
            let Some((lo, hi)) = chromagrain_core::auto::param_range(param) else {
                return false;
            };
            let v = lo as f64 + (hi - lo) as f64 * f;
            project
                .tracks
                .get_mut(*track)
                .and_then(|t| t.clips.get_mut(*clip))
                .map(|c| c.grains.set_param(param, v).is_ok())
                .unwrap_or(false)
        }
        CcTarget::TrackLevel { track } => match project.tracks.get_mut(*track) {
            Some(t) => {
                t.level = (f * 1.5) as f32;
                true
            }
            None => false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_note_and_cc() {
        assert_eq!(
            parse(&[0x90, 60, 100]),
            Some(MidiMsg::NoteOn { note: 60, velocity: 100 })
        );
        assert_eq!(parse(&[0x90, 60, 0]), Some(MidiMsg::NoteOff { note: 60 }));
        assert_eq!(parse(&[0x81, 61, 0]), Some(MidiMsg::NoteOff { note: 61 }));
        assert_eq!(parse(&[0xB0, 21, 64]), Some(MidiMsg::Cc { cc: 21, value: 64 }));
        assert_eq!(parse(&[0xF8]), None);
        assert_eq!(parse(&[]), None);
    }

    #[test]
    fn cc_maps_into_param_range() {
        let mut p = chromagrain_core::timeline::Project::demo();
        let target = CcTarget::ClipParam { track: 1, clip: 0, param: "density".into() };
        assert!(apply_cc(&mut p, &target, 127));
        let (_, hi) = chromagrain_core::auto::param_range("density").unwrap();
        assert!((p.tracks[1].clips[0].grains.density - hi).abs() < 1e-4);
        assert!(apply_cc(&mut p, &target, 0));
        let (lo, _) = chromagrain_core::auto::param_range("density").unwrap();
        assert!((p.tracks[1].clips[0].grains.density - lo).abs() < 1e-4);

        let lvl = CcTarget::TrackLevel { track: 0 };
        assert!(apply_cc(&mut p, &lvl, 127));
        assert!((p.tracks[0].level - 1.5).abs() < 1e-5);
        // Out-of-range targets fail quietly.
        let bogus = CcTarget::TrackLevel { track: 99 };
        assert!(!apply_cc(&mut p, &bogus, 64));
    }
}
