//! UI widgets for the TUI
//!
//! This module contains reusable widget rendering functions

mod logs;
mod detail;
mod input;

pub use logs::render_log_list;
pub use detail::render_detail_pane;
pub use detail::render_ai_pane;
pub use input::render_input_bar;