use std::borrow::Borrow;
use std::ops::Index;

use augmented_midi::{
    MIDIFile, MIDIFileChunk, MIDIFileDivision, MIDIMessage, MIDIMessageNote, MIDITrackInner,
    parse_midi_file,
};
use itertools::Itertools;

#[derive(Debug, Clone)]
pub struct MidiTrackAccumulateEvent {
    pub time: u64,
    pub inner: MidiEvent,
}

#[derive(Debug, Clone)]
pub struct MidiTrack {
    pub events: Vec<MidiTrackAccumulateEvent>,
    pub ticks_per_beat: u16,
}

impl MidiTrack {
    pub fn from_midi_file<
        StringRepr: Borrow<str>,
        Buffer: Borrow<[u8]> + Clone + Index<usize, Output = u8>,
    >(
        file: MIDIFile<StringRepr, Buffer>,
    ) -> Self {
        let events = file
            .chunks
            .iter()
            .filter_map(|chunk| match chunk {
                MIDIFileChunk::Track { events } => Some(events.clone()),
                _ => None,
            })
            .enumerate()
            .flat_map(|(i, track)| {
                let mut time = 0;
                track
                    .iter()
                    .filter_map(|event| {
                        time += event.delta_time as u64;
                        let inner = match &event.inner {
                            MIDITrackInner::Message(MIDIMessage::NoteOn(MIDIMessageNote {
                                channel,
                                note,
                                velocity,
                            })) => MidiEvent::NoteOn {
                                channel: (*channel).max(i as u8), // Workaround for DAWs that don't set the channel
                                note: *note,
                                velocity: *velocity,
                            },
                            MIDITrackInner::Message(MIDIMessage::NoteOff(MIDIMessageNote {
                                channel,
                                note,
                                velocity: _,
                            })) => MidiEvent::NoteOff {
                                channel: (*channel).max(i as u8),
                                note: *note,
                            },
                            MIDITrackInner::Meta(meta) if meta.meta_type == 0x51 => {
                                let microseconds_per_beat = u32::from_be_bytes([
                                    0,
                                    meta.bytes[0],
                                    meta.bytes[1],
                                    meta.bytes[2],
                                ]);
                                let tempo = 60_000_000.0 / microseconds_per_beat as f64;
                                MidiEvent::SetTempo { tempo }
                            }
                            _ => return None,
                        };
                        Some(MidiTrackAccumulateEvent { time, inner })
                    })
                    .collect::<Vec<_>>()
            })
            .sorted_by_key(|event| event.time)
            .collect::<Vec<_>>();

        Self {
            events,
            ticks_per_beat: match file
                .header()
                .expect("MIDI file must have a header chunk")
                .division
            {
                MIDIFileDivision::TicksPerQuarterNote {
                    ticks_per_quarter_note,
                } => ticks_per_quarter_note,
                _ => panic!("Invalid MIDI file division"),
            },
        }
    }

    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self::from_midi_file(
            parse_midi_file::<String, Vec<u8>>(bytes)
                .expect("Failed to parse MIDI file")
                .1,
        )
    }
}

#[derive(Debug, Clone)]
pub enum MidiEvent {
    NoteOn { channel: u8, note: u8, velocity: u8 },
    NoteOff { channel: u8, note: u8 },
    SetTempo { tempo: f64 },
}
