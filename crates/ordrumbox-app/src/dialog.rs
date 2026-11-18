//! The system's file dialogs, for what the session asks with a `FileRequest`.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};

use ordrumbox_ui::{FileKind, FileRequest, Waker};

/// A dialog open, on a thread of its own: the window keeps drawing and the
/// sound keeps being fed while it is up.
#[derive(Debug)]
pub struct Open {
    request: FileRequest,
    answer: Receiver<Option<PathBuf>>,
}

/// What an open dialog has come to.
#[derive(Debug)]
pub enum Answer {
    /// The path picked, `None` when cancelled.
    Done(FileRequest, Option<PathBuf>),
    /// The thread ended without an answer.
    Lost,
}

impl Open {
    /// Show the dialog `request` asks for; `waker` is called when it is answered.
    pub fn show(request: FileRequest, waker: Waker) -> io::Result<Self> {
        let (sender, answer) = mpsc::channel();
        let asked = request.clone();
        std::thread::Builder::new().name("file dialog".into()).spawn(move || {
            let path = run(&asked);
            // The receiver is gone only when the app is quitting.
            if sender.send(path).is_ok() {
                waker.wake();
            }
        })?;
        Ok(Self { request, answer })
    }

    pub fn poll(self) -> Result<Answer, Self> {
        match self.answer.try_recv() {
            Ok(path) => Ok(Answer::Done(self.request, path)),
            Err(TryRecvError::Empty) => Err(self),
            Err(TryRecvError::Disconnected) => Ok(Answer::Lost),
        }
    }
}

fn run(request: &FileRequest) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new().set_title(request.title());
    if let Some(start) = request.start() {
        dialog = dialog.set_directory(start);
    }
    match request.kind() {
        FileKind::Open(extensions) => dialog.add_filter("Files", extensions).pick_file(),
        FileKind::Folder => dialog.pick_folder(),
        FileKind::Save(name) => {
            dialog = dialog.set_file_name(name.as_str());
            match Path::new(name).extension().and_then(|e| e.to_str()) {
                Some(extension) => dialog.add_filter(extension.to_uppercase(), &[extension]).save_file(),
                None => dialog.save_file(),
            }
        }
    }
}
