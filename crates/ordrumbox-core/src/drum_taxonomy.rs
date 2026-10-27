//! The drum taxonomy (`src/core/drum_taxonomy.js`): where a canonical drum
//! name sits in the stereo field, and which coarse type a track name belongs to.

/// Coarse instrument type of a track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackType {
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

impl TrackType {
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
const TRACK_KINDS: [(&str, f64, TrackType); 8] = [
    ("KICK", 0.0, TrackType::Kick),
    ("SNARE", 0.3, TrackType::Snare),
    ("TOM", 0.5, TrackType::Perc),
    ("CLAP", -0.4, TrackType::Clap),
    ("COWBELL", 0.4, TrackType::Cowbell),
    ("CHH", -0.3, TrackType::Hat),
    ("OHH", -0.2, TrackType::Hat),
    ("CRASH", 1.0, TrackType::Perc),
];

/// Default pan of a track from its exact name (`"KICK"`, `"OHH"`…), 0 for any
/// other name. Case-sensitive, like `getPanFromTrackName()`.
pub fn pan_from_track_name(name: &str) -> f64 {
    TRACK_KINDS.iter().find(|(kind, _, _)| *kind == name).map_or(0.0, |&(_, pan, _)| pan)
}

/// Coarse type of a track: the canonical names first, then substring rules for
/// names like `"808 KICK"` or `"SYNTH BASS"` (`detectTrackType()`).
pub fn detect_track_type(name: &str) -> TrackType {
    let name = name.to_uppercase();
    if let Some(&(_, _, kind)) = TRACK_KINDS.iter().find(|(kind, _, _)| *kind == name) {
        return kind;
    }
    let has = |parts: &[&str]| parts.iter().any(|part| name.contains(part));
    if has(&["KICK", "BD"]) {
        TrackType::Kick
    } else if has(&["SNARE", "SD"]) {
        TrackType::Snare
    } else if has(&["OHH", "HAT", "CHH"]) {
        TrackType::Hat
    } else if has(&["CLAP", "CLP", "CP"]) {
        TrackType::Clap
    } else if has(&["BASS"]) {
        TrackType::Bass
    } else if has(&["PIANO"]) {
        TrackType::Piano
    } else if has(&["COWBELL", "COW"]) {
        TrackType::Cowbell
    } else if has(&["ORGAN"]) {
        TrackType::Organ
    } else if has(&["SYNTH"]) {
        TrackType::Bass
    } else {
        TrackType::Perc
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
        assert_eq!(detect_track_type("tom"), TrackType::Perc);
        assert_eq!(detect_track_type("808 KICK"), TrackType::Kick);
        assert_eq!(detect_track_type("Synth Bass"), TrackType::Bass);
        assert_eq!(detect_track_type("SYNTH LEAD"), TrackType::Bass);
        assert_eq!(detect_track_type("RIDE"), TrackType::Perc);
        assert!(detect_track_type("PIANO").is_melodic());
        assert!(!detect_track_type("CHH").is_melodic());
        assert!(detect_track_type("COWBELL").is_drum());
        assert_eq!(detect_track_type("OPEN HAT").name(), "HAT");
    }
}
