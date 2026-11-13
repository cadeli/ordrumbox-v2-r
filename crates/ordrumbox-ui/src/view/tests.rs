use super::*;

#[test]
fn a_wide_grid_shows_more_beats_a_page() {
    let one = Scale::ONE;
    assert_eq!(page_beats(500, one), BEATS_PER_PAGE);
    assert_eq!(page_beats(900, one), 8);
    assert_eq!(page_beats(1600, one), 16);
    assert_eq!(page_beats(1600, Scale::whole(2)), 8, "the same steps at twice the scale");
    assert!(page_beats(-10, one) == BEATS_PER_PAGE);
}

#[test]
fn the_compact_grid_shows_a_full_drum_pattern() {
    let tracks: Vec<String> = (0..COMPACT_ROWS).map(|t| format!(r#"{{"name":"T{t}"}}"#)).collect();
    let json = format!(r#"{{"patterns":[{{"name":"p","tracks":[{}]}}]}}"#, tracks.join(","));
    let library = ordrumbox_core::Library::parse(&json).unwrap().0;
    let pattern = &library.patterns[0];
    for quarters in 4..=Scale::MAX.quarters() {
        let scale = Scale::from_quarters(quarters);
        for note_editor in [false, true] {
            let shown = Shown { note_editor, ..Shown::grid_alone(false) };
            let (width, height) = min_window_size(scale, shown);
            assert!(height < min_window_size(scale, Shown::all(false)).1, "scale {scale}");
            let body = Body::new(width, height, scale, 0, MIN_STATUS_LINES, false, note_editor);
            let layout = Layout::new(body.main_w, body.editor_h, scale, pattern, MIN_STATUS_LINES, 0);
            assert_eq!(layout.tracks, pattern.tracks.len(), "scale {scale}, note editor {note_editor}");
            assert!(layout.bottom <= body.note.y.min(body.main_bottom) || !note_editor, "scale {scale}");
        }
    }
}

#[test]
fn whole_scales_need_whole_multiples_of_the_smallest_window() {
    let (w, h) = min_window_size(Scale::ONE, Shown::all(true));
    assert_eq!(min_window_size(Scale::whole(2), Shown::all(true)), (2 * w, 2 * h));
    // Without the output strip (a plugin) the layout needs less height.
    assert!(min_window_size(Scale::ONE, Shown::all(false)).1 < h);
}

#[test]
fn the_header_groups_are_apart_and_inside_the_header() {
    for quarters in 4..=Scale::MAX.quarters() {
        let scale = Scale::from_quarters(quarters);
        let c = header_controls(scale);
        let groups = c.groups(scale);
        let (width, _) = min_window_size(scale, Shown::all(true));
        let logo_end = margin(scale) + text_width("orDrumbox", header_text(scale));
        assert!(groups[0].x > logo_end, "scale {scale}: the logo");
        assert!(groups[4].x + groups[4].w <= width, "scale {scale}: the window's edge");
        for pair in groups.windows(2) {
            assert!(pair[0].x + pair[0].w < pair[1].x, "scale {scale}: {pair:?}");
        }
        for group in groups {
            assert!(group.y >= menu::bar_height(scale), "scale {scale}: the menu bar");
            assert!(group.y + group.h <= status_line(scale), "scale {scale}: the status line");
        }
    }
}
use ordrumbox_core::Library;

fn library() -> Library {
    Library::parse(
        r#"{"patterns":[{"name":"p","tracks":[{"name":"KICK","loopLength":3,"notes":[{},{"beat":9}]},{"name":"X","stepsPerBeat":0},{"name":"H","stepsPerBeat":2}]}]}"#,
    )
    .unwrap()
    .0
}

#[test]
fn a_sample_label_puts_the_kit_above_the_name_without_its_extension() {
    let label = |path: &str| {
        let l = SoundLabel::sample(path);
        (l.source, l.name)
    };
    let owned = |a: &str, b: &str| (a.to_owned(), b.to_owned());
    assert_eq!(label("real/kick.wav"), owned("real", "kick"));
    assert_eq!(label("human/toms/tom.lo.wav"), owned("human/toms", "tom.lo"), "the last dot only");
    assert_eq!(label("chh"), owned("", "chh"), "no kit, no extension");
    assert_eq!(label("real/.wav"), owned("real", ".wav"), "a name is never emptied");
}

#[test]
fn a_host_transport_is_shown_in_the_transport_group_and_nowhere_else() {
    let scale = Scale::ONE;
    let controls = header_controls(scale);
    let group = controls.groups(scale)[1];
    let (w, h) = (400usize, 80usize);
    let drawn = |host: Option<HostState>| {
        let mut pixels = vec![0; w * h];
        draw_host_state(&mut Canvas::new(&mut pixels, w, h), &controls, host, scale);
        let inside = |i: usize| {
            let (x, y) = ((i % w) as i32, (i / w) as i32);
            x >= group.x && x < group.x + group.w && y >= group.y && y < group.y + group.h
        };
        assert!(pixels.iter().enumerate().all(|(i, p)| *p == 0 || inside(i)), "{host:?}");
        let count = |color: Rgb| pixels.iter().filter(|p| **p == color).count();
        (count(PLAYING), count(theme::muted()))
    };
    let (playing, stopped) = (drawn(Some(HostState { playing: true, bpm: 97.5 })).0, drawn(None).1);
    assert!(playing > 0, "a triangle while the host plays");
    assert!(stopped >= 64, "a square before the host was seen");
    assert_eq!(drawn(Some(HostState { playing: false, bpm: 300.0 })).0, 0, "nothing lit while stopped");
}

#[test]
fn draws_without_panicking_at_any_size() {
    let library = library();
    for (w, h) in [(1, 1), (40, 30), (1024, 640)] {
        let mut pixels = vec![0; w * h];
        let mut canvas = Canvas::new(&mut pixels, w, h);
        let view = View {
            pattern: library.patterns.first(),
            playhead: Some(0.5),
            waiting: None,
            song: Some("SONG demo  measure 3/66"),
            kit_view: None,
            sounds: vec![
                SoundLabel::sample("real/kick.wav"),
                SoundLabel { source: "synth".into(), name: "BASS5".into() },
            ],
            arrangement: None,
            piano_roll: None,
            output: None,
            synth: false,
            right: 300,
            current_page: 0,
            lane: (None, 0),
            note_editor: true,
            levels: vec![0.0; 2],
            show_vus: true,
            transport: Transport::Own,
            track_drop: None,
            host_muted: 0b10,
            cursor: Some(Cursor { track: 0, beat: 1, step: 2 }),
            selected: Some((0, 0)),
            zone: None,
            undo: Some("Add note"),
            redo: None,
            message: "hello",
            status: &["warning".into()],
        };
        draw(&mut canvas, &view, Scale::ONE);
    }
}

#[test]
fn a_note_fills_its_cell_as_high_as_it_is_loud() {
    let filled = |velocity: f64| {
        let (w, h) = (10usize, 40usize);
        let mut pixels = vec![0; w * h];
        let mut canvas = Canvas::new(&mut pixels, w, h);
        let note = Note { velocity, ..Note::default() };
        draw_note_cell(&mut canvas, Rect::new(0, 0, 10, 40), &note, 1);
        pixels.chunks(w).filter(|row| row[5] != 0).count()
    };
    assert_eq!(filled(0.5), 20, "half loud, half high");
    assert_eq!(filled(0.1), 10, "the softest still shows");
    assert_eq!(filled(1.0), 40, "the loudest fills it");
}

#[test]
fn the_ruler_sits_above_the_rows_and_its_beats_start_their_cells() {
    let library = library();
    let pattern = library.patterns.first().unwrap();
    for scale in [Scale::ONE, Scale::from_quarters(6), Scale::whole(2)] {
        let layout = Layout::new(1100 * scale, 688 * scale, scale, pattern, 0, 0);
        assert_eq!(layout.top, body_top(scale) + RULER_HEIGHT * scale);
        assert_eq!(layout.hit(pattern, layout.grid_left + 2, layout.top - 2), None, "the ruler is not a row");
        for beat in 0..layout.beats {
            let cell = layout.cell_rect(0, layout.start_beat + beat, 0, &pattern.tracks[0]);
            assert_eq!(layout.beat_x(beat), cell.x, "beat {beat}");
        }
    }
}

#[test]
fn body_splits_the_window_above_the_output_strip() {
    let library = library();
    let pattern = library.patterns.first().unwrap();
    for scale in [Scale::ONE, Scale::from_quarters(6), Scale::whole(2)] {
        for (w, h) in [(1100, 680), (1280, 800)] {
            for status in [0, 1, 3] {
                for right in [0, 300 * scale] {
                    let body = Body::new(w, h, scale, right, status, true, true);
                    assert_eq!(body.main_w, w - right);
                    // The strip's bottom edge is the footer's top, less a margin.
                    assert_eq!(body.output.y + body.output.h, footer_top(h, scale, status) - margin(scale) / 2);
                    // The main editor stops above the strip, with a gap.
                    assert!(body.main_bottom < body.output.y);
                    // The editor height is virtual: a layout built with it
                    // (and the real status count) ends exactly at main_bottom.
                    let grid = Layout::new(body.main_w, body.editor_h, scale, pattern, status, 0);
                    assert_eq!(grid.bottom, body.note.y - NOTE_GAP * scale);
                    assert_eq!(body.note.y + body.note.h, body.main_bottom);
                    let alone = Body::new(w, h, scale, right, status, true, false);
                    let grid = Layout::new(alone.main_w, alone.editor_h, scale, pattern, status, 0);
                    assert_eq!((grid.bottom, alone.note.h), (body.main_bottom, 0));
                    // Without the strip (a plugin), the same down to where it was.
                    let bare = Body::new(w, h, scale, right, status, false, true);
                    assert_eq!((bare.output.h, bare.main_bottom), (0, body.main_bottom + body.output.h));
                    let grid = Layout::new(bare.main_w, bare.editor_h, scale, pattern, status, 0);
                    assert_eq!(grid.bottom, bare.note.y - NOTE_GAP * scale);
                }
            }
        }
    }
}

#[test]
fn kit_editor_draws_and_hits() {
    let samples: Vec<&str> = (0..40).map(|_| "kick.wav").collect();
    let envelope = [0.0, 1.0, 0.5, 0.25];
    for (w, h) in [(1, 1), (40, 30), (1024, 640)] {
        let mut pixels = vec![0; w * h];
        let mut canvas = Canvas::new(&mut pixels, w, h);
        let kit = KitView {
            kit: "real",
            folder: "/a/very/long/folder/name/that/does/not/fit/in/the/header/line/of/the/kit/editor/real".into(),
            samples: samples.clone(),
            users: vec![vec!["KICK".into(), "CRASH".into()]],
            selected: 39,
            details_top: 200,
            facts: vec![("Pitch", "A4 0ct".into()); KIT_FACTS],
            envelope: &envelope,
            decay_marker: Some(0.5),
        };
        draw_kit(&mut canvas, &kit, &KitLayout::new(w as i32, h as i32, Scale::ONE, 0));
    }
    let layout = KitLayout::new(1024, 640, Scale::ONE, 0);
    // The columns in order, the instrument select inside its row, the
    // details under the table, the measurements right of the controls.
    assert!(layout.left < layout.name_x && layout.name_x < layout.instrument_x);
    let select = layout.instrument_rect(1);
    let auto = layout.auto_rect(1);
    assert!(select.x + select.w < auto.x && auto.x + auto.w < layout.users_x);
    assert!(select.y >= layout.list_top + layout.row && select.y + select.h <= layout.list_top + 2 * layout.row);
    assert!(layout.desc_rect(KIT_DESC_LINES - 1).y < layout.columns_y);
    assert!(layout.list_top + layout.list_rows * layout.row < layout.detail_y);
    let controls = layout.controls_area();
    assert!(controls.x + controls.w < layout.facts_x);
    assert!(layout.facts_bottom() < layout.bottom);
    assert_eq!(layout.hit(0, 40, layout.left + 2, layout.list_top + 1), Some(0));
    assert_eq!(layout.hit(0, 1, layout.left + 2, layout.list_top + layout.row + 1), None);
    assert_eq!(layout.hit(0, 40, layout.left + 2, layout.detail_y), None);
}

#[test]
fn hit_testing_matches_the_drawing() {
    let library = library();
    let pattern = &library.patterns[0];
    let layout = Layout::new(1024, 640, Scale::ONE, pattern, 0, 0);
    let y = layout.row_top(2) + layout.row / 2;
    assert_eq!(layout.hit(pattern, layout.box_x(false) + 1, y), Some(Hit::Mute(2)));
    assert_eq!(layout.hit(pattern, layout.box_x(true) + 1, y), Some(Hit::Solo(2)));
    assert_eq!(layout.hit(pattern, layout.name_x + 2, y), Some(Hit::Name(2)));
    assert_eq!(layout.hit(pattern, layout.volume_x + 2, y), None);
    assert!(layout.volume_rect(2).contains(layout.volume_x + 2, y));
    // Track H: 2 steps per beat; the middle of beat 1's second step.
    let x = layout.step_x(3, 2) + 2;
    assert_eq!(layout.hit(pattern, x, y), Some(Hit::Cell { track: 2, beat: 1, step: 1 }));
    assert_eq!(layout.hit(pattern, layout.grid_left + layout.grid_width + 5, y), None);
    assert_eq!(layout.hit(pattern, x, layout.row_top(3) + 1), None);
    assert_eq!(layout.hit(pattern, x, layout.top - 1), None);
}
