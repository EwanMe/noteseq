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
    /// Note format is on scientific pitch notation followed by ':' and the divisor of a note value
    /// fraction, i.e. <pitch>:<note value>. The pitch format is subdivided into
    /// <note name><accidentals><octave number>. Note name is a case insensitive letter from A-G.
    /// Accidentals are optional and can be any number of '#' and 'b' symbols. Octave number is a
    /// single number from 0-9. The note value part of the note is any number that is a power of
    /// two, i.e. 1, 2, 4, 8, etc. This number represents the fraction of a whole note, where the
    /// provided number is the divisor, e.g. 8 represents an eight note (1/8). Dotted notes can be
    /// played by appending up to 4 dots to the note value. The pitch of the note may also be
    /// omitted, which produces a pause instead of a note.
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

fn get_frequency(
    note_name: &str,
    accidentals: &str,
    octave: Option<i32>,
    tuning: f32,
) -> Result<f32, String> {
    fn count_chars(string: &str, c: char) -> i32 {
        string
            .chars()
            .filter(|x| *x == c)
            .count()
            .try_into()
            .unwrap()
    }
    let mut offset: i32 = 0;
    offset += count_chars(accidentals, '#');
    offset -= count_chars(accidentals, 'b');
    let semitone_offset = offset;

    let octave_num = match octave {
        Some(octave_num) => octave_num,
        None => REFERENCE_OCTAVE,
    };

    let mut semitone_distance: i32 = match note_name.to_uppercase().as_str() {
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
        if self.current_note?.num_samples != 0 {
            if self.sample_num >= self.current_note?.num_samples {
                self.sample_num = 0;
                self.current_note = self.next_note();
            } else {
                self.sample_num += 1;
            }
        }
        self.current_note
    }

    fn get_next_sample(&mut self) -> Option<f32> {
        static POS: AtomicU32 = AtomicU32::new(0);

        let last_freq = self
            .current_note
            .expect("Last note was None, which should not happen before the current note is None")
            .frequency;

        let next_note = self.next_note_val()?;
        let pos = match last_freq == next_note.frequency {
            true => POS.fetch_add(1, Ordering::SeqCst),
            false => {
                let pos = ((last_freq / next_note.frequency) * (POS.load(Ordering::SeqCst) as f32))
                    .round() as u32;
                POS.store(pos, Ordering::SeqCst);
                pos
            }
        };
        let t = pos as f32 / self.sample_rate as f32;
        Some((2.0 * PI * next_note.frequency * t).sin() * next_note.amplitude)
    }
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

fn get_note_duration(note_value: u32, tempo: u32) -> Result<Duration, ArgumentParseError> {
    // Check if number is power of two
    if (note_value != 0) && (note_value & (note_value - 1)) == 0 {
        // Calculate duration of note based on beats per minute (tempo)
        // with 1/4 as one beat.
        let beat_duration = 60.0 * 1000.0 / tempo as f32;
        let beat = 1f32 / 4f32;
        let scale = 1f32 / note_value as f32 / beat;
        let duration = scale * beat_duration;
        Ok(Duration::from_millis(duration as u64))
    } else {
        Err(ArgumentParseError::new(
            format!("Note duration {note_value} was not a power of two").as_str(),
        ))
    }
}

fn get_dotting_duration(num_dots: usize, note_duration: Duration) -> Duration {
    let mut new_duration = Duration::new(0, 0);
    for i in 1..num_dots + 1 {
        new_duration += note_duration / 2u32.pow(i as u32);
    }
    new_duration
}

fn get_note(
    raw_note: &str,
    amplitude: f32,
    tuning: f32,
    tempo: u32,
    sample_rate: u32,
) -> Result<Note, String> {
    let note_re = Regex::new(
        r"^(?P<note>[a-gA-G])?(?P<accidental>(#|b)*)(?P<octave>[0-9]*)(:(?P<value>\d{1,2}))?(?P<dotting>\.{1,4})?$",
    )
    .expect("Invalid regex string for note parsing");
    let captures = match note_re.captures(raw_note) {
        Some(captures) => captures,
        None => {
            return Err(format!(
                "Invalid input '{raw_note}', see --help for correct note syntax"
            ))
        }
    };

    let acc = captures.name("accidental").unwrap().as_str();

    let octave: Option<i32> = match captures.name("octave").unwrap().as_str() {
        "" => None,
        octave => Some(octave.parse().unwrap()),
    };

    let note_value = match captures.name("value") {
        Some(duration) => duration.as_str().parse::<u32>().unwrap(),
        None => 4,
    };
    let mut duration = get_note_duration(note_value, tempo).map_err(|x| x.msg)?;

    match captures.name("dotting") {
        Some(dotting) => duration += get_dotting_duration(dotting.len(), duration),
        None => (),
    };

    match captures.name("note") {
        Some(n) => Note::new(
            get_frequency(n.as_str(), acc, octave, tuning)?,
            amplitude,
            sample_rate,
            duration,
        ),
        // No pitch means this is a pause
        None => Note::new(0f32, amplitude, sample_rate, duration),
    }
}

fn get_dynamic(dynamic_indication: &str) -> Result<f32, String> {
    let dynamics = vec!["ppp", "pp", "p", "mp", "mf", "f", "ff", "fff"];

    match dynamics.iter().position(|elem| elem == &dynamic_indication) {
        Some(i) => Ok((1 * (i + 1)) as f32 / dynamics.len() as f32),
        None => {
            return Err(format!(
                "Unknown dynamic indication '{dynamic_indication}', expected one of {}",
                dynamics.join(", ")
            ))
        }
    }
}

fn get_notes(
    raw_sequence: &Vec<String>,
    tuning: f32,
    tempo: u32,
    sample_rate: u32,
) -> Result<Vec<Note>, String> {
    let mut note_sequence: Vec<Result<Note, String>> = vec![];
    let mut amplitude = 0.5;

    let dynamic_re: Regex = Regex::new(r"(?P<dynamic>^p{1,3}$|^mp$|^mf$|^f{1,3}$)")
        .expect("Invalid regex string for parsing dynamics");

    for arg in raw_sequence.iter() {
        match dynamic_re.captures(arg) {
            Some(n) => {
                let name = n
                    .name("dynamic")
                    .expect("Failed to capture group 'dynamic'")
                    .as_str();
                amplitude = get_dynamic(name)?;
            }
            None => {
                note_sequence.push(get_note(arg, amplitude, tuning, tempo, sample_rate));
            }
        };
    }
    note_sequence.into_iter().collect()
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

    let mut notes = get_notes(&cli.sequence, cli.tuning, cli.tempo, config.sample_rate.0)?;
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
                            break;
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
            println!("Press Enter to stop playback...");
            let _ = std::io::stdin().read_line(&mut String::new());
        }
        false => while !done.load(Ordering::SeqCst) {},
    }
    Ok(())
}
