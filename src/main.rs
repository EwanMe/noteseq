use clap::{Parser, Subcommand};
use core::panic;
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Device, StreamConfig,
};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::{f32::consts::PI, sync::Arc, time::Duration};

const REFERENCE_OCTAVE: i32 = 4;
const REFERENCE_PITCH: f32 = 440.0;

#[derive(Parser)]
#[command(version, about, long_about=None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Single {
        #[arg(help = "The note you want to play")]
        note: String,
    },
    Sequence {
        #[arg(help = "Sequence of notes to play")]
        notes: Vec<String>,
    },
}

#[derive(Copy, Clone)]
struct Note {
    frequency: f32,
    amplitude: f32,
    num_samples: u128,
}

impl Note {
    fn new(
        frequency: f32,
        amplitude: f32,
        duration: Option<Duration>,
        sample_rate: Option<&u128>,
    ) -> Self {
        const FIXME: u128 = 2;
        Note {
            frequency,
            amplitude,
            num_samples: match sample_rate {
                Some(sr) => match duration {
                    Some(d) => d.as_millis() / 1000 * sr * FIXME,
                    None => 0,
                },
                None => 0,
            },
        }
    }
}

fn get_device_config(device: &Device) -> StreamConfig {
    let mut supported_configs_range = device
        .supported_output_configs()
        .expect("Error while querying configs");
    supported_configs_range
        .next()
        .expect("No supported config")
        .with_max_sample_rate()
        .config()
}

fn get_frequency_from_note(note_name: &str) -> f32 {
    if note_name.len() < 2 {
        panic!("Note name too short");
    } else if note_name.len() > 3 {
        panic!("Note name too long");
    }

    let (note, octave): (&str, &str);
    if note_name.contains("#") || note_name.contains("b") {
        (note, octave) = note_name.split_at(2);
    } else {
        (note, octave) = note_name.split_at(1);
    };

    let octave_num = octave
        .parse::<i32>()
        .expect("Failed to parse octave number");

    let mut semitone_distance: i32 = match note {
        "C" => -9,
        "C#" | "Db" => -8,
        "D" => -7,
        "D#" | "Eb" => -6,
        "E" => -5,
        "F" => -4,
        "F#" | "Gb" => -3,
        "G" => -2,
        "G#" | "Ab" => -1,
        "A" => 0,
        "A#" | "Bb" => 1,
        "B" => 2,
        unknown => panic!("Unknown note: {}", unknown),
    };

    semitone_distance += 12 * (octave_num - REFERENCE_OCTAVE);
    2f32.powf(semitone_distance as f32 / 12.0) * REFERENCE_PITCH
}

fn get_frequencies_from_notes(notes: &Vec<String>) -> Vec<f32> {
    notes.iter().map(|f| get_frequency_from_note(f)).collect()
}

struct Player {
    pos: std::vec::IntoIter<Note>,
    sample_rate: u32,
    sample_num: u32,
    current_note: Option<Note>,
}

impl Player {
    fn new(notes: Vec<Note>, sample_rate: u32) -> Self {
        let mut pos = notes.clone().into_iter();
        let current_note = match pos.next() {
            Some(n) => n,
            None => panic!(""),
        };

        Player {
            pos,
            sample_rate,
            sample_num: 0,
            current_note: Some(current_note),
        }
    }

    fn next_note(&mut self) -> Option<Note> {
        self.pos.next()
    }

    fn next_note_val(&mut self) -> Option<Note> {
        match self.current_note {
            Some(current_note) => {
                if current_note.num_samples != 0 {
                    let current_sample_num = self.sample_num;
                    self.sample_num += 1;

                    if (current_sample_num as u128) >= current_note.num_samples {
                        self.sample_num = 0;
                        self.current_note = self.next_note();
                    }
                }
                Some(current_note)
            }
            None => None,
        }
    }

    fn get_next_sample(&mut self) -> Option<f32> {
        static POS: AtomicU32 = AtomicU32::new(0);

        match self.next_note_val() {
            Some(n) => {
                let t = POS.fetch_add(1, Ordering::SeqCst) as f32 / self.sample_rate as f32;
                Some((2.0 * PI * n.frequency * t).sin() * n.amplitude)
            }
            None => None,
        }
    }
}

fn get_note(note_name: &str) -> Vec<Note> {
    let freq = get_frequencies_from_notes(&vec![String::from(note_name)]);
    if freq.len() != 1 {
        panic!("Wtf");
    }

    return vec![Note::new(*freq.first().unwrap(), 0.5, None, None)];
}

fn get_notes(note_names: &Vec<String>) -> Vec<Note> {
    get_frequencies_from_notes(note_names)
        .iter()
        .map(|f| Note::new(*f, 0.5, Some(Duration::from_secs(1)), Some(&48000)))
        .collect()
}

fn main() {
    let cli = Cli::parse();

    let notes = match &cli.command {
        Commands::Single { note } => get_note(note),
        Commands::Sequence { notes } => get_notes(notes),
    };

    let mut player = Player::new(notes, 48000);

    // let wanted_device = "Speakers (Steam Streaming Speakers)";
    let wanted_device = "Speakers (Focusrite USB Audio)";
    // let wanted_device = "CABLE Input (VB-Audio Virtual Cable)";

    let host = cpal::default_host();
    let device = host
        .output_devices()
        .expect("Failed to get output devices")
        .find(|device| device.name().expect("Failed to access name of a device") == wanted_device)
        .expect("Failed to find device");

    let config = get_device_config(&device);

    let done = Arc::new(AtomicBool::new(false));
    let done_clone = Arc::clone(&done);

    let stream = device
        .build_output_stream(
            &config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                for output_sample in data.iter_mut() {
                    match player.get_next_sample() {
                        Some(sample) => {
                            *output_sample = sample;
                        }
                        None => {
                            done_clone.store(true, Ordering::SeqCst);
                        }
                    };
                }
            },
            move |err| {
                eprintln!("Output stream callback failed: {}", err);
            },
            None,
        )
        .expect("Failed to build output stream");

    stream.play().expect("Failed to play audio");

    match &cli.command {
        Commands::Single { note: _ } => {
            println!("Press Enter to exit...");
            let _ = std::io::stdin().read_line(&mut String::new());
        }
        Commands::Sequence { notes: _ } => while !done.load(Ordering::SeqCst) {},
    }
}
