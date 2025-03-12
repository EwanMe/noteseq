use clap::Parser;
use core::panic;
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Device, SampleRate, StreamConfig,
};
use regex::Regex;
use std::error::Error;
use std::{f32::consts::PI, sync::Arc, time::Duration};
use std::{
    fmt,
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
};

const REFERENCE_OCTAVE: i32 = 4;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    /// Note format is on scientific pitch notation followed by ':' and the divisor of a note value fraction.
    /// Note sequence to play. Format of notes is <pitch>:<note value>. The pitch part is on the
    /// format <note name><accidentals><octave number>. Note name is a case insensitive letter
    /// from A-G. Accidentals are optional and can be any number of '#' and 'b' symbols. Octave
    /// number is a single number from 0-9. The note value part of the note is any number that is
    /// a power of two, i.e. 1, 2, 4, 8, etc. This number represents the fraction of a whole note,
    /// where the provided number is the divisor, e.g. 8 represents an eight note (1/8).
    #[arg(required = true)]
    sequence: Vec<String>,

    /// Hold last note of sequence until stopped by the user
    #[arg(short, long)]
    fermata: bool,

    /// Tempo for note sequence
    #[arg(short, long, default_value_t = 120)]
    tempo: u32,

    /// Reference pitch for note tuning
    #[arg(long, default_value_t = 440.0)]
    tuning: f32,

    /// Device to play playback from
    #[arg(short, long)]
    device: Option<String>,

    /// Sample rate of playback
    #[arg(short, long, default_value_t = 48000)]
    sample_rate: u32,
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
        duration: Duration,
    ) -> Result<Self, String> {
        if frequency > sample_rate as f32 / 2.0 {
            Err(String::from(format!(
                "Cannot create note of frequency {frequency} Hz \
                when the sample rate is {sample_rate} Hz, \
                since it exceeds the Nyquist frequency of {nyquist} Hz.",
                nyquist = sample_rate / 2
            )))
        } else {
            Ok(Note {
                frequency,
                amplitude,
                // Order of operations is important here to avoid truncation
                num_samples: sample_rate as u128 * duration.as_millis() / 1000,
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

fn get_frequency_from_note(note_name: &str, tuning: f32) -> Result<f32, String> {
    fn count_chars(string: &str, c: char) -> i32 {
        string
            .chars()
            .filter(|x| *x == c)
            .count()
            .try_into()
            .unwrap()
    }
    let re = Regex::new(r"^(?P<note>[a-gA-G])(?P<accidental>(#|b)*)(?P<octave>[0-9]?)$").unwrap();
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
    Ok(2f32.powf(semitone_distance as f32 / 12.0) * tuning)
}

struct Player {
    pos: std::vec::IntoIter<Note>,
    sample_rate: u32,
    sample_num: u128,
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

                    if current_sample_num >= current_note.num_samples {
                        self.sample_num = 0;
                        self.current_note = self.next_note();
                    }
                }
                self.current_note
            }
            None => None,
        }
    }

    fn get_next_sample(&mut self) -> Option<f32> {
        static POS: AtomicU32 = AtomicU32::new(0);
        let last_freq = self.current_note.unwrap().frequency;

        match self.next_note_val() {
            Some(n) => {
                let pos = match last_freq != n.frequency {
                    true => {
                        let pos = ((last_freq / n.frequency) * (POS.load(Ordering::SeqCst) as f32))
                            .round() as u32;
                        POS.store(pos, Ordering::SeqCst);
                        pos
                    }
                    false => POS.fetch_add(1, Ordering::SeqCst),
                };
                let t = pos as f32 / self.sample_rate as f32;
                Some((2.0 * PI * n.frequency * t).sin() * n.amplitude)
            }
            None => None,
        }
    }
}

fn get_note(
    note_name: &str,
    tuning: f32,
    sample_rate: u32,
    duration: Duration,
) -> Result<Note, String> {
    Note::new(
        get_frequency_from_note(note_name, tuning)?,
        0.8,
        sample_rate,
        duration,
    )
}

#[derive(Debug)]
struct ArgumentParseError {
    msg: String,
}

impl ArgumentParseError {
    fn new(msg: &str) -> Self {
        ArgumentParseError {
            msg: msg.to_string(),
        }
    }
}

impl fmt::Display for ArgumentParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.msg)
    }
}

impl Error for ArgumentParseError {
    fn description(&self) -> &str {
        &self.msg
    }
}

fn parse_duration(s: &str, tempo: u32) -> Result<Duration, ArgumentParseError> {
    let num = match s.parse::<u32>() {
        Ok(n) => n,
        Err(e) => {
            return Err(ArgumentParseError::new(
                format!("Failed parse integer from string '{s}': {e}").as_str(),
            ))
        }
    };

    // Check if number is power of two
    if (num != 0) && (num & (num - 1)) == 0 {
        // Calculate duration of note based on beats per minute (tempo)
        // with 1/4 as one beat.
        let beat_duration = 60.0 * 1000.0 / tempo as f32;
        let beat = 1f32 / 4f32;
        let scale = 1f32 / num as f32 / beat;
        let duration = scale * beat_duration;
        Ok(Duration::from_millis(duration as u64))
    } else {
        Err(ArgumentParseError::new(
            format!("Note duration {num} was not a power of two").as_str(),
        ))
    }
}

fn parse_notes(
    s: &Vec<String>,
    tuning: f32,
    tempo: u32,
    sample_rate: u32,
) -> Result<Vec<Note>, String> {
    let t: Result<Vec<Note>, ArgumentParseError> = s
        .iter()
        .map(|n| {
            let notes: Vec<&str> = n.split(':').collect();
            if notes.len() < 2 {
                return Ok(get_note(notes[0], tuning, sample_rate, Duration::new(1, 0)).unwrap());
            } else if notes.len() < 3 {
                return Ok(get_note(
                    notes[0],
                    tuning,
                    sample_rate,
                    parse_duration(notes[1], tempo).unwrap(),
                )
                .unwrap());
            } else {
                return Err(ArgumentParseError::new(""));
            }
        })
        .collect();
    t.map_err(|e| e.to_string())
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

    let mut notes = parse_notes(&cli.sequence, cli.tuning, cli.tempo, config.sample_rate.0)?;
    if cli.fermata {
        let last = notes.len() - 1;
        notes[last].num_samples = 0;
    }

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

    match &cli.fermata {
        true => {
            println!("Press Enter to exit...");
            let _ = std::io::stdin().read_line(&mut String::new());
        }
        false => while !done.load(Ordering::SeqCst) {},
    }
    Ok(())
}
