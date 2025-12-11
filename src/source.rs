use std::collections::VecDeque;
use std::io::Cursor;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bevy::platform::collections::{HashMap, HashSet};
use bevy::{audio::Source, prelude::*};
use num_enum::TryFromPrimitive;
use rustysynth::{SampleHeader, SoundFont};

use crate::Note;
use crate::midi::{MidiEvent, MidiTrack};

#[derive(Asset, TypePath)]
pub struct MidiAudio {
    tracks: HashMap<MidiAudioTrackHandle, MidiAudioTrack>,
    soundfont: SoundFontBank,
    num_audio_channels: u16,
    current_audio_channel: u16,
    samples_per_second: f64,
    buffer: Arc<Mutex<VecDeque<i16>>>,
    buffer_events: Vec<(Instant, MidiBufferMessage)>,
    buffer_event_now: Instant,
}

impl MidiAudio {
    pub fn new(soundfont: Arc<SoundFont>) -> Self {
        Self {
            tracks: HashMap::new(),
            soundfont: SoundFontBank::new(soundfont),
            num_audio_channels: 2,
            current_audio_channel: 0,
            samples_per_second: 44100.0,
            buffer: Arc::new(Mutex::new(VecDeque::new())),
            buffer_events: vec![],
            buffer_event_now: Instant::now(),
        }
    }

    pub fn add_track(&mut self, midi_track: MidiAudioTrack) -> MidiAudioTrackHandle {
        let handle = MidiAudioTrackHandle(self.tracks.len());
        self.tracks.insert(handle, midi_track);
        handle
    }

    pub fn with_track(mut self, midi_track: MidiAudioTrack) -> Self {
        self.add_track(midi_track);
        self
    }

    pub fn from_bytes(soundfont_bytes: &[u8]) -> Self {
        let soundfont = Arc::new(SoundFont::new(&mut Cursor::new(soundfont_bytes)).unwrap());
        Self::new(soundfont)
    }

    pub fn tick(&mut self, delta: Duration) {
        self.buffer_event_now += delta;

        let ticks = delta.as_secs_f64() * self.samples_per_second;
        let max_ticks = self.samples_per_second
            - self.buffer.lock().unwrap().len() as f64 / self.num_audio_channels as f64;
        let ticks = ticks.min(max_ticks) as usize;

        let mut buffer = VecDeque::with_capacity(ticks * self.num_audio_channels as usize);
        self.tick_n_times(ticks, &mut buffer);

        let buffer = buffer
            .into_iter()
            .enumerate()
            .filter_map(|(i, message)| match message {
                MidiBufferMessage::Audio(sample) => Some(sample),
                #[allow(unreachable_patterns)]
                _ => {
                    self.buffer_events.push((
                        self.buffer_event_now
                            + Duration::from_secs_f64(i as f64 / self.samples_per_second),
                        message,
                    ));
                    None
                }
            });
        self.buffer.lock().unwrap().extend(buffer);

        self.buffer_events
            .retain(|(time, _)| *time > self.buffer_event_now);
    }

    fn tick_n_times(&mut self, ticks: usize, buffer: &mut VecDeque<MidiBufferMessage>) {
        for _ in 0..ticks * self.num_audio_channels as usize {
            self.tick_once(buffer);
        }
    }

    fn tick_once(&mut self, buffer: &mut VecDeque<MidiBufferMessage>) {
        if self.current_audio_channel == 0 {
            let mut timings = HashSet::new();
            for track in self.tracks.values_mut().filter(|track| track.is_playing) {
                track.tick_timing(&mut timings);
            }

            for track in self.tracks.values_mut() {
                let mut new_queue = vec![];
                track.queue.retain(|event| {
                    if timings.contains(&event.timing) {
                        match &event.event {
                            MidiQueueEventType::Play => track.is_playing = true,
                            MidiQueueEventType::Stop => track.is_playing = false,
                            MidiQueueEventType::Queue(new_event) => {
                                new_queue.push(new_event.as_ref().clone())
                            }
                        }
                        event.looping == MidiQueueLooping::Loop
                    } else {
                        true
                    }
                });
                track.queue.append(&mut new_queue);
            }

            for track in self.tracks.values_mut().filter(|track| track.is_playing) {
                track.tick_midi(&self.soundfont);
            }
        }

        let sample = self
            .tracks
            .values_mut()
            .flat_map(|track| track.channels.values())
            .flat_map(|channel| channel.voices.values())
            .map(|voice| {
                voice.sample(
                    self.soundfont.soundfont.get_wave_data(),
                    self.current_audio_channel,
                )
            })
            .sum::<i32>()
            .clamp(i16::MIN as i32, i16::MAX as i32) as i16;

        if self.current_audio_channel == 0 {
            self.tracks
                .values_mut()
                .flat_map(|track| track.channels.values_mut())
                .flat_map(|channel| channel.voices.values_mut())
                .for_each(Voice::tick);
        }
        self.current_audio_channel = (self.current_audio_channel + 1) % self.num_audio_channels;

        buffer.push_back(MidiBufferMessage::Audio(sample));
    }

    pub fn queue(&mut self, handle: MidiAudioTrackHandle, event: MidiQueueEvent) {
        if let Some(track) = self.tracks.get_mut(&handle) {
            track.queue.push(event)
        }
    }

    pub fn start_playing_note(&mut self, note: Note) -> Result<(), NoTracksError> {
        self.tracks
            .get_mut(&MidiAudioTrackHandle(0))
            .ok_or(NoTracksError)?
            .interpret_event(
                MidiEvent::NoteOn {
                    channel: 0,
                    note: note.position(),
                    velocity: 127,
                },
                &self.soundfont,
            );
        Ok(())
    }

    pub fn stop_playing_note(&mut self, note: Note) -> Result<(), NoTracksError> {
        self.tracks
            .get_mut(&MidiAudioTrackHandle(0))
            .ok_or(NoTracksError)?
            .interpret_event(
                MidiEvent::NoteOff {
                    channel: 0,
                    note: note.position(),
                },
                &self.soundfont,
            );
        Ok(())
    }

    pub fn is_playing(&self, handle: &MidiAudioTrackHandle) -> bool {
        self.tracks
            .get(handle)
            .is_some_and(|track| track.is_playing)
    }

    pub fn beats_per_second(&self, handle: &MidiAudioTrackHandle) -> Option<f64> {
        self.tracks.get(handle).map(|track| track.beats_per_second)
    }

    pub fn beats_per_bar(&self, handle: &MidiAudioTrackHandle) -> Option<f64> {
        self.tracks.get(handle).map(|track| track.beats_per_bar)
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NoTracksError;

pub struct MidiAudioTrack {
    midi_track: MidiTrack,
    /// Track => Channel => Note => Voice
    channels: HashMap<u8, Channel>,
    ticks_per_sample: f64,
    samples_per_second: f64,
    beats_per_second: f64,
    tick: f64,
    beat: f64,
    event_index: usize,
    beats_per_bar: f64,
    queue: Vec<MidiQueueEvent>,
    is_playing: bool,
}

impl MidiAudioTrack {
    pub fn new(midi_track: MidiTrack, time_signature: f64) -> Self {
        let samples_per_second = 44100.0;
        let beats_per_second = 120.0 / 60.0;
        let ticks_per_beat = midi_track.ticks_per_beat as f64;
        let ticks_per_sample = (ticks_per_beat * beats_per_second) / samples_per_second;

        let beats_per_bar = time_signature * 4.0;

        let channels = (0..16)
            .map(|i| {
                (
                    i,
                    Channel {
                        bank_number: if i == 9 { 128 } else { 0 },
                        patch_number: 0,
                        voices: HashMap::new(),
                    },
                )
            })
            .collect();

        Self {
            midi_track,
            channels,
            ticks_per_sample,
            samples_per_second,
            beats_per_second,
            tick: 0.0,
            beat: 0.0,
            event_index: 0,
            beats_per_bar,
            queue: vec![],
            is_playing: true,
        }
    }

    pub fn from_bytes(track_bytes: &[u8], time_signature: f64) -> Self {
        Self::new(MidiTrack::from_bytes(track_bytes), time_signature)
    }

    pub fn with_channel_patch(
        mut self,
        channel_number: u8,
        bank_number: u8,
        patch_number: u8,
    ) -> Self {
        self.channels.insert(
            channel_number,
            Channel {
                bank_number,
                patch_number,
                voices: HashMap::new(),
            },
        );
        self
    }

    pub fn with_queue(mut self, event: MidiQueueEvent) -> Self {
        self.queue.push(event);
        self
    }

    pub fn stopped(mut self) -> Self {
        self.is_playing = false;
        self
    }

    pub fn tick_timing(&mut self, timings: &mut HashSet<MidiQueueTiming>) {
        self.tick += self.ticks_per_sample;

        if self.beat == 0.0 {
            timings.insert(MidiQueueTiming::Loop);
        }

        let last_beat = self.beat.floor();
        let last_bar = (last_beat / self.beats_per_bar).floor();
        self.beat += self.beats_per_second / self.samples_per_second;
        let current_beat = self.beat.floor();
        let current_bar = (current_beat / self.beats_per_bar).floor();

        if last_beat != current_beat {
            timings.insert(MidiQueueTiming::Beat);
            if last_bar != current_bar {
                timings.insert(MidiQueueTiming::Bar);
            }
        }
    }

    pub fn tick_midi(&mut self, soundfont: &SoundFontBank) {
        while let Some(event) = self
            .midi_track
            .events
            .get(self.event_index)
            .filter(|event| event.time <= self.tick as u64)
        {
            self.interpret_event(event.inner.clone(), soundfont);
            self.event_index += 1;

            if self.event_index >= self.midi_track.events.len() {
                self.event_index = 0;
                self.tick = 0.0;
                self.beat = 0.0;
            }
        }
    }

    pub fn interpret_event(&mut self, event: MidiEvent, soundfont: &SoundFontBank) {
        match event {
            MidiEvent::NoteOn {
                channel,
                note,
                velocity,
            } => {
                if let Some(voice) = self.create_voice(channel, note, velocity, soundfont)
                    && let Some(channel) = self.channels.get_mut(&channel)
                {
                    channel.voices.insert(note, voice);
                }
            }
            MidiEvent::NoteOff { channel, note } => {
                if let Some(channel) = self.channels.get_mut(&channel) {
                    channel.voices.remove(&note);
                }
            }
            MidiEvent::SetTempo {
                tempo: beats_per_minute,
            } => {
                self.beats_per_second = beats_per_minute / 60.0;
                self.ticks_per_sample = (self.midi_track.ticks_per_beat as f64
                    * self.beats_per_second)
                    / self.samples_per_second;
            }
        }
    }

    fn create_voice(
        &self,
        channel_index: u8,
        note: u8,
        velocity: u8,
        soundfont: &SoundFontBank,
    ) -> Option<Voice> {
        let note = note as i32;
        let velocity = velocity as i32;
        let volume = velocity as f32 / 127.0;

        let channel = &self.channels[&channel_index];
        let sample_headers = soundfont.get_sample_headers(
            note,
            velocity,
            channel.bank_number,
            channel.patch_number,
        )?;
        let samples = sample_headers
            .into_iter()
            .map(|sample| VoiceSample {
                speed: 2_f32.powf(
                    (note as f32 - sample.get_original_pitch() as f32
                        + sample.get_pitch_correction() as f32 / 100.0)
                        / 12.0,
                ),
                current_sample: sample.get_start() as f64,
                end_sample: sample.get_end() as f64,
                sample_type: sample.get_sample_type().try_into().unwrap(),
                volume,
            })
            .collect::<Vec<_>>();
        if samples.is_empty() {
            return None;
        }
        Some(Voice { samples })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect)]
pub struct MidiAudioTrackHandle(usize);

pub struct MidiDecoder {
    buffer: Arc<Mutex<VecDeque<i16>>>,
    num_audio_channels: u16,
    samples_per_second: u32,
}

impl Iterator for MidiDecoder {
    type Item = i16;

    fn next(&mut self) -> Option<Self::Item> {
        self.buffer.lock().unwrap().pop_front().or(Some(0))
    }
}

impl Source for MidiDecoder {
    fn current_frame_len(&self) -> Option<usize> {
        if self.buffer.lock().unwrap().is_empty() {
            Some(1)
        } else {
            None
        }
    }

    fn channels(&self) -> u16 {
        self.num_audio_channels
    }

    fn sample_rate(&self) -> u32 {
        self.samples_per_second
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

impl Decodable for MidiAudio {
    type DecoderItem = <MidiDecoder as Iterator>::Item;

    type Decoder = MidiDecoder;

    fn decoder(&self) -> Self::Decoder {
        MidiDecoder {
            buffer: self.buffer.clone(),
            num_audio_channels: self.num_audio_channels,
            samples_per_second: self.samples_per_second as u32,
        }
    }
}

struct Voice {
    samples: Vec<VoiceSample>,
}

impl Voice {
    fn tick(&mut self) {
        self.samples.iter_mut().for_each(VoiceSample::tick);
    }

    fn sample(&self, wave_data: &[i16], current_audio_channel: u16) -> i32 {
        self.samples
            .iter()
            .filter(|sample| sample.current_sample < sample.end_sample) // Remove this once loops are implemented
            .filter(|sample| {
                sample.sample_type == SampleType::Mono || {
                    if current_audio_channel == 0 {
                        sample.sample_type == SampleType::Left
                    } else {
                        sample.sample_type == SampleType::Right
                    }
                }
            })
            .map(|sample| {
                // This seems like such a hassle... Do we really need to interpolate?
                let current_sample = sample.current_sample;
                let floor = wave_data[current_sample.floor() as usize] as f32;
                let ceil = wave_data[current_sample.ceil() as usize] as f32;
                let fraction = current_sample.fract() as f32;
                ((ceil * fraction + floor * (1.0 - fraction)) * sample.volume) as i32
            })
            .sum::<i32>()
    }
}

struct VoiceSample {
    speed: f32,
    current_sample: f64,
    end_sample: f64,
    sample_type: SampleType,
    volume: f32,
}

impl VoiceSample {
    fn tick(&mut self) {
        self.current_sample += self.speed as f64;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, TryFromPrimitive)]
#[repr(i32)]
enum SampleType {
    Mono = 1,
    Right = 2,
    Left = 4,
    // There's also a "linked" type but I'm unsure when this would be used, usually `link` is just the other stereo channel
}

struct Channel {
    bank_number: u8,
    patch_number: u8,
    voices: HashMap<u8, Voice>,
}

#[derive(Default, Clone)]
pub struct SyncedMidiInfo {
    pub beat: f64,
    pub beats_per_second: f64,
}

pub enum MidiBufferMessage {
    Audio(i16),
}

pub struct SoundFontBank {
    soundfont: Arc<SoundFont>,
    preset_index: HashMap<(u8, u8), usize>,
}

impl SoundFontBank {
    pub fn new(soundfont: Arc<SoundFont>) -> Self {
        let preset_index = soundfont
            .get_presets()
            .iter()
            .enumerate()
            .map(|(index, preset)| {
                (
                    (
                        preset.get_bank_number() as u8,
                        preset.get_patch_number() as u8,
                    ),
                    index,
                )
            })
            .collect();
        Self {
            soundfont,
            preset_index,
        }
    }

    pub fn get_sample_headers(
        &self,
        note: i32,
        velocity: i32,
        bank_number: u8,
        patch_number: u8,
    ) -> Option<Vec<&SampleHeader>> {
        let &preset_index = self.preset_index.get(&(bank_number, patch_number))?;
        let preset = &self.soundfont.get_presets()[preset_index];
        let preset_regions = preset
            .get_regions()
            .iter()
            .filter(|region| region.contains(note, velocity));
        let instruments = preset_regions
            .map(|region| &self.soundfont.get_instruments()[region.get_instrument_id()]);
        let instrument_regions = instruments.flat_map(|instrument| {
            instrument
                .get_regions()
                .iter()
                .filter(|region| region.contains(note, velocity))
        });
        let sample_headers = instrument_regions
            .map(|region| &self.soundfont.get_sample_headers()[region.get_sample_id()]);
        Some(sample_headers.collect())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MidiQueueEvent {
    pub event: MidiQueueEventType,
    pub timing: MidiQueueTiming,
    pub looping: MidiQueueLooping,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MidiQueueTiming {
    Loop,
    Bar,
    Beat,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MidiQueueEventType {
    Play,
    Stop,
    Queue(Box<MidiQueueEvent>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MidiQueueLooping {
    Loop,
    Once,
}
