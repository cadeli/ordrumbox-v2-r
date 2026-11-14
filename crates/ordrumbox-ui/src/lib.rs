//! orDrumbox user interface, independent of the window and of the sound
//! device: the session (the library being edited, playback, saving), its
//! software-rendered views and the input that drives them.
//!
//! A program gives it a sound output ([`AudioOutput`]), translates its window
//! events into [`ui_input`] and hands [`Ui`] a pixel buffer to draw in. The
//! app does it with winit, softbuffer and cpal; a plugin with its host's.

mod action_log;
mod assets;
#[cfg(test)]
mod assign_tests;
mod audio_output;
mod canvas;
mod control;
mod control_help;
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
mod sample_overlay;
mod scale;
mod scroll;
mod session;
mod song_view;
mod spectrum;
mod startup_error;
mod store;
mod synth_view;
#[cfg(test)]
mod test_support;
mod theme;
mod ui;
pub mod ui_input;
mod update;
mod user_data;
mod view;

pub use audio_output::{
    AudioOutput, HostOutput, MidiInput, OutputDevices, StartOutput, StartedOutput, chosen_device, chosen_midi_input,
    host_output,
};
pub use font::load as load_font;
pub use mcp_link::run_bridge as run_mcp_bridge;
pub use menu::Command;
pub use panel::Id as PanelId;
pub use scale::Scale;
pub use scroll::{MIN_WINDOW, grip_side};
pub use session::{LibrarySource, Session};
pub use startup_error::report as report_startup_error;
pub use ui::{Idle, Ui};
pub use update::{BUILD, Waker};
pub use view::{Shown, min_window_size};
