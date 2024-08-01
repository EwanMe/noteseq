use clap::{Parser, Subcommand};
use core::panic;
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Device, SampleRate, StreamConfig,
};
use regex::Regex;
use std::error::Error;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::{f32::consts::PI, sync::Arc, time::Duration};

const REFERENCE_OCTAVE: i32 = 4;
const REFERENCE_PITCH: f32 = 440.0;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Device to play playback from
    #[arg(short, long)]
    device: Option<String>,

    /// Sample rate of playback
    #[arg(short, long, default_value_t = 48000)]
    sample_rate: u32,
}

#[derive(Subcommand)]
enum Commands {
    /// Play single note until manually stopped
    #[clap(alias = "fer")]
    Fermata {
        /// The note you want to play
        note: String,
    },

    /// Play sequence of notes
    #[clap(alias = "seq")]
    Sequence {
        /// Sequence of notes to play
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
        sample_rate: u32,
        duration: Option<Duration>,
    ) -> Result<Self, String> {
        if frequency > sample_rate as f32 / 2.0 {
            Err(String::from(format!(
                "Cannot play note of frequency {frequency} Hz \
                when the sample rate is {sample_rate} Hz, \
                since it exceeds the Nyquist frequency of {nyquist} Hz.",
                nyquist = sample_rate / 2
            )))
        } else {
            Ok(Note {
                frequency,
                amplitude,
                num_samples: match duration {
                    Some(d) => d.as_millis() / 1000 * sample_rate as u128,
                    None => 0,
                },
            })
        }
    }
}

fn get_device_config(device: &Device, sample_rate: u32) -> StreamConfig {
    device
        .supported_output_configs()
        .expect("Error while querying configs")
        .next()
        .expect("No supported config")
        .with_sample_rate(SampleRate(sample_rate))
        .config()
}

fn get_frequency_from_note(note_name: &str) -> Result<f32, String> {
    fn count_chars(string: &str, c: char) -> i32 {
        string
            .chars()
            .filter(|x| *x == c)
            .count()
            .try_into()
            .unwrap()
    }
    let re = Regex::new(r"^(?P<note>[a-gA-G])(?P<accidental>#*|b*)(?P<octave>[0-9]?)$").unwrap();
    let captures = match re.captures(note_name) {
        Some(captures) => captures,
        None => {
            return Err(format!(
                "Invalid note: {note_name}. Must be letter from A-G (case insensitive), \
            optionally followed by accidental # or b and an octave number 0-9. E.g. C#4."
            ))
        }
    };

    let note = captures.name("note").unwrap().as_str();
    let semitone_offset = match captures.name("accidental").unwrap().as_str() {
        "" => 0,
        accidental => {
            let mut offset: i32 = 0;
            offset += count_chars(accidental, '#');
            offset -= count_chars(accidental, 'b');
            offset
        }
    };

    let octave_num = match captures.name("octave").unwrap().as_str() {
        "" => REFERENCE_OCTAVE,
        octave => octave
            .parse::<i32>()
            .expect(format!("Failed to parse '{octave}' into octave number").as_str()),
    };

    let mut semitone_distance: i32 = match note.to_uppercase().as_str() {
        "C" => -9,
        "D" => -7,
        "E" => -5,
        "F" => -4,
        "G" => -2,
        "A" => 0,
        "B" => 2,
        unknown => return Err(format!("Unknown note: {unknown}")),
    };

    semitone_distance += 12 * (octave_num - REFERENCE_OCTAVE) + semitone_offset;
    Ok(2f32.powf(semitone_distance as f32 / 12.0) * REFERENCE_PITCH)
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

fn get_note(note_name: &str, sample_rate: u32, duration: Option<Duration>) -> Result<Note, String> {
    Note::new(
        get_frequency_from_note(note_name)?,
        0.8,
        sample_rate,
        duration,
    )
}

fn get_notes(
    note_names: &Vec<String>,
    sample_rate: u32,
    duration: Option<Duration>,
) -> Result<Vec<Note>, String> {
    note_names
        .iter()
        .map(|note_name| get_note(note_name, sample_rate, duration))
        .collect()
}

fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();

    let host = cpal::default_host();

    let device = match cli.device {
        Some(wanted_device) => host
            .output_devices()
            .expect("Failed to get output devices")
            .find(|device| {
                device.name().expect("Failed to access name of a device") == wanted_device
            })
            .expect(format!("Failed to find device {}", wanted_device).as_str()),
        None => host.default_output_device().unwrap(),
    };
    let config = get_device_config(&device, cli.sample_rate);

    let notes = match &cli.command {
        Commands::Fermata { note } => vec![get_note(note, config.sample_rate.0, None)?],
        Commands::Sequence { notes } => {
            get_notes(notes, config.sample_rate.0, Some(Duration::new(1, 0)))?
        }
    };

    let mut player = Player::new(notes, config.sample_rate.0);

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
        Commands::Fermata { note: _ } => {
            println!("Press Enter to exit...");
            let _ = std::io::stdin().read_line(&mut String::new());
        }
        Commands::Sequence { notes: _ } => while !done.load(Ordering::SeqCst) {},
    }
    Ok(())
}
