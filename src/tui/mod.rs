//! TUI module for Teams CLI
//!
//! Terminal user interface using Ratatui.

mod activity;
mod app;
mod backend;
mod chat_names;
mod compose;
mod debug_log;
mod help;
mod hyperlinks;
mod log_capture;
mod messages;
mod mouse;
mod notifications;
mod search;
mod sidebar;
mod text_input;
mod ui;
mod unread;

pub use app::run;
pub use log_capture::LogBuffer;
