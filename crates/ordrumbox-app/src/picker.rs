//! The file picker: a folder at a time, folders first, only the files a
//! command can open; Enter (or a second click) opens a folder or picks a
//! file, Backspace goes up, a letter jumps to the next name starting with
//! it, Escape cancels. On Windows, above a drive are the drives.

use std::fs;
use std::path::{Path, PathBuf};

use crate::canvas::{Canvas, Rgb, text_width};

/// What the picked file is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    ImportMidi,
    AddSample,
    ReplaceSample,
    /// The track editor's load button: the track's sample replaced.
    ReplaceTrackSample,
    LoadMapping,
    /// A folder, not a file.
    ImportFolder,
}

impl Purpose {
    pub fn title(self) -> &'static str {
        match self {
            Self::ImportMidi => "Import MIDI file",
            Self::AddSample => "Add sample to the kit",
            Self::ReplaceSample => "Replace the selected sample",
            Self::ReplaceTrackSample => "Replace the track's sample",
            Self::LoadMapping => "Load kit mapping",
            Self::ImportFolder => "Import folder as kit",
        }
    }

    /// File extensions shown (lowercase); none for a folder.
    fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::ImportMidi => &["mid", "midi"],
            Self::AddSample | Self::ReplaceSample | Self::ReplaceTrackSample => &["wav"],
            Self::LoadMapping => &["json"],
            Self::ImportFolder => &[],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    /// Pick the folder shown (folder pickers).
    ThisFolder,
    Parent,
    Folder(String),
    File(String),
    /// A drive (Windows, at the top).
    Drive(PathBuf),
}

impl Entry {
    fn label(&self) -> String {
        match self {
            Self::ThisFolder => "[ Use this folder ]".into(),
            Self::Parent => "..".into(),
            Self::Folder(name) => format!("{name}/"),
            Self::File(name) => name.clone(),
            Self::Drive(path) => path.display().to_string(),
        }
    }

    fn name(&self) -> &str {
        match self {
            Self::Folder(name) | Self::File(name) => name,
            _ => "",
        }
    }
}

/// What a step of the picker ends with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Still picking.
    Browsing,
    Picked(PathBuf),
}

#[derive(Debug, Clone)]
pub struct Picker {
    pub purpose: Purpose,
    /// `None`: the list of drives (Windows).
    pub dir: Option<PathBuf>,
    pub entries: Vec<Entry>,
    pub selected: usize,
    /// Why the folder could not be listed.
    pub error: Option<String>,
}

/// The user's home folder, else the working folder.
pub fn home() -> PathBuf {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    var(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn drives() -> Vec<Entry> {
    if !cfg!(windows) {
        return Vec::new();
    }
    (b'A'..=b'Z').map(|l| PathBuf::from(format!("{}:\\", l as char))).filter(|p| p.is_dir()).map(Entry::Drive).collect()
}

impl Picker {
    /// A picker for `purpose`, in `start` (else the home folder).
    pub fn new(purpose: Purpose, start: Option<&Path>) -> Self {
        let dir = start.filter(|d| d.is_dir()).map_or_else(home, Path::to_path_buf);
        let mut picker = Self { purpose, dir: Some(dir), entries: Vec::new(), selected: 0, error: None };
        picker.list();
        picker
    }

    /// Read the folder shown.
    fn list(&mut self) {
        self.error = None;
        self.selected = 0;
        let Some(dir) = &self.dir else {
            self.entries = drives();
            return;
        };
        let mut entries = Vec::new();
        if self.purpose == Purpose::ImportFolder {
            entries.push(Entry::ThisFolder);
        }
        if dir.parent().is_some() || cfg!(windows) {
            entries.push(Entry::Parent);
        }
        let (mut folders, mut files) = (Vec::new(), Vec::new());
        match fs::read_dir(dir) {
            Ok(read) => {
                for entry in read.flatten() {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if name.starts_with('.') {
                        continue;
                    }
                    let path = entry.path();
                    if path.is_dir() {
                        folders.push(name);
                    } else if self.accepts(&path) {
                        files.push(name);
                    }
                }
            }
            Err(e) => self.error = Some(format!("{}: {e}", dir.display())),
        }
        let by_name = |a: &String, b: &String| a.to_lowercase().cmp(&b.to_lowercase()).then(a.cmp(b));
        folders.sort_by(by_name);
        files.sort_by(by_name);
        entries.extend(folders.into_iter().map(Entry::Folder));
        entries.extend(files.into_iter().map(Entry::File));
        self.entries = entries;
    }

    fn accepts(&self, path: &Path) -> bool {
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        self.purpose.extensions().contains(&ext.as_str())
    }

    /// Show `dir`, the entry of `select` (a child name) selected.
    fn show(&mut self, dir: Option<PathBuf>, select: Option<&str>) {
        self.dir = dir;
        self.list();
        if let Some(name) = select {
            self.selected = self.entries.iter().position(|e| e.name() == name).unwrap_or(0);
        }
    }

    /// Backspace: the parent folder (the drives above a Windows drive).
    pub fn up(&mut self) {
        let Some(dir) = self.dir.clone() else { return };
        let name = dir.file_name().map(|n| n.to_string_lossy().into_owned());
        match dir.parent() {
            Some(parent) => self.show(Some(parent.to_path_buf()), name.as_deref()),
            None if cfg!(windows) => self.show(None, None),
            None => {}
        }
    }

    /// Enter: open the selected folder, or pick the selected file.
    pub fn activate(&mut self) -> Outcome {
        let Some(entry) = self.entries.get(self.selected).cloned() else { return Outcome::Browsing };
        match entry {
            Entry::ThisFolder => {
                if let Some(dir) = &self.dir {
                    return Outcome::Picked(dir.clone());
                }
            }
            Entry::Parent => self.up(),
            Entry::Folder(name) => {
                let dir = self.dir.as_ref().map(|d| d.join(&name));
                self.show(dir, None);
            }
            Entry::Drive(path) => self.show(Some(path), None),
            Entry::File(name) => {
                if let Some(dir) = &self.dir {
                    return Outcome::Picked(dir.join(name));
                }
            }
        }
        Outcome::Browsing
    }

    /// Move the selection by `delta` entries (clamped).
    pub fn step(&mut self, delta: isize) {
        let last = self.entries.len().saturating_sub(1) as isize;
        self.selected = (self.selected as isize + delta).clamp(0, last) as usize;
    }

    pub fn home_end(&mut self, end: bool) {
        self.selected = if end { self.entries.len().saturating_sub(1) } else { 0 };
    }

    /// A typed letter: the next entry whose name starts with it.
    pub fn jump(&mut self, letter: char) {
        let letter = letter.to_lowercase().next().unwrap_or(letter);
        let n = self.entries.len();
        for k in 1..=n {
            let i = (self.selected + k) % n;
            if self.entries[i].name().to_lowercase().starts_with(letter) {
                self.selected = i;
                return;
            }
        }
    }

    /// A click on entry `i`: select it, or (already selected) activate it.
    pub fn click(&mut self, i: usize) -> Outcome {
        if i >= self.entries.len() {
            return Outcome::Browsing;
        }
        if i == self.selected {
            return self.activate();
        }
        self.selected = i;
        Outcome::Browsing
    }
}

// ─── Drawing ─────────────────────────────────────────────────────────────────

const SHADE: Rgb = 0x0010_1014;
const PANEL: Rgb = 0x0024_2430;
const BORDER: Rgb = 0x0048_4858;
const HIGHLIGHT: Rgb = 0x0038_3850;
const TEXT: Rgb = 0x00E6_E6EA;
const DIM: Rgb = 0x0080_8090;
const ACCENT: Rgb = 0x00F2_A93B;
const WARNING: Rgb = 0x00E8_6A5A;

/// Where the picker's list is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PickerLayout {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    list_top: i32,
    row: i32,
    rows: usize,
    scale: i32,
}

impl PickerLayout {
    pub fn new(width: i32, height: i32, scale: i32) -> Self {
        let margin = 40 * scale;
        let (w, h) = ((width - 2 * margin).max(80 * scale), (height - 2 * margin).max(80 * scale));
        let (x, y) = ((width - w) / 2, (height - h) / 2);
        let row = 12 * scale;
        let list_top = y + 36 * scale;
        let rows = usize::try_from((y + h - 20 * scale - list_top) / row).unwrap_or(0).max(1);
        Self { x, y, width: w, height: h, list_top, row, rows, scale }
    }

    /// First entry shown so that `selected` is visible.
    fn first(&self, selected: usize) -> usize {
        selected.saturating_sub(self.rows - 1)
    }

    /// The entry under `(x, y)`.
    pub fn hit(&self, picker: &Picker, x: i32, y: i32) -> Option<usize> {
        if x < self.x || x >= self.x + self.width || y < self.list_top {
            return None;
        }
        let row = usize::try_from((y - self.list_top) / self.row).ok()?;
        let i = self.first(picker.selected) + row;
        (row < self.rows && i < picker.entries.len()).then_some(i)
    }

    /// Inside the panel.
    pub fn contains(&self, x: i32, y: i32) -> bool {
        (self.x..self.x + self.width).contains(&x) && (self.y..self.y + self.height).contains(&y)
    }
}

pub fn draw(canvas: &mut Canvas, picker: &Picker, layout: &PickerLayout) {
    let s = layout.scale;
    canvas.fill_rect(layout.x - 4 * s, layout.y - 4 * s, layout.width + 8 * s, layout.height + 8 * s, SHADE);
    canvas.fill_rect(layout.x, layout.y, layout.width, layout.height, BORDER);
    canvas.fill_rect(layout.x + s, layout.y + s, layout.width - 2 * s, layout.height - 2 * s, PANEL);
    let x = layout.x + 10 * s;
    canvas.text(x, layout.y + 8 * s, picker.purpose.title(), s, ACCENT);
    let place = picker.dir.as_ref().map_or_else(|| "Drives".to_owned(), |d| d.display().to_string());
    // Keep the end of a long path visible.
    let room = ((layout.width - 20 * s) / text_width("W", s)).max(4) as usize;
    let count = place.chars().count();
    let place =
        if count > room { format!("...{}", place.chars().skip(count - room + 3).collect::<String>()) } else { place };
    canvas.text(x, layout.y + 20 * s, &place, s, DIM);
    if let Some(error) = &picker.error {
        canvas.text(x, layout.list_top, error, s, WARNING);
    } else if picker.entries.is_empty() {
        canvas.text(x, layout.list_top, "(nothing to open here)", s, DIM);
    }
    let first = layout.first(picker.selected);
    for (row, (i, entry)) in picker.entries.iter().enumerate().skip(first).take(layout.rows).enumerate() {
        let y = layout.list_top + row as i32 * layout.row;
        let selected = i == picker.selected;
        if selected {
            canvas.fill_rect(layout.x + 4 * s, y - 2 * s, layout.width - 8 * s, layout.row, HIGHLIGHT);
        }
        let color = match entry {
            _ if selected => ACCENT,
            Entry::File(_) => TEXT,
            _ => DIM,
        };
        canvas.text(x, y, &entry.label(), s, color);
    }
    let hint = "ENTER or click again: open / choose   BACKSPACE: up   a letter: jump   ESC: cancel";
    canvas.text(x, layout.y + layout.height - 14 * s, hint, s, DIM);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browses_and_picks() {
        let dir = std::env::temp_dir().join(format!("ordrumbox-picker-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("Beats")).unwrap();
        fs::create_dir_all(dir.join("alpha")).unwrap();
        for name in ["b.mid", "A.MID", "notes.txt", ".hidden.mid"] {
            fs::write(dir.join(name), b"x").unwrap();
        }
        fs::write(dir.join("Beats").join("groove.midi"), b"x").unwrap();
        let mut picker = Picker::new(Purpose::ImportMidi, Some(&dir));
        let labels: Vec<String> = picker.entries.iter().map(Entry::label).collect();
        assert_eq!(labels, ["..", "alpha/", "Beats/", "A.MID", "b.mid"]);
        picker.jump('b');
        assert_eq!(picker.entries[picker.selected], Entry::Folder("Beats".into()));
        assert_eq!(picker.activate(), Outcome::Browsing);
        assert_eq!(picker.dir.as_deref(), Some(dir.join("Beats").as_path()));
        assert_eq!(picker.click(1), Outcome::Browsing); // select groove.midi
        assert_eq!(picker.click(1), Outcome::Picked(dir.join("Beats").join("groove.midi")));
        picker.up();
        assert_eq!(picker.entries[picker.selected], Entry::Folder("Beats".into()));

        let mut folders = Picker::new(Purpose::ImportFolder, Some(&dir));
        assert_eq!(folders.entries[0], Entry::ThisFolder);
        assert!(folders.entries.iter().all(|e| !matches!(e, Entry::File(_))));
        assert_eq!(folders.activate(), Outcome::Picked(dir.clone()));

        let layout = PickerLayout::new(800, 600, 1);
        let mut pixels = vec![0; 800 * 600];
        draw(&mut Canvas::new(&mut pixels, 800, 600), &picker, &layout);
        assert_eq!(layout.hit(&picker, layout.x + 20, layout.list_top + 1), Some(0));
        fs::remove_dir_all(dir).unwrap();
    }
}
