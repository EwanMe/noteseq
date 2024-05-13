use clap::Parser;
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Device, StreamConfig,
};
use std::f32::consts::PI;
use std::sync::atomic::{AtomicI32, Ordering};

const REFERENCE_OCTAVE: i32 = 4;
const REFERENCE_PITCH: f32 = 440.0;

#[derive(Parser)]
#[command(version, about, long_about=None)]
struct Cli {
    #[arg(help = "The note you want to play")]
    note: String,
}

fn get_device_config(device: &Device) -> StreamConfig {
    let mut supported_configs_range = device
        .supported_output_configs()
        .expect("Error while querying configs");
    return supported_configs_range
        .next()
        .expect("No supported config")
        .with_max_sample_rate()
        .config();
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
    return 2f32.powf(semitone_distance as f32 / 12.0) * REFERENCE_PITCH;
}

fn get_next_sample(frequency: f32, amplitude: &f32, sample_rate: &f32) -> f32 {
    static POS: AtomicI32 = AtomicI32::new(0);
    let t = POS.fetch_add(1, Ordering::Release) as f32 / sample_rate;
    (2.0 * PI * frequency * t).sin() * amplitude
}

fn main() {
    let cli = Cli::parse();
    let frequency = get_frequency_from_note(&cli.note);
    let amplitude = 0.5;

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

    let stream = device
        .build_output_stream(
            &config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                for output_sample in data.iter_mut() {
                    *output_sample =
                        get_next_sample(frequency, &amplitude, &(config.sample_rate.0 as f32));
                }
            },
            move |err| {
                eprintln!("Error: {}", err);
            },
            None,
        )
        .expect("Failed to build output stream");

    println!("Playing: {}", frequency);
    stream.play().expect("Failed to play audio");

    println!("Press Enter to exit...");
    let _ = std::io::stdin().read_line(&mut String::new());
}
