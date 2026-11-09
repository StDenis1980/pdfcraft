//! pdfcraft-platform — L7 operating-system services shared by the desktop app (`ui-egui`) and the
//! automation tools (`automation`), which sit on the same layer and can't depend on each other,
//! and by the two executables (`owner_only`, for the `--control` file).

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod desktop_theme;
#[cfg(not(target_arch = "wasm32"))]
pub mod office;
pub mod owner_only;
pub mod staging;
pub mod trust;
#[cfg(not(target_arch = "wasm32"))]
pub mod tsa;
pub mod zone;
