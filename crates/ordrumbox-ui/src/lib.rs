//! orDrumbox user interface, independent of the window and of the sound
//! device: the session (the library being edited, playback, saving), its
//! software-rendered views and the input that drives them.
//!
//! A program gives it a sound output ([`AudioOutput`]), translates its window
//! events into [`ui_input`] and hands [`Ui`] a pixel buffer to draw in. The
//! app does it with winit, softbuffer and cpal; a plugin with its host's.

mod action_log;
mod assets;
mod audio_output;
mod canvas;
mod control;
mod editors;
mod font;
mod frame;
mod kits;
mod master;
mod mcp;
mod mcp_link;
mod menu;
mod note_drag;
mod output;
mod panel;
mod piano_roll;
mod picker;
mod session;
mod song_view;
mod spectrum;
mod store;
mod synth_view;
mod theme;
mod ui;
pub mod ui_input;
mod update;
mod user_data;
mod view;

pub use audio_output::{AudioOutput, HostOutput, StartOutput, StartedOutput, host_output};
pub use font::load as load_font;
pub use mcp_link::run_bridge as run_mcp_bridge;
pub use menu::Command;
pub use panel::Id as PanelId;
pub use session::{LibrarySource, Session};
pub use ui::{Idle, Ui};
pub use update::{BUILD, Waker};
