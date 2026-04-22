//! UI module for Ratatui-based terminal interface.

mod app;
mod render;

pub use app::App;
#[allow(unused_imports)]
pub use app::LayoutMode;
pub use render::render;
