//! `ordrumbox-probe`: measures synth presets and WAV files (`README.md`,
//! "Sound measurements").

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use ordrumbox_core::json::{self, Object, Value};
use ordrumbox_engine::SynthPresets;
use ordrumbox_engine::synth_preset::note_seconds;
use ordrumbox_engine::{SampleBuffer, wav};
use ordrumbox_probe::kit::{parse_note, summarize};
use ordrumbox_probe::measure::{Measures, measure};
use ordrumbox_probe::mix::measure_mix;
use ordrumbox_probe::plot::to_svg;
use ordrumbox_probe::render::{NoteSettings, render_note, repeat_difference_db};
use ordrumbox_probe::report::{
    compare, kit_to_json, kit_to_text, mix_to_json, mix_to_text, report_to_json, sound_to_json, sound_to_text,
};

const USAGE: &str = "\
ordrumbox-probe preset <name>... | --all  [options]   measure synth presets
ordrumbox-probe wav <file>...  [--json f] [--svg d]   measure WAV files, one sound each
ordrumbox-probe kit [<kit>...]  [--kits d] [...]      review kits: a summary and what is worth a look
ordrumbox-probe mix <file>...  [--bpm n] [...]        measure exports: loudness, true peak, hits
ordrumbox-probe compare <before.json> <after.json>    what changed between two reports

options of preset:
  --presets <file>        presets file (default: assets/data/synth_presets.json of this checkout)
  --note <C3>             note played (default C3)
  --velocity <0-1>        default 0.8
  --pan <-1..1>           default 0
  --bpm <n>               default 120
  --steps-per-beat <n>    default 4; the app holds a synth note for one step
  --gate <seconds>        hold the note this long instead
  --rate <hz>             default 48000
options of kit:
  --kits <folder>         one folder per kit (default: assets/kits of this checkout)
options of mix:
  --bpm <n>               check the hits on a grid of this tempo
  --steps-per-beat <n>    of that grid, default 4
all:
  --json <file>           write the report, curves included
  --svg <folder>          write one picture per sound
";
const PRESETS_FILE: &str = "assets/data/synth_presets.json";
const KITS_FOLDER: &str = "assets/kits";
const WAV_EXTENSION: &str = "wav";
/// MIDI note of ratio 1 (`synth_preset::C3_FREQ`).
const C3_MIDI: i32 = 48;
const MAX_INDENT: usize = 2;

/// The options after a command's positional arguments.
#[derive(Debug, Default)]
struct Options {
    positional: Vec<String>,
    all: bool,
    presets: Option<PathBuf>,
    kits: Option<PathBuf>,
    /// Given with `--bpm`: a mix is checked on a grid only when asked.
    bpm_given: bool,
    json: Option<PathBuf>,
    svg: Option<PathBuf>,
    note_text: Option<String>,
    settings: NoteSettings,
}

/// Parse `text` as `key`'s number, within `range`.
fn number_in(key: &str, text: &str, range: std::ops::RangeInclusive<f64>) -> Result<f64, String> {
    let value: f64 = text.parse().map_err(|_| format!("{key}: {text} is not a number"))?;
    if !range.contains(&value) {
        return Err(format!("{key}: {text} is out of {} to {}", range.start(), range.end()));
    }
    Ok(value)
}

/// Pitch ratio of a note name such as `C3` or `F#2`.
fn note_ratio(text: &str) -> Result<f64, String> {
    let midi = parse_note(text).map_err(|e| format!("--note: {e}"))?;
    Ok(2f64.powf(f64::from(midi - C3_MIDI) / 12.0))
}

fn parse_options(arguments: &[String]) -> Result<Options, String> {
    let mut options = Options::default();
    let mut iter = arguments.iter();
    while let Some(argument) = iter.next() {
        if argument == "--all" {
            options.all = true;
            continue;
        }
        if !argument.starts_with("--") {
            options.positional.push(argument.clone());
            continue;
        }
        let value = iter.next().ok_or_else(|| format!("{argument} needs a value"))?;
        let settings = &mut options.settings;
        match argument.as_str() {
            "--presets" => options.presets = Some(value.into()),
            "--kits" => options.kits = Some(value.into()),
            "--json" => options.json = Some(value.into()),
            "--svg" => options.svg = Some(value.into()),
            "--note" => {
                settings.ratio = note_ratio(value)?;
                options.note_text = Some(value.clone());
            }
            "--velocity" => settings.velocity = number_in(argument, value, 0.0..=1.0)?,
            "--pan" => settings.pan = number_in(argument, value, -1.0..=1.0)?,
            "--bpm" => {
                settings.bpm = number_in(argument, value, 1.0..=999.0)?;
                options.bpm_given = true;
            }
            "--steps-per-beat" => settings.steps_per_beat = number_in(argument, value, 1.0..=64.0)? as i32,
            "--gate" => settings.gate_seconds = Some(number_in(argument, value, 0.0..=60.0)?),
            "--rate" => settings.sample_rate = number_in(argument, value, 8000.0..=192_000.0)? as u32,
            _ => return Err(format!("unknown option {argument}\n\n{USAGE}")),
        }
    }
    Ok(options)
}

/// `relative` in the checkout around the current folder.
fn in_checkout(relative: &str, option: &str) -> Result<PathBuf, String> {
    let current = std::env::current_dir().map_err(|e| format!("current folder: {e}"))?;
    current
        .ancestors()
        .map(|folder| folder.join(relative))
        .find(|path| path.exists())
        .ok_or_else(|| format!("no {relative} above {}: give {option}", current.display()))
}

/// Every WAV file under `folder`, sorted.
fn wav_files(folder: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    for entry in entries {
        let path = entry.map_err(|e| format!("{}: {e}", folder.display()))?.path();
        if path.is_dir() {
            wav_files(&path, files)?;
        } else if path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case(WAV_EXTENSION)) {
            files.push(path);
        }
    }
    files.sort();
    Ok(())
}

/// A name for each file: its stem, or with its folder when two share a stem.
fn unique_names(files: &[String]) -> Vec<String> {
    let stem = |file: &String| {
        Path::new(file).with_extension("").file_name().map_or(file.clone(), |s| s.to_string_lossy().into_owned())
    };
    files
        .iter()
        .map(|file| {
            let name = stem(file);
            if files.iter().filter(|other| stem(other) == name).count() == 1 {
                return name;
            }
            let parent = Path::new(file).parent().and_then(Path::file_name);
            parent.map_or(name.clone(), |folder| format!("{}/{name}", folder.to_string_lossy()))
        })
        .collect()
}

fn read_wav(path: &Path) -> Result<SampleBuffer, String> {
    let bytes = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    wav::decode(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}

fn write_svg(name: &str, m: &Measures, options: &Options) -> Result<(), String> {
    let Some(folder) = &options.svg else { return Ok(()) };
    fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    let file: String = name.chars().map(|c| if c.is_alphanumeric() || c == '-' { c } else { '_' }).collect();
    let path = folder.join(format!("{file}.svg"));
    fs::write(&path, to_svg(name, m)).map_err(|e| format!("{}: {e}", path.display()))
}

fn write_report(report: &Value, options: &Options) -> Result<(), String> {
    let Some(path) = &options.json else { return Ok(()) };
    fs::write(path, report.to_json_pretty(MAX_INDENT)).map_err(|e| format!("{}: {e}", path.display()))
}

fn read_json(path: &Path) -> Result<Value, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    json::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Write what the options ask for and print the sound.
#[derive(Debug, Default)]
struct Outputs {
    sounds: Vec<Value>,
}

impl Outputs {
    fn add(&mut self, name: &str, m: &Measures, repeat: Option<Option<f64>>, options: &Options) -> Result<(), String> {
        print!("{}", sound_to_text(name, m, repeat));
        self.sounds.push(sound_to_json(name, m, repeat));
        write_svg(name, m, options)
    }

    fn finish(self, settings: Object, options: &Options) -> Result<(), String> {
        write_report(&report_to_json(settings, self.sounds), options)
    }
}

fn run_presets(arguments: &[String]) -> Result<(), String> {
    let options = parse_options(arguments)?;
    let path = options.presets.clone().map_or_else(|| in_checkout(PRESETS_FILE, "--presets"), Ok)?;
    let value = read_json(&path)?;
    if value.as_object().is_none() {
        return Err(format!("{}: not an object of presets", path.display()));
    }
    let presets = SynthPresets::from_json(&value);
    let names: Vec<String> = if options.all {
        presets.keys().iter().map(|&key| key.to_owned()).collect()
    } else {
        options.positional.clone()
    };
    if names.is_empty() {
        return Err(format!("give preset names or --all; {} holds: {}", path.display(), presets.keys().join(" ")));
    }
    let settings = options.settings;
    let mut outputs = Outputs::default();
    for name in &names {
        let preset = presets
            .get(name)
            .ok_or_else(|| format!("no preset {name}; {} holds: {}", path.display(), presets.keys().join(" ")))?;
        let buffer = render_note(preset, &settings).map_err(|e| format!("{name}: {e}"))?;
        let repeat = repeat_difference_db(preset, &settings, &buffer).map_err(|e| format!("{name}: {e}"))?;
        outputs.add(name, &measure(&buffer), Some(repeat), &options)?;
    }
    let mut described = Object::new();
    described.insert("note", Value::from(options.note_text.as_deref().unwrap_or("C3")));
    described.insert("velocity", Value::from(settings.velocity));
    described.insert("pan", Value::from(settings.pan));
    described.insert("bpm", Value::from(settings.bpm));
    described.insert("gateSeconds", Value::from(settings.gate()));
    described.insert("sampleRate", Value::from(settings.sample_rate));
    outputs.finish(described, &options)
}

fn run_wavs(arguments: &[String]) -> Result<(), String> {
    let options = parse_options(arguments)?;
    if options.positional.is_empty() {
        return Err(format!("give WAV files\n\n{USAGE}"));
    }
    let mut outputs = Outputs::default();
    for (file, name) in options.positional.iter().zip(unique_names(&options.positional)) {
        outputs.add(&name, &measure(&read_wav(Path::new(file))?), None, &options)?;
    }
    outputs.finish(Object::new(), &options)
}

/// Review each kit: its folder is a kit, its samples are named `kit/sample`. A
/// sample that cannot be read is listed and fails the run, after the others.
fn run_kits(arguments: &[String]) -> Result<(), String> {
    let options = parse_options(arguments)?;
    let root = options.kits.clone().map_or_else(|| in_checkout(KITS_FOLDER, "--kits"), Ok)?;
    let entries = fs::read_dir(&root).map_err(|e| format!("{}: {e}", root.display()))?;
    let mut folders = Vec::new();
    for entry in entries {
        let path = entry.map_err(|e| format!("{}: {e}", root.display()))?.path();
        if path.is_dir() {
            folders.push(path);
        }
    }
    folders.sort();
    let kit_name = |folder: &PathBuf| folder.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned());
    let known: Vec<String> = folders.iter().map(kit_name).collect();
    if let Some(unknown) = options.positional.iter().find(|name| !known.contains(name)) {
        return Err(format!("no kit {unknown}; {} holds: {}", root.display(), known.join(" ")));
    }
    let (mut sounds, mut kits, mut unreadable) = (Vec::new(), Vec::new(), Vec::new());
    for (folder, kit) in folders.iter().zip(&known) {
        if !options.positional.is_empty() && !options.positional.contains(kit) {
            continue;
        }
        let mut files = Vec::new();
        wav_files(folder, &mut files)?;
        let mut samples = Vec::new();
        for file in &files {
            let relative = file.strip_prefix(folder).unwrap_or(file).with_extension("");
            let name = format!("{kit}/{}", relative.display());
            match read_wav(file) {
                Ok(buffer) => {
                    let m = measure(&buffer);
                    sounds.push(sound_to_json(&name, &m, None));
                    write_svg(&name, &m, &options)?;
                    samples.push((name, m));
                }
                Err(message) => unreadable.push(message),
            }
        }
        let summary = summarize(kit, &samples);
        print!("{}", kit_to_text(&summary));
        kits.push(kit_to_json(&summary));
    }
    let mut settings = Object::new();
    settings.insert("kits", Value::from(kits));
    write_report(&report_to_json(settings, sounds), &options)?;
    if unreadable.is_empty() {
        return Ok(());
    }
    Err(format!("{} samples could not be read:\n{}", unreadable.len(), unreadable.join("\n")))
}

fn run_mixes(arguments: &[String]) -> Result<(), String> {
    let options = parse_options(arguments)?;
    if options.positional.is_empty() {
        return Err(format!("give WAV files\n\n{USAGE}"));
    }
    let settings = options.settings;
    let step_ms = options.bpm_given.then(|| note_seconds(settings.bpm, settings.steps_per_beat) * 1000.0);
    let mut sounds = Vec::new();
    for (file, name) in options.positional.iter().zip(unique_names(&options.positional)) {
        let buffer = read_wav(Path::new(file))?;
        let mut m = measure(&buffer);
        let mix = measure_mix(&buffer, step_ms);
        print!("{}", mix_to_text(&name, &m, &mix));
        sounds.push(mix_to_json(&name, &m, &mix));
        // A mix has no one pitch: the picture would show the detector's guesses.
        m.pitch_track.clear();
        m.note = None;
        write_svg(&name, &m, &options)?;
    }
    write_report(&report_to_json(Object::new(), sounds), &options)
}

fn run(arguments: &[String]) -> Result<(), String> {
    match arguments.split_first() {
        Some((command, rest)) if command == "preset" => run_presets(rest),
        Some((command, rest)) if command == "wav" => run_wavs(rest),
        Some((command, rest)) if command == "kit" => run_kits(rest),
        Some((command, rest)) if command == "mix" => run_mixes(rest),
        Some((command, [before, after])) if command == "compare" => {
            print!("{}", compare(&read_json(Path::new(before))?, &read_json(Path::new(after))?)?);
            Ok(())
        }
        Some((command, _)) if command == "--help" || command == "-h" => {
            print!("{USAGE}");
            Ok(())
        }
        _ => Err(USAGE.to_owned()),
    }
}

#[expect(clippy::print_stderr, reason = "a command-line tool: its terminal is where a failure goes")]
fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match run(&arguments) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_give_their_ratio() {
        assert_eq!(note_ratio("C3").unwrap(), 1.0);
        assert_eq!(note_ratio("c4").unwrap(), 2.0);
        assert!((note_ratio("F#2").unwrap() - 2f64.powf(-6.0 / 12.0)).abs() < 1e-12);
        assert!(note_ratio("H3").is_err() && note_ratio("C").is_err());
    }

    #[test]
    fn names_are_unique() {
        let files = ["a/kick.wav", "b/kick.wav", "b/snare.wav"].map(str::to_owned);
        assert_eq!(unique_names(&files), ["a/kick", "b/kick", "snare"]);
    }

    #[test]
    fn options_refuse_what_they_cannot_use() {
        let parse = |list: &[&str]| parse_options(&list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>());
        let options = parse(&["BASS0", "--velocity", "0.5", "--gate", "1"]).unwrap();
        assert_eq!(options.positional, ["BASS0"]);
        assert_eq!((options.settings.velocity, options.settings.gate_seconds), (0.5, Some(1.0)));
        assert!(parse(&["--velocity", "2"]).is_err());
        assert!(parse(&["--bpm"]).is_err());
        assert!(parse(&["--nope", "1"]).is_err());
    }
}
