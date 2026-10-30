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
    c!(
        "device.wireless",
        "Wi-Fi settings",
        "",
        None,
        r#"{"device", "port"?: "Wireless0", "ssid": "network name (empty: radio off)", "key"?: "WPA2-PSK passphrase, 8-63 characters (none: open)", "channel"?: 1-13, "user"?: "WPA2-Enterprise username (hosts)", "password"?} → {associated_with, problem}"#,
        true
    ),
    c!(
        "snmp.request",
        "SNMP request",
        "",
        None,
        r#"{"device": manager, "target": agent address, "version"?: "2c"|"3", "community"?: "public" (2c), "user"? (3), "auth"?: "none"|"md5"|"sha" (3), "password"? (3, 8+ characters), "operation"?: "get"|"get-next"|"set", "oid": "1.3.6.1.2.1.1.5.0", "value"?: for a set} → {"bindings": [{"oid", "value"}], "error"}"#,
        true
    ),
    c!(
        "snmp.traps",
        "SNMP traps received",
        "",
        None,
        r#"{"device": a host, "clear"?: true} → {"traps": [{"at_ms", "from", "community", "trap": "linkDown"|"linkUp"|"coldStart"|OID, "bindings": [{"oid", "value"}]}]}"#,
        false
    ),
    c!(
        "mail.configure",
        "Email account",
        "",
        None,
        r#"{"device", "name"?, "address": "ana@lab.local", "incoming": "POP3 server address or name", "outgoing": "SMTP server", "user", "password"}"#,
        true
    ),
    c!("mail.send", "Send email", "", None, r#"{"device", "to", "subject"?, "body"?} → {"answer"}: SMTP through the outgoing server"#, true),
    c!(
        "mail.receive",
        "Receive email",
        "",
        None,
        r#"{"device"} → {"received": n}: POP3 from the incoming server; the messages move to the host"#,
        true
    ),
    c!(
        "wlc.wlan",
        "Controller WLAN",
        "",
        None,
        r#"{"device": controller, "id": 1-16, "ssid"?: "network name", "security"?: "open"|"psk"|"enterprise", "key"?: "WPA2-PSK passphrase", "vlan"?: 1-4094 (the VLAN its hosts land in, tagged on the APs' uplinks), "remove"?: true} → {"config": the line applied}"#,
        true
    ),
    c!(
        "lap.controller",
        "Access point's controller",
        "",
        None,
        r#"{"device": lightweight AP, "address"?: "controller address (none: any in the AP's subnet)"} → {"config", "joined": controller name or null}"#,
        true
    ),
    c!(
        "home.settings",
        "Home router settings",
        "",
        None,
        r#"{"device", "lan"?+"lan_mask"?, "dhcp"?: true|false, "dhcp_start"?, "internet"?+"internet_mask"?+"gateway"?} → {"config": the lines applied}"#,
        true
    ),
    c!(
        "activity.show",
        "Activity",
        "",
        None,
        r#"{} → {"activity" (or null; a locked one without its checks), "total_points", "checks", "locked"}"#,
        false
    ),
    c!(
        "activity.set",
        "Activity Wizard…",
        "Extensions",
        None,
        r#"{"title"?, "instructions"?, "hidden"?: true to store configuration answers as hashes, "time_limit"?: minutes (0: none), "score_only"?: while locked, Check Results shows the score only, "variables"?: {"name": ["value", …]} picked per student, used as {name}}"#,
        true
    ),
    c!(
        "activity.capture",
        "Capture answers from this lab",
        "",
        None,
        r#"{} → {"checks": n}: configuration lines and host addresses become checks (pings and command checks are kept)"#,
        true
    ),
    c!(
        "activity.add_check",
        "Add check",
        "",
        None,
        r#"{"type": "config"|"ping"|"host-address"|"output", "device" (or "from"), "line" | "to"+"succeed"? | "address"+"gateway"? | "command"+"contains", "points"?: 1, "label"?, "feedback"?: hint shown when it fails, "section"?: the part it counts towards (the device by default)}"#,
        true
    ),
    c!("activity.remove_check", "Remove check", "", None, r#"{"index": n}"#, true),
    c!("activity.clear", "Remove the activity", "", None, "{}", true),
    c!(
        "activity.start",
        "Start the activity",
        "",
        None,
        r#"{"student": "name"} → {"values", "instructions"}: picks this student's variable values"#,
        true
    ),
    c!("activity.lock", "Lock the activity", "", None, r#"{"password"}: the activity can't be changed or its checks seen until unlocked"#, true),
    c!("activity.unlock", "Unlock the activity", "", None, r#"{"password"}"#, true),
    c!(
        "activity.check",
        "Check Results",
        "Extensions",
        None,
        r#"{} → {"results": [{"label", "points", "passed", "feedback", "section"}] and "sections": [{"name", "score", "total", "percent"}] (both empty when locked with score_only), "score", "total", "percent", "detail"}"#,
        false
    ),
    c!("activity.reset", "Reset Activity", "Extensions", None, "{}: back to the lab as the activity was opened", false),
    c!("file.new", "New lab", "File", Some("Ctrl+N"), r#"{"sample"?: "sample id (see sample.list)"}"#, false),
    c!("file.open", "Open lab…", "File", Some("Ctrl+O"), r#"{"json": "lab file contents"} or {"path": "file (desktop/CLI)"}"#, false),
    c!("file.save", "Save lab", "File", Some("Ctrl+S"), r#"{"path"?: "file to write (desktop/CLI)"} → {"json"}"#, false),
    c!(
        "file.export_pcap",
        "Export capture (pcap)…",
        "File",
        None,
        r#"{"path"?: "file to write", "link"?: "ethernet" (default) | "802.11" (the Wi-Fi hops as they crossed the air)} → {"frames", "bytes"}"#,
        false
    ),
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
    c!("device.models", "Device catalogue", "", None, "{} → [{id, kind, label, description, ports, slots}]", false),
    c!(
        "device.power",
        "Power switch",
        "",
        None,
        r#"{"device", "on": true|false or "on"|"off"} → {"power", "changed", "errors"}: switched off, no port carries anything and a router or switch loses its running configuration; switched on, it boots from the saved one"#,
        true
    ),
    c!(
        "device.module.add",
        "Fit a card",
        "",
        None,
        r#"{"device", "slot": "0/1", "module": "Card-2S|Card-4S|Card-1GE|Card-2GE|Card-1FE|Card-1GF"} → {"slots", "added": [port names]}: modular routers, switched off"#,
        true
    ),
    c!(
        "device.module.remove",
        "Remove a card",
        "",
        None,
        r#"{"device", "slot": "0/1"} → {"slots", "removed": [port names]}: switched off; cables on the card's ports are unplugged"#,
        true
    ),
    c!(
        "iot.configure",
        "Smart thing settings",
        "",
        None,
        r#"{"device": smart sensor or device, "server"?: "IoT server address (empty: none)", "value"?: sensor reading -1000..1000} → {"kind", "server", "value", "registered", "on", "message"} (reports at once)"#,
        true
    ),
    c!("iot.sync", "IoT reports", "", None, r#"{} → {"things": [...]}: every smart thing reports to its server over HTTP (sensors first)"#, false),
    c!(
        "iot.server",
        "IoT service",
        "",
        None,
        r#"{"device": server, "service"?: true|false, "rules"?: [{"sensor", "above", "actuator"}]} → {"service", "rules", "things": [{"name", "kind", "address", "value", "on"}]}"#,
        true
    ),
    c!(
        "iot.switch",
        "Switch a smart device",
        "",
        None,
        r#"{"device": server, "name": registered smart device, "on": true|false} → the server's IoT page"#,
        false
    ),
    c!(
        "phone.status",
        "Phone screen",
        "",
        None,
        r#"{"device": IP phone} → {"address", "tftp", "voice_vlan", "registered", "number", "server", "message", "call": {"state": "dialing"|"ringback"|"ringing"|"connected", "peer", "peer_address", "rtp_sent", "rtp_received"}} (lets the phone register first)"#,
        false
    ),
    c!("phone.dial", "Dial a number", "", None, r#"{"device": IP phone, "number": "1002"} → the phone's status: the far phone rings"#, false),
    c!(
        "phone.answer",
        "Answer the phone",
        "",
        None,
        r#"{"device": a ringing IP phone} → its status: the call connects and voice packets flow"#,
        false
    ),
    c!("phone.hangup", "Hang up", "", None, r#"{"device": IP phone} → its status: the call ends at both phones"#, false),
    c!(
        "device.module.list",
        "Card catalogue",
        "",
        None,
        r#"{"device"?} → {"modules": [{id, label, description, ports, type}], "device": {"power", "slots": [{slot, module, label, ports}]}}"#,
        false
    ),
    c!(
        "note.add",
        "Place note or drawing",
        "Insert",
        None,
        r##"{"x", "y", "text"?, "shape"?: "text|rectangle|ellipse|line|freehand|polygon", "w"?, "h"?, "points": [[x, y], …] (freehand: simplified, at most 512 kept; polygon: 3-64 corners), "color"?: "#rrggbb"} → {"id"} (placed in the cluster being viewed)"##,
        true
    ),
    c!("note.edit", "Edit note", "", None, r#"{"id", "text"}"#, true),
    c!("note.move", "Move note", "", None, r#"{"id", "x", "y", "transient"?: false}"#, true),
    c!("note.remove", "Delete note", "", None, r#"{"id"}"#, true),
    c!("note.color", "Drawing colour", "", None, r##"{"id", "color": "#rrggbb"}"##, true),
    c!(
        "cluster.create",
        "Group into a cluster",
        "Edit",
        Some("Ctrl+G"),
        r#"{"devices": ["name or id", …], "clusters"?: [cluster ids to nest], "name"?} → {"id", "name"} (all from the same view; the network is unchanged)"#,
        true
    ),
    c!("cluster.rename", "Rename cluster", "", None, r#"{"id", "name"}"#, true),
    c!("cluster.move", "Move cluster", "", None, r#"{"id", "x", "y", "transient"?: false}"#, true),
    c!("cluster.dissolve", "Uncluster", "Edit", None, r#"{"id"}: its devices go back to the view it sat in"#, true),
    c!(
        "view.physical",
        "Physical workspace",
        "View",
        None,
        r#"{} → {"physical": true, "place", "kind", "path": [{"id", "name", "kind"}]} (cities, buildings and wiring closets)"#,
        false
    ),
    c!("view.logical", "Logical workspace", "View", None, r#"{} → {"physical": false, …}"#, false),
    c!(
        "view.enter_place",
        "Open place",
        "",
        None,
        r#"{"id": place number} → {"physical", "place", "kind", "path"} (switches to the physical view)"#,
        false
    ),
    c!("view.place_up", "Back to the enclosing place", "", None, r#"{} → {"physical", "place", "kind", "path"}"#, false),
    c!("view.intercity", "Back to the Intercity map", "", None, r#"{} → {"physical", "place": null, "path": []}"#, false),
    c!(
        "place.add",
        "New place",
        "",
        None,
        r#"{"kind": "city"|"building"|"closet", "name"?, "parent"?: place number (a building goes in a city, a closet in a building; default: the place being viewed), "x"?, "y"?} → {"id", "name", "parent"}"#,
        true
    ),
    c!("place.rename", "Rename place", "", None, r#"{"id", "name"}"#, true),
    c!("place.move", "Move place", "", None, r#"{"id", "x", "y", "transient"?: false}: its icon on the map it sits in"#, true),
    c!(
        "place.remove",
        "Remove place",
        "",
        None,
        r#"{"id"}: with the places inside it; their devices go to the building above, else the default closet (the last city stays)"#,
        true
    ),
    c!(
        "place.list",
        "List places",
        "",
        None,
        r#"{} → {"implicit": true while the default layout is shown, "places": [{"id", "kind", "name", "x", "y", "parent", "devices": [names]}], "cables": [{"a", "b", "meters", "manual", "problem"}]}"#,
        false
    ),
    c!(
        "device.place",
        "Move to place",
        "",
        None,
        r#"{"device" | "devices": [names or ids], "place": place number, "x"?, "y"?, "transient"?: false}: network devices go in wiring closets, hosts in a closet or a building"#,
        true
    ),
    c!(
        "link.length",
        "Cable length",
        "",
        None,
        r#"{"device", "port": "interface", "meters": 1-20000000 | null (the length the places give)} → {"meters", "manual", "problem"} (copper over 100 m carries nothing)"#,
        true
    ),
    c!("view.enter_cluster", "Open cluster", "", None, r#"{"id"} → {"cluster", "path": [{"id", "name"}]}"#, false),
    c!("view.up", "Back to the parent cluster", "", None, r#"{} → {"cluster", "path"}"#, false),
    c!("view.root", "Back to the top level", "", None, r#"{} → {"cluster": null, "path": []}"#, false),
    c!("device.remove", "Delete device", "Edit", Some("Delete"), r#"{"device": "name or id"}"#, true),
    c!("device.move", "Move device", "", None, r#"{"device", "x", "y", "transient"?: false}"#, true),
    c!("device.rename", "Rename device", "", None, r#"{"device", "name"}"#, true),
    c!(
        "device.list",
        "List devices",
        "",
        None,
        "{} → [{id, name, kind, x, y, cluster, place, ports: [{name, ip, shutdown, up, problem, peer, medium, cable}], console: {port, peer} | null}]",
        false
    ),
    c!(
        "link.add",
        "Connect with a cable",
        "Insert",
        None,
        r#"{"a": "device", "a_port"?: "interface", "b": "device", "b_port"?: "interface", "cable"?: "auto" (default) | "straight-through" | "cross-over" | "fiber" | "console" | "serial-dce" | "serial-dte"} → {"a", "b", "cable", "resolved"}: a cable joins two ports of its medium; a console cable joins a PC's RS-232 port to a router's or switch's Console port"#,
        true
    ),
    c!("link.remove", "Remove cable", "Edit", None, r#"{"device", "port": "interface", or "Console" / "RS-232" for a console cable}"#, true),
    c!(
        "link.list",
        "List cables",
        "",
        None,
        r#"{} → [{"a", "b", "cable", "resolved", "up", "problem"}] (console cables: cable "console", resolved "console")"#,
        false
    ),
    c!(
        "pc.terminal",
        "Terminal (console cable)",
        "",
        None,
        r#"{"pc": "PC or laptop"} → {"device", "prompt", "log"}: the router or switch its console cable leads to; type into it with cli.exec on that device (the same console its CLI tab shows)"#,
        false
    ),
    c!("cli.exec", "Type a command", "", None, r#"{"device", "line"} → {"output", "prompt", "clear"}"#, true),
    c!("cli.script", "Run commands", "", None, r#"{"device", "lines": ["…"] or "text"} → {"outputs"}"#, true),
    c!("cli.help", "Command help (?)", "", None, r#"{"device", "line"} → {"help"}"#, false),
    c!("cli.complete", "Complete a command (Tab)", "", None, r#"{"device", "line"} → {"line"|null}"#, false),
    c!("cli.prompt", "Console prompt", "", None, r#"{"device"} → {"prompt"}"#, false),
    c!("net.ping", "Ping", "Simulation", None, r#"{"from": "device", "to": "IPv4 or IPv6 address", "count"?: 4} → {"probes", "text"}"#, false),
    c!("net.traceroute", "Traceroute", "Simulation", None, r#"{"from": "device", "to": "IPv4 or IPv6 address"} → {"hops", "text"}"#, false),
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
    c!(
        "pdu.simple",
        "Add Simple PDU",
        "Simulation",
        None,
        r#"{"from": "device", "to": "address or device"} → the PDU {num, status, note, first_event…}: one echo request, added to the scenario's PDU list and sent now"#,
        true
    ),
    c!(
        "pdu.complex",
        "Add Complex PDU",
        "Simulation",
        None,
        r#"{"from": "device", "port"?: "source interface", "to"?: "address or device" (not for dhcp), "app"?: "ping"|"dns"|"http"|"telnet"|"ssh"|"ftp"|"smtp"|"pop3"|"dhcp"|"tftp"|"snmp"|"arp"|"udp"|"tcp", "src_port"?, "dst_port"?, "ttl"?: 1-255, "tos"?: 0-255 or "dscp"?: 0-63|"ef"|"af31"…, "seq"?, "size"?: bytes (0-1400), "text"?: DNS name, HTTP page, TFTP file or SNMP community, "period"?: seconds (periodic; none: one-shot), "start"?: seconds from now} → the PDU; sent now when "start" is 0, else by sim.advance"#,
        true
    ),
    c!("pdu.fire", "Fire PDU", "", None, r#"{"num"} → the PDU with its new status and "first_event""#, false),
    c!("pdu.fire_all", "Fire every PDU", "", None, r#"{} → {"pdus": [...]}"#, false),
    c!("pdu.delete", "Delete PDU", "", None, r#"{"num"} or {"all": true} → the PDU list"#, true),
    c!(
        "pdu.list",
        "PDU list",
        "",
        None,
        r#"{} → {"scenario", "scenarios", "description", "pdus": [{num, source, destination, type, app, status, last, fired, note, period…}]}"#,
        false
    ),
    c!("scenario.list", "Scenarios", "", None, r#"{} → {"current", "scenarios": [{index, description, pdus}]}"#, false),
    c!("scenario.new", "New scenario", "", None, r#"{"description"?} → the scenarios (the new one in use)"#, true),
    c!("scenario.delete", "Delete scenario", "", None, r#"{"index"?: the one in use} → the scenarios (the last one is only emptied)"#, true),
    c!("scenario.select", "Switch scenario", "", None, r#"{"index"} → its PDU list"#, false),
];

pub fn find(id: &str) -> Option<&'static CommandInfo> {
    CATALOG.iter().find(|c| c.id == id)
}
