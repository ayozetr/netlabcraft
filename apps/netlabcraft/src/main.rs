//! NetLabCraft on the desktop: `netlabcraft [LAB.nlcraft] [--sample ID]`.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![cfg_attr(windows, windows_subsystem = "windows")]

use netlabcraft_engine::Session;
use netlabcraft_ui_egui::i18n::{tr, trf};
use netlabcraft_ui_egui::{PacketApp, PrintChoices, Toolbars, UiRequest};
use serde_json::json;

/// What to do with unsaved changes when the window closes.
#[derive(PartialEq, Eq)]
enum Closing {
    No,
    Asking,
    Confirmed,
}

struct App {
    app: PacketApp,
    title: String,
    closing: Closing,
    /// The lab's path last frame, to notice opens and saves for the recent list.
    last_path: Option<String>,
    /// The "device windows as separate windows" choice last saved.
    native_saved: bool,
    /// The toolbars shown (View ▸ Toolbars) as last saved.
    toolbars_saved: Toolbars,
    /// The native-window geometries as last written (compared as text), and when.
    geometry_text: String,
    geometry_at: f64,
    /// The preferences as last written (compared as text).
    prefs_text: String,
    /// The copies of the report PDF made for the viewer or the printer.
    temp: TempFiles,
}

/// A folder in the system's temporary folder for the PDF copies handed to a viewer or a printer; it
/// is deleted, with everything in it, when the app closes (so no print file is left behind).
struct TempFiles(std::path::PathBuf);

impl TempFiles {
    fn new() -> Self {
        TempFiles(std::env::temp_dir().join(format!("netlabcraft-{}", std::process::id())))
    }

    /// Writes `bytes` as `name` in the folder (made on first use, private to the user) and says where.
    fn write(&self, name: &str, bytes: &[u8]) -> Result<std::path::PathBuf, String> {
        let mut dir = std::fs::DirBuilder::new();
        dir.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt as _;
            dir.mode(0o700);
        }
        dir.create(&self.0).map_err(|e| format!("{}: {e}", self.0.display()))?;
        let file = std::path::Path::new(name).file_name().map_or_else(|| "workspace.pdf".into(), std::ffi::OsString::from);
        let path = self.0.join(file);
        std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(path)
    }
}

impl Drop for TempFiles {
    fn drop(&mut self) {
        // The viewer may still hold a copy (Windows refuses): a leftover temp file is not an error.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Whether `program` is a file in one of the folders of `PATH`.
fn on_path(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(program).is_file()))
}

/// What File ▸ Print… can offer on this system: a program to open the file with, and `lp` (CUPS)
/// to print it straight away.
fn print_choices() -> PrintChoices {
    let viewer = cfg!(any(windows, target_os = "macos")) || on_path("xdg-open");
    let direct_unavailable = (cfg!(windows) || !on_path("lp"))
        .then(|| "Direct printing needs the lp command, which this system does not have: save the PDF or open it in a viewer".to_string());
    PrintChoices { viewer, direct_unavailable }
}

/// Where the recent-files list lives: the platform's configuration folder.
fn recent_file() -> Option<std::path::PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(std::path::PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))
    }?;
    Some(base.join("netlabcraft").join("recent.txt"))
}

/// The saved interface language, else the system's (`LC_ALL`, `LC_MESSAGES`, `LANG`).
fn pick_language() {
    let saved = recent_file().and_then(|f| std::fs::read_to_string(f.with_file_name("language")).ok());
    let system = ["LC_ALL", "LC_MESSAGES", "LANG"].iter().find_map(|v| std::env::var(v).ok().filter(|s| !s.is_empty()));
    if let Some(l) = saved.or(system) {
        netlabcraft_ui_egui::i18n::set_lang(netlabcraft_ui_egui::i18n::Lang::from_locale(l.trim()));
    }
}

fn save_language() {
    if let Some(f) = recent_file() {
        let _ = std::fs::write(f.with_file_name("language"), netlabcraft_ui_egui::i18n::lang().code());
    }
}

/// Whether device windows open as their own operating-system windows (off unless chosen).
fn load_native_windows() -> bool {
    recent_file().and_then(|f| std::fs::read_to_string(f.with_file_name("native-windows")).ok()).is_some_and(|s| s.trim() == "on")
}

fn save_native_windows(on: bool) {
    if let Some(f) = recent_file() {
        // A lost choice only costs convenience: errors are ignored.
        let _ = std::fs::write(f.with_file_name("native-windows"), if on { "on" } else { "off" });
    }
}

/// The Preferences window's settings, the profile name and the theme, kept between sessions.
fn load_preferences() -> String {
    recent_file().and_then(|f| std::fs::read_to_string(f.with_file_name("preferences")).ok()).unwrap_or_default()
}

fn save_preferences(text: &str) {
    let Some(f) = recent_file().map(|f| f.with_file_name("preferences")) else { return };
    if let Some(dir) = f.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // A lost setting only costs convenience: errors are ignored.
    let _ = std::fs::write(f, text);
}

/// Plays a WAV file with the system's player, without waiting for it.
fn play_wav(bytes: &[u8]) {
    let path = std::env::temp_dir().join(format!("netlabcraft-sound-{}.wav", std::process::id()));
    if std::fs::write(&path, bytes).is_err() {
        return;
    }
    let path = path.to_string_lossy();
    // No sound device or player is not an error worth showing: the tone is a convenience.
    let _ = if cfg!(target_os = "windows") {
        netlabcraft_core::process::spawn_detached(
            "powershell",
            &["-NoProfile", "-Command", &format!("(New-Object Media.SoundPlayer '{path}').PlaySync()")],
        )
    } else if cfg!(target_os = "macos") {
        netlabcraft_core::process::spawn_detached("afplay", &[&path])
    } else {
        netlabcraft_core::process::spawn_detached("paplay", &[&path]).or_else(|_| netlabcraft_core::process::spawn_detached("aplay", &["-q", &path]))
    };
}

/// The toolbars the person left shown or hidden (all shown until they choose).
fn load_toolbars() -> Toolbars {
    recent_file().and_then(|f| std::fs::read_to_string(f.with_file_name("toolbars")).ok()).map_or_else(Toolbars::default, |t| Toolbars::from_text(&t))
}

fn save_toolbars(t: Toolbars) {
    let Some(f) = recent_file().map(|f| f.with_file_name("toolbars")) else { return };
    if let Some(dir) = f.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // A lost choice only costs convenience: errors are ignored.
    let _ = std::fs::write(f, t.to_text());
}

/// Where the size and position of each device's native window are kept between sessions.
fn geometry_file() -> Option<std::path::PathBuf> {
    recent_file().map(|f| f.with_file_name("native-geometry"))
}

fn load_native_geometry() -> std::collections::BTreeMap<String, netlabcraft_ui_egui::NativeGeometry> {
    let text = geometry_file().and_then(|f| std::fs::read_to_string(f).ok()).unwrap_or_default();
    netlabcraft_ui_egui::native_geometry_from_text(&text)
}

fn save_native_geometry(text: &str) {
    let Some(f) = geometry_file() else { return };
    if let Some(dir) = f.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // A lost position only costs convenience: errors are ignored.
    let _ = std::fs::write(f, text);
}

fn load_recent() -> Vec<String> {
    let Some(f) = recent_file() else { return Vec::new() };
    std::fs::read_to_string(f).map(|s| s.lines().filter(|l| !l.trim().is_empty()).take(8).map(str::to_string).collect()).unwrap_or_default()
}

fn save_recent(list: &[String]) {
    let Some(f) = recent_file() else { return };
    if let Some(dir) = f.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // A lost list only costs convenience, never the lab: errors are ignored.
    let _ = std::fs::write(f, list.join("\n"));
}

impl App {
    fn serve(&mut self, ctx: &egui::Context) {
        for r in self.app.take_requests() {
            match r {
                UiRequest::Open => {
                    if let Some(p) = self.dialog().add_filter("NetLabCraft lab", &["nlcraft"]).pick_file() {
                        self.app.open_path(&p.to_string_lossy());
                    }
                }
                UiRequest::Save { path, .. } => {
                    let path = path
                        .map(std::path::PathBuf::from)
                        .or_else(|| self.dialog().add_filter("NetLabCraft lab", &["nlcraft"]).set_file_name("lab.nlcraft").save_file());
                    if let Some(p) = path {
                        let p = p.to_string_lossy().to_string();
                        if self.app.run("file.save", json!({"path": p})).is_some() {
                            self.app.saved_to(&p);
                            if self.closing == Closing::Asking {
                                self.closing = Closing::Confirmed;
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                        }
                    }
                }
                UiRequest::ExportPcap(bytes) => {
                    if let Some(p) = self.dialog().add_filter("Packet capture", &["pcap"]).set_file_name("capture.pcap").save_file() {
                        match std::fs::write(&p, &bytes) {
                            Ok(()) => self.app.info(format!("Wrote {} ({} bytes)", p.display(), bytes.len())),
                            Err(e) => self.app.ui.status = Some((format!("{}: {e}", p.display()), true)),
                        }
                    }
                }
                UiRequest::ExportImage(bytes) => {
                    if let Some(p) = self.dialog().add_filter("PNG image", &["png"]).set_file_name("workspace.png").save_file() {
                        match std::fs::write(&p, &bytes) {
                            Ok(()) => self.app.info(format!("Wrote {}", p.display())),
                            Err(e) => self.app.ui.status = Some((format!("{}: {e}", p.display()), true)),
                        }
                    }
                }
                UiRequest::SavePdf { name, bytes } => self.save_pdf(&name, &bytes),
                UiRequest::OpenPdf { name, bytes } => self.open_pdf(&name, &bytes),
                UiRequest::PrintPdf { name, bytes } => self.print_pdf(&name, &bytes),
                UiRequest::SaveText { name, text } => {
                    if let Some(p) = self.dialog().add_filter("Text", &["txt"]).set_file_name(&name).save_file() {
                        match std::fs::write(&p, text.as_bytes()) {
                            Ok(()) => self.app.info(format!("Wrote {}", p.display())),
                            Err(e) => self.app.ui.status = Some((format!("{}: {e}", p.display()), true)),
                        }
                    }
                }
                UiRequest::OpenText { device, merge } => {
                    if let Some(p) = self.dialog().add_filter("Text", &["txt", "cfg", "conf"]).pick_file() {
                        match std::fs::read(&p) {
                            Ok(b) if b.len() <= 1_000_000 => self.app.text_opened(device, merge, &String::from_utf8_lossy(&b)),
                            Ok(_) => self.app.ui.status = Some(("That file is too large to be a configuration.".into(), true)),
                            Err(e) => self.app.ui.status = Some((format!("{}: {e}", p.display()), true)),
                        }
                    }
                }
                UiRequest::ImportWebFile { device } => {
                    if let Some(p) = self.dialog().add_filter("Web page", &["html", "htm", "txt"]).pick_file() {
                        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "page.html".into());
                        match std::fs::read(&p) {
                            Ok(b) if b.len() <= 200_000 => self.app.web_file_imported(device, &name, &String::from_utf8_lossy(&b)),
                            Ok(_) => self.app.ui.status = Some(("That file is too large for the simulated web server.".into(), true)),
                            Err(e) => self.app.ui.status = Some((format!("{}: {e}", p.display()), true)),
                        }
                    }
                }
                UiRequest::ImportEnvironment { place } => {
                    if let Some(p) = self.dialog().add_filter("Environment", &["json"]).pick_file() {
                        match std::fs::read(&p) {
                            Ok(b) if b.len() <= 1_000_000 => self.app.environment_imported(place, &String::from_utf8_lossy(&b)),
                            Ok(_) => self.app.ui.status = Some(("That file is too large to be an environment.".into(), true)),
                            Err(e) => self.app.ui.status = Some((format!("{}: {e}", p.display()), true)),
                        }
                    }
                }
                UiRequest::PickImage => {
                    if let Some(p) = self.dialog().add_filter("Picture", &["png", "jpg", "jpeg"]).pick_file() {
                        let limit = netlabcraft_engine::images::MAX_SOURCE_BYTES as u64;
                        match std::fs::metadata(&p).map(|m| m.len()) {
                            Ok(n) if n <= limit => match std::fs::read(&p) {
                                Ok(b) => self.app.image_picked(b),
                                Err(e) => self.app.ui.status = Some((format!("{}: {e}", p.display()), true)),
                            },
                            Ok(_) => self.app.ui.status = Some(("That file is too large to open as a picture.".into(), true)),
                            Err(e) => self.app.ui.status = Some((format!("{}: {e}", p.display()), true)),
                        }
                    }
                }
                UiRequest::PickFolder => {
                    if let Some(p) = self.dialog().pick_folder() {
                        self.app.folder_picked(&p.to_string_lossy());
                    }
                }
                UiRequest::Backup { stem, folder } => self.backup(&stem, folder.as_deref()),
                UiRequest::PlaySound(bytes) => play_wav(&bytes),
                UiRequest::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            }
        }
    }

    /// A file dialog that starts in the user folder (Preferences ▸ Administrative), when it is set
    /// and exists.
    fn dialog(&self) -> rfd::FileDialog {
        let dialog = rfd::FileDialog::new();
        match self.app.user_folder().map(std::path::Path::new) {
            Some(dir) if dir.is_dir() => dialog.set_directory(dir),
            _ => dialog,
        }
    }

    /// Writes a backup copy of the open lab: `<stem>-<time>.nlcraft` in the `backups` folder of the
    /// user folder, or of the settings folder. The engine refuses to replace a file.
    fn backup(&mut self, stem: &str, folder: Option<&str>) {
        let base = folder
            .map(std::path::PathBuf::from)
            .filter(|f| f.is_dir())
            .or_else(|| recent_file().and_then(|f| f.parent().map(std::path::Path::to_path_buf)));
        let Some(dir) = base.map(|b| b.join("backups")) else { return };
        if let Err(e) = std::fs::create_dir_all(&dir) {
            self.app.ui.status = Some((format!("{}: {e}", dir.display()), true));
            return;
        }
        let stamp = netlabcraft_engine::instructor::now_utc().replace(':', "-");
        let path = dir.join(format!("{stem}-{stamp}.nlcraft"));
        if self.app.run("file.backup", json!({"path": path.to_string_lossy()})).is_some() {
            self.app.info(trf("Backup written to {}", &[&path.display()]));
        }
    }

    /// Asks where to keep the report PDF (File ▸ Save as PDF, and the Print window's first choice).
    fn save_pdf(&mut self, name: &str, bytes: &[u8]) {
        let Some(p) = self.dialog().add_filter("PDF document", &["pdf"]).set_file_name(name).save_file() else { return };
        let p = if p.extension().is_none() { p.with_extension("pdf") } else { p };
        match std::fs::write(&p, bytes) {
            Ok(()) => self.app.info(trf("Saved the PDF to {}", &[&p.display()])),
            Err(e) => self.app.ui.status = Some((format!("{}: {e}", p.display()), true)),
        }
    }

    /// Opens a temporary copy in the system's PDF viewer; without one, falls back to the save dialog.
    fn open_pdf(&mut self, name: &str, bytes: &[u8]) {
        match self.temp.write(name, bytes) {
            Err(e) => self.app.ui.status = Some((e, true)),
            Ok(path) => match open_with_system(&path) {
                Ok(()) => self.app.info(trf(
                    "Opened {} in the PDF viewer. This temporary copy is deleted when NetLabCraft closes: use Save as PDF to keep the file",
                    &[&path.display()],
                )),
                Err(_) => {
                    let _ = std::fs::remove_file(&path);
                    self.app.ui.status = Some((tr("No PDF viewer found: use Save as PDF").to_string(), true));
                    self.save_pdf(name, bytes);
                }
            },
        }
    }

    /// Sends the report to the default printer with `lp`, then deletes the copy made for it.
    fn print_pdf(&mut self, name: &str, bytes: &[u8]) {
        let path = match self.temp.write(name, bytes) {
            Ok(p) => p,
            Err(e) => {
                self.app.ui.status = Some((e, true));
                return;
            }
        };
        let sent = netlabcraft_core::process::run("lp", &[&path.to_string_lossy()], std::time::Duration::from_secs(20));
        let _ = std::fs::remove_file(&path);
        match sent {
            Ok(_) => self.app.info(tr("Sent the PDF to the default printer").to_string()),
            Err(e) => self.app.ui.status = Some((trf("Could not print: {}", &[&e.trim()]), true)),
        }
    }

    fn unsaved_dialog(&mut self, ctx: &egui::Context) {
        if self.closing != Closing::Asking {
            return;
        }
        egui::Window::new("Unsaved changes").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.label(format!("Save the changes to {} before closing?", self.app.session.title().trim_end_matches(" •")));
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    self.app.save(false);
                }
                if ui.button("Discard").clicked() {
                    self.closing = Closing::Confirmed;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                if ui.button("Cancel").clicked() {
                    self.closing = Closing::No;
                }
            });
        });
    }
}

impl eframe::App for App {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        for f in std::mem::take(&mut raw.dropped_files) {
            if let Some(p) = f.path().to_str().filter(|p| !p.is_empty()) {
                self.app.open_path(p);
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if ctx.input(|i| i.viewport().close_requested()) && self.app.session.dirty && self.closing != Closing::Confirmed {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.closing = Closing::Asking;
        }
        self.app.ui(ui);
        if std::mem::take(&mut self.app.ui.lang_changed) {
            save_language();
        }
        if self.app.ui.native_windows != self.native_saved {
            self.native_saved = self.app.ui.native_windows;
            save_native_windows(self.native_saved);
        }
        let prefs = self.app.preferences_text();
        if prefs != self.prefs_text {
            save_preferences(&prefs);
            self.prefs_text = prefs;
        }
        if self.app.ui.toolbars != self.toolbars_saved {
            self.toolbars_saved = self.app.ui.toolbars;
            save_toolbars(self.toolbars_saved);
        }
        // Where each device's native window sits: written when it changes, at most once a second
        // while a window is open (it moves with every frame of a drag), and at once after.
        let now = ctx.input(|i| i.time);
        let geometry = netlabcraft_ui_egui::native_geometry_to_text(&self.app.ui.native_remembered);
        if geometry != self.geometry_text && (!self.app.ui.native_window_open() || now - self.geometry_at >= 1.0) {
            save_native_geometry(&geometry);
            self.geometry_text = geometry;
            self.geometry_at = now;
        }
        if self.app.session.path != self.last_path {
            self.last_path = self.app.session.path.clone();
            if let Some(p) = &self.last_path {
                let list = &mut self.app.ui.recent;
                list.retain(|x| x != p);
                list.insert(0, p.clone());
                list.truncate(8);
                save_recent(list);
            }
        }
        self.unsaved_dialog(&ctx);
        self.serve(&ctx);
        let title = self.app.title();
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }
}

/// The window icon (the hicolor 256 px rendering of `assets/app-icon/netlabcraft.png`).
fn icon() -> egui::IconData {
    let bytes = include_bytes!("../../../assets/app-icon/hicolor/256x256/apps/netlabcraft.png");
    match image::load_from_memory_with_format(bytes, image::ImageFormat::Png) {
        Ok(img) => {
            let img = img.to_rgba8();
            egui::IconData { width: img.width(), height: img.height(), rgba: img.into_raw() }
        }
        Err(_) => egui::IconData::default(),
    }
}

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut session = Session::new();
    let mut i = 0;
    while let Some(a) = args.get(i) {
        let r = match a.as_str() {
            "--sample" => {
                i += 1;
                session.execute("file.new", json!({"sample": args.get(i).map_or("lan", String::as_str)}))
            }
            "--version" => {
                println!("netlabcraft {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            path => session.execute("file.open", json!({"path": path})),
        };
        if let Err(e) = r {
            eprintln!("netlabcraft: {e}");
        }
        i += 1;
    }
    pick_language();
    let mut app = PacketApp::new(session);
    app.ui.recent = load_recent();
    app.ui.print_choices = Some(print_choices());
    app.ui.prefs_ui.desktop = true;
    app.load_preferences(&load_preferences());
    let prefs_text = app.preferences_text();
    let native_saved = load_native_windows();
    app.ui.native_windows = native_saved;
    let toolbars_saved = load_toolbars();
    app.ui.toolbars = toolbars_saved;
    app.ui.native_remembered = load_native_geometry();
    let geometry_text = netlabcraft_ui_egui::native_geometry_to_text(&app.ui.native_remembered);
    // None: a lab opened from the command line goes on the recent list too.
    let last_path = None;
    let title = app.title();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(title.clone())
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([720.0, 480.0])
            .with_app_id("io.github.ayozetr.netlabcraft")
            .with_icon(icon()),
        ..Default::default()
    };
    eframe::run_native(
        "NetLabCraft",
        options,
        Box::new(move |cc| {
            netlabcraft_ui_egui::fonts::install(&cc.egui_ctx);
            app.apply_theme(&cc.egui_ctx);
            Ok(Box::new(App {
                app,
                title,
                closing: Closing::No,
                last_path,
                native_saved,
                toolbars_saved,
                geometry_text,
                geometry_at: 0.0,
                prefs_text,
                temp: TempFiles::new(),
            }))
        }),
    )
}

/// Opens a file with the system's default application.
fn open_with_system(path: &std::path::Path) -> Result<(), String> {
    let path = path.to_string_lossy();
    if cfg!(target_os = "windows") {
        netlabcraft_core::process::spawn_detached("cmd", &["/C", "start", "", &path])
    } else if cfg!(target_os = "macos") {
        netlabcraft_core::process::spawn_detached("open", &[&path])
    } else {
        netlabcraft_core::process::spawn_detached("xdg-open", &[&path])
    }
}
