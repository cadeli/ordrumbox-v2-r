//! The words of the tooltip over a track's host mark: what a plugin host
//! does to the track, one line per setting it moves off its default.

use ordrumbox_engine::HostTrack;

/// The tooltip of track `track_index` (from 0) named `name`: a title, then
/// each setting `host` moves, with the amount, and the output when the track
/// plays on its own (`own_output`). Empty when the host does nothing to it.
pub(crate) fn host_track_lines(name: &str, track_index: usize, host: &HostTrack, own_output: bool) -> Vec<String> {
    let default = HostTrack::default();
    let mut moves = Vec::new();
    if host.gain != default.gain {
        moves.push(if host.gain > 0.0 {
            format!("volume {:+.1} dB", 20.0 * host.gain.log10())
        } else {
            "volume off".to_owned()
        });
    }
    let amounts = [
        ("pan", host.pan, default.pan),
        ("cutoff", host.cutoff_octaves, default.cutoff_octaves),
        ("q", host.q, default.q),
        ("reverb", host.reverb, default.reverb),
        ("delay", host.delay, default.delay),
        ("feedback", host.feedback, default.feedback),
        ("saturation", host.saturation, default.saturation),
        ("compressor", host.compressor, default.compressor),
    ];
    for (label, value, default) in amounts {
        if value != default {
            let unit = if label == "cutoff" { " oct" } else { "" };
            moves.push(format!("{label} {value:+.2}{unit}"));
        }
    }
    if host.threshold_db != default.threshold_db {
        moves.push(format!("threshold {:+.1} dB", host.threshold_db));
    }
    if host.transpose != default.transpose {
        moves.push(format!("transpose {:+} st", host.transpose));
    }
    if own_output {
        moves.push(format!("own output: Track {}", track_index + 1));
    }
    if moves.is_empty() {
        return moves;
    }
    moves.insert(0, format!("{name}: set by the host"));
    moves
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_track_the_host_leaves_alone_has_no_tip() {
        assert!(host_track_lines("KICK", 0, &HostTrack::default(), false).is_empty());
    }

    #[test]
    fn each_move_is_a_line_with_its_amount() {
        let host = HostTrack {
            gain: 2.0,
            pan: -0.3,
            cutoff_octaves: 1.0,
            threshold_db: -6.0,
            transpose: 2,
            ..HostTrack::default()
        };
        assert_eq!(
            host_track_lines("SNARE", 2, &host, true),
            [
                "SNARE: set by the host",
                "volume +6.0 dB",
                "pan -0.30",
                "cutoff +1.00 oct",
                "threshold -6.0 dB",
                "transpose +2 st",
                "own output: Track 3",
            ]
        );
        let silent = HostTrack { gain: 0.0, ..HostTrack::default() };
        assert_eq!(host_track_lines("TOM", 3, &silent, false), ["TOM: set by the host", "volume off"]);
    }
}
