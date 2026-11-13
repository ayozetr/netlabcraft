//! The browser shell: file picker, downloads and the eframe web runner.

use netlabcraft_core::sync::Mutex;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use netlabcraft_engine::Session;
use netlabcraft_ui_egui::{PacketApp, UiRequest};
use wasm_bindgen::JsCast as _;

const CANVAS_ID: &str = "netlabcraft_canvas";
const LOADING_ID: &str = "netlabcraft_loading";

/// Files the picker read, waiting for the next frame: (name, contents, the device a
/// configuration file is for and whether to merge it).
type Inbox = Arc<Mutex<Vec<(String, String, Target)>>>;

/// What a picked file is for.
#[derive(Clone, Copy)]
enum Target {
    Lab,
    Config(netlabcraft_engine::DeviceId, bool),
    WebFile(netlabcraft_engine::DeviceId),
}

struct WebShell {
    app: PacketApp,
    inbox: Inbox,
    /// Unsaved changes, read by the page's `beforeunload` handler.
    dirty: Arc<AtomicBool>,
}

/// Makes the browser ask before closing or reloading the tab while the lab has unsaved
/// changes (browsers show their own wording; the text is only a hint).
fn guard_unsaved_changes(dirty: Arc<AtomicBool>) {
    let Some(window) = web_sys::window() else { return };
    let handler = wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::BeforeUnloadEvent)>::new(move |e: web_sys::BeforeUnloadEvent| {
        if dirty.load(Ordering::Relaxed) {
            e.prevent_default();
            e.set_return_value("The lab has unsaved changes.");
        }
    });
    window.set_onbeforeunload(Some(handler.as_ref().unchecked_ref()));
    // The page lives as long as the tab: the handler is never dropped.
    handler.forget();
}

fn query() -> String {
    web_sys::window().and_then(|w| w.location().search().ok()).unwrap_or_default()
}

fn download(name: &str, bytes: &[u8], mime: &str) {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else { return };
    let array = js_sys::Uint8Array::from(bytes);
    let parts = js_sys::Array::new();
    parts.push(&array.buffer());
    let opts = web_sys::BlobPropertyBag::new();
    opts.set_type(mime);
    let Ok(blob) = web_sys::Blob::new_with_buffer_source_sequence_and_options(&parts, &opts) else { return };
    let Ok(url) = web_sys::Url::create_object_url_with_blob(&blob) else { return };
    if let Some(a) = document.create_element("a").ok().and_then(|e| e.dyn_into::<web_sys::HtmlAnchorElement>().ok()) {
        a.set_href(&url);
        a.set_download(name);
        a.click();
    }
    let _ = web_sys::Url::revoke_object_url(&url);
}

impl eframe::App for WebShell {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let pending = std::mem::take(&mut *self.inbox.lock());
        for (name, text, target) in pending {
            match target {
                Target::Config(device, merge) => self.app.text_opened(device, merge, &text),
                Target::WebFile(device) => self.app.web_file_imported(device, &name, &text),
                Target::Lab => self.app.open_json(&text, Some(&name)),
            }
        }
        self.app.ui(ui);
        self.dirty.store(self.app.session.dirty, Ordering::Relaxed);
        for r in self.app.take_requests() {
            match r {
                UiRequest::Open => {
                    let inbox = self.inbox.clone();
                    let ctx = ui.ctx().clone();
                    wasm_bindgen_futures::spawn_local(async move {
                        if let Some(f) = rfd::AsyncFileDialog::new().add_filter("NetLabCraft lab", &["nlcraft"]).pick_file().await {
                            let bytes = f.read().await;
                            inbox.lock().push((f.file_name(), String::from_utf8_lossy(&bytes).to_string(), Target::Lab));
                            ctx.request_repaint();
                        }
                    });
                }
                UiRequest::Save { json, path } => {
                    let name = path.unwrap_or_else(|| "lab.nlcraft".into());
                    download(&name, json.as_bytes(), "application/json");
                    self.app.saved_to(&name);
                }
                UiRequest::ExportPcap(bytes) => download("capture.pcap", &bytes, "application/vnd.tcpdump.pcap"),
                UiRequest::SaveText { name, text } => download(&name, text.as_bytes(), "text/plain"),
                UiRequest::ExportImage(bytes) => download("workspace.png", &bytes, "image/png"),
                UiRequest::Print(bytes) => download("workspace.pdf", &bytes, "application/pdf"),
                UiRequest::OpenText { device, merge } => {
                    let inbox = self.inbox.clone();
                    let ctx = ui.ctx().clone();
                    wasm_bindgen_futures::spawn_local(async move {
                        if let Some(f) = rfd::AsyncFileDialog::new().add_filter("Text", &["txt", "cfg", "conf"]).pick_file().await {
                            let bytes = f.read().await;
                            inbox.lock().push((f.file_name(), String::from_utf8_lossy(&bytes).to_string(), Target::Config(device, merge)));
                            ctx.request_repaint();
                        }
                    });
                }
                UiRequest::ImportWebFile { device } => {
                    let inbox = self.inbox.clone();
                    let ctx = ui.ctx().clone();
                    wasm_bindgen_futures::spawn_local(async move {
                        if let Some(f) = rfd::AsyncFileDialog::new().add_filter("Web page", &["html", "htm", "txt"]).pick_file().await {
                            let bytes = f.read().await;
                            inbox.lock().push((f.file_name(), String::from_utf8_lossy(&bytes).to_string(), Target::WebFile(device)));
                            ctx.request_repaint();
                        }
                    });
                }
                UiRequest::Quit => {}
            }
        }
    }
}

pub fn start() {
    eframe::WebLogger::init(log::LevelFilter::Info).ok();
    wasm_bindgen_futures::spawn_local(async {
        let Some(document) = web_sys::window().and_then(|w| w.document()) else {
            log::error!("no document");
            return;
        };
        let Some(canvas) = document.get_element_by_id(CANVAS_ID).and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok()) else {
            log::error!("missing <canvas id=\"{CANVAS_ID}\">");
            return;
        };
        let options = eframe::WebOptions::default();
        let result = eframe::WebRunner::new()
            .start(
                canvas,
                options,
                Box::new(|cc| {
                    if let Some(l) = web_sys::window().and_then(|w| w.navigator().language()) {
                        netlabcraft_ui_egui::i18n::set_lang(netlabcraft_ui_egui::i18n::Lang::from_locale(&l));
                    }
                    let mut session = Session::new();
                    let q = query();
                    let sample = q.trim_start_matches('?').split('&').find_map(|kv| kv.strip_prefix("sample=")).unwrap_or("two-subnets").to_string();
                    let _ = session.execute("file.new", serde_json::json!({"sample": sample}));
                    let app = PacketApp::new(session);
                    app.apply_theme(&cc.egui_ctx);
                    let dirty = Arc::new(AtomicBool::new(false));
                    guard_unsaved_changes(dirty.clone());
                    Ok(Box::new(WebShell { app, inbox: Inbox::default(), dirty }))
                }),
            )
            .await;
        if let Some(loading) = document.get_element_by_id(LOADING_ID) {
            match result {
                Ok(()) => loading.remove(),
                Err(e) => loading.set_inner_html(&format!("NetLabCraft could not start: {e:?}")),
            }
        }
    });
}
