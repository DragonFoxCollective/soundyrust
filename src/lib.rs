use bevy::audio::AddAudioSource;
use bevy::prelude::*;

pub use midi::MidiTrack;
pub use notes::Note;
pub use rustysynth::SoundFont;
pub use source::{
    MidiAudio, MidiAudioTrack, MidiAudioTrackHandle, MidiBufferMessage, MidiQueueEvent,
    MidiQueueEventType, MidiQueueLooping, MidiQueueTiming, SyncedMidiInfo,
};

mod midi;
mod notes;
mod source;

#[derive(Default)]
pub struct SoundyPlugin;

impl Plugin for SoundyPlugin {
    fn build(&self, app: &mut App) {
        app.add_audio_source::<MidiAudio>()
            .add_systems(PreUpdate, tick_sequencers);
    }
}

fn tick_sequencers(mut audios: ResMut<Assets<MidiAudio>>, time: Res<Time>) {
    for (_id, audio) in audios.iter_mut() {
        audio.tick(time.delta());
    }
}
