//! A sound's measurements as one SVG picture: envelope, spectrogram with its
//! pitch and centroid, average spectrum.

use crate::measure::{Measures, SPECTROGRAM_BANDS, SPECTROGRAM_MIN_HZ, TimePoint, band_hz};

const WIDTH: f64 = 960.0;
const LEFT: f64 = 60.0;
const PLOT_WIDTH: f64 = WIDTH - LEFT - 20.0;
const ENVELOPE_TOP: f64 = 40.0;
const ENVELOPE_HEIGHT: f64 = 130.0;
const SPECTROGRAM_TOP: f64 = 200.0;
const SPECTROGRAM_HEIGHT: f64 = 240.0;
const SPECTRUM_TOP: f64 = 470.0;
const SPECTRUM_HEIGHT: f64 = 130.0;
const HEIGHT: f64 = SPECTRUM_TOP + SPECTRUM_HEIGHT + 30.0;
/// Most points of the envelope drawn: a song has one per millisecond.
const ENVELOPE_POINTS: usize = 2000;
/// Lowest level drawn, dB.
const RANGE_DB: f64 = 80.0;
const BACKGROUND: &str = "#14161a";
const TEXT: &str = "#d8dce2";
const GRID: &str = "#343841";
const ENVELOPE_COLOR: &str = "#6fc3ff";
const SPECTRUM_COLOR: &str = "#9be28c";
const PITCH_COLOR: &str = "#ffffff";
const CENTROID_COLOR: &str = "#ffb454";
const FREQUENCY_TICKS: [f64; 7] = [50.0, 100.0, 500.0, 1000.0, 2000.0, 5000.0, 10_000.0];

/// Black to violet to orange to pale yellow, for 0 (quietest) to 1 (loudest).
fn heat(level: f64) -> String {
    const STOPS: [[f64; 3]; 4] = [[14.0, 16.0, 22.0], [94.0, 36.0, 140.0], [240.0, 120.0, 40.0], [255.0, 245.0, 190.0]];
    let position = level.clamp(0.0, 1.0) * (STOPS.len() - 1) as f64;
    let index = (position as usize).min(STOPS.len() - 2);
    let fraction = position - index as f64;
    let channel = |c: usize| (STOPS[index][c] + (STOPS[index + 1][c] - STOPS[index][c]) * fraction) as u8;
    format!("#{:02x}{:02x}{:02x}", channel(0), channel(1), channel(2))
}

fn label(svg: &mut String, x: f64, y: f64, text: &str) {
    *svg += &format!("<text x=\"{x:.1}\" y=\"{y:.1}\" fill=\"{TEXT}\" font-size=\"11\">{text}</text>\n");
}

fn line(svg: &mut String, from: (f64, f64), to: (f64, f64)) {
    *svg += &format!(
        "<line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" stroke=\"{GRID}\"/>\n",
        from.0, from.1, to.0, to.1
    );
}

fn polyline(svg: &mut String, points: &[(f64, f64)], color: &str) {
    let list: Vec<String> = points.iter().map(|(x, y)| format!("{x:.1},{y:.1}")).collect();
    *svg +=
        &format!("<polyline points=\"{}\" fill=\"none\" stroke=\"{color}\" stroke-width=\"1.5\"/>\n", list.join(" "));
}

/// Keep `<`, `&` and quotes of a sound's name out of the markup.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The picture of `m`, titled `name`.
pub fn to_svg(name: &str, m: &Measures) -> String {
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{WIDTH}\" height=\"{HEIGHT}\" viewBox=\"0 0 {WIDTH} {HEIGHT}\" \
         font-family=\"monospace\">\n<rect width=\"{WIDTH}\" height=\"{HEIGHT}\" fill=\"{BACKGROUND}\"/>\n"
    );
    let note = m.note.as_deref().unwrap_or("no note");
    svg += &format!(
        "<text x=\"{LEFT}\" y=\"22\" fill=\"{TEXT}\" font-size=\"14\">{} | peak {:.3} | {note}</text>\n",
        escape(name),
        m.peak
    );
    let duration = m.duration_ms.max(1.0);
    let time_x = |ms: f64| LEFT + (ms / duration).clamp(0.0, 1.0) * PLOT_WIDTH;

    // Envelope, 0 dB at the top.
    label(&mut svg, 4.0, ENVELOPE_TOP + 10.0, "0 dB");
    label(&mut svg, 4.0, ENVELOPE_TOP + ENVELOPE_HEIGHT, "-80");
    line(&mut svg, (LEFT, ENVELOPE_TOP), (LEFT + PLOT_WIDTH, ENVELOPE_TOP));
    line(&mut svg, (LEFT, ENVELOPE_TOP + ENVELOPE_HEIGHT), (LEFT + PLOT_WIDTH, ENVELOPE_TOP + ENVELOPE_HEIGHT));
    let db_y = |db: f64, top: f64, height: f64| top + (-db / RANGE_DB).clamp(0.0, 1.0) * height;
    let envelope: Vec<(f64, f64)> = m
        .envelope_db
        .iter()
        .step_by((m.envelope_db.len() / ENVELOPE_POINTS).max(1))
        .map(|p| (time_x(p.ms), db_y(p.value.max(-RANGE_DB), ENVELOPE_TOP, ENVELOPE_HEIGHT)))
        .collect();
    polyline(&mut svg, &envelope, ENVELOPE_COLOR);
    let tick_count = 8;
    for tick in 0..=tick_count {
        let ms = duration * f64::from(tick) / f64::from(tick_count);
        label(&mut svg, time_x(ms) - 10.0, ENVELOPE_TOP + ENVELOPE_HEIGHT + 14.0, &format!("{ms:.0}"));
    }
    label(&mut svg, LEFT + PLOT_WIDTH - 20.0, ENVELOPE_TOP - 4.0, "ms");

    // Spectrogram, log frequency.
    let nyquist = f64::from(m.sample_rate) / 2.0;
    let log_share =
        |hz: f64| (hz.max(SPECTROGRAM_MIN_HZ) / SPECTROGRAM_MIN_HZ).ln() / (nyquist / SPECTROGRAM_MIN_HZ).ln();
    let gram_y = |hz: f64| SPECTROGRAM_TOP + SPECTROGRAM_HEIGHT * (1.0 - log_share(hz).clamp(0.0, 1.0));
    let columns = m.spectrogram_db.len().max(1) as f64;
    let cell_width = PLOT_WIDTH / columns;
    for (c, column) in m.spectrogram_db.iter().enumerate() {
        for (band, &db) in column.iter().enumerate() {
            let (y_low, y_high) = (gram_y(band_hz(band, nyquist)), gram_y(band_hz(band + 1, nyquist)));
            svg += &format!(
                "<rect x=\"{:.1}\" y=\"{y_high:.1}\" width=\"{:.2}\" height=\"{:.2}\" fill=\"{}\"/>\n",
                LEFT + c as f64 * cell_width,
                cell_width + 0.5,
                y_low - y_high + 0.5,
                heat(1.0 + db / RANGE_DB)
            );
        }
    }
    debug_assert!(m.spectrogram_db.iter().all(|column| column.len() == SPECTROGRAM_BANDS));
    for hz in FREQUENCY_TICKS.iter().filter(|&&hz| hz < nyquist) {
        label(&mut svg, 4.0, gram_y(*hz) + 4.0, &format!("{hz:.0}"));
    }
    let track_points = |track: &[TimePoint]| track.iter().map(|p| (time_x(p.ms), gram_y(p.value))).collect::<Vec<_>>();
    polyline(&mut svg, &track_points(&m.centroid_track), CENTROID_COLOR);
    polyline(&mut svg, &track_points(&m.pitch_track), PITCH_COLOR);
    label(&mut svg, LEFT, SPECTROGRAM_TOP - 6.0, "Hz | white: pitch | orange: centroid");

    // Average spectrum, log frequency.
    let spectrum_x = |hz: f64| LEFT + log_share(hz).clamp(0.0, 1.0) * PLOT_WIDTH;
    line(&mut svg, (LEFT, SPECTRUM_TOP), (LEFT + PLOT_WIDTH, SPECTRUM_TOP));
    line(&mut svg, (LEFT, SPECTRUM_TOP + SPECTRUM_HEIGHT), (LEFT + PLOT_WIDTH, SPECTRUM_TOP + SPECTRUM_HEIGHT));
    label(&mut svg, 4.0, SPECTRUM_TOP + 10.0, "0 dB");
    label(&mut svg, 4.0, SPECTRUM_TOP + SPECTRUM_HEIGHT, "-80");
    let spectrum: Vec<(f64, f64)> = m
        .spectrum_db
        .iter()
        .enumerate()
        .map(|(i, &db)| (i as f64 * m.spectrum_bin_hz, db))
        .filter(|(hz, _)| *hz >= SPECTROGRAM_MIN_HZ)
        .map(|(hz, db)| (spectrum_x(hz), db_y(db.max(-RANGE_DB), SPECTRUM_TOP, SPECTRUM_HEIGHT)))
        .collect();
    polyline(&mut svg, &spectrum, SPECTRUM_COLOR);
    for hz in FREQUENCY_TICKS.iter().filter(|&&hz| hz < nyquist) {
        label(&mut svg, spectrum_x(*hz) - 10.0, SPECTRUM_TOP + SPECTRUM_HEIGHT + 14.0, &format!("{hz:.0}"));
    }
    label(&mut svg, LEFT + PLOT_WIDTH - 20.0, SPECTRUM_TOP - 4.0, "Hz");
    svg + "</svg>\n"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measure::measure;
    use ordrumbox_dsp::SampleBuffer;

    #[test]
    fn the_picture_holds_only_finite_coordinates() {
        let samples = (0..6000).map(|i| (f64::from(i) * 0.05).sin() as f32 * (1.0 - f64::from(i) / 6000.0) as f32);
        let svg = to_svg("a<b", &measure(&SampleBuffer::mono(samples.collect(), 48_000)));
        assert!(svg.starts_with("<svg") && svg.ends_with("</svg>\n"));
        assert!(svg.contains("a&lt;b"));
        assert!(!svg.contains("NaN") && !svg.contains("inf"));
        // Silence draws too.
        let silent = to_svg("x", &measure(&SampleBuffer::mono(vec![0.0; 100], 48_000)));
        assert!(!silent.contains("NaN"));
    }
}
