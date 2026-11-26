//! orDrumbox user interface, independent of the window and of the sound
//! device: the session (the library being edited, playback, saving), its
//! software-rendered views and the input that drives them.
//!
//! A program gives it a sound output ([`AudioOutput`]), translates its window
//! events into [`ui_input`] and hands [`Ui`] a pixel buffer to draw in. The
//! app does it with winit, softbuffer and cpal.

mod action_log;
mod assets;
#[cfg(test)]
mod assign_tests;
mod audio_output;
mod canvas;
mod control;
mod control_help;
mod demo_browser;
mod editors;
mod file_request;
mod font;
mod frame;
mod image;
mod kits;
mod loop_marks;
mod master;
mod mcp;
mod mcp_link;
mod menu;
mod menu_action;
mod midi_settings;
mod mixer;
mod note_drag;
mod output;
mod panel;
mod pattern_ruler;
mod piano_roll;
mod picker;
mod returns;
mod sample_overlay;
mod scale;
mod scroll;
mod scroll_thumb;
mod session;
mod settings_section;
mod single_instance;
mod song_view;
mod spectrum;
mod startup_error;
mod store;
mod synth_matrix;
mod synth_strip;
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
    AudioOutput, HandedOutput, OutputDevices, StartOutput, StartedOutput, chosen_device, handed_output,
};
pub use file_request::{FileKind, FileRequest};
pub use font::load as load_font;
pub use mcp_link::run_bridge as run_mcp_bridge;
pub use menu::Command;
pub use midi_settings::{MidiPort, chosen_clock_in, chosen_clock_out, chosen_midi_input, chosen_midi_output};
pub use panel::Id as PanelId;
pub use scale::Scale;
pub use scroll::MIN_WINDOW;
pub use session::{LibrarySource, Session};
pub use single_instance::{Entry as InstanceEntry, enter as enter_single_instance};
pub use startup_error::report as report_startup_error;
pub use ui::{Idle, Ui};
pub use update::{BUILD, Waker};
pub use view::{Shown, min_window_size};
