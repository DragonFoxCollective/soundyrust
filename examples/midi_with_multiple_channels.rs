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
	.run();
}

fn setup(mut assets: ResMut<Assets<MidiAudio>>, mut commands: Commands) {
	let audio_handle = assets.add(
		MidiAudio::from_bytes(include_bytes!("../assets/hl4mgm.sf2")).with_track(
			MidiAudioTrack::from_bytes(include_bytes!("../assets/fray.mid"), 4.0 / 4.0),
		),
	);
	commands.spawn((AudioPlayer(audio_handle),));
}
