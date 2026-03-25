//! `egui-term` integrates [`egui`] with a terminal by:
//! - translating `crossterm` input events to `egui::Event`
//! - rendering with `egui_wgpu` to an offscreen RGBA image
//! - presenting frames through the kitty graphics protocol

mod kitty;
mod renderer;
mod runner;
mod state;

pub use egui;

pub use crate::runner::{run, run_with, Frame, RunOptions};
pub use crate::state::{EventResponse, State, StateOptions};

pub(crate) use crate::kitty::KittyFrame;
pub(crate) use crate::renderer::OffscreenRenderer;

use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("terminal io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("wgpu initialization failed: {0}")]
    WgpuInit(String),

    #[error("wgpu rendering failed: {0}")]
    WgpuRender(String),
}
