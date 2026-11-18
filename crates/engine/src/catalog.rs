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
        r#"{"device", "port"?: "Wireless0", "ssid": "network name (empty: radio off)", "mode"?: "open"|"wep"|"wpa-psk"|"wpa2-psk"|"wpa-enterprise"|"wpa2-enterprise" (none: WPA2-PSK with a key, else open), "key"?: "WPA-PSK or WPA2-PSK passphrase, 8-63 characters", "wep_key"?: "5 or 13 ASCII characters, or 10 or 26 hex digits", "encryption"?: "aes"|"tkip" (WPA modes, default aes), "channel"?: 1-13, "coverage_m"?: 1-2000 (access points, default 250), "radius_ip"?, "radius_secret"? (access points in an enterprise mode), "user"?: "enterprise username (hosts)", "password"?} → {associated_with, problem, mode, encryption, coverage_m}"#,
        true
    ),
    c!(
        "cell.settings",
        "Cellular settings",
        "",
        None,
        r#"{"device": smartphone or tablet, "data"?: true|false (Cellular Data), "apn"?: "access point name, up to 32 characters (empty: none)"} → the phone's status as cell.status"#,
        true
    ),
    c!(
        "cell.core",
        "Central office service",
        "",
        None,
        r#"{"device": Central Office Server, "pool"?: "10.64.0.0/24" (the network the devices' addresses come from; null: none), "apn"?: "the one APN it admits (empty or null: any)", "plan"?: true|false (the data plan)} → the server's status as cell.status"#,
        true
    ),
    c!(
        "cell.status",
        "Cellular status",
        "",
        None,
        r#"{"device": phone, tablet, tower or central office} → phone: {data, apn, active: "wifi"|"cellular"|"none", tower, bars: 0-4, attached, address, gateway, refused, attach_answer}; tower: {clients}; central office: {pool, apn, plan, admitted, refused}"#,
        false
    ),
    c!(
        "editor.list",
        "Text Editor files",
        "",
        None,
        r#"{"device": PC, laptop or server} → {"files": [{"name", "bytes"}], "max_files": 32, "max_bytes": 16384}"#,
        false
    ),
    c!("editor.read", "Open text file", "", None, r#"{"device", "name"} → {"name", "text", "bytes"}"#, false),
    c!(
        "editor.write",
        "Save text file",
        "",
        None,
        r#"{"device": PC, laptop or server, "name": "up to 64 characters, no spaces or slashes", "text": "up to 16384 bytes"} → {"name", "bytes", "created"}: replaces the file of that name, else adds one (at most 32 files; the dir and ftp commands of the Command Prompt see them too)"#,
        true
    ),
    c!("editor.delete", "Delete text file", "", None, r#"{"device", "name"}"#, true),
    c!(
        "host.firewall",
        "Firewall",
        "",
        None,
        r#"{"device": PC, laptop or server, "version"?: 4 or 6 (default 4: each has its own list and switch), "on"?: true or false, "add"?: {"action": "allow" or "deny", "protocol"?: "any", "tcp", "udp" or "icmp", "remote"?: "10.0.0.0/24" or "any", "port"?: 80 or "1000-2000" (tcp and udp), "at"?: position from 1 (default: last)}, "remove"?: rule number from 1, "clear"?: true} → {"version", "on", "rules": [{"n", "action", "protocol", "remote", "port"}]}. Off: everything is let in. On: the first matching rule decides and no match is dropped; echo requests, new TCP connections and UDP datagrams are matched, answers to what the device sent always pass; at most 32 rules"#,
        true
    ),
    c!(
        "traffic.send",
        "Send traffic",
        "",
        None,
        r#"{"device": PC, laptop or server, "to": "address or device name", "protocol"?: "icmp" (default), "udp" or "tcp", "port"?: 1-65535 (required for udp and tcp), "size"?: 0-1400 bytes (icmp and udp payload; default 32), "count"?: 1-100 (default 4), "interval"?: 0-5 simulated seconds between packets (default 1)} → {"sent", "received", "lost", "results": [{"n", "ok", "note", "at"}], …}: sends the packets one after another as PDUs (the same path, firewalls and access lists as pdu.complex) and returns when the last one is answered or has timed out; no packet is answered for a closed port or a blocked path"#,
        false
    ),
    c!(
        "traffic.status",
        "Traffic Generator results",
        "",
        None,
        r#"{"device": PC, laptop or server} → the last burst the device sent, as traffic.send answers, or {"ran": false, "max_count", "max_interval", "max_size"} when it has sent none"#,
        false
    ),
    c!(
        "pppoe.connect",
        "PPPoE connect",
        "",
        None,
        r#"{"device": PC, laptop or server, "username"?: account name, "password"?: text (both default to the ones last used)} → {"state", "connected", "server", "address", "mask", "gateway", "auth", …}: the device's first port must reach a server whose PPPoE service is on (straight, or through switches); PADI, PADO, PADR and PADS, then PPP in session frames (LCP, PAP or CHAP, IPCP), cross the cables and show in the event list. The server gives the first free address of its pool, with its own address as the default gateway. Fails when no server answers, the account is refused or the pool is empty"#,
        true
    ),
    c!(
        "pppoe.disconnect",
        "PPPoE disconnect",
        "",
        None,
        r#"{"device": PC, laptop or server} → the status: an LCP Terminate-Request and a PADT go to the server, and the address and the gateway the session gave are removed"#,
        true
    ),
    c!(
        "pppoe.status",
        "PPPoE status",
        "",
        None,
        r#"{"device": PC, laptop or server} → {"state": "disconnected"|"connected"|"lost", "connected", "username", "server_found", "server", "address", "mask", "gateway", "auth": "pap"|"chap"}: a connected session is proved by a keep-alive (LCP echo) over it, a disconnected one by a discovery request"#,
        false
    ),
    c!(
        "dialup.connect",
        "Dial-up connect",
        "",
        None,
        r#"{"device": Dial-up PC (a computer whose first port is a modem, Phone0, cabled to a telephone cloud), "number"?: telephone number (digits; spaces, dashes, dots, brackets and a leading + are ignored; default: the last one used), "username"?: account name, "password"?: text (both default to the ones last used)} → {"state", "connected", "number", "carrier", "called", "address", "mask", "gateway", "auth", …} as pppoe.status: the call joins the modem's port in the telephone cloud to the port that has the number (busy if either is in a call), then PPP (LCP, PAP or CHAP, IPCP) runs over it with the access server's modem line. Fails when the number is not in the cloud, is busy, has nothing plugged in, or no modem line answers, or when the account is refused"#,
        true
    ),
    c!(
        "dialup.disconnect",
        "Dial-up hang up",
        "",
        None,
        r#"{"device": Dial-up PC} → the status: PPP is closed and the call's pair in the telephone cloud is parted, so the line is free again"#,
        true
    ),
    c!(
        "dialup.status",
        "Dial-up status",
        "",
        None,
        r#"{"device": Dial-up PC} → {"state": "disconnected"|"connected"|"lost", "connected", "number", "carrier": true while the call's circuit is up, "called": the number the circuit goes to, "cloud", "username", "server", "address", "mask", "gateway", "auth"}"#,
        false
    ),
    c!(
        "vpnclient.connect",
        "VPN Client connect",
        "",
        None,
        r#"{"device": PC, laptop or server, "host"?: the VPN concentrator's address, "group"?: group name, "key"?: group key, "username"?: text, "password"?: text (each defaults to the one last used)} → {"state", "connected", "server", "address", "transforms", "esp", …}: sends a request to UDP port 10000 of the concentrator, a router set up as a remote-access server (a client group with its key and pool, a dynamic map, a crypto map with its authentication and authorization lists on the interface the host reaches). The router checks the group key and the user and gives the first free address of the group's pool; from then on the device's traffic beyond its own subnet goes through an ESP tunnel to the router. Fails when nothing answers or the router refuses; the simulator's own exchange, not IKE with Xauth and mode-config"#,
        true
    ),
    c!(
        "vpnclient.disconnect",
        "VPN Client disconnect",
        "",
        None,
        r#"{"device": PC, laptop or server} → the status: the router is told and the tunnel and the VPN address are gone"#,
        false
    ),
    c!(
        "vpnclient.status",
        "VPN Client status",
        "",
        None,
        r#"{"device": PC, laptop or server} → {"state": "disconnected"|"connected", "connected", "host", "group", "username", "server", "address", "transforms", "esp", "encapsulated", "decapsulated"} (the key and the password are never returned)"#,
        false
    ),
    c!(
        "netflow.listen",
        "Netflow Collector start",
        "",
        None,
        r#"{"device": PC, laptop or server, "port"?: 1-65535 (default: the one last used, else 2055)} → the collector's state as netflow.flows: opens the UDP port; routers set up with `ip flow-export destination` send it NetFlow version 5 or 9 datagrams. The port is saved with the lab, what was received is not"#,
        true
    ),
    c!(
        "netflow.stop",
        "Netflow Collector stop",
        "",
        None,
        r#"{"device": PC, laptop or server} → the collector's state as netflow.flows: closes the port (what was received stays until cleared); fails when it is not listening"#,
        true
    ),
    c!(
        "netflow.flows",
        "Netflow Collector flows",
        "",
        None,
        r#"{"device": PC, laptop or server, "limit"?: 1-500 flows to list (default 100, the newest)} → {"listening", "port", "datagrams", "bad_datagrams", "flows_received", "packets", "bytes", "top_talkers": [{"address", "flows", "packets", "bytes"}] (ten source addresses by bytes), "protocols": [{"protocol", "number", "flows", "packets", "bytes"}], "flows": [{"at_ms", "exporter", "version", "src", "dst", "next_hop", "protocol", "src_port", "dst_port", "packets", "bytes", "in_if", "out_if", "tcp_flags", "first_ms", "last_ms"}]} (at most 2000 flows are kept, the oldest go first)"#,
        false
    ),
    c!(
        "netflow.clear",
        "Netflow Collector clear",
        "",
        None,
        r#"{"device": PC, laptop or server} → the collector's state as netflow.flows, with nothing received"#,
        false
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
        r#"{"student"?: "name" (default: the profile's name, see profile.set)} → {"values", "instructions"}: picks this student's variable values"#,
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
        r#"{} → {"items": [{"key", "device", "device_kind": "router"|"switch"|"end"|"other", "kind": "config"|"address"|"cable", "text", "points" (0: not graded), "feedback"}], "points"}: what the answer network holds, per device"#,
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
        r#"{"format"?: "json"|"csv", "path"?, "time"?, "elapsed_s"?} → {"text", "file_name", "score", "total", "percent", "replayable", "replay_agrees"}: a results file with the student, the time, the score per section, signed with HMAC-SHA1 under a key from the activity's salt; a JSON file also embeds the student's final network (lab file format) so the score can be replayed"#,
        false
    ),
    c!(
        "activity.verify_results",
        "Verify a results file",
        "",
        None,
        r#"{"text" or "path"} → {"valid", "same_activity", "signature_ok", "replayed", "replay_matches", "replay_score", "replay_total", "mismatches", "message", "student", "score", "total", "percent"}: checks the signature (hand edits) and, for a file with an embedded network, runs the activity's checks again and compares (forgery); a CSV or older file is not replayed and says so"#,
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
        "file.backup",
        "Backup copy",
        "",
        None,
        r#"{"path": "file to write (desktop/CLI); it must not exist"} → {"path", "bytes"}: writes a copy of the open lab without changing its path or its unsaved mark, and never replaces a file"#,
        false
    ),
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
        r#"{"device", "services": {"http": {"on", "files": [{"name", "content"}]}, "dns": {"on", "records": [{"name", "address"}]}, "dhcp": {"on", "pools": [{"name", "gateway", "dns", "start", "mask", "max_users"}]}, "pppoe": {"on", "auth": "pap"|"chap", "pool_start": address, "pool_size": 0-250, "users": [{"name", "password"}]}}}"#,
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
        r#"{"device", "slot": "0/1", "module": "Card-2S|Card-4S|Card-1GE|Card-2GE|Card-1FE|Card-1GF|Card-8A"} → {"slots", "added": [port names]}: modular routers, switched off"#,
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
        "port.transceiver",
        "Fiber transceiver",
        "",
        None,
        r#"{"device", "port": "fiber or combo port", "mode": "multi-mode" (850 nm, the default) | "single-mode" (1310 nm)} → {"device", "port", "mode"}: the transceiver in the port; both ends of a fiber link need the same one, and a single mode fiber cable needs single-mode transceivers"#,
        true
    ),
    c!(
        "iot.configure",
        "Smart thing settings",
        "",
        None,
        r#"{"device": smart sensor or device, "server"?: "IoT server address (empty: none)", "value"?: sensor reading -1000..1000 (0 or 1 for the switches, the metal sensor, the trip wire and the water detector; 0-100 for the flex sensor and the membrane potentiometer), "press"?: true (flips a push button toggle switch or a toggle push button), "humidity"?: 0-100 (a humiture sensor's second reading), "variable"?: environment variable id or name (the generic environment sensor reads it), "level"?: 0-100 (dimmable LED) | 0-180 (servo) | 0-16777215 (RGB LED, 0xRRGGBB), "red"?|"green"?|"blue"?: 0-255 (RGB LED), "brightness"?: smart LED 0..100, "colour"?: smart LED colour as hex rrggbb or #rrggbb, "frequency"?: 0..20000 Hz (a signal generator's output, the pitch a speaker, player, buzzer, alarm or siren plays (0: its own), the pitch a sound detector is tuned to (0: any; it hears only sounds within a tenth of it)), "pairing_code"?: "0000".."9999" (a Bluetooth speaker pairs with a player that has the same code), "profile"?: "a2dp"|"headset" (a Bluetooth speaker; a headset pairs with no player), "tag"?: an RFID card's id typed in (a number, or up to eight hex digits; 0: the card's own), "crossing"?: true|false (a trip sensor: read 1 for five seconds after something that moves crosses its 50 m wire instead of while something is near), "angle"?: 0..359 (a trip sensor: the wire's direction)} → {"kind", "server", "value", "second", "value2" (a second reading), "variable", "level", "brightness", "colour", "frequency", "registered", "on", "active", "paired", "pairing_code", "profile", "tag", "crossing", "angle", "message"} (reports at once)"#,
        true
    ),
    c!(
        "iot.beacon",
        "Beacon log",
        "",
        None,
        r#"{"device": beacon} → {"device", "audience": [names of the things that hear it], "log": [{"time_s", "thing", "event": "heard"|"lost"}]}: what a beacon keeps (the last 60 things that came into its 100 m range or left it, as of the last step of the simulator)"#,
        false
    ),
    c!(
        "board.program",
        "Board program",
        "",
        None,
        r#"{"device": programmable board, "source"?: "program text", "language"?: "boardscript"|"python" (Board SBC)|"javascript" (Board MCU), "run"?: true|false, "server"?: "IoT server address for get() and report (empty: none)"} → the board's status: run starts it from the top, false stops it; a program that doesn't compile in its language is refused with its line"#,
        true
    ),
    c!(
        "board.status",
        "Board status",
        "",
        None,
        r#"{"device": programmable board} → {"program", "language", "languages", "run", "state": "not started"|"running"|"stopped", "line", "message", "log": [...], "vars": {...}, "pins": [{"pin", "analog", "wired", "what", "level"}], "server"}"#,
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
    c!(
        "iot.environment",
        "IoT environment",
        "",
        None,
        r#"{"enabled"?: true|false, "temperature"?: -50..100, "humidity"?: 0..100, "soil"?: 0..100, "smoke"?: 0..100, "co"?: 0..1000, "motion"?: 0|1, "fire"?: 0..100, "co2"?: 0..10000, "outside_temperature"?, "outside_humidity"?, "outside_light"?: 0..100, "reset"?: true} → {"enabled", "outside", "start", "room"}: the smart things' shared room; while enabled, heaters, furnaces, air conditioners, fans, ceiling fans, humidifiers, sprinklers, fire sprinklers, running cars, windows, garages, lamps and anything playing sound change it over simulated time and the sensors read it (given values are set now and become the starting values)"#,
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
        "environment.get",
        "Environments",
        "Options",
        Some("Shift+E"),
        r#"{"place"?: place number, name or "intercity" (default: the first wiring closet)} → {"place": {id, name, kind}, "path": [the places from the Intercity map down to it], "time": "HH:MM:SS", "seconds", "clock": {paused, real_minutes, sim_minutes}, "notes", "show_notes", "name", "interpolation", "hidden", "hidden_from", "variables": [{id, label, group, unit, value, source: "own"|"inherited"|"default"|"room"|"live", from, default, min, max, drives, keys: [{time, seconds, value}]}]}: the surroundings of a place at the environment clock's time now. A place that sets nothing inherits from the place it stands in (closet, building, city, Intercity), then the default (an ordinary physical constant or 0); "room" is the smart-home room's live reading for what its sensors read, "live" what a place holds now where its parts are moving it (environment.physics)."#,
        false
    ),
    c!(
        "environment.set",
        "Set an environment variable",
        "",
        None,
        r#"{"place"?, "variable"?: id or label, "value"?: number within the variable's range (a constant for the whole day: its keyframes are replaced), "notes"?: text (up to 4000 characters), "show_notes"?: true|false} → environment.get; one undo step. Needs a variable and its value, or notes, or show_notes"#,
        true
    ),
    c!(
        "environment.clear",
        "Clear an environment variable",
        "",
        None,
        r#"{"place"?, "variable"?: id or label (none: every variable of the place)} → environment.get: the place inherits again; one undo step"#,
        true
    ),
    c!(
        "environment.keyframe.add",
        "Add an environment keyframe",
        "",
        None,
        r#"{"place"?, "variable", "time": "HH:MM:SS" or seconds from midnight (0-86399), "value": number within the variable's range} → environment.get: adds a keyframe or changes the one at that time (at most 288 per variable); between keyframes the value moves in a straight line, and after the last it moves toward the first of the next day; one undo step"#,
        true
    ),
    c!(
        "environment.keyframe.remove",
        "Remove an environment keyframe",
        "",
        None,
        r#"{"place"?, "variable", "time"} → environment.get: removes the keyframe at that time; the variable is inherited again when none is left; one undo step"#,
        true
    ),
    c!(
        "environment.clock",
        "Environment clock",
        "",
        None,
        r#"{"time"?: "HH:MM:SS" or seconds from midnight (set the time of day now), "paused"?: true|false, "real_minutes"?: 1-1440, "sim_minutes"?: 1-1440 (every `real_minutes` of simulated lab time pass `sim_minutes` in the environment; the time shown does not jump when the ratio changes)} → environment.get. The clock follows the lab's simulated time (it starts at 08:00:00); one undo step when something changes"#,
        true
    ),
    c!(
        "environment.create",
        "Create custom environment",
        "",
        None,
        r#"{"place"?, "name": text up to 60 characters} → environment.get: gives the place a custom environment of its own with this name (what it sets and the variables it defines are its own); one undo step. environment.clear with no variable removes it again"#,
        true
    ),
    c!(
        "environment.custom",
        "Define a custom variable",
        "",
        None,
        r#"{"place"?, "id": lowercase letters, digits and `_` (up to 32, not a built-in id), "label"?, "unit"?, "min"?, "max"?, "default"? (what it reads while the formula is empty or fails), "formula"?: text up to 200 characters} → environment.get: defines or changes a variable of the place by a formula over `time` (seconds), `hour` (0-24), `day` (0-1), the built-in variables (as the place reads them) and other custom variables, with + - * / % ^, comparisons, and, or, not, sin cos abs sqrt floor ceil round exp ln min max clamp if(c, a, b), pi and e; at most 16 per place; a formula that cannot be computed shows its error and the default; a generic environment sensor can read the variable (iot.configure "variable"); one undo step"#,
        true
    ),
    c!("environment.custom.remove", "Remove a custom variable", "", None, r#"{"place"?, "id"} → environment.get; one undo step"#, true),
    c!(
        "environment.shows",
        "Environment shows",
        "",
        None,
        r#"{"place"?, "hidden"?: [variable ids the window leaves out for this place], "copy_from_parent"?: true (Copy Shows from Parent), "only_active"?: true (Show Only Actives: leave out every variable no place from here up sets), "reset"?: true (read the place above again)} → environment.get with "hidden" and "hidden_from": the shows are saved with the lab; one undo step when something changes"#,
        true
    ),
    c!(
        "environment.defaults",
        "Environment defaults",
        "",
        None,
        r#"{"place"?, "name"?: "desert"|"tropical"|"arctic"|"indoor-office"|"greenhouse"} → with no name {"defaults": [{id, label}]}: the bundled environments of ours; with a name, the place sets what that environment sets (replacing what it set) and takes its name → environment.get; one undo step"#,
        true
    ),
    c!(
        "environment.settings",
        "Environment settings",
        "",
        None,
        r#"{"place"?, "interpolation"?: "linear"|"step"|"smooth" (how this place's keyframes are joined: a straight line, the value of the last keyframe until the next, or a half cosine)} → environment.get; one undo step when something changes"#,
        true
    ),
    c!(
        "environment.physics",
        "Physics of the places",
        "",
        None,
        r#"{"enabled"?: true|false} → {"physics"}: with it on, each place that holds smart things has its own physics: the heaters, air conditioners, fans, furnaces, humidifiers, sprinklers, fire sprinklers, windows and lamps standing in a place change that place's variables by rates (a part moves a variable toward where it settles; the variable covers 63 % of the way in 0.5 to 20 minutes, depending on the variable), the place settles back toward what it sets when the part stops, flames make smoke, carbon monoxide and heat, an open window pulls the place toward the one above it (faster in wind), and a sound fades with the distance to the sensor; environment.get shows what a place holds now as source "live". Off (the default): a running part adds a fixed amount at once, as before; one undo step"#,
        true
    ),
    c!(
        "environment.export",
        "Export environment",
        "",
        None,
        r#"{"place"?} → {"place", "file": suggested file name, "text"}: the variables and notes the place sets itself, as a JSON file of its own (format "netlabcraft-environment", docs/file-format.md); the app saves it with a dialog"#,
        false
    ),
    c!(
        "environment.import",
        "Import environment",
        "",
        None,
        r#"{"place"?, "text": contents of an exported environment, "merge"?: true (only replace the variables the file names; default: replace everything the place sets)} → environment.get plus "imported": {variables, skipped}: unknown variables are skipped and values are clamped to their range; one undo step"#,
        true
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
        "softphone.config",
        "Softphone settings",
        "",
        None,
        r#"{"device": PC, laptop or server, "enabled"?: true|false (switch the Softphone on or off), "server"?: "call manager address (empty: the TFTP server DHCP option 150 gave)", "protocol"?: "sip"|"own"} → the Softphone's status (it registers again; the extension is the one the call manager gives its MAC address)"#,
        true
    ),
    c!(
        "softphone.status",
        "Softphone screen",
        "",
        None,
        r#"{"device": PC, laptop or server with the Softphone on} → {"address", "mac", "softphone", "call_manager", "tftp", "protocol", "registered", "number", "server", "message", "call": {"state": "dialing"|"ringback"|"ringing"|"connected", "peer", "peer_address", "rtp_sent", "rtp_received"}} (lets it register first)"#,
        false
    ),
    c!(
        "softphone.dial",
        "Softphone: dial a number",
        "",
        None,
        r#"{"device": PC with the Softphone on, "number": "1002"} → its status: the far phone or Softphone rings"#,
        false
    ),
    c!(
        "softphone.answer",
        "Softphone: answer",
        "",
        None,
        r#"{"device": a PC whose Softphone is ringing} → its status: the call connects and voice packets flow"#,
        false
    ),
    c!("softphone.hangup", "Softphone: hang up", "", None, r#"{"device": PC with the Softphone on} → its status: the call ends at both ends"#, false),
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
        "cloud.number",
        "Telephone numbers",
        "",
        None,
        r#"{"device": telephone cloud, "port"?: "Phone0", "number"?: "5551001" (digits; null removes it)} → {"numbers": [{"port", "number"}…], "calls": [[a, b]…], "ports"}: gives a phone port its number (one port has each number), or lists the numbers and the calls in progress"#,
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
        r##"{"x", "y", "text"?, "shape"?: "text|rectangle|ellipse|line|freehand|polygon", "w"?, "h"?, "points": [[x, y], …] (freehand: simplified, at most 512 kept; polygon: 3-64 corners), "color"?: "#rrggbb" (the fill; a line's colour), "outline"?: true|false, "outline_color"?: "#rrggbb", "outline_width"?: 1-8, "fill"?: "none"|"#rrggbb" (rectangle, ellipse, polygon)} → {"id"} (placed in the cluster being viewed); without the style keys a drawing keeps the single-colour look of older labs"##,
        true
    ),
    c!("note.edit", "Edit note", "", None, r#"{"id", "text"}"#, true),
    c!("note.move", "Move note", "", None, r#"{"id", "x", "y", "transient"?: false}"#, true),
    c!(
        "note.resize",
        "Resize drawing",
        "",
        None,
        r#"{"id", "x", "y", "w", "h", "transient"?: false}: rectangle and ellipse: top-left corner and size (normalised, sides kept between 4 and 100000); line: start and the offset to its far end (at least 4 long); text, freehand and polygon are refused"#,
        true
    ),
    c!("note.remove", "Delete note", "", None, r#"{"id"}"#, true),
    c!("note.color", "Drawing colour", "", None, r##"{"id", "color": "#rrggbb"}"##, true),
    c!(
        "note.style",
        "Drawing style",
        "",
        None,
        r##"{"id", "outline"?: true|false, "outline_color"?: "#rrggbb", "outline_width"?: 1-8, "fill"?: "none"|"#rrggbb", "color"?: "#rrggbb" (same as a fill colour)}: at least one key; a rectangle, ellipse or polygon takes all of them; a line or freehand stroke takes the outline colour and width (it is all outline: `fill` only sets its base colour like `color`, and `none` is refused); a text note is refused; a refused style changes nothing"##,
        true
    ),
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
        r#"{"kind": "city"|"building"|"closet"|"container", "name"?, "parent"?: place number (a building goes in a city, a closet in a building, a container in a city or a building; default: the place being viewed), "x"?, "y"?} → {"id", "name", "parent"}"#,
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
        "place.scene.set",
        "Set place pictures",
        "",
        None,
        r#"{"place"?: number, name or "intercity" (default: the place being viewed), "background"?: {"data": base64 PNG or JPEG | "path": file (desktop and CLI) | "image": id of a picture the lab holds (none: the one there now), "mode"?: "original"|"tiled"|"fit"|"width"|"height", "custom"?: width or height in map units for the last two} or null, "icon"?: {"data" | "path" | "image"} or null, "size"?: {"width", "length"?, "height"? in metres, "scale"? metres per map unit} or null}: what is left out stays, null clears; one undo step. Pictures are checked, stripped of metadata and scaled down to 4096 pixels and 8 MiB; at most 64 pictures and 32 MiB in a lab → {"place", "background", "icon", "size", "scale", "scaled_from"?: [w, h]}"#,
        true
    ),
    c!(
        "place.background.set",
        "Set place background",
        "",
        None,
        r#"{"place"?, "data" | "path" | "image", "mode"?: "original"|"tiled"|"fit"|"width"|"height", "custom"?: map units} → as place.scene.set: the picture under the map inside the place (the Intercity map is place 0)"#,
        true
    ),
    c!("place.background.clear", "Clear place background", "", None, r#"{"place"?} → as place.scene.set"#, true),
    c!(
        "place.icon.set",
        "Set place icon",
        "",
        None,
        r#"{"place": a city, building or closet, "data" | "path" | "image"} → as place.scene.set: the picture the place wears on the map above it, clipped to a rounded square"#,
        true
    ),
    c!("place.icon.clear", "Clear place icon", "", None, r#"{"place"} → as place.scene.set"#, true),
    c!(
        "place.size.set",
        "Set place size",
        "",
        None,
        r#"{"place"?, "width": metres (kept when left out), "length"?: metres (default: the proportions of the background), "height"?: metres (default 3), "scale"?: metres per map unit (default: width over the width the background is drawn at)} → as place.scene.set: the scale replaces the built-in one (1 m on a city, 100 m on the Intercity map) for cable lengths, cell tower reach and the Wi-Fi range of devices in the same place"#,
        true
    ),
    c!("place.size.clear", "Clear place size", "", None, r#"{"place"?} → as place.scene.set"#, true),
    c!(
        "workspace.background.set",
        "Set workspace background",
        "",
        None,
        r#"{"data" | "path" | "image", "mode"?: "original"|"tiled"|"fit"|"width"|"height", "custom"?: map units} → {"background": {"image", "mode", "custom", "pixels", "shown"}, "scaled_from"?}: the picture under the logical workspace"#,
        true
    ),
    c!("workspace.background.clear", "Clear workspace background", "", None, r#"{}"#, true),
    c!(
        "image.list",
        "List pictures",
        "",
        None,
        r#"{} → {"images": [{"id", "kind", "w", "h", "bytes"}], "uses": [{"kind": "logical"|"background"|"icon", "place", "place_name", "image"}], "total_bytes", "limits"}"#,
        false
    ),
    c!(
        "image.remove",
        "Remove picture",
        "",
        None,
        r#"{"kind": "logical"|"background"|"icon", "place"? (not for logical)} → image.list: stops using the picture there; the picture goes when nothing else uses it"#,
        true
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
    c!(
        "closet.list",
        "List a wiring closet",
        "",
        None,
        r#"{"place"?: closet number (default: the closet being viewed)} → {"place", "furniture": [{"id", "kind", "name", "size", "items": [{"device" | "passive": name, "slot" (from 1), "units"}]}], "passive": [{"id", "kind", "name", "ports", "on", "furniture", "slot", "plugged": [{"device", "outlet"}]}], "loose": [names standing on the floor], "wires": [{"a", "b"}], "paths": [{"a", "b"}] (devices cabled through patch panels)}"#,
        false
    ),
    c!(
        "furniture.add",
        "Add furniture",
        "",
        None,
        r#"{"kind": "rack" | "shelf" | "table" | "inventory-shelf" | "pegboard" | "icp", "name"?, "place"?: wiring closet or container number, "size"?: rack units 4-48 (default 12), shelf or inventory shelf levels 1-8 (default 4), pegboard rows of hooks 1-8 (default 4) or control panel DIN slots 6-24 (default 12), "x"?, "y"?} → {"id", "name", "kind", "size", "place"}: furniture stands only in wiring closets and containers; a device put on an inventory shelf is switched off; a control panel takes industrial devices only; a pegboard takes cables, not devices"#,
        true
    ),
    c!("furniture.rename", "Rename furniture", "", None, r#"{"furniture": name or number, "name"}"#, true),
    c!(
        "furniture.resize",
        "Resize furniture",
        "",
        None,
        r#"{"furniture", "size": rack units 4-48 or shelf levels 1-8} → null: refused when something stands past the new size; a table has one size"#,
        true
    ),
    c!("furniture.move", "Move furniture", "", None, r#"{"furniture", "x", "y", "transient"?: false}: where it stands in the closet"#, true),
    c!("furniture.remove", "Remove furniture", "", None, r#"{"furniture"}: what stood on it stays in the closet, loose"#, true),
    c!(
        "furniture.mount",
        "Put on furniture",
        "",
        None,
        r#"{"device" | "passive": name or number, "furniture": name or number, "slot"?: rack unit or shelf level counted from 1 at the top (default: the first free one), "units"?: 1-4 rack units (default 1)} → {"furniture", "slot", "units"}: the item moves into the furniture's closet; a rack takes no overlapping units, a shelf level holds 4 items, a table 8"#,
        true
    ),
    c!("furniture.unmount", "Take off furniture", "", None, r#"{"device" | "passive"}: it stays in the closet, loose"#, true),
    c!(
        "furniture.hang",
        "Hang a cable",
        "",
        None,
        r#"{"furniture": cable pegboard name or number, "device", "port": either end of the cable, "peg"?: hook counted from 1 (default: the first free one)} → {"furniture", "peg"}: one of the cable's ends stands in the pegboard's place; a cable hangs on one peg"#,
        true
    ),
    c!("furniture.unhang", "Take a cable off its peg", "", None, r#"{"device", "port": either end of the cable}"#, true),
    c!(
        "place.working.set",
        "Set as working closet",
        "",
        None,
        r#"{"id"?: wiring closet number (default: the closet being viewed)} → {"id", "name"}: the closet the Home button goes to"#,
        true
    ),
    c!(
        "place.working.get",
        "Working closet",
        "",
        None,
        r#"{} → {"id", "name"}: the marked closet, else the first closet; null when there is none"#,
        false
    ),
    c!(
        "passive.add",
        "Add a passive device",
        "",
        None,
        r#"{"kind": "copper-patch-panel" | "fiber-patch-panel" | "copper-wall-mount" | "fiber-wall-mount" | "power-strip", "name"?, "place"?, "ports"?: ports or outlets (panels 4-48, wall mounts 1-4, power strip 2-16), "furniture"?, "slot"?, "units"?} → {"id", "name", "kind", "ports", "place"}"#,
        true
    ),
    c!("passive.rename", "Rename a passive device", "", None, r#"{"passive": name or number, "name"}"#, true),
    c!(
        "passive.remove",
        "Remove a passive device",
        "",
        None,
        r#"{"passive"}: with its patch wires (a cable they carried is unplugged) and its plugs"#,
        true
    ),
    c!(
        "patch.wire",
        "Patch wire",
        "",
        None,
        r#"{"a", "b"}, each {"panel": passive name, "port": from 1} or {"device": name, "port": "Fa0/1"}: a patch cord joins two panel or wall-mount ports, a drop joins one to a free device port of the same medium (copper or fiber), all in one closet; a drop at each end of a cord is a patch path and cables the two devices (an automatic cable) → {"cables": [the ends cabled]}"#,
        true
    ),
    c!(
        "patch.unwire",
        "Remove a patch wire",
        "",
        None,
        r#"{"a", "b"?}: the wire between the two ends, or with no `b` every wire at `a`; a cable the wire's path carried is unplugged"#,
        true
    ),
    c!(
        "power.plug",
        "Plug into a power strip",
        "",
        None,
        r#"{"device", "strip": power distribution device, "outlet"?: from 1 (default: the first free one)} → {"strip", "outlet"}: the device and the strip stand in one closet; plugged into a strip that is off, the device switches off"#,
        true
    ),
    c!("power.unplug", "Unplug from a power strip", "", None, r#"{"device"}"#, true),
    c!(
        "power.strip",
        "Power strip switch",
        "",
        None,
        r#"{"strip": power distribution device, "on": true|false or "on"|"off"} → {"on", "tripped", "devices"}: the devices plugged into it switch off or boot from their saved configuration as with device.power; one plugged into a strip that is off cannot be switched on; switching on more than the breaker rating carries trips it again, and a tripped strip stays off while what is plugged in is over its rating"#,
        true
    ),
    c!(
        "power.rating",
        "Power strip breaker rating",
        "",
        None,
        r#"{"strip": power distribution device, "watts": 100-3680} → {"rating_w", "load_w", "tripped"}: the breaker carries the nominal draws of the devices that are on (Device::nominal_watts, a table per model); a rating below the load trips it at once"#,
        true
    ),
    c!(
        "power.load",
        "Power strip load",
        "",
        None,
        r#"{"strip": power distribution device} → {"strip", "on", "tripped", "rating_w", "load_w" (devices that are on), "demand_w" (everything plugged in), "outlets": [{"outlet", "device", "watts", "on"}]}; over the rating the breaker trips: the strip switches off and its devices lose power, and power.strip on resets it once what is plugged in fits"#,
        false
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
        r#"{"a": "device", "a_port"?: "interface", "b": "device", "b_port"?: "interface", "cable"?: "auto" (default) | "console" | "straight-through" | "cross-over" | "fiber-single-mode" | "fiber-multi-mode" | "phone" | "coaxial" | "serial-dce" | "serial-dte" | "octal" | "iot-custom" | "usb" ("fiber" reads as multi-mode), "line"?: 0-7 (octal)} → {"a", "b", "cable", "resolved"}: a cable joins two ports of its medium; a console cable joins a PC's RS-232 port to a router's or switch's Console port, a USB cable a PC's USB port to a router's or switch's USB console port, an octal cable a line of a router's octal card (Card-8A) to another router's or switch's Console port (a is the router with the card unless only b has one)"#,
        true
    ),
    c!(
        "link.remove",
        "Remove cable",
        "Edit",
        None,
        r#"{"device", "port": "interface", or "Console" / "RS-232" / "USB" for a console or USB cable, "Console" on the far device or "Async" on the card's router for an octal cable}"#,
        true
    ),
    c!(
        "link.list",
        "List cables",
        "",
        None,
        r#"{} → [{"a", "b", "cable", "resolved", "up", "problem"}] (console cables: cable "console", resolved "console"; USB cables "usb"; octal cables "octal")"#,
        false
    ),
    c!(
        "link.info",
        "Cable information",
        "",
        None,
        r#"{"device", "port": "interface"} → {"a", "b", "a_device", "a_port", "b_device", "b_port", "cable", "resolved", "kind", "meters", "manual", "up", "problem", "why_down", "transceivers": null | {"a": "multi-mode" | "single-mode", "b"}, "budget": null | {"meters", "connectors", "splices", "loss_db", "budget_db", "margin_db", "state": "ok" | "degraded" | "over"}}: the cable plugged into that port (one of its two ends names it); `why_down` says what stops it carrying anything; a fiber cable's `budget` weighs its length, connectors and splices against the transceivers' power budget (over: the link is down; degraded: under 1 dB to spare)"#,
        false
    ),
    c!(
        "link.splices",
        "Fiber splices",
        "",
        None,
        r#"{"device", "port": "interface", "count": 0-50} → the cable as `link.info` reports it: the splices on a fiber cable, each costing 0.3 dB of its loss budget (a copper cable has none)"#,
        true
    ),
    c!(
        "pc.terminal",
        "Terminal (console cable)",
        "",
        None,
        r#"{"pc": "PC or laptop"} → {"device", "prompt", "log"}: the router or switch its console cable leads to; type into it with cli.exec on that device (the same console its CLI tab shows)"#,
        false
    ),
    c!(
        "octal.terminal",
        "Terminal (octal cable)",
        "",
        None,
        r#"{"router": "router with an octal card", "line": 0-7} → {"router", "line", "device", "prompt", "log"}: the router or switch an octal cable reaches from that line of the card; type into it with cli.exec on that device (reverse-telnet style access from a router's own console is not modelled)"#,
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
    c!(
        "log.list",
        "View Command Log",
        "Options",
        Some("Ctrl+Shift+V"),
        r#"{"since"?: entry number (only newer ones), "limit"?: 1-2000 (default 200, the newest)} → {"enabled", "total", "entries": [{n, command, params, ok, error, sim_us, count}]}: every command run this session, oldest first; passwords and keys in the parameters are hidden, long values cut, and a drag that fires one move after another is a single entry with a count; log.* commands are not recorded"#,
        false
    ),
    c!(
        "log.export",
        "Export Log",
        "Options",
        None,
        r#"{"path"?: file to write} → {"text", "file_name", "entries", "path"?}: the command log as text, one line per command"#,
        false
    ),
    c!("log.clear", "Clear command log", "", None, r#"{} → {"removed"}"#, false),
    c!(
        "log.enable",
        "Enable Logging",
        "Options",
        None,
        r#"{"on": true|false} → {"on"}: whether commands are recorded in the command log (on at the start of every session)"#,
        false
    ),
    c!(
        "profile.get",
        "User Profile",
        "Options",
        Some("Ctrl+Shift+U"),
        r#"{} → {"name", "email", "info"}: the profile kept on this computer (empty: none); never saved in a lab and never sent anywhere"#,
        false
    ),
    c!(
        "profile.set",
        "Set display name",
        "",
        None,
        r#"{"name"?: "up to 64 characters, no control characters; empty clears it", "email"?: "up to 120 characters, no spaces", "info"?: "up to 1000 characters, newlines allowed"} → {"name", "email", "info"}: sets the fields given (at least one). Only the name is used: it names the student in activity results when the activity was not started with a name. None is saved in a lab or sent anywhere"#,
        false
    ),
    c!(
        "physical.length_effects",
        "Enable Cable Length Effects",
        "Options",
        None,
        r#"{"on"?: true|false} → {"on"}: whether a cable longer than its medium reaches (copper 100 m, fiber 2 km, coaxial and phone lines their own limits) carries nothing; on unless the lab says otherwise, saved with the lab; without "on" it only reports"#,
        true
    ),
    c!("sim.advance", "Fast forward time", "Simulation", None, r#"{"seconds"?: 30}"#, false),
    c!("sim.reset", "Reset simulation", "Simulation", None, "{} (forget ARP/MAC tables and events)", false),
    c!("sim.clear_events", "Clear event list", "Simulation", None, "{}", false),
    c!(
        "sim.prune_events",
        "Keep only some events",
        "",
        None,
        r#"{"keep": ["ARP", "ICMP", …] (protocol names as the event list shows them)} → {"removed", "events"}: drops the recorded events of every other protocol"#,
        false
    ),
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
        r#"{"pc": "PC, laptop or server", "method"?: "GET"|"PUT"|"PATCH"|"POST"|"DELETE"|…, "url": "https://10.0.0.1/restconf/data/ietf-interfaces:interfaces", "headers"?: {"Accept": "application/yang-data+json" or "application/yang-data+xml"} or [["Name", "value"]], "body"?: text, JSON or XML text (Content-Type follows it unless given), "user"?, "password"?: Basic login, "trust"?: "R1" or ["R1", …] (https only: the client trusts the CA certificates held in those routers' trustpoints and refuses a server whose certificate chain does not verify against them, at the client's clock), "verify_host"?: true or false (with `trust`: also refuse a server whose certificate is not for the host in the URL: an address must be among the certificate's address entries, a name among its DNS names or its common name)} → {"status", "reason", "headers", "body", "json", "applied": [{"device", "lines", "failed"}], "tls"?: {"version", "cipher", "certificate": null | {"subject", "issuer", "serial", "not_before", "not_after", "key_bits", "chain_length"}, "verified": bool}}: over TCP through the lab, IPv4 or IPv6 ([2001:db8::1] in the URL); https:// runs a TLS 1.2 handshake first: DHE-RSA with the server's certificate when the router has `ip http secure-trustpoint`, otherwise anonymous Diffie-Hellman (AES-128-CBC either way)"#,
        true
    ),
    c!(
        "netconf.session",
        "NETCONF session",
        "",
        None,
        r#"{"pc": "PC, laptop or server", "target": "router address (IPv4 or IPv6) or name", "user", "password", "framing"?: "1.1"|"1.0", "rpcs"?: ["<get-config><source><running/></source></get-config>", …] (bare operations are wrapped in <rpc message-id=…>), "keep_open"?: true (the session stays open: its number comes back as "session"), "session"?: number (go on with a kept session: no target or login), "close"?: true (end a kept session)} → {"connected", "error"?, "framing", "session", "session_id", "hello", "replies": [XML], "applied": [{"device", "lines", "failed"}]}: TCP port 830 with RFC 6242 framing; running and candidate datastores, locks"#,
        true
    ),
    c!(
        "controller.devices",
        "Controller: managed routers",
        "",
        None,
        r#"{"host": "the controller (PC, laptop or server)", "add"?: {"address" (IPv4 or IPv6), "user", "password", "vars"?: {"name": "value"}}, "remove"?: address, "set_vars"?: {"address", "vars": {"name": "value"}}, "refresh"?: true (read each router's hostname, model, software and interfaces over RESTCONF)} → {"devices": [{"address", "user", "vars", "reachable"?, "hostname"?, "model"?, "os"?, "interfaces"?, "up"?, "error"?}]}"#,
        true
    ),
    c!(
        "controller.push",
        "Controller: push a template",
        "",
        None,
        r#"{"host", "path": "/restconf/data/…", "method"?: "PATCH"|"PUT"|"POST"|"DELETE", "body": JSON template with {{name}} variables ({{address}} and {{hostname}} built in), "variables"?: {"router address": {"name": "value"}} (on top of each router's stored vars), "devices"?: [addresses] (all managed routers when left out)} → {"results": [{"address", "status", "error"?, "applied"}]}"#,
        true
    ),
    c!(
        "controller.discover",
        "Controller: discover routers",
        "",
        None,
        r#"{"host", "seed": "first router's address", "user", "password", "max"?: 1-32 (16), "add"?: false (only report)} → {"devices": [inventory, with "neighbors"], "links": [{"from", "from_interface", "to", "to_interface", "platform", "protocols": ["cdp"|"lldp"]}], "unreachable": [{"address", "error"}]}: breadth first over each router's CDP and LLDP neighbours (RESTCONF netlabcraft-discovery:neighbors); routers that answer with the login become managed"#,
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
