use bevy::audio::{AudioPlugin, Volume};
use bevy::prelude::*;
use soundyrust::*;

fn main() {
	let mut app = App::new();
	app.add_plugins(DefaultPlugins.set(AudioPlugin {
		global_volume: GlobalVolume::new(Volume::Linear(0.2)),
		..default()
	}))
	.add_plugins(SoundyPlugin)
	.add_systems(Startup, setup)
	.add_systems(Update, play_keyboard)
	.run();
}

fn setup(mut assets: ResMut<Assets<MidiAudio>>, mut commands: Commands) {
	let audio_handle = assets.add(
		MidiAudio::from_bytes(include_bytes!("../assets/hl4mgm.sf2")).with_track(
			MidiAudioTrack::from_bytes(include_bytes!("../assets/octave.mid"), 4.0 / 4.0),
		),
	);
	commands.spawn((AudioPlayer(audio_handle),));
}

fn play_keyboard(mut assets: ResMut<Assets<MidiAudio>>, input: Res<ButtonInput<KeyCode>>) {
	let notes = [
		(Note::C5, KeyCode::KeyA),
		(Note::D5, KeyCode::KeyS),
		(Note::E5, KeyCode::KeyD),
		(Note::F5, KeyCode::KeyF),
		(Note::G5, KeyCode::KeyG),
		(Note::A5, KeyCode::KeyH),
		(Note::B5, KeyCode::KeyJ),
		(Note::C6, KeyCode::KeyK),
		(Note::D6, KeyCode::KeyL),
		(Note::E6, KeyCode::Semicolon),
	];

	for (note, key) in notes.iter() {
		if input.just_pressed(*key) {
			assets
				.iter_mut()
				.next()
				.unwrap()
				.1
				.start_playing_note(*note)
				.unwrap();
		} else if input.just_released(*key) {
			assets
				.iter_mut()
				.next()
				.unwrap()
				.1
				.stop_playing_note(*note)
				.unwrap();
		}
	}
}
