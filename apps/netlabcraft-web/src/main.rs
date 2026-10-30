//! NetLabCraft in the browser.
//!
//! Runs the same [`netlabcraft_ui_egui::PacketApp`] as the desktop app through eframe's web
//! runner. Build with `trunk build --release` from this directory (output in `dist/web`).
//! Open uses the browser file picker; Save and Export download files. `?sample=ID` opens a
//! sample lab.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(target_arch = "wasm32")]
fn main() {
    web::start();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("netlabcraft-web only runs in the browser: build it with `trunk build --release` in apps/netlabcraft-web");
}
