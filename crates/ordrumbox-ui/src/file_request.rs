//! A file or folder asked of the program's own dialog (the system's), in
//! place of the file picker drawn in the window.

use std::path::{Path, PathBuf};

use crate::picker::{Export, Purpose};

/// What the dialog asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileKind {
    /// An existing file, one of these extensions (lowercase).
    Open(&'static [&'static str]),
    /// An existing folder.
    Folder,
    /// A file to write, offered under this name.
    Save(String),
}

/// A dialog the program is asked to show: [`Session::take_file_request`]
/// gives it, [`Session::file_chosen`] takes the answer.
///
/// [`Session::take_file_request`]: crate::Session::take_file_request
/// [`Session::file_chosen`]: crate::Session::file_chosen
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRequest {
    purpose: Purpose,
    start: Option<PathBuf>,
    kind: FileKind,
}

impl FileRequest {
    /// A request for `purpose`, the dialog opening in `start`; `file_name` is
    /// what an export of a single file is called.
    pub(crate) fn new(purpose: Purpose, start: Option<&Path>, file_name: Option<String>) -> Self {
        let kind = match (purpose, file_name) {
            (Purpose::Export(export), Some(name)) if export.writes_file() => FileKind::Save(name),
            _ if purpose.picks_folder() => FileKind::Folder,
            _ => FileKind::Open(purpose.extensions()),
        };
        Self { purpose, start: start.filter(|d| d.is_dir()).map(Path::to_path_buf), kind }
    }

    pub(crate) fn purpose(&self) -> Purpose {
        self.purpose
    }

    pub fn title(&self) -> &'static str {
        self.purpose.title()
    }

    /// The folder to open in.
    pub fn start(&self) -> Option<&Path> {
        self.start.as_deref()
    }

    pub fn kind(&self) -> &FileKind {
        &self.kind
    }

    /// The export this writes to the file picked, as it is named in full.
    pub(crate) fn export_to_file(&self) -> Option<Export> {
        match (self.purpose, &self.kind) {
            (Purpose::Export(export), FileKind::Save(_)) => Some(export),
            _ => None,
        }
    }
}
