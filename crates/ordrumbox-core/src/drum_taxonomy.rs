//! The drum taxonomy (`src/core/drum_taxonomy.js`): where a canonical drum
//! name sits in the stereo field, and which coarse type a track name belongs to.

/// Coarse instrument type of a track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstrumentFamily {
    Kick,
    Snare,
    Hat,
    Clap,
    Cowbell,
    Perc,
    Bass,
    Piano,
    Organ,
}

impl InstrumentFamily {
    /// BASS, PIANO and ORGAN tracks are melodic (auto-generation, empty-track pruning).
    pub fn is_melodic(self) -> bool {
        matches!(self, Self::Bass | Self::Piano | Self::Organ)
    }

    /// KICK, SNARE, HAT, CLAP, COWBELL and PERC: what the drum generation covers (`DRUM_TYPES`).
    pub fn is_drum(self) -> bool {
        !self.is_melodic()
    }

    /// The JS type name (`"KICK"`, `"HAT"`…).
    pub fn name(self) -> &'static str {
        match self {
            Self::Kick => "KICK",
            Self::Snare => "SNARE",
            Self::Hat => "HAT",
            Self::Clap => "CLAP",
            Self::Cowbell => "COWBELL",
            Self::Perc => "PERC",
            Self::Bass => "BASS",
            Self::Piano => "PIANO",
            Self::Organ => "ORGAN",
        }
    }
}

/// Canonical names: `(name, default pan, type)` (`TRACK_KINDS` + `PAN_MAP`).
const TRACK_KINDS: [(&str, f64, InstrumentFamily); 8] = [
    ("KICK", 0.0, InstrumentFamily::Kick),
    ("SNARE", 0.3, InstrumentFamily::Snare),
    ("TOM", 0.5, InstrumentFamily::Perc),
    ("CLAP", -0.4, InstrumentFamily::Clap),
    ("COWBELL", 0.4, InstrumentFamily::Cowbell),
    ("CHH", -0.3, InstrumentFamily::Hat),
    ("OHH", -0.2, InstrumentFamily::Hat),
    ("CRASH", 1.0, InstrumentFamily::Perc),
];

/// Default pan of a track from its exact name (`"KICK"`, `"OHH"`…), 0 for any
/// other name. Case-sensitive, like `getPanFromTrackName()`.
pub fn pan_from_track_name(name: &str) -> f64 {
    TRACK_KINDS.iter().find(|(kind, _, _)| *kind == name).map_or(0.0, |&(_, pan, _)| pan)
}

/// Coarse type of a track: the canonical names first, then substring rules for
/// names like `"808 KICK"` or `"SYNTH BASS"` (`detectTrackType()`).
pub fn instrument_family(name: &str) -> InstrumentFamily {
    let name = name.to_uppercase();
    if let Some(&(_, _, kind)) = TRACK_KINDS.iter().find(|(kind, _, _)| *kind == name) {
        return kind;
    }
    let has = |parts: &[&str]| parts.iter().any(|part| name.contains(part));
    if has(&["KICK", "BD"]) {
        InstrumentFamily::Kick
    } else if has(&["SNARE", "SD"]) {
        InstrumentFamily::Snare
    } else if has(&["OHH", "HAT", "CHH"]) {
        InstrumentFamily::Hat
    } else if has(&["CLAP", "CLP", "CP"]) {
        InstrumentFamily::Clap
    } else if has(&["BASS"]) {
        InstrumentFamily::Bass
    } else if has(&["PIANO"]) {
        InstrumentFamily::Piano
    } else if has(&["COWBELL", "COW"]) {
        InstrumentFamily::Cowbell
    } else if has(&["ORGAN"]) {
        InstrumentFamily::Organ
    } else if has(&["SYNTH"]) {
        InstrumentFamily::Bass
    } else {
        InstrumentFamily::Perc
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pan_follows_the_exact_name() {
        assert_eq!(pan_from_track_name("SNARE"), 0.3);
        assert_eq!(pan_from_track_name("CRASH"), 1.0);
        assert_eq!(pan_from_track_name("snare"), 0.0);
        assert_eq!(pan_from_track_name("KICK_01"), 0.0);
    }

    #[test]
    fn type_from_name() {
        assert_eq!(instrument_family("tom"), InstrumentFamily::Perc);
        assert_eq!(instrument_family("808 KICK"), InstrumentFamily::Kick);
        assert_eq!(instrument_family("Synth Bass"), InstrumentFamily::Bass);
        assert_eq!(instrument_family("SYNTH LEAD"), InstrumentFamily::Bass);
        assert_eq!(instrument_family("RIDE"), InstrumentFamily::Perc);
        assert!(instrument_family("PIANO").is_melodic());
        assert!(!instrument_family("CHH").is_melodic());
        assert!(instrument_family("COWBELL").is_drum());
        assert_eq!(instrument_family("OPEN HAT").name(), "HAT");
    }
}
