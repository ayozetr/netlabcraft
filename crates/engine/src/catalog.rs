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
        "server.syslog",
        "Syslog messages received",
        "",
        None,
        r#"{"device": server, "clear"?: true} → {"messages": [{"at_ms", "from", "text"}]} (what devices sent its Syslog service, oldest first)"#,
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
    c!(
        "activity.reset",
        "Reset Activity",
        "Extensions",
        None,
        "{}: back to the initial network, or the lab as the activity was opened → {\"from\": \"initial\"|\"opened\"}",
        false
    ),
    c!(
        "activity.save_answer",
        "Save as the answer network",
        "",
        None,
        r#"{} → {"devices", "scheme"}: the lab as it is now becomes the answer network, kept in the clear ("plain"), under a key from the salt when answers are hidden ("salt": hides it from a casual look only), or under the password while locked ("password")"#,
        true
    ),
    c!(
        "activity.answer",
        "Answer Network",
        "",
        None,
        r#"{} → {"lab": the answer network as a lab file's JSON, "devices", "links", "scheme"}"#,
        false
    ),
    c!(
        "activity.load_answer",
        "Edit the answer network",
        "",
        None,
        "{}: puts the answer network in the workspace (the activity stays); `activity.save_answer` keeps the changes",
        true
    ),
    c!("activity.clear_answer", "Remove the answer network", "", None, "{}", true),
    c!(
        "activity.tree",
        "Assessment tree",
        "",
        None,
        r#"{} → {"items": [{"key", "device", "kind": "config"|"address"|"cable", "text", "points" (0: not graded), "feedback"}], "points"}: what the answer network holds, per device"#,
        false
    ),
    c!(
        "activity.weight",
        "Set an item's points",
        "",
        None,
        r#"{"key": tree item, "points"?: 0-1000 (0: not graded), "feedback"?: hint when it fails}"#,
        true
    ),
    c!(
        "activity.tree_checks",
        "Make checks from the assessment tree",
        "",
        None,
        r#"{} → {"checks", "points"}: the graded tree items replace the configuration, address and cable checks (pings and command checks stay)"#,
        true
    ),
    c!(
        "activity.save_initial",
        "Save as the initial network",
        "",
        None,
        r#"{} → {"devices"}: the lab as it is now is where the student starts (Reset Activity returns to it)"#,
        true
    ),
    c!(
        "activity.load_initial",
        "Edit the initial network",
        "",
        None,
        "{}: puts the initial network in the workspace (the activity stays); `activity.save_initial` keeps the changes",
        true
    ),
    c!("activity.clear_initial", "Remove the initial network", "", None, "{}", true),
    c!(
        "activity.translate",
        "Instructions in a language",
        "",
        None,
        r#"{"lang": "es", "text"?: the instructions in it (empty removes them), "default"?: true makes it the main language} → {"languages", "default"}"#,
        true
    ),
    c!("activity.edit_check", "Edit check", "", None, r#"{"index": n, "points"?, "feedback"?, "label"?, "section"?}"#, true),
    c!(
        "activity.connectivity",
        "Connectivity Tests",
        "Extensions",
        None,
        r#"{} → {"tests": [{"label", "from", "to", "expect", "passed", "actual"}], "passed", "total", "detail"}: the activity's ping checks, run now"#,
        false
    ),
    c!(
        "activity.export_results",
        "Export results…",
        "",
        None,
        r#"{"format"?: "json"|"csv", "path"?, "time"?, "elapsed_s"?} → {"text", "file_name", "score", "total", "percent"}: a results file with the student, the time, the score per section, signed with HMAC-SHA1 under a key from the activity's salt"#,
        false
    ),
    c!(
        "activity.verify_results",
        "Verify a results file",
        "",
        None,
        r#"{"text" or "path"} → {"valid", "same_activity", "student", "score", "total", "percent"}"#,
        false
    ),
    c!(
        "activity.locks",
        "Activity locks",
        "",
        None,
        r#"{"devices"?: true (no adding or removing devices), "cables"?: true (no cable changes), "config"?: ["R1", …] (devices whose configuration can't change)}: they hold while the activity is locked"#,
        true
    ),
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
    c!(
        "board.program",
        "Board program",
        "",
        None,
        r#"{"device": programmable board, "source"?: "BoardScript program", "run"?: true|false, "server"?: "IoT server address for get() and report (empty: none)"} → the board's status: run starts it from the top, false stops it; a program that doesn't compile is refused with its line"#,
        true
    ),
    c!(
        "board.status",
        "Board status",
        "",
        None,
        r#"{"device": programmable board} → {"program", "run", "state": "not started"|"running"|"stopped", "line", "message", "log": [...], "vars": {...}, "pins": [{"pin", "analog", "wired", "what", "level"}], "server"}"#,
        false
    ),
    c!(
        "board.wire",
        "Wire a board pin",
        "",
        None,
        r#"{"device": programmable board, "pin": "D0"|"A0"…, "thing"?: smart thing (none: unplug the pin)} → the board's status. Also link.add with "cable": "iot-custom""#,
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
        "device.adapter",
        "Power adapter",
        "",
        None,
        r#"{"device": IP phone or lightweight AP, "on": true|false} → the power source: plugged in, the device runs without power over Ethernet; unplugged, it needs a PoE switch port"#,
        true
    ),
    c!(
        "phone.configure",
        "Phone settings",
        "",
        None,
        r#"{"device": IP phone, "protocol"?: "sip"|"own", "server"?: "SIP registrar address (empty: the TFTP server DHCP gave)", "address"?: "dhcp" or "A.B.C.D", "mask"?, "gateway"?} → the phone's status (it registers again)"#,
        true
    ),
    c!(
        "device.psu",
        "Power supplies",
        "",
        None,
        r#"{"device": router or switch, "bay"?: 1|2, "fitted"?: true|false} → {"bays": [{"bay", "fitted"}], "power", "no_supply"}: fits or takes out a power supply; taking out the last one switches the device off, and it can't be switched on without one"#,
        true
    ),
    c!(
        "cloud.map",
        "Cloud port pairs",
        "",
        None,
        r#"{"device": cloud, "a"?: "Coaxial0", "b"?: "Phone0", "remove"?: true} → {"pairs": [[a, b]…], "ports"}: joins two ports (a frame in one leaves by the other), parts them, or lists the pairs"#,
        true
    ),
    c!(
        "device.power.source",
        "Power source",
        "",
        None,
        r#"{"device"} → {"powered_device", "adapter", "source": "mains"|"adapter"|"poe"|"none"|"off", "detail" (the PoE port, or why there is no power), "draw_w"}"#,
        false
    ),
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
    c!("sim.status", "Simulation status", "", None, "{} → {time_us, events, storm, constant_delay_us, capture}", false),
    c!(
        "sim.constant_delay",
        "Constant Delay",
        "Simulation",
        None,
        r#"{"on": true|false, "us"?: hop time in µs (1-1000000, default 1000)} → {"on", "us"}: every hop then takes the same simulated time, whatever queues would add"#,
        false
    ),
    c!(
        "sim.capture_filter",
        "Capture only filtered events",
        "Simulation",
        None,
        r#"{"on": true|false, "protocols"?: ["ICMP", "ARP", …] (needed when on), "prune"?: true (also drop recorded events of other protocols)} → {"on", "protocols", "removed"}: the event list and the pcap capture then keep only these protocols"#,
        false
    ),
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
    c!(
        "sim.event",
        "PDU details",
        "",
        None,
        r#"{"id": event} → {id, at_us, from, to, protocol, summary, dropped, cause, "explain": [what the receiver decided], "osi": {"in": [{layer, lines}], "out": [...]}, "sent": [events it caused]}"#,
        false
    ),
    c!(
        "pc.http_request",
        "HTTP request (Programming)",
        "",
        None,
        r#"{"pc": "PC, laptop or server", "method"?: "GET"|"PUT"|"PATCH"|"POST"|"DELETE"|…, "url": "https://10.0.0.1/restconf/data/ietf-interfaces:interfaces", "headers"?: {"Accept": "application/yang-data+json"} or [["Name", "value"]], "body"?: text or JSON, "user"?, "password"?: Basic login} → {"status", "reason", "headers", "body", "json", "applied": [{"device", "lines", "failed"}]}: over TCP through the lab (port 443 carries no TLS)"#,
        true
    ),
    c!(
        "netconf.session",
        "NETCONF session",
        "",
        None,
        r#"{"pc": "PC, laptop or server", "target": "router address or name", "user", "password", "framing"?: "1.1"|"1.0", "rpcs"?: ["<get-config><source><running/></source></get-config>", …] (bare operations are wrapped in <rpc message-id=…>)} → {"connected", "error"?, "framing", "hello", "replies": [XML], "applied": [{"device", "lines", "failed"}]}: TCP port 830 with RFC 6242 framing; a <close-session> ends it"#,
        true
    ),
    c!(
        "controller.devices",
        "Controller: managed routers",
        "",
        None,
        r#"{"host": "the controller (PC, laptop or server)", "add"?: {"address", "user", "password"}, "remove"?: address, "refresh"?: true (read each router's hostname and interfaces over RESTCONF)} → {"devices": [{"address", "user", "reachable"?, "hostname"?, "interfaces"?, "up"?, "error"?}]}"#,
        true
    ),
    c!(
        "controller.push",
        "Controller: push a template",
        "",
        None,
        r#"{"host", "path": "/restconf/data/…", "method"?: "PATCH"|"PUT"|"POST"|"DELETE", "body": JSON template, "devices"?: [addresses] (all managed routers when left out)} → {"results": [{"address", "status", "error"?, "applied"}]}"#,
        true
    ),
    c!(
        "pc.http_saved",
        "Saved HTTP requests",
        "",
        None,
        r#"{"pc", "save"?: {"name", "method", "url", "headers"?, "body"?}, "remove"?: "name"} → {"saved": [{"name", "method", "url", "headers", "body"}]}"#,
        true
    ),
];

pub fn find(id: &str) -> Option<&'static CommandInfo> {
    CATALOG.iter().find(|c| c.id == id)
}
