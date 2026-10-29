//! The command catalog: what every command is called, where it lives in the menus, and its
//! parameters. `cargo xtask parity` reads it.

/// One command's metadata.
#[derive(Debug, Clone, Copy)]
pub struct CommandInfo {
    pub id: &'static str,
    pub label: &'static str,
    /// Menu path (`File`), or empty for commands only reachable programmatically.
    pub menu: &'static str,
    pub shortcut: Option<&'static str>,
    /// Parameters, as JSON with descriptions.
    pub params: &'static str,
    /// Changes the lab (gets an undo step).
    pub mutates: bool,
}

macro_rules! c {
    ($id:literal, $label:literal, $menu:literal, $short:expr, $params:literal, $mutates:literal) => {
        CommandInfo { id: $id, label: $label, menu: $menu, shortcut: $short, params: $params, mutates: $mutates }
    };
}

pub static CATALOG: &[CommandInfo] = &[
    c!("file.new", "New lab", "File", Some("Ctrl+N"), r#"{"sample"?: "sample id (see sample.list)"}"#, false),
    c!("file.open", "Open lab…", "File", Some("Ctrl+O"), r#"{"json": "lab file contents"} or {"path": "file (desktop/CLI)"}"#, false),
    c!("file.save", "Save lab", "File", Some("Ctrl+S"), r#"{"path"?: "file to write (desktop/CLI)"} → {"json"}"#, false),
    c!("file.export_pcap", "Export capture (pcap)…", "File", None, r#"{"path"?: "file to write"} → {"frames", "bytes"}"#, false),
    c!("edit.undo", "Undo", "Edit", Some("Ctrl+Z"), "{}", false),
    c!("edit.redo", "Redo", "Edit", Some("Ctrl+Y"), "{}", false),
    c!("edit.checkpoint", "Start an undo step", "", None, "{} (before a series of transient moves)", false),
    c!("lab.notes", "Lab notes", "Edit", None, r#"{"text": "notes shown with the lab"}"#, true),
    c!(
        "device.add",
        "Add device",
        "Insert",
        None,
        r#"{"kind": "router|switch|hub|pc|laptop|server|printer" or "model": "Switch-48…", "name"?: "hostname", "x"?: 0, "y"?: 0}"#,
        true
    ),
    c!(
        "device.duplicate",
        "Copy and paste device",
        "Edit",
        None,
        r#"{"device", "x"?, "y"?} → {"id", "name"} (same model and configuration, new name, no cables)"#,
        true
    ),
    c!(
        "server.services",
        "Server services",
        "",
        None,
        r#"{"device", "services": {"http": {"on", "files": [{"name", "content"}]}, "dns": {"on", "records": [{"name", "address"}]}, "dhcp": {"on", "pools": [{"name", "gateway", "dns", "start", "mask", "max_users"}]}}}"#,
        true
    ),
    c!(
        "browser.get",
        "Open a web page",
        "",
        None,
        r#"{"device": "host", "url": "http://name-or-address/page.html"} → {"status", "body", "url"}"#,
        false
    ),
    c!(
        "device.startup",
        "Startup configuration (NVRAM)",
        "",
        None,
        r#"{"device", "action": "save|erase|load", "text"?: "configuration lines (load)"}"#,
        true
    ),
    c!("device.label", "Set display name", "", None, r#"{"device", "label": "workspace label (empty: the hostname)"}"#, true),
    c!("device.models", "Device catalogue", "", None, "{} → [{id, kind, label, description, ports}]", false),
    c!(
        "note.add",
        "Place note or drawing",
        "Insert",
        None,
        r##"{"x", "y", "text"?, "shape"?: "text|rectangle|ellipse|line", "w"?, "h"?, "color"?: "#rrggbb"} → {"id"}"##,
        true
    ),
    c!("note.edit", "Edit note", "", None, r#"{"id", "text"}"#, true),
    c!("note.move", "Move note", "", None, r#"{"id", "x", "y", "transient"?: false}"#, true),
    c!("note.remove", "Delete note", "", None, r#"{"id"}"#, true),
    c!("device.remove", "Delete device", "Edit", Some("Delete"), r#"{"device": "name or id"}"#, true),
    c!("device.move", "Move device", "", None, r#"{"device", "x", "y", "transient"?: false}"#, true),
    c!("device.rename", "Rename device", "", None, r#"{"device", "name"}"#, true),
    c!("device.list", "List devices", "", None, "{} → [{id, name, kind, x, y, ports}]", false),
    c!("link.add", "Connect with a cable", "Insert", None, r#"{"a": "device", "a_port"?: "interface", "b": "device", "b_port"?: "interface"}"#, true),
    c!("link.remove", "Remove cable", "Edit", None, r#"{"device", "port": "interface"}"#, true),
    c!("cli.exec", "Type a command", "", None, r#"{"device", "line"} → {"output", "prompt", "clear"}"#, true),
    c!("cli.script", "Run commands", "", None, r#"{"device", "lines": ["…"] or "text"} → {"outputs"}"#, true),
    c!("cli.help", "Command help (?)", "", None, r#"{"device", "line"} → {"help"}"#, false),
    c!("cli.complete", "Complete a command (Tab)", "", None, r#"{"device", "line"} → {"line"|null}"#, false),
    c!("cli.prompt", "Console prompt", "", None, r#"{"device"} → {"prompt"}"#, false),
    c!("net.ping", "Ping", "Simulation", None, r#"{"from": "device", "to": "address", "count"?: 4} → {"probes", "text"}"#, false),
    c!("net.traceroute", "Traceroute", "Simulation", None, r#"{"from": "device", "to": "address"} → {"hops", "text"}"#, false),
    c!(
        "device.configure",
        "Apply configuration lines",
        "",
        None,
        r#"{"device", "lines": ["…"] or "text"} → {"applied", "errors"} (configuration mode, the console's own mode untouched)"#,
        true
    ),
    c!("sim.advance", "Fast forward time", "Simulation", None, r#"{"seconds"?: 30}"#, false),
    c!("sim.reset", "Reset simulation", "Simulation", None, "{} (forget ARP/MAC tables and events)", false),
    c!("sim.clear_events", "Clear event list", "Simulation", None, "{}", false),
    c!("sim.events", "Event list", "", None, r#"{"since"?: event id, "limit"?: 200} → [{id, at_us, from, to, protocol, summary, dropped}]"#, false),
    c!("sim.status", "Simulation status", "", None, "{} → {time_us, events, storm}", false),
    c!("topology.get", "The whole lab", "", None, "{} → the lab document", false),
    c!("sample.list", "Sample labs", "", None, "{} → [{id, title, description}]", false),
];

pub fn find(id: &str) -> Option<&'static CommandInfo> {
    CATALOG.iter().find(|c| c.id == id)
}
