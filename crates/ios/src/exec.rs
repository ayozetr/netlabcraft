//! Running one command line on a device.

use std::net::Ipv4Addr;

use packetcraft_core::{DeviceId, Ipv4Net, VlanId, VlanSet, parse_ip};
use packetcraft_model::{
    Device, DeviceKind, DhcpPool, MAX_DESCRIPTION, MAX_DHCP_POOLS, MAX_ROUTES, NextHop, StaticRoute, SwitchportMode, Topology, ifname, secret_hash,
    unobscure,
};
use packetcraft_model::{NatPool, NatRule, NatSide, NatTarget, Ospf, OspfNetwork, RemoteProto, acl};
use packetcraft_sim::{DebugKind, PingOptions, Sim, port_up};

use crate::grammar::{self, Id, Modes, ParseError};
use crate::{host, render};

/// Lines of history kept per session.
const MAX_HISTORY: usize = 256;
/// Interfaces one `interface range` may select.
const MAX_RANGE: usize = 64;

/// One configured interface: a port, or one of its subinterfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IfRef {
    pub port: usize,
    pub sub: Option<u32>,
}

/// The command mode.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Mode {
    /// `R1>`
    #[default]
    User,
    /// `R1#`
    Exec,
    /// `R1(config)#`
    Config,
    /// `R1(config-if)#`, `R1(config-subif)#`, `R1(config-if-range)#`
    Iface(Vec<IfRef>),
    /// `S1(config-vlan)#`
    Vlan(Vec<VlanId>),
    /// `R1(config-line)#`: `true` for the vty lines, `false` for the console.
    Line(bool),
    /// `R1(dhcp-config)#`: the pool's name.
    DhcpPool(String),
    /// `S1(config-if)#` on `interface vlan N`.
    Svi(VlanId),
    /// `R1(config-router)#`
    Ospf,
    /// `R1(config-router)#` under `router rip`.
    Rip,
    /// `R1(config-rtr)#` under `ipv6 router ospf`.
    Ospf6,
    /// `R1(config-ipv6-acl)#`: the list's name.
    Acl6(String),
    /// `R1(config-dhcpv6)#`: the pool's name.
    Dhcp6(String),
    /// `R1(config-std-nacl)#` / `R1(config-ext-nacl)#`: the list's name.
    Acl(String),
}

impl Mode {
    fn bits(&self) -> Modes {
        match self {
            Mode::User => Modes::USER,
            Mode::Exec => Modes::EXEC,
            Mode::Config => Modes::CONFIG,
            Mode::Iface(ifs) if ifs.iter().all(|i| i.sub.is_some()) && !ifs.is_empty() => Modes::SUBIF,
            Mode::Iface(_) => Modes::IF,
            Mode::Vlan(_) => Modes::VLAN,
            Mode::Line(_) => Modes::LINE,
            Mode::DhcpPool(_) => Modes::DHCP,
            Mode::Svi(_) => Modes::SVI,
            Mode::Ospf => Modes::OSPF,
            Mode::Rip => Modes::RIP,
            Mode::Ospf6 => Modes::OSPF6,
            Mode::Acl6(_) => Modes::ACL6,
            Mode::Dhcp6(_) => Modes::DHCP6,
            Mode::Acl(_) => Modes::ACL,
        }
    }
}

/// A device's command-line session.
#[derive(Debug, Clone, Default)]
pub struct Cli {
    pub mode: Mode,
    pub history: Vec<String>,
    /// Passed the console line's password (when it asks for one).
    pub logged_in: bool,
    /// Waiting for the enable password: failed attempts so far.
    pub enable_prompt: Option<u8>,
    /// A remote login (vty) session: `enable` needs a password to exist.
    pub vty: bool,
    /// Waiting for the RSA key size after `crypto key generate rsa`.
    pub modulus_prompt: bool,
    /// A Telnet or SSH session this console opened to another device.
    pub remote: Option<Box<Remote>>,
    /// A host's `ftp` session.
    pub ftp: Option<crate::host::FtpClient>,
}

/// A login to another device.
#[derive(Debug, Clone)]
pub struct Remote {
    pub target: DeviceId,
    pub proto: RemoteProto,
    pub stage: LoginStage,
    pub cli: Cli,
    tries: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginStage {
    Username,
    Password(Option<String>),
    Ready,
}

/// The prompt asking for the RSA key size.
pub const MODULUS_PROMPT: &str = "Key size in bits (360-4096) [1024]: ";

/// Opens a Telnet or SSH session from `dev` to the device at `dst`.
pub fn connect(t: &mut Topology, sim: &mut Sim, dev: DeviceId, cli: &mut Cli, proto: RemoteProto, dst: Ipv4Addr, user: Option<String>) -> String {
    if proto == RemoteProto::Ssh && user.is_none() {
        return "% Give a user name: ssh -l <user> <address>".into();
    }
    if let Err(e) = sim.tcp_connect(t, dev, dst, proto.port()) {
        let e = e.to_string();
        return if e.contains("refused") {
            format!("Trying {dst} ...\n% Connection refused by the remote host")
        } else {
            format!("Trying {dst} ...\n% {e}")
        };
    }
    let Some(target) = t.devices.iter().find(|d| packetcraft_sim::l3_ifaces(d).iter().any(|i| i.net.addr == dst)).map(|d| d.id) else {
        return format!("% {dst} answered but isn't a device in this lab");
    };
    // `login block-for`: the quiet period after too many failed logins.
    if let Some(secs) = sim.login_blocked(target) {
        return format!(
            "Trying {dst} ... Open\n% Logins are blocked for {secs} more seconds (too many failed attempts)\nConnection closed by the remote host"
        );
    }
    let local = t.device(target).is_some_and(|d| d.access.vty.login_local || d.access.aaa);
    let stage = match (local, user) {
        (true, Some(u)) => LoginStage::Password(Some(u)),
        (true, None) => LoginStage::Username,
        (false, _) => LoginStage::Password(None),
    };
    let remote_cli = Cli { vty: true, logged_in: true, ..Cli::default() };
    cli.remote = Some(Box::new(Remote { target, proto, stage, cli: remote_cli, tries: 0 }));
    let banner = t.device(target).map(|d| d.access.banner_motd.clone()).unwrap_or_default();
    let mut out = format!("Trying {dst} ... Open");
    if !banner.is_empty() {
        out.push_str(&format!("\n\n{banner}"));
    }
    if proto == RemoteProto::Telnet {
        out.push_str("\n\nUser access over Telnet (unencrypted)");
    }
    out
}

/// Runs DHCP on every `ip address dhcp` port that is up and has no address yet; returns a line
/// per port about what happened.
fn dhcp_leases(t: &mut Topology, sim: &mut Sim, dev: DeviceId) -> Vec<String> {
    let Some(d) = t.device(dev) else { return Vec::new() };
    let wanted: Vec<usize> = d
        .ports
        .iter()
        .enumerate()
        .filter(|(i, p)| p.dhcp && p.ip.is_none() && !p.shutdown && port_up(t, packetcraft_model::End { device: dev, port: *i }))
        .map(|(i, _)| i)
        .collect();
    let mut out = Vec::new();
    for port in wanted {
        sim.topology_changed(t);
        let r = sim.dhcp_acquire_port(t, dev, port);
        let Some(p) = t.device_mut(dev).and_then(|d| d.ports.get_mut(port)) else { continue };
        match r {
            Ok(lease) => {
                p.ip = Some(lease.address);
                p.dhcp_router = lease.router;
                out.push(format!(
                    "%DHCP: {} assigned to {}{}",
                    lease.address,
                    p.name,
                    lease.router.map(|g| format!(", default route via {g}")).unwrap_or_default()
                ));
            }
            Err(e) => out.push(format!("{}: no address yet ({e}); it asks again when the interface comes up", p.name)),
        }
    }
    sim.topology_changed(t);
    out
}

/// A login checked through the device's AAA methods, in order: a RADIUS server's answer is
/// final; when none answers, the next method is tried.
fn aaa_login(t: &Topology, sim: &mut Sim, target: &packetcraft_model::Device, user: &str, password: &str) -> bool {
    for m in target.access.methods() {
        match m {
            packetcraft_model::LoginMethod::Radius => {
                if let Some(accepted) = sim.radius_login(t, target.id, user, password) {
                    return accepted;
                }
            }
            packetcraft_model::LoginMethod::Tacacs => {
                if let Some(accepted) = sim.tacacs_login(t, target.id, user, password) {
                    return accepted;
                }
            }
            packetcraft_model::LoginMethod::Local => return target.access.users.iter().any(|x| x.name == user && x.matches(password)),
        }
    }
    false
}

/// A line typed while a remote session is open.
fn remote_line(t: &mut Topology, sim: &mut Sim, cli: &mut Cli, line: &str) -> Outcome {
    let Some(r) = cli.remote.as_mut() else { return Outcome::default() };
    let Some(target) = t.device(r.target).cloned() else {
        cli.remote = None;
        return Outcome { output: "% The remote device is gone; connection closed".into(), ..Outcome::default() };
    };
    match r.stage.clone() {
        LoginStage::Username => {
            r.stage = LoginStage::Password(Some(line.trim().to_string()));
            Outcome::default()
        }
        LoginStage::Password(user) => {
            let ok = if target.access.aaa {
                aaa_login(t, sim, &target, user.as_deref().unwrap_or(""), line)
            } else {
                match (&user, target.access.vty.login_local) {
                    (Some(u), true) => target.access.users.iter().any(|x| &x.name == u && x.matches(line)),
                    (_, _) => target.access.vty.password.as_deref() == Some(line),
                }
            };
            if ok {
                r.stage = LoginStage::Ready;
                // A local user with privilege 15 starts in privileged mode.
                let top = user.as_ref().is_some_and(|u| target.access.users.iter().any(|x| &x.name == u && x.privilege >= 15 && x.matches(line)));
                if top {
                    r.cli.mode = Mode::Exec;
                }
                return Outcome::default();
            }
            if let Some(secs) = sim.login_failed(target.id, target.access.login_block) {
                cli.remote = None;
                return Outcome {
                    output: format!("% Login invalid\n% Too many failed logins: logins are blocked for {secs} seconds"),
                    ..Outcome::default()
                };
            }
            r.tries += 1;
            if r.tries >= 3 {
                cli.remote = None;
                return Outcome { output: "% Login failed three times; connection closed".into(), ..Outcome::default() };
            }
            r.stage = if target.access.vty.login_local || target.access.aaa { LoginStage::Username } else { LoginStage::Password(None) };
            Outcome { output: "% Login invalid".into(), ..Outcome::default() }
        }
        LoginStage::Ready => {
            let word = line.split_whitespace().next().unwrap_or("").to_ascii_lowercase();
            let leaving = matches!(r.cli.mode, Mode::User | Mode::Exec)
                && r.cli.enable_prompt.is_none()
                && !word.is_empty()
                && ["exit", "logout", "quit"].iter().any(|w| w.starts_with(&word) && word.len() >= 2);
            if leaving {
                let name = target.name.clone();
                cli.remote = None;
                return Outcome { output: format!("Connection to {name} closed."), ..Outcome::default() };
            }
            let target_id = r.target;
            let mut inner = std::mem::take(&mut r.cli);
            let out = execute(t, sim, target_id, &mut inner, line);
            if let Some(r) = cli.remote.as_mut() {
                r.cli = inner;
            }
            out
        }
    }
}

/// The prompt shown while a password is being asked for.
pub const PASSWORD_PROMPT: &str = "Password: ";

/// Whether the console asks for the line password before anything else.
fn console_login(d: &Device) -> bool {
    d.access.console.login && d.access.console.password.is_some()
}

/// The text shown when a console session starts (the banner, if any).
pub fn greeting(t: &Topology, dev: DeviceId) -> String {
    let Some(d) = t.device(dev) else { return String::new() };
    let mut s = String::new();
    if !d.access.banner_motd.is_empty() {
        s.push_str(&d.access.banner_motd);
        s.push_str("\n\n");
    }
    if console_login(d) {
        s.push_str("Console access is password protected.\n\n");
    }
    s
}

fn enable_protected(d: &Device) -> bool {
    d.access.enable_secret.is_some() || d.access.enable_password.is_some()
}

fn enable_matches(d: &Device, typed: &str) -> bool {
    match (&d.access.enable_secret, &d.access.enable_password) {
        (Some(h), _) => *h == secret_hash(typed),
        (None, Some(p)) => p == typed,
        (None, None) => true,
    }
}

/// What a line did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outcome {
    pub output: String,
    /// The lab changed (configuration or names): worth an undo step and a save.
    pub changed: bool,
    /// The terminal should be cleared (`cls`).
    pub clear: bool,
}

fn device_bits(kind: DeviceKind) -> Modes {
    if kind == DeviceKind::Switch { Modes::SWITCH } else { Modes::ROUTER }
}

/// The prompt for the session's current mode.
pub fn prompt(t: &Topology, dev: DeviceId, cli: &Cli) -> String {
    let Some(d) = t.device(dev) else { return "> ".into() };
    if let Some(f) = &cli.ftp {
        return f.prompt().into();
    }
    if let Some(r) = &cli.remote {
        return match &r.stage {
            LoginStage::Username => "Username: ".into(),
            LoginStage::Password(_) => PASSWORD_PROMPT.into(),
            LoginStage::Ready => prompt(t, r.target, &r.cli),
        };
    }
    if d.kind.is_host() {
        return host::prompt(d);
    }
    if d.kind == DeviceKind::Hub {
        return format!("{}> ", d.name);
    }
    if cli.modulus_prompt {
        return MODULUS_PROMPT.into();
    }
    if cli.enable_prompt.is_some() || (!cli.logged_in && console_login(d)) {
        return PASSWORD_PROMPT.to_string();
    }
    let n = &d.name;
    match &cli.mode {
        Mode::User => format!("{n}>"),
        Mode::Exec => format!("{n}#"),
        Mode::Config => format!("{n}(config)#"),
        Mode::Iface(ifs) if ifs.len() > 1 => format!("{n}(config-if-range)#"),
        Mode::Iface(ifs) if ifs.first().is_some_and(|i| i.sub.is_some()) => format!("{n}(config-subif)#"),
        Mode::Iface(_) => format!("{n}(config-if)#"),
        Mode::Vlan(_) => format!("{n}(config-vlan)#"),
        Mode::Line(_) => format!("{n}(config-line)#"),
        Mode::DhcpPool(_) => format!("{n}(dhcp-config)#"),
        Mode::Svi(_) => format!("{n}(config-if)#"),
        Mode::Ospf | Mode::Rip => format!("{n}(config-router)#"),
        Mode::Ospf6 => format!("{n}(config-rtr)#"),
        Mode::Acl6(_) => format!("{n}(config-ipv6-acl)#"),
        Mode::Dhcp6(_) => format!("{n}(config-dhcpv6)#"),
        Mode::Acl(name) => {
            let ext = d.acls.iter().find(|a| &a.name == name).is_some_and(|a| a.extended);
            format!("{n}(config-{}-nacl)#", if ext { "ext" } else { "std" })
        }
    }
}

/// Runs one line typed at a device.
pub fn execute(t: &mut Topology, sim: &mut Sim, dev: DeviceId, cli: &mut Cli, line: &str) -> Outcome {
    let Some(d) = t.device(dev) else { return Outcome { output: "% That device no longer exists".into(), ..Outcome::default() } };
    let line = line.trim_end_matches(['\r', '\n']);
    if cli.remote.is_some() {
        return remote_line(t, sim, cli, line);
    }
    if cli.modulus_prompt {
        cli.modulus_prompt = false;
        let n: u16 = if line.trim().is_empty() { 1024 } else { line.trim().parse().unwrap_or(0) };
        if !(360..=4096).contains(&n) {
            return Outcome { output: "% The key size is 360-4096 bits; no keys generated".into(), ..Outcome::default() };
        }
        if let Some(dm) = t.device_mut(dev) {
            dm.access.rsa_bits = Some(n);
        }
        sim.topology_changed(t);
        return Outcome { output: format!("RSA keys generated ({n} bits). SSH is available."), changed: true, clear: false };
    }
    if d.kind == DeviceKind::Hub {
        return Outcome { output: "A hub has no console: it repeats every frame and has nothing to configure.".into(), ..Outcome::default() };
    }
    if d.kind.is_host() {
        // Inside `ftp` nothing is remembered: the password is typed there.
        if cli.ftp.is_some() {
            return host::ftp_line(t, sim, dev, cli, line);
        }
        remember(cli, line);
        return host::execute(t, sim, dev, cli, line);
    }
    // Password prompts come first; what is typed there is never echoed or remembered.
    if let Some(tries) = cli.enable_prompt {
        if enable_matches(d, line) {
            cli.enable_prompt = None;
            cli.mode = Mode::Exec;
            return Outcome::default();
        }
        let tries = tries + 1;
        if tries >= 3 {
            cli.enable_prompt = None;
            return Outcome { output: "% Wrong password three times; still in user mode".into(), ..Outcome::default() };
        }
        cli.enable_prompt = Some(tries);
        return Outcome { output: "% Wrong password".into(), ..Outcome::default() };
    }
    if !cli.logged_in && console_login(d) {
        if d.access.console.password.as_deref() == Some(line) {
            cli.logged_in = true;
            cli.mode = Mode::User;
            return Outcome::default();
        }
        return Outcome { output: "% Wrong password".into(), ..Outcome::default() };
    }
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('!') {
        return Outcome::default();
    }
    if let Some(q) = trimmed.strip_suffix('?') {
        return Outcome { output: help_line(t, dev, cli, q), ..Outcome::default() };
    }
    // Changing the console password mid-session applies to the next login, not this one.
    cli.logged_in = true;
    remember(cli, trimmed);
    let before = t.device(dev).cloned();
    // `show … | include …`: the command runs, then its output is filtered.
    let (command, filter) = match crate::pipe::split(trimmed) {
        Some((c, f)) => (c, Some(f)),
        None => (trimmed, None),
    };
    let output = match (dispatch(t, sim, dev, cli, command), filter) {
        (Ok(s), Some(f)) => crate::pipe::apply(&s, f).unwrap_or_else(|e| e),
        (Ok(s), None) => s,
        (Err(e), _) => e,
    };
    // `ip address dhcp` ports ask for their address when they are brought up.
    let mut output = output;
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("no sh") && t.device(dev).is_some_and(|d| d.ports.iter().any(|p| p.dhcp && p.ip.is_none())) {
        for line in dhcp_leases(t, sim, dev) {
            output = if output.is_empty() { line } else { format!("{output}\n{line}") };
        }
    }
    let changed = t.device(dev).cloned() != before;
    if changed {
        // VLAN or VTP changes reach the rest of the domain at once.
        packetcraft_sim::vtp::sync(t);
        sim.topology_changed(t);
    }
    Outcome { output, changed, clear: false }
}

fn remember(cli: &mut Cli, line: &str) {
    let l = line.trim();
    if l.is_empty() || cli.history.last().is_some_and(|h| h == l) {
        return;
    }
    if cli.history.len() >= MAX_HISTORY {
        cli.history.remove(0);
    }
    cli.history.push(l.chars().take(1_024).collect());
}

/// `?` help for a line typed so far.
pub fn help_line(t: &Topology, dev: DeviceId, cli: &Cli, line: &str) -> String {
    let Some(d) = t.device(dev) else { return String::new() };
    if d.kind.is_host() {
        return host::HELP.to_string();
    }
    let ends_in_space = line.is_empty() || line.ends_with(char::is_whitespace);
    let mut words: Vec<&str> = line.split_whitespace().collect();
    let partial = if ends_in_space { "" } else { words.pop().unwrap_or("") };
    let items = grammar::help(&words, partial, cli.mode.bits(), device_bits(d.kind));
    if items.is_empty() {
        return "% No command matches".into();
    }
    let width = items.iter().map(|(w, _)| w.len()).max().unwrap_or(0);
    items.iter().map(|(w, h)| format!("  {w:<width$}  {h}").trim_end().to_string()).collect::<Vec<_>>().join("\n")
}

/// Tab completion: the line with its last word completed, if that word starts exactly one
/// keyword.
pub fn complete_line(t: &Topology, dev: DeviceId, cli: &Cli, line: &str) -> Option<String> {
    let d = t.device(dev)?;
    if d.kind.is_host() || line.ends_with(char::is_whitespace) {
        return None;
    }
    let mut words: Vec<&str> = line.split_whitespace().collect();
    let partial = words.pop()?;
    let kw = grammar::complete(&words, partial, cli.mode.bits(), device_bits(d.kind))?;
    let head = line.get(..line.len() - partial.len()).unwrap_or("");
    Some(format!("{head}{kw} "))
}

/// Replays saved configuration lines (startup-config) in configuration mode. Returns the lines
/// that failed.
pub fn apply_config(t: &mut Topology, sim: &mut Sim, dev: DeviceId, config: &str) -> Vec<String> {
    let mut cli = Cli { mode: Mode::Config, logged_in: true, ..Cli::default() };
    let mut failed = Vec::new();
    for raw in config.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('!') || line.eq_ignore_ascii_case("end") {
            continue;
        }
        if let Err(e) = dispatch(t, sim, dev, &mut cli, line) {
            failed.push(format!("{line}: {e}"));
        }
        if matches!(cli.mode, Mode::User | Mode::Exec) {
            cli.mode = Mode::Config;
        }
    }
    packetcraft_sim::vtp::sync(t);
    sim.topology_changed(t);
    failed
}

fn caret(line: &str, word: usize) -> String {
    let mut col = 0;
    let mut seen = 0;
    let mut in_word = false;
    for (i, c) in line.char_indices() {
        if !c.is_whitespace() && !in_word {
            if seen == word {
                col = line.get(..i).map_or(0, |s| s.chars().count());
                break;
            }
            seen += 1;
        }
        in_word = !c.is_whitespace();
    }
    format!("{}^", " ".repeat(col))
}

type R = Result<String, String>;

fn dispatch(t: &mut Topology, sim: &mut Sim, dev: DeviceId, cli: &mut Cli, line: &str) -> R {
    let kind = t.device(dev).map(|d| d.kind).ok_or("% That device no longer exists")?;
    let words: Vec<&str> = line.split_whitespace().collect();
    let dbits = device_bits(kind);
    let mut resolved = grammar::resolve(&words, cli.mode.bits(), dbits);
    // A sub-mode falls back to global configuration commands, which leave the sub-mode.
    if resolved.is_err()
        && matches!(
            cli.mode,
            Mode::Iface(_)
                | Mode::Vlan(_)
                | Mode::Line(_)
                | Mode::DhcpPool(_)
                | Mode::Svi(_)
                | Mode::Ospf
                | Mode::Ospf6
                | Mode::Rip
                | Mode::Acl(_)
                | Mode::Acl6(_)
                | Mode::Dhcp6(_)
        )
        && let Ok(r) = grammar::resolve(&words, Modes::CONFIG, dbits)
    {
        cli.mode = Mode::Config;
        resolved = Ok(r);
    }
    let (spec, args) = match resolved {
        Ok(r) => r,
        Err(ParseError::Unknown(i)) => return Err(format!("{}\n% Not understood at the marker", caret(line, i))),
        Err(ParseError::Ambiguous(i, opts)) => {
            return Err(format!("% Ambiguous: `{}` could be {}", words.get(i).unwrap_or(&""), opts.join(", ")));
        }
        Err(ParseError::Incomplete) => return Err("% Incomplete command (type ? for what can follow)".into()),
    };
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    let mut c = Ctx { t, sim, dev, cli };
    match spec.id {
        Id::Enable => {
            if c.cli.vty && c.cli.mode == Mode::User && !enable_protected(c.d()?) {
                return Err("% No password set (remote sessions need an enable password or secret)".into());
            }
            if c.cli.mode == Mode::User && enable_protected(c.d()?) {
                c.cli.enable_prompt = Some(0);
                return Ok(String::new());
            }
            c.set_mode(Mode::Exec)
        }
        Id::Disable => c.set_mode(Mode::User),
        Id::Logout => c.logout(),
        Id::Exit => {
            let next = match c.cli.mode {
                Mode::User => return c.logout(),
                Mode::Exec => Mode::User,
                Mode::Config => Mode::Exec,
                Mode::Iface(_)
                | Mode::Vlan(_)
                | Mode::Line(_)
                | Mode::DhcpPool(_)
                | Mode::Svi(_)
                | Mode::Ospf
                | Mode::Ospf6
                | Mode::Rip
                | Mode::Acl(_)
                | Mode::Acl6(_)
                | Mode::Dhcp6(_) => Mode::Config,
            };
            c.set_mode(next)
        }
        Id::End => {
            if !matches!(c.cli.mode, Mode::Exec | Mode::User) {
                let by = if c.cli.vty { "a remote session" } else { "the console" };
                c.sim.log(c.dev, &format!("Configuration changed from {by}"));
            }
            c.set_mode(Mode::Exec)
        }
        Id::ConfigureTerminal => {
            c.cli.mode = Mode::Config;
            Ok("Configuring from the terminal, one command per line; `end` returns to privileged mode.".into())
        }
        Id::Ping => c.ping(arg(0), 5, None),
        Id::PingRepeat => {
            let n: u16 = arg(1).parse().ok().filter(|n| (1..=1000).contains(n)).ok_or("% Repeat count must be 1-1000")?;
            c.ping(arg(0), n, None)
        }
        Id::PingSize => {
            let n: usize = arg(1).parse().ok().filter(|n| (36..=18_024).contains(n)).ok_or("% Datagram size 36-18024")?;
            // The size counts the IP and ICMP headers (28 bytes); the rest is data.
            c.ping(arg(0), 5, Some(n - 28))
        }
        Id::Traceroute => c.traceroute(arg(0)),
        Id::ShowRun => Ok(render::running_config(c.d()?)),
        Id::ShowStartup => c.d()?.startup.clone().ok_or_else(|| "% No saved configuration (use `copy running-config startup-config`)".into()),
        Id::ShowIpIntBrief => Ok(render::ip_int_brief(c.t, c.d()?)),
        Id::ShowIpRoute => {
            let full = render::ip_route(c.t, c.d()?);
            let which = arg(0).to_ascii_lowercase();
            if which.is_empty() {
                return Ok(full);
            }
            // `show ip route A.B.C.D`: the route that address follows.
            if let Ok(a) = parse_ip(&which) {
                let d = c.d()?.clone();
                return Ok(match c.sim.route(c.t, &d, a) {
                    Some((out, nh)) => {
                        let line = full
                            .lines()
                            .filter(|l| l.len() > 5)
                            .filter_map(|l| {
                                let net = l.get(5..)?.split_whitespace().next()?.parse::<Ipv4Net>().ok()?;
                                net.contains(a).then_some((net.prefix, l))
                            })
                            .max_by_key(|x| x.0)
                            .map_or(String::new(), |x| x.1.to_string());
                        let via = if nh == a { "directly connected".to_string() } else { format!("via {nh}") };
                        format!("Routing entry for {a}\n  {line}\n  Out {}, {via}", out.name)
                    }
                    None => format!("% {a} is not in the routing table"),
                });
            }
            // The codes each source shows; the header stays.
            let codes: &[&str] = match () {
                _ if "connected".starts_with(&which) => &["C ", "L "],
                _ if "static".starts_with(&which) => &["S "],
                _ if "ospf".starts_with(&which) => &["O "],
                _ if "rip".starts_with(&which) => &["R "],
                _ => return Err("% connected, static, ospf or rip".into()),
            };
            let (head, body) = full.split_once("\n\n").unwrap_or(("", full.as_str()));
            let lines: Vec<&str> = body.lines().filter(|l| codes.iter().any(|c| l.starts_with(c) || (*c == "O " && l.starts_with("O*")))).collect();
            Ok(format!("{head}\n\n{}", if lines.is_empty() { "(no such routes)".to_string() } else { lines.join("\n") }))
        }
        Id::ShowInterfaces => Ok(render::interfaces(c.t, c.d()?, None)),
        Id::ShowInterface => {
            // `show interfaces vlan 1`
            let lower = arg(0).trim().to_ascii_lowercase();
            if let Some(v) = lower.strip_prefix("vlan").map(str::trim) {
                let v: u16 = v.parse().map_err(|_| "% show interfaces vlan <1-4094>")?;
                return render::svi_interface(c.t, c.d()?, v);
            }
            // `show interfaces fa0/1 switchport`
            if let Some(name) = arg(0).trim().strip_suffix("switchport").filter(|n| !n.trim().is_empty()) {
                let r = c.iface_ref(name.trim())?;
                return render::switchport(c.t, c.d()?, r.port);
            }
            let d = c.d()?;
            let r = c.iface_ref(arg(0))?;
            Ok(render::interfaces(c.t, d, Some(r)))
        }
        Id::ShowIntStatus => Ok(render::interface_status(c.t, c.d()?)),
        Id::ShowIpInterface => {
            let only = match args.first().map(|a| a.trim()).filter(|a| !a.is_empty()) {
                Some(a) => Some(c.iface_ref(a)?),
                None => None,
            };
            Ok(render::ip_interface(c.t, c.d()?, only))
        }
        Id::ShowRunInterface => {
            let r = c.iface_ref(arg(0))?;
            let d = c.d()?;
            let p = d.ports.get(r.port).ok_or("% No such port")?;
            let name = r.sub.map_or(p.name.clone(), |s| format!("{}.{s}", p.name));
            Ok(render::running_config_interface(d, &name))
        }
        Id::ShowLogging => {
            let lines: Vec<String> = c.sim.state(c.dev).map(|s| s.log.iter().map(|(_, l)| l.clone()).collect()).unwrap_or_default();
            Ok(format!(
                "Logging buffer: {} message{} (at most {})\n{}",
                lines.len(),
                if lines.len() == 1 { "" } else { "s" },
                packetcraft_sim::MAX_LOG,
                lines.join("\n")
            )
            .trim_end()
            .to_string())
        }
        Id::ClearLogging => {
            c.sim.clear_log(c.dev);
            Ok(String::new())
        }
        Id::LoggingBuffered => Ok(String::new()),
        Id::DebugIpIcmp => {
            c.sim.set_debug(c.dev, DebugKind::Icmp, true);
            Ok("ICMP debugging is on: echo messages this device receives are reported.".into())
        }
        Id::DebugIpOther | Id::NoDebugIpOther => {
            let on = spec.id == Id::DebugIpOther;
            let w: Vec<String> = line.split_whitespace().map(str::to_ascii_lowercase).collect();
            let topic = w.get(if on { 2 } else { 3 }).map(String::as_str).unwrap_or("");
            let (kind, name) = if "packet".starts_with(topic) {
                (DebugKind::Packet, "IP packet")
            } else if "nat".starts_with(topic) {
                (DebugKind::Nat, "NAT")
            } else {
                (DebugKind::Dhcp, "DHCP server")
            };
            c.sim.set_debug(c.dev, kind, on);
            Ok(if on { format!("{name} debugging is on.") } else { format!("{name} debugging is off.") })
        }
        Id::DebugIpRip | Id::NoDebugIpRip => {
            let on = spec.id == Id::DebugIpRip;
            c.sim.set_debug(c.dev, DebugKind::Rip, on);
            Ok(if on { "RIP debugging is on: the updates this router hears are reported." } else { "RIP debugging is off." }.into())
        }
        Id::DebugIpOspf | Id::NoDebugIpOspf => {
            let what = arg(0).trim();
            if !what.is_empty() && !["adj", "events", "hello"].iter().any(|w| w.starts_with(what)) {
                return Err("% debug ip ospf adj, events or hello".into());
            }
            let on = spec.id == Id::DebugIpOspf;
            c.sim.set_debug(c.dev, DebugKind::Ospf, on);
            Ok(if on { "OSPF debugging is on: the hellos this router hears are reported." } else { "OSPF debugging is off." }.into())
        }
        Id::UndebugAll => {
            for k in DebugKind::ALL {
                c.sim.set_debug(c.dev, k, false);
            }
            Ok("All debugging is off.".into())
        }
        Id::ShowDebugging => {
            let on = c.sim.debugs(c.dev);
            Ok(if on.is_empty() { "No debugging is on.".into() } else { format!("Debugging is on for: {}.", on.join(", ")) })
        }
        Id::ShowClock => {
            let (now, set) = c.sim.clock(c.dev);
            Ok(crate::clock::show(now, set))
        }
        Id::ClockSet => {
            let t = crate::clock::parse_set(arg(0))?;
            c.sim.set_clock(c.dev, t);
            Ok(String::new())
        }
        Id::ShowFlash => Ok(render::flash(c.d()?)),
        Id::ShowUsers => Ok(render::users(c.cli)),
        Id::TerminalLength => {
            let w: Vec<&str> = arg(0).split_whitespace().collect();
            match w.as_slice() {
                ["length" | "width", n] if n.parse::<u16>().is_ok() => Ok(String::new()),
                ["history"] | ["history", "size", _] => Ok(String::new()),
                ["monitor"] | ["no", "monitor"] => Ok(String::new()),
                _ => Err("% terminal length N, terminal width N or terminal monitor".into()),
            }
        }
        Id::CdpRun | Id::NoCdpRun => {
            c.dm()?.cdp_off = spec.id == Id::NoCdpRun;
            Ok(String::new())
        }
        Id::CdpEnable | Id::NoCdpEnable => {
            let off = spec.id == Id::NoCdpEnable;
            c.each_if(|p, sub| {
                if sub.is_some() {
                    return Err("% Neighbour discovery is set on the physical port".into());
                }
                p.cdp_off = off;
                Ok(())
            })
        }
        Id::LldpRun | Id::NoLldpRun => {
            c.dm()?.lldp_on = spec.id == Id::LldpRun;
            Ok(String::new())
        }
        Id::LldpTransmit | Id::NoLldpTransmit | Id::LldpReceive | Id::NoLldpReceive => {
            let off = matches!(spec.id, Id::NoLldpTransmit | Id::NoLldpReceive);
            let tx = matches!(spec.id, Id::LldpTransmit | Id::NoLldpTransmit);
            c.each_if(|p, sub| {
                if sub.is_some() {
                    return Err("% LLDP is set on the physical port".into());
                }
                if tx {
                    p.lldp_tx_off = off
                } else {
                    p.lldp_rx_off = off
                }
                Ok(())
            })
        }
        Id::ShowLldp => Ok(render::lldp(c.d()?)),
        Id::ShowLldpNeighbors | Id::ShowLldpNeighborsDetail => Ok(render::lldp_neighbors(c.t, c.sim, c.d()?, spec.id == Id::ShowLldpNeighborsDetail)),
        Id::ShowProcesses => Ok(render::processes(c.t, c.sim, c.d()?)),
        Id::ShowMemory => Ok(render::memory(c.d()?)),
        Id::ShowCdp => Ok(render::cdp(c.d()?)),
        Id::ShowCdpNeighbors | Id::ShowCdpNeighborsDetail => Ok(render::cdp_neighbors(c.t, c.sim, c.d()?, spec.id == Id::ShowCdpNeighborsDetail)),
        Id::ShowIntTrunk => Ok(render::trunks(c.t, c.d()?)),
        Id::ShowVlan | Id::ShowVlanBrief => Ok(render::vlans(c.t, c.d()?)),
        Id::ShowMac => Ok(render::mac_table(c.t, c.sim, c.d()?)),
        Id::ShowArp => Ok(render::arp(c.t, c.sim, c.d()?)),
        Id::ShowVersion => Ok(render::version(c.sim, c.d()?)),
        Id::ShowHistory => Ok(c.cli.history.iter().map(|h| format!("  {h}")).collect::<Vec<_>>().join("\n")),
        Id::CopyRunStart | Id::WriteMemory => {
            let cfg = render::running_config(c.d()?);
            c.dm()?.startup = Some(cfg);
            Ok("Configuration saved to startup-config.".into())
        }
        Id::EraseStartup => {
            c.dm()?.startup = None;
            Ok("Startup configuration erased; the next reload starts from factory settings.".into())
        }
        Id::Reload => c.reload(),
        Id::ClearMac => {
            c.sim.clear_macs(c.dev);
            Ok(String::new())
        }
        Id::ClearArp => {
            c.sim.clear_arp(c.dev);
            Ok(String::new())
        }
        Id::Hostname => {
            c.t.rename(c.dev, arg(0)).map_err(|e| format!("% {e}"))?;
            Ok(String::new())
        }
        Id::Interface => {
            if let Some(n) = port_channel(arg(0)) {
                let n = n?;
                let members: Vec<IfRef> = c
                    .d()?
                    .ports
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| p.channel.is_some_and(|g| g.number == n))
                    .map(|(port, _)| IfRef { port, sub: None })
                    .collect();
                if members.is_empty() {
                    return Err(format!("% Port-channel{n} has no ports yet: put them in with `channel-group {n} mode ...`"));
                }
                // Settings typed here go to every member, which keeps the bundle consistent.
                c.cli.mode = Mode::Iface(members);
                return Ok(String::new());
            }
            if let Some(v) = vlan_iface(arg(0)) {
                let v = v?;
                if c.d()?.kind != DeviceKind::Switch {
                    return Err("% VLAN interfaces are for switches; routers use subinterfaces (`interface g0/0.10`)".into());
                }
                c.dm()?.svi_mut(v).ok_or("% Too many VLAN interfaces")?;
                c.cli.mode = Mode::Svi(v);
                return Ok(String::new());
            }
            let r = c.iface_ref(arg(0))?;
            if let Some(n) = r.sub {
                c.dm()?.ports.get_mut(r.port).and_then(|p| p.subif_mut(n)).ok_or("% Too many subinterfaces on this port")?;
            }
            c.cli.mode = Mode::Iface(vec![r]);
            Ok(String::new())
        }
        Id::InterfaceRange => {
            let refs = c.iface_range(arg(0))?;
            c.cli.mode = Mode::Iface(refs);
            Ok(String::new())
        }
        Id::NoInterface => {
            let r = c.iface_ref(arg(0))?;
            let n = r.sub.ok_or("% Only subinterfaces can be deleted")?;
            let p = c.dm()?.ports.get_mut(r.port).ok_or("% No such port")?;
            let before = p.subifs.len();
            p.subifs.retain(|s| s.number != n);
            if p.subifs.len() == before { Err("% No such subinterface".into()) } else { Ok(String::new()) }
        }
        Id::IpRoute => c.ip_route(arg(0), arg(1), Some(arg(2)), true),
        Id::NoIpRoute => c.ip_route(arg(0), arg(1), args.get(2).map(String::as_str), false),
        Id::Vlan => {
            let set = VlanSet::parse_list(arg(0)).map_err(|e| format!("% {e}"))?;
            let ids: Vec<VlanId> = set.iter().collect();
            let d = c.dm()?;
            let missing = ids.iter().any(|v| !d.vlans.contains_key(v));
            if missing {
                vtp_may_edit(d, &ids)?;
            }
            let before = d.vlans.clone();
            for v in &ids {
                d.vlans.entry(*v).or_insert_with(|| Device::default_vlan_name(*v));
            }
            vtp_bump(d, &before);
            c.cli.mode = Mode::Vlan(ids);
            Ok(String::new())
        }
        Id::NoVlan => {
            let set = VlanSet::parse_list(arg(0)).map_err(|e| format!("% {e}"))?;
            if set.contains(VlanId::DEFAULT) {
                return Err("% VLAN 1 is the default VLAN and can't be deleted".into());
            }
            let d = c.dm()?;
            let ids: Vec<VlanId> = set.iter().collect();
            vtp_may_edit(d, &ids)?;
            let before = d.vlans.clone();
            for v in set.iter() {
                d.vlans.remove(&v);
            }
            vtp_bump(d, &before);
            Ok(String::new())
        }
        Id::Ipv6UnicastRouting | Id::NoIpv6UnicastRouting => {
            let d = c.dm()?;
            if d.kind == DeviceKind::Switch && !d.catalogue_model().multilayer() {
                return Err(format!("% A {} can't route; use a multilayer switch (Switch-ML-24)", d.catalogue_model().label));
            }
            d.ipv6_routing = spec.id == Id::Ipv6UnicastRouting;
            Ok(String::new())
        }
        Id::Ipv6Route | Id::NoIpv6Route => {
            let net: packetcraft_core::Ipv6Net = arg(0).parse().map_err(|e| format!("% {e}"))?;
            let net = net.network();
            let via = match args.get(1) {
                Some(rest) => Some(c.route6_via(rest)?),
                None => None,
            };
            let d = c.dm()?;
            if spec.id == Id::NoIpv6Route {
                let before = d.routes6.len();
                d.routes6.retain(|r| r.net != net || via.as_ref().is_some_and(|(i, n)| (&r.iface, &r.next_hop) != (i, n)));
                return if d.routes6.len() == before { Err("% No such IPv6 route".into()) } else { Ok(String::new()) };
            }
            let (iface, next_hop) = via.ok_or("% Give a next hop, an interface, or both")?;
            let r = packetcraft_model::StaticRoute6 { net, iface, next_hop };
            if !d.routes6.contains(&r) {
                if d.routes6.len() >= packetcraft_model::MAX_ROUTES {
                    return Err("% Too many routes".into());
                }
                d.routes6.push(r);
                d.routes6.sort();
            }
            Ok(String::new())
        }
        Id::Ipv6Address | Id::Ipv6AddressEui => {
            let net: packetcraft_core::Ipv6Net = arg(0).parse().map_err(|e| format!("% {e}"))?;
            if net.addr.is_multicast() || net.addr.is_unspecified() || net.addr.is_loopback() {
                return Err(format!("% {} can't be an interface address", net.addr));
            }
            if packetcraft_core::Ipv6Net::is_link_local(net.addr) {
                return Err("% For a link-local address use `ipv6 address FE80::X link-local`".into());
            }
            if spec.id == Id::Ipv6AddressEui && net.prefix != 64 {
                return Err("% EUI-64 needs a /64 prefix".into());
            }
            let eui = spec.id == Id::Ipv6AddressEui;
            let mine = c.v6_names();
            let clash = packetcraft_sim::v6::l3_ifaces6(c.d()?)
                .into_iter()
                .find(|i| !mine.contains(&i.name) && i.addresses.iter().any(|a| a.network() == net.network()));
            if let Some(i) = clash {
                return Err(format!("% {} overlaps with {}", net.network(), i.name));
            }
            c.each_v6(|v6, mac, name| {
                let a = if eui { packetcraft_core::Ipv6Net::new(packetcraft_core::eui64(net.addr, mac), 64) } else { net };
                if !v6.addresses.contains(&a) {
                    if v6.addresses.len() >= packetcraft_model::MAX_IPV6_ADDRESSES {
                        return Err(format!("% {name} has {} IPv6 addresses already", packetcraft_model::MAX_IPV6_ADDRESSES));
                    }
                    v6.addresses.push(a);
                }
                Ok(())
            })
        }
        Id::Ipv6AddressLinkLocal => {
            let a: std::net::Ipv6Addr = arg(0).parse().map_err(|_| format!("% `{}` is not an IPv6 address", arg(0)))?;
            if !packetcraft_core::Ipv6Net::is_link_local(a) {
                return Err("% A link-local address is in FE80::/10".into());
            }
            c.each_v6(|v6, _, _| {
                v6.link_local = Some(a);
                Ok(())
            })
        }
        Id::NoIpv6Address => {
            let one = match args.first() {
                Some(a) => {
                    let first = a.split_whitespace().next().unwrap_or("");
                    Some(
                        first
                            .split('/')
                            .next()
                            .unwrap_or("")
                            .parse::<std::net::Ipv6Addr>()
                            .map_err(|_| format!("% `{first}` is not an IPv6 address"))?,
                    )
                }
                None => None,
            };
            c.each_v6(|v6, _, _| {
                match one {
                    Some(a) => {
                        v6.addresses.retain(|n| n.addr != a);
                        if v6.link_local == Some(a) {
                            v6.link_local = None;
                        }
                    }
                    None => {
                        v6.addresses.clear();
                        v6.link_local = None;
                    }
                }
                Ok(())
            })
        }
        Id::Ipv6Enable | Id::NoIpv6Enable => {
            let on = spec.id == Id::Ipv6Enable;
            c.each_v6(|v6, _, _| {
                v6.enabled = on;
                Ok(())
            })
        }
        Id::ShowIpv6IntBrief => Ok(render::ipv6_brief(c.t, c.d()?)),
        Id::ShowIpv6Route => {
            let d = c.d()?.clone();
            let full = render::ipv6_route(c.t, c.sim, &d);
            let which = arg(0).to_ascii_lowercase();
            if which.is_empty() {
                return Ok(full);
            }
            let codes: &[&str] = match () {
                _ if "connected".starts_with(&which) => &["C ", "L "],
                _ if "static".starts_with(&which) => &["S "],
                _ if "ospf".starts_with(&which) => &["O ", "OI ", "OE2"],
                _ => return Err("% connected, static or ospf".into()),
            };
            let mut lines = full.lines();
            let head: Vec<&str> = lines.by_ref().take_while(|l| l.starts_with("Codes") || l.starts_with("IPv6 forwarding")).collect();
            let body: Vec<&str> = full.lines().filter(|l| codes.iter().any(|c| l.starts_with(c))).collect();
            Ok(format!("{}\n{}", head.join("\n"), if body.is_empty() { "(no such routes)".to_string() } else { body.join("\n") }))
        }
        Id::ShowIpv6Neighbors => Ok(render::ipv6_neighbors(c.sim, c.d()?)),
        Id::IpRouting | Id::NoIpRouting => {
            let d = c.dm()?;
            if !d.catalogue_model().multilayer() {
                return Err(format!("% A {} can't route; use a multilayer switch (Switch-ML-24)", d.catalogue_model().label));
            }
            d.ip_routing = spec.id == Id::IpRouting;
            Ok(String::new())
        }
        Id::NoSwitchport | Id::SwitchportOn => {
            let routed = spec.id == Id::NoSwitchport;
            let ml = c.d()?.catalogue_model().multilayer();
            c.each_if(|p, _| {
                if routed && !ml {
                    return Err("% Only a multilayer switch has routed ports".into());
                }
                if routed && p.channel.is_some() {
                    return Err(format!("% {} is in an EtherChannel", p.name));
                }
                p.routed = routed;
                if !routed {
                    p.ip = None;
                }
                Ok(())
            })
        }
        Id::VtpModeCmd => {
            let mode = match arg(0).to_ascii_lowercase().as_str() {
                "server" => packetcraft_model::VtpMode::Server,
                "client" => packetcraft_model::VtpMode::Client,
                "transparent" => packetcraft_model::VtpMode::Transparent,
                _ => return Err("% server, client or transparent".into()),
            };
            let d = c.dm()?;
            if mode != packetcraft_model::VtpMode::Transparent && d.vlans.keys().any(|v| v.get() > packetcraft_model::VTP_MAX_VLAN) {
                return Err("% Delete the VLANs above 1005 first: only a transparent switch can keep them".into());
            }
            // Leaving or entering transparent mode starts the database revision again.
            if (mode == packetcraft_model::VtpMode::Transparent) != (d.vtp.mode == packetcraft_model::VtpMode::Transparent) {
                d.vtp.revision = 0;
            }
            d.vtp.mode = mode;
            Ok(format!("This switch is now a VTP {}.", mode.name()))
        }
        Id::VtpDomain => {
            let name: String = arg(0).chars().take(32).collect();
            let d = c.dm()?;
            if d.vtp.domain != name {
                d.vtp.domain = name.clone();
                d.vtp.revision = 0;
            }
            Ok(format!("VTP domain set to {name}."))
        }
        Id::VtpPassword | Id::NoVtpPassword => {
            let pw = (spec.id == Id::VtpPassword).then(|| arg(0).chars().take(64).collect::<String>());
            c.dm()?.vtp.password = pw;
            Ok(String::new())
        }
        Id::VtpVersion => {
            let v: u8 = arg(0).parse().ok().filter(|v| (1..=2).contains(v)).ok_or("% Version 1 or 2")?;
            c.dm()?.vtp.version = v;
            Ok(String::new())
        }
        Id::ShowVtpStatus => Ok(render::vtp(c.d()?)),
        Id::ShowVtpPassword => Ok(match &c.d()?.vtp.password {
            Some(p) => format!("VTP password: {p}"),
            None => "No VTP password is set.".into(),
        }),
        Id::StandbyIp | Id::StandbyIpGroup0 | Id::StandbyPriority | Id::StandbyPreempt | Id::NoStandby => {
            let group: u8 = if spec.id == Id::StandbyIpGroup0 { 0 } else { arg(0).parse().map_err(|_| "% The group is 0-255")? };
            let ip = match spec.id {
                Id::StandbyIp => Some(parse_ip(arg(1)).map_err(|e| format!("% {e}"))?),
                Id::StandbyIpGroup0 => Some(parse_ip(arg(0)).map_err(|e| format!("% {e}"))?),
                _ => None,
            };
            let priority: Option<u8> =
                if spec.id == Id::StandbyPriority { Some(arg(1).parse().map_err(|_| "% The priority is 0-255")?) } else { None };
            let id = spec.id;
            let apply = |list: &mut Vec<packetcraft_model::Standby>, net: Option<Ipv4Net>| -> Result<(), String> {
                if id == Id::NoStandby {
                    list.retain(|s| s.group != group);
                    return Ok(());
                }
                if let (Some(v), Some(n)) = (ip, net)
                    && !n.contains(v)
                {
                    return Err(format!("% {v} is not on {}", n.network()));
                }
                if !list.iter().any(|s| s.group == group) {
                    if list.len() >= packetcraft_model::MAX_STANDBY {
                        return Err("% Too many HSRP groups on this interface".into());
                    }
                    list.push(packetcraft_model::Standby::new(group));
                }
                if let Some(s) = list.iter_mut().find(|s| s.group == group) {
                    if ip.is_some() {
                        s.ip = ip;
                    }
                    if let Some(p) = priority {
                        s.priority = p;
                    }
                    if id == Id::StandbyPreempt {
                        s.preempt = true;
                    }
                }
                Ok(())
            };
            if let Mode::Svi(v) = c.cli.mode {
                let svi = c.dm()?.svi_mut(v).ok_or("% Too many VLAN interfaces")?;
                let net = svi.ip;
                apply(&mut svi.standby, net)?;
                return Ok(String::new());
            }
            c.each_if(|p, sub| match sub {
                Some(s) => {
                    let net = s.ip;
                    apply(&mut s.standby, net)
                }
                None => {
                    let net = p.ip;
                    apply(&mut p.standby, net)
                }
            })
        }
        Id::PppAuthentication | Id::NoPppAuthentication | Id::PppPapSent => {
            let methods = if spec.id == Id::PppAuthentication {
                let mut list = Vec::new();
                for w in arg(0).split_whitespace() {
                    let m = match w.to_ascii_lowercase().as_str() {
                        "chap" => packetcraft_model::PppAuth::Chap,
                        "pap" => packetcraft_model::PppAuth::Pap,
                        _ => return Err(format!("% `{w}`: chap or pap")),
                    };
                    if !list.contains(&m) {
                        list.push(m);
                    }
                }
                if list.is_empty() {
                    return Err("% chap, pap, or both in the order to try".into());
                }
                list
            } else {
                Vec::new()
            };
            let pap = (spec.id == Id::PppPapSent).then(|| (arg(0).chars().take(64).collect::<String>(), arg(1).chars().take(64).collect::<String>()));
            let id = spec.id;
            c.each_if(|p, _| {
                if !packetcraft_model::ifname::is_serial(&p.name) {
                    return Err(format!("% {} is not a serial interface", p.name));
                }
                if p.serial.encapsulation != packetcraft_model::Encapsulation::Ppp {
                    return Err("% Set `encapsulation ppp` first".into());
                }
                match id {
                    Id::PppPapSent => p.serial.pap_sent = pap.clone(),
                    _ => p.serial.ppp_auth = methods.clone(),
                }
                Ok(())
            })
        }
        Id::IpOspfCost | Id::NoIpOspfCost | Id::IpOspfPriority | Id::NoIpOspfPriority => {
            let value: Option<u32> = match spec.id {
                Id::IpOspfCost => Some(arg(0).parse::<u32>().ok().filter(|c| (1..=65_535).contains(c)).ok_or("% The cost is 1-65535")?),
                Id::IpOspfPriority => Some(u32::from(arg(0).parse::<u8>().map_err(|_| "% The priority is 0-255")?)),
                _ => None,
            };
            let names: Vec<String> = match &c.cli.mode {
                Mode::Svi(v) => vec![format!("Vlan{v}")],
                Mode::Iface(ifs) => {
                    let d = c.d()?;
                    ifs.iter().filter_map(|r| d.ports.get(r.port).map(|p| r.sub.map_or(p.name.clone(), |s| format!("{}.{s}", p.name)))).collect()
                }
                _ => return Err("% Not in interface mode".into()),
            };
            let id = spec.id;
            let d = c.dm()?;
            for n in names {
                let e = d.ospf_if.entry(n.clone()).or_default();
                match id {
                    Id::IpOspfCost | Id::NoIpOspfCost => e.cost = value,
                    _ => e.priority = value.and_then(|v| u8::try_from(v).ok()),
                }
                if *e == packetcraft_model::OspfIf::default() {
                    d.ospf_if.remove(&n);
                }
            }
            Ok(String::new())
        }
        Id::AutoCost => {
            let mbps: u32 = arg(0).parse().ok().filter(|v| (1..=4_294_967).contains(v)).ok_or("% 1-4294967 Mbit/s")?;
            c.dm()?.ospf.as_mut().ok_or("% Not in OSPF mode")?.reference_bw = Some(mbps);
            Ok("Make the reference bandwidth the same on every router.".into())
        }
        Id::AreaStub | Id::NoAreaStub => {
            let area: u32 = arg(0).parse().map_err(|_| "% Area number 1-4294967295")?;
            if area == 0 {
                return Err("% The backbone (area 0) can't be a stub area".into());
            }
            let no_summary = words.last().is_some_and(|w| w.starts_with("no-") && "no-summary".starts_with(*w));
            let o = c.dm()?.ospf.as_mut().ok_or("% Not in OSPF mode")?;
            let a = o.areas.entry(area).or_default();
            match (spec.id == Id::AreaStub, no_summary) {
                (true, ns) => {
                    a.stub = true;
                    a.no_summary = ns;
                }
                // `no area N stub no-summary` keeps the stub area, with summaries.
                (false, true) => a.no_summary = false,
                (false, false) => {
                    a.stub = false;
                    a.no_summary = false;
                }
            }
            if a.is_default() {
                o.areas.remove(&area);
            }
            Ok(String::new())
        }
        Id::AreaRange | Id::NoAreaRange => {
            let area: u32 = arg(0).parse().map_err(|_| "% Area number 0-4294967295")?;
            let ip = |s: &str| parse_ip(s).map_err(|e| format!("% {e}"));
            let (addr, mask) = (ip(arg(1))?, ip(arg(2))?);
            let net = Ipv4Net::with_mask(addr, mask).map_err(|e| format!("% {e}"))?.network();
            let o = c.dm()?.ospf.as_mut().ok_or("% Not in OSPF mode")?;
            let a = o.areas.entry(area).or_default();
            a.ranges.retain(|r| *r != net);
            if spec.id == Id::AreaRange {
                if a.ranges.len() >= packetcraft_model::MAX_AREA_RANGES {
                    return Err("% Too many ranges in this area".into());
                }
                a.ranges.push(net);
            }
            if a.is_default() {
                o.areas.remove(&area);
            }
            Ok(String::new())
        }
        Id::ShowIpOspf => {
            let st = c.sim.ospf_state(c.t);
            Ok(render::ospf_summary(c.d()?, &st))
        }
        Id::ShowIpOspfInterface => {
            let st = c.sim.ospf_state(c.t);
            Ok(render::ospf_interfaces(c.d()?, &st))
        }
        Id::NtpServer | Id::NoNtpServer => {
            let a = parse_ip(arg(0)).map_err(|e| format!("% {e}"))?;
            let add = spec.id == Id::NtpServer;
            let d = c.dm()?;
            d.ntp_servers.retain(|x| *x != a);
            if add {
                if d.ntp_servers.len() >= 4 {
                    return Err("% At most 4 NTP servers".into());
                }
                d.ntp_servers.push(a);
            }
            if !add {
                return Ok(String::new());
            }
            // Ask at once, as a router does when the server is configured.
            Ok(match c.sim.ntp_sync(c.t, c.dev) {
                Some(s) => format!("Clock synchronised with {s}."),
                None => format!("{a} didn't answer yet; the clock will follow when it does."),
            })
        }
        Id::ShowNtp => {
            let d = c.d()?;
            if d.ntp_servers.is_empty() {
                return Ok("No NTP server is configured; the clock runs on its own.".into());
            }
            let synced = c.sim.state(c.dev).and_then(|s| s.ntp_synced);
            let (now, _) = c.sim.clock(c.dev);
            let mut s = match synced {
                Some(x) => format!("Clock synchronised with {x}\n"),
                None => "Clock not synchronised\n".to_string(),
            };
            for a in &d.ntp_servers {
                s.push_str(&format!("  server {a}{}\n", if synced == Some(*a) { "  (in use)" } else { "" }));
            }
            s.push_str(&crate::clock::show(now, true));
            Ok(s)
        }
        Id::LoggingHost | Id::NoLoggingHost => {
            let a = parse_ip(arg(0)).map_err(|e| format!("% {e}"))?;
            let d = c.dm()?;
            d.log_hosts.retain(|x| *x != a);
            if spec.id == Id::LoggingHost {
                if d.log_hosts.len() >= 4 {
                    return Err("% At most 4 syslog servers".into());
                }
                d.log_hosts.push(a);
            }
            Ok(String::new())
        }
        Id::CopyToTftp | Id::CopyFromTftp => c.tftp_copy(spec.id == Id::CopyToTftp, line, arg(0)),
        Id::CopyToFtp | Id::CopyFromFtp => c.ftp_copy(spec.id == Id::CopyToFtp, line, arg(0)),
        Id::IpFtpUsername | Id::IpFtpPassword => {
            let v: String = arg(0).chars().take(64).collect();
            let d = c.dm()?;
            let (mut u, mut p) = d.ftp_login.clone().unwrap_or_default();
            if spec.id == Id::IpFtpUsername {
                u = v;
            } else {
                p = v;
            }
            d.ftp_login = Some((u, p));
            Ok(String::new())
        }
        Id::NoIpFtpLogin => {
            c.dm()?.ftp_login = None;
            Ok(String::new())
        }
        Id::StpMode => {
            let rapid = match arg(0).to_ascii_lowercase().as_str() {
                "pvst" => false,
                "rapid-pvst" => true,
                _ => return Err("% pvst or rapid-pvst".into()),
            };
            c.dm()?.rapid_stp = rapid;
            Ok(String::new())
        }
        Id::ShowStandby => {
            let roles = c.sim.hsrp_roles(c.t);
            Ok(render::standby(c.d()?, roles.get(&c.dev).map_or(&[][..], Vec::as_slice)))
        }
        Id::Do => {
            let saved = std::mem::replace(&mut c.cli.mode, Mode::Exec);
            let r = dispatch(c.t, c.sim, c.dev, c.cli, arg(0));
            c.cli.mode = saved;
            r
        }
        Id::IpAddress | Id::NoIpAddress | Id::Shutdown | Id::NoShutdown | Id::Description | Id::NoDescription
            if matches!(c.cli.mode, Mode::Svi(_)) =>
        {
            let Mode::Svi(v) = c.cli.mode else { return Err("% Not in a VLAN interface".into()) };
            let ip = if spec.id == Id::IpAddress {
                let net = Ipv4Net::with_mask(parse_ip(arg(0)).map_err(|e| format!("% {e}"))?, parse_ip(arg(1)).map_err(|e| format!("% {e}"))?)
                    .map_err(|e| format!("% {e}"))?;
                if net.is_network_or_broadcast() {
                    return Err(format!("% {} is the network or broadcast address of {}", net.addr, net.network()));
                }
                Some(net)
            } else {
                None
            };
            let text: String = arg(0).chars().take(MAX_DESCRIPTION).collect();
            let svi = c.dm()?.svi_mut(v).ok_or("% Too many VLAN interfaces")?;
            match spec.id {
                Id::IpAddress => svi.ip = ip,
                Id::NoIpAddress => svi.ip = None,
                Id::Shutdown => svi.shutdown = true,
                Id::NoShutdown => svi.shutdown = false,
                Id::Description => svi.description = text,
                _ => svi.description.clear(),
            }
            Ok(match spec.id {
                Id::NoShutdown => format!("Vlan{v} is now enabled"),
                Id::Shutdown => format!("Vlan{v} is now administratively down"),
                _ => String::new(),
            })
        }
        Id::AccessList => {
            let name = arg(0).to_string();
            let extended = acl::numbered_kind(&name).ok_or("% Numbered lists are 1-99 (standard) or 100-199 (extended)")?;
            let words: Vec<&str> = arg(1).split_whitespace().collect();
            if words.first().is_some_and(|w| w.eq_ignore_ascii_case("remark")) {
                return Ok(String::new());
            }
            let entry = parse_acl_entry(&words, extended)?;
            c.acl_add(&name, extended, entry)
        }
        Id::NoAccessList => {
            let d = c.dm()?;
            let before = d.acls.len();
            d.acls.retain(|a| a.name != arg(0));
            if d.acls.len() == before { Err(format!("% No access list {}", arg(0))) } else { Ok(String::new()) }
        }
        Id::IpAccessListStandard | Id::IpAccessListExtended => {
            let name: String = arg(0).chars().take(32).collect();
            let extended = spec.id == Id::IpAccessListExtended;
            let d = c.dm()?;
            match d.acls.iter().find(|a| a.name == name) {
                Some(a) if a.extended != extended => {
                    return Err(format!("% {name} is a{} list", if a.extended { "n extended" } else { " standard" }));
                }
                Some(_) => {}
                None => {
                    if d.acls.len() >= acl::MAX_ACLS {
                        return Err(format!("% At most {} access lists", acl::MAX_ACLS));
                    }
                    d.acls.push(acl::Acl { name: name.clone(), extended, entries: Vec::new() });
                }
            }
            c.cli.mode = Mode::Acl(name);
            Ok(String::new())
        }
        Id::NoIpAccessList => {
            let d = c.dm()?;
            let before = d.acls.len();
            d.acls.retain(|a| a.name != arg(1));
            if d.acls.len() == before { Err(format!("% No access list {}", arg(1))) } else { Ok(String::new()) }
        }
        Id::AclEntryLine | Id::NoAclEntryLine => {
            let Mode::Acl(name) = c.cli.mode.clone() else { return Err("% Not in access-list mode".into()) };
            let extended = c.d()?.acls.iter().find(|a| a.name == name).is_some_and(|a| a.extended);
            let words: Vec<&str> = line.split_whitespace().skip(usize::from(spec.id == Id::NoAclEntryLine)).collect();
            let entry = parse_acl_entry(&words, extended)?;
            if spec.id == Id::AclEntryLine {
                c.acl_add(&name, extended, entry)
            } else {
                let list = c.dm()?.acls.iter_mut().find(|a| a.name == name).ok_or("% The list was deleted")?;
                let before = list.entries.len();
                list.entries.retain(|e| *e != entry);
                if list.entries.len() == before { Err("% No such line".into()) } else { Ok(String::new()) }
            }
        }
        Id::AccessGroup | Id::NoAccessGroup => {
            let inbound = match arg(1).to_ascii_lowercase().as_str() {
                "in" => true,
                "out" => false,
                _ => return Err("% `in` or `out`".into()),
            };
            let name = arg(0).to_string();
            let add = spec.id == Id::AccessGroup;
            c.each_if(|p, sub| {
                let g = match sub {
                    Some(s) => &mut s.access,
                    None => &mut p.access,
                };
                let slot = if inbound { &mut g.inbound } else { &mut g.outbound };
                if add {
                    *slot = Some(name.clone());
                } else if slot.as_deref() == Some(name.as_str()) {
                    *slot = None;
                }
                Ok(())
            })?;
            let exists = c.d()?.acls.iter().any(|a| a.name == name);
            Ok(if add && !exists { format!("Access list {name} doesn't exist yet: until it does, everything passes.") } else { String::new() })
        }
        Id::ShowAccessLists => Ok(render::access_lists(c.sim, c.d()?)),
        Id::ClearAclCounters => {
            c.sim.clear_acl_hits(c.dev);
            Ok(String::new())
        }
        Id::Username => {
            let name: String = arg(0).chars().take(32).collect();
            let words: Vec<&str> = arg(1).split_whitespace().collect();
            // `username NAME privilege 15 secret …`: the level comes first.
            let (privilege, words) = match words.as_slice() {
                [p, n, rest @ ..] if p.eq_ignore_ascii_case("privilege") => {
                    let n: u8 = n.parse().ok().filter(|n| *n <= 15).ok_or("% Privilege level 0-15")?;
                    (n, rest.to_vec())
                }
                _ => (1, words),
            };
            let (kind, rest) = match words.split_first() {
                Some((k, rest)) => (k.to_ascii_lowercase(), rest.join(" ")),
                None => return Err("% username <name> [privilege N] secret <password>".into()),
            };
            let user = match kind.as_str() {
                "secret" => {
                    let hash = match password_words(&rest) {
                        (Some("5"), Some(h)) if h.starts_with("$pc$") => h.to_string(),
                        (_, Some(p)) => secret_hash(p),
                        _ => return Err("% Give a password".into()),
                    };
                    if let (None | Some("0"), Some(p)) = password_words(&rest) {
                        c.check_length(p)?;
                    }
                    packetcraft_model::User { name: name.clone(), credential: hash, hashed: true, privilege }
                }
                "password" => {
                    let p = line_password(&rest)?;
                    c.check_length(&p)?;
                    packetcraft_model::User { name: name.clone(), credential: p, hashed: false, privilege }
                }
                _ => return Err("% username <name> secret|password <password>".into()),
            };
            let d = c.dm()?;
            d.access.users.retain(|u| u.name != name);
            if d.access.users.len() >= 64 {
                return Err("% At most 64 users".into());
            }
            d.access.users.push(user);
            Ok(String::new())
        }
        Id::NoUsername => {
            let d = c.dm()?;
            let before = d.access.users.len();
            d.access.users.retain(|u| u.name != arg(0));
            if d.access.users.len() == before { Err(format!("% No user {}", arg(0))) } else { Ok(String::new()) }
        }
        Id::DomainName | Id::NoDomainName => {
            let v = (spec.id == Id::DomainName).then(|| arg(0).chars().take(64).collect::<String>());
            c.dm()?.access.domain_name = v;
            Ok(String::new())
        }
        Id::CryptoKey => {
            if c.d()?.access.domain_name.is_none() {
                return Err("% Set `ip domain-name` first: the keys are named after it".into());
            }
            match args.first().and_then(|a| a.split_whitespace().last()).map(|n| n.parse::<u16>()) {
                None => {
                    c.cli.modulus_prompt = true;
                    Ok(format!("The key name will be {}.{}", c.d()?.name, c.d()?.access.domain_name.clone().unwrap_or_default()))
                }
                Some(Ok(n)) if (360..=4096).contains(&n) => {
                    c.dm()?.access.rsa_bits = Some(n);
                    Ok(format!("RSA keys generated ({n} bits). SSH is available."))
                }
                Some(_) => Err("% The key size is 360-4096 bits".into()),
            }
        }
        Id::CryptoKeyZeroize => {
            c.dm()?.access.rsa_bits = None;
            Ok("RSA keys deleted; SSH is off.".into())
        }
        Id::SshVersion => {
            let v: u8 = arg(0).parse().ok().filter(|v| *v == 1 || *v == 2).ok_or("% The version is 1 or 2")?;
            c.dm()?.access.ssh_version = Some(v);
            Ok(String::new())
        }
        Id::LoginLocal => {
            let Mode::Line(vty) = c.cli.mode else { return Err("% Not in line mode".into()) };
            let d = c.dm()?;
            let line = if vty { &mut d.access.vty } else { &mut d.access.console };
            line.login_local = true;
            line.login = false;
            Ok(if d.access.users.is_empty() { "No local users yet: add one with `username <name> secret <password>`.".into() } else { String::new() })
        }
        Id::AaaNewModel | Id::NoAaaNewModel => {
            c.dm()?.access.aaa = spec.id == Id::AaaNewModel;
            Ok(String::new())
        }
        Id::AaaLogin => {
            let mut methods = Vec::new();
            let words: Vec<String> = arg(0).split_whitespace().map(str::to_ascii_lowercase).collect();
            let mut i = 0;
            while let Some(w) = words.get(i) {
                match w.as_str() {
                    "group" => match words.get(i + 1).map(String::as_str) {
                        Some("radius") => {
                            methods.push(packetcraft_model::LoginMethod::Radius);
                            i += 2;
                        }
                        Some("tacacs+") => {
                            methods.push(packetcraft_model::LoginMethod::Tacacs);
                            i += 2;
                        }
                        _ => return Err("% group radius or group tacacs+".into()),
                    },
                    "local" => {
                        methods.push(packetcraft_model::LoginMethod::Local);
                        i += 1;
                    }
                    other => return Err(format!("% `{other}`: group radius, group tacacs+ or local")),
                }
            }
            if methods.is_empty() {
                return Err("% Give the methods: group radius, local".into());
            }
            methods.dedup();
            let d = c.dm()?;
            if !d.access.aaa {
                return Err("% Turn AAA on first: aaa new-model".into());
            }
            d.access.login_methods = methods;
            Ok(String::new())
        }
        Id::NoAaaLogin => {
            c.dm()?.access.login_methods.clear();
            Ok(String::new())
        }
        Id::TacacsHost | Id::NoTacacsHost => {
            let a = parse_ip(arg(0)).map_err(|e| format!("% {e}"))?;
            let key: String = arg(1).chars().take(63).collect();
            let d = c.dm()?;
            d.access.tacacs_servers.retain(|(x, _)| *x != a);
            if spec.id == Id::TacacsHost {
                if d.access.tacacs_servers.len() >= packetcraft_model::MAX_RADIUS_SERVERS {
                    return Err("% Too many TACACS+ servers".into());
                }
                d.access.tacacs_servers.push((a, key));
            }
            Ok(String::new())
        }
        Id::RadiusHost | Id::NoRadiusHost => {
            let a = parse_ip(arg(0)).map_err(|e| format!("% {e}"))?;
            let key: String = arg(1).chars().take(63).collect();
            let d = c.dm()?;
            d.access.radius_servers.retain(|(x, _)| *x != a);
            if spec.id == Id::RadiusHost {
                if d.access.radius_servers.len() >= packetcraft_model::MAX_RADIUS_SERVERS {
                    return Err("% Too many RADIUS servers".into());
                }
                d.access.radius_servers.push((a, key));
            }
            Ok(String::new())
        }
        Id::LoginAuthentication => {
            if !c.d()?.access.aaa {
                return Err("% Turn AAA on first: aaa new-model".into());
            }
            Ok(String::new())
        }
        Id::Ipv6DhcpPool => {
            let name: String = arg(0).chars().take(32).collect();
            let d = c.dm()?;
            if !d.dhcp6_pools.iter().any(|p| p.name == name) {
                if d.dhcp6_pools.len() >= MAX_DHCP_POOLS {
                    return Err("% Too many DHCPv6 pools".into());
                }
                d.dhcp6_pools.push(packetcraft_model::Dhcp6Pool { name: name.clone(), ..Default::default() });
            }
            c.cli.mode = Mode::Dhcp6(name);
            Ok(String::new())
        }
        Id::NoIpv6DhcpPool => {
            let d = c.dm()?;
            let before = d.dhcp6_pools.len();
            d.dhcp6_pools.retain(|p| p.name != arg(0));
            if d.dhcp6_pools.len() == before { Err(format!("% No DHCPv6 pool {}", arg(0))) } else { Ok(String::new()) }
        }
        Id::Dhcp6AddressPrefix | Id::Dhcp6DnsServer | Id::Dhcp6DomainName => {
            let Mode::Dhcp6(name) = c.cli.mode.clone() else { return Err("% Not in DHCPv6 pool mode".into()) };
            let id = spec.id;
            let value = arg(0).to_string();
            let pool = c.dm()?.dhcp6_pools.iter_mut().find(|p| p.name == name).ok_or("% The pool was deleted")?;
            match id {
                Id::Dhcp6AddressPrefix => {
                    let n: packetcraft_core::Ipv6Net = value.parse().map_err(|e| format!("% {e}"))?;
                    if n.prefix != 64 {
                        return Err("% Pools hand out from a /64".into());
                    }
                    pool.prefix = Some(n.network());
                }
                Id::Dhcp6DnsServer => {
                    let a: std::net::Ipv6Addr = value.parse().map_err(|_| format!("% `{value}` is not an IPv6 address"))?;
                    if !pool.dns.contains(&a) && pool.dns.len() < 4 {
                        pool.dns.push(a);
                    }
                }
                _ => pool.domain = Some(value.chars().take(64).collect()),
            }
            Ok(String::new())
        }
        Id::Ipv6DhcpServer | Id::NoIpv6DhcpServer | Id::NdManaged | Id::NoNdManaged | Id::NdOther | Id::NoNdOther => {
            let id = spec.id;
            let pool = arg(0).to_string();
            c.each_v6(|v6, _, _| {
                match id {
                    Id::Ipv6DhcpServer => v6.dhcp_server = Some(pool.clone()),
                    Id::NoIpv6DhcpServer => v6.dhcp_server = None,
                    Id::NdManaged | Id::NoNdManaged => v6.nd_managed = id == Id::NdManaged,
                    _ => v6.nd_other = id == Id::NdOther,
                }
                Ok(())
            })
        }
        Id::ShowIpv6DhcpPool => Ok(render::dhcp6_pools(c.sim, c.d()?)),
        Id::ShowIpv6DhcpBinding => Ok(render::dhcp6_bindings(c.sim, c.d()?)),
        Id::Ipv6AccessList => {
            let name: String = arg(0).chars().take(32).collect();
            let d = c.dm()?;
            if !d.acls6.iter().any(|a| a.name == name) {
                if d.acls6.len() >= acl::MAX_ACLS {
                    return Err(format!("% At most {} IPv6 access lists", acl::MAX_ACLS));
                }
                d.acls6.push(acl::Acl6 { name: name.clone(), entries: Vec::new() });
            }
            c.cli.mode = Mode::Acl6(name);
            Ok(String::new())
        }
        Id::NoIpv6AccessList => {
            let d = c.dm()?;
            let before = d.acls6.len();
            d.acls6.retain(|a| a.name != arg(0));
            if d.acls6.len() == before { Err(format!("% No IPv6 access list {}", arg(0))) } else { Ok(String::new()) }
        }
        Id::Acl6EntryLine | Id::NoAcl6EntryLine => {
            let Mode::Acl6(name) = c.cli.mode.clone() else { return Err("% Not in IPv6 access-list mode".into()) };
            let words: Vec<&str> = line.split_whitespace().skip(usize::from(spec.id == Id::NoAcl6EntryLine)).collect();
            let entry = acl::Acl6Entry::parse(&words).map_err(|e| format!("% {e}"))?;
            let list = c.dm()?.acls6.iter_mut().find(|a| a.name == name).ok_or("% The list was deleted")?;
            if spec.id == Id::Acl6EntryLine {
                if list.entries.len() >= acl::MAX_ENTRIES {
                    return Err("% The list is full".into());
                }
                if !list.entries.contains(&entry) {
                    list.entries.push(entry);
                }
                Ok(String::new())
            } else {
                let before = list.entries.len();
                list.entries.retain(|e| *e != entry);
                if list.entries.len() == before { Err("% No such line".into()) } else { Ok(String::new()) }
            }
        }
        Id::TrafficFilter | Id::NoTrafficFilter => {
            let inbound = match arg(1).to_ascii_lowercase().as_str() {
                "in" => true,
                "out" => false,
                _ => return Err("% `in` or `out`".into()),
            };
            let name = arg(0).to_string();
            let add = spec.id == Id::TrafficFilter;
            c.each_v6(|v6, _, _| {
                let slot = if inbound { &mut v6.filter_in } else { &mut v6.filter_out };
                if add {
                    *slot = Some(name.clone());
                } else if slot.as_deref() == Some(name.as_str()) {
                    *slot = None;
                }
                Ok(())
            })?;
            let exists = c.d()?.acls6.iter().any(|a| a.name == name);
            Ok(if add && !exists { format!("IPv6 access list {name} doesn't exist yet: until it does, everything passes.") } else { String::new() })
        }
        Id::Ipv6DhcpRelay | Id::NoIpv6DhcpRelay => {
            let a: std::net::Ipv6Addr = arg(0).parse().map_err(|_| format!("% `{}` is not an IPv6 address", arg(0)))?;
            if a.is_multicast() || a.is_unspecified() || packetcraft_core::Ipv6Net::is_link_local(a) {
                return Err("% The server's global address".into());
            }
            let add = spec.id == Id::Ipv6DhcpRelay;
            c.each_v6(|v6, _, name| {
                v6.dhcp_relay.retain(|x| *x != a);
                if add {
                    if v6.dhcp_relay.len() >= 8 {
                        return Err(format!("% {name} relays to 8 servers already"));
                    }
                    v6.dhcp_relay.push(a);
                }
                Ok(())
            })
        }
        Id::ShowIpv6AccessList => Ok(render::ipv6_access_lists(c.sim, c.d()?)),
        Id::AccessClass | Id::NoAccessClass => {
            let Mode::Line(vty) = c.cli.mode else { return Err("% Not in line mode".into()) };
            if !vty {
                return Err("% access-class filters remote logins: use it under `line vty 0 4`".into());
            }
            let name = (spec.id == Id::AccessClass).then(|| arg(0).to_string());
            c.dm()?.access.vty.access_class = name;
            Ok(String::new())
        }
        Id::TransportInput => {
            let Mode::Line(vty) = c.cli.mode else { return Err("% Not in line mode".into()) };
            let mut list = Vec::new();
            for w in arg(0).split_whitespace().map(str::to_ascii_lowercase) {
                match w.as_str() {
                    "ssh" => list.push(RemoteProto::Ssh),
                    "telnet" => list.push(RemoteProto::Telnet),
                    "all" => list = vec![RemoteProto::Telnet, RemoteProto::Ssh],
                    "none" => list.clear(),
                    other => return Err(format!("% `{other}`: ssh, telnet, all or none")),
                }
            }
            list.dedup();
            let d = c.dm()?;
            let line = if vty { &mut d.access.vty } else { &mut d.access.console };
            line.transport = Some(list);
            Ok(String::new())
        }
        Id::ChannelGroup | Id::NoChannelGroup => {
            let group = if spec.id == Id::ChannelGroup {
                let n = arg(0).parse::<u16>().ok().filter(|n| (1..=packetcraft_model::MAX_CHANNEL_GROUP).contains(n)).ok_or("% The group is 1-48")?;
                let mode = packetcraft_model::ChannelMode::parse(arg(1)).ok_or("% on, active, passive, desirable or auto")?;
                Some(packetcraft_model::ChannelGroup { number: n, mode })
            } else {
                None
            };
            // Every port of a group speaks the same protocol.
            if let Some(g) = group {
                let mine: Vec<usize> = match &c.cli.mode {
                    Mode::Iface(ifs) => ifs.iter().map(|r| r.port).collect(),
                    _ => Vec::new(),
                };
                let clash = c.d()?.ports.iter().enumerate().find_map(|(i, p)| {
                    let o = p.channel.filter(|o| o.number == g.number && !mine.contains(&i))?;
                    (o.mode.protocol() != g.mode.protocol()).then(|| (p.name.clone(), o.mode))
                });
                if let Some((name, m)) = clash {
                    return Err(format!("% Port-channel{} already uses {} ({name} is `mode {}`)", g.number, m.protocol(), m.name()));
                }
            }
            c.each_if(|p, sub| {
                if sub.is_some() {
                    return Err("% EtherChannel is for physical switch ports".into());
                }
                if group.is_some() && p.port_security.is_some() {
                    return Err(format!("% {} has port security: a channel member can't", p.name));
                }
                p.channel = group;
                Ok(())
            })
        }
        Id::ShowEtherchannel => Ok(render::etherchannel(c.t, c.d()?)),
        Id::PortSecurity | Id::NoPortSecurity | Id::PortSecMax | Id::PortSecViolation | Id::PortSecSticky | Id::PortSecMac => {
            let max = if spec.id == Id::PortSecMax {
                Some(arg(0).parse::<u16>().ok().filter(|n| (1..=132).contains(n)).ok_or("% The maximum is 1-132")?)
            } else {
                None
            };
            let violation = if spec.id == Id::PortSecViolation {
                Some(match arg(0).to_ascii_lowercase().as_str() {
                    "shutdown" => packetcraft_model::Violation::Shutdown,
                    "restrict" => packetcraft_model::Violation::Restrict,
                    "protect" => packetcraft_model::Violation::Protect,
                    _ => return Err("% shutdown, restrict or protect".into()),
                })
            } else {
                None
            };
            let mac = if spec.id == Id::PortSecMac { Some(arg(0).parse::<packetcraft_core::MacAddr>().map_err(|e| format!("% {e}"))?) } else { None };
            let id = spec.id;
            c.each_if(|p, _| {
                if id == Id::NoPortSecurity {
                    p.port_security = None;
                    p.err_disabled = false;
                    return Ok(());
                }
                if p.switchport.mode != SwitchportMode::Access {
                    return Err(format!("% {} is {}: port security needs `switchport mode access`", p.name, p.switchport.mode.name()));
                }
                if p.channel.is_some() {
                    return Err(format!("% {} is in an EtherChannel: port security can't be used there", p.name));
                }
                let ps = p.port_security.get_or_insert_with(packetcraft_model::PortSecurity::default);
                if let Some(m) = max {
                    ps.max = m;
                }
                if let Some(v) = violation {
                    ps.violation = v;
                }
                if id == Id::PortSecSticky {
                    ps.sticky = true;
                }
                if let Some(m) = mac
                    && !ps.macs.contains(&m)
                {
                    ps.macs.push(m);
                }
                Ok(())
            })
        }
        Id::ShowPortSecurity => Ok(render::port_security(c.sim, c.d()?)),
        Id::Telnet | Id::Ssh => {
            let (proto, user, addr) = if spec.id == Id::Telnet {
                (RemoteProto::Telnet, None, arg(0).to_string())
            } else {
                let w: Vec<&str> = arg(0).split_whitespace().collect();
                match w.as_slice() {
                    ["-l", u, a] | [a, "-l", u] => (RemoteProto::Ssh, Some(u.to_string()), a.to_string()),
                    [ua] if ua.contains('@') => {
                        let (u, a) = ua.split_once('@').unwrap_or(("", ua));
                        (RemoteProto::Ssh, Some(u.to_string()), a.to_string())
                    }
                    _ => return Err("% ssh -l <user> <address>".into()),
                }
            };
            let dst = parse_ip(&addr).map_err(|e| format!("% {e}"))?;
            Ok(connect(c.t, c.sim, c.dev, c.cli, proto, dst, user))
        }
        Id::ShowIpSsh => {
            let a = &c.d()?.access;
            Ok(match a.rsa_bits {
                Some(b) => format!(
                    "SSH enabled - version {}\nKey size {b} bits\nDomain {}\nLogin time-out {} s, {} password tries",
                    a.ssh_version.unwrap_or(2),
                    a.domain_name.clone().unwrap_or_default(),
                    a.ssh_timeout.unwrap_or(120),
                    a.ssh_retries.unwrap_or(3)
                ),
                None => "SSH disabled: generate keys with `crypto key generate rsa` (after `ip domain-name`).".into(),
            })
        }
        Id::SshTimeout => {
            let n: u32 = arg(0).parse().ok().filter(|n| (1..=120).contains(n)).ok_or("% 1-120 seconds")?;
            c.dm()?.access.ssh_timeout = Some(n);
            Ok(String::new())
        }
        Id::SshRetries => {
            let n: u8 = arg(0).parse().ok().filter(|n| *n <= 5).ok_or("% 0-5 tries")?;
            c.dm()?.access.ssh_retries = Some(n);
            Ok(String::new())
        }
        Id::ShowSsh => Ok(render::ssh_sessions(c.cli)),
        Id::SecurityMinLength | Id::NoSecurityMinLength => {
            let n = if spec.id == Id::SecurityMinLength {
                Some(arg(0).parse::<u8>().ok().filter(|n| *n <= 16).ok_or("% 0-16 characters")?)
            } else {
                None
            };
            c.dm()?.access.min_password_len = n.filter(|n| *n > 0);
            Ok(String::new())
        }
        Id::LoginBlockFor | Id::NoLoginBlockFor => {
            let block = if spec.id == Id::LoginBlockFor {
                let n = |i: usize| arg(i).parse::<u32>().ok().filter(|n| (1..=65_535).contains(n)).ok_or("% 1-65535");
                Some((n(0)?, n(1)?, n(2)?))
            } else {
                None
            };
            c.dm()?.access.login_block = block;
            Ok(String::new())
        }
        Id::IfSpeed => {
            let words: Vec<String> = line.split_whitespace().map(str::to_ascii_lowercase).collect();
            let value = if words.first().is_some_and(|w| w == "no") { "auto".to_string() } else { arg(0).to_ascii_lowercase() };
            let speed = match value.as_str() {
                "auto" => None,
                v => Some(v.parse::<u32>().ok().filter(|s| [10, 100, 1000].contains(s)).ok_or("% 10, 100, 1000 or auto")?),
            };
            c.each_if(|p, _| {
                let max = packetcraft_sim::port_max_speed(&p.name).ok_or(format!("% {} has no Ethernet speed to set", p.name))?;
                if speed.is_some_and(|s| s > max) {
                    return Err(format!("% {} goes up to {max} Mbit/s", p.name));
                }
                p.speed = speed;
                Ok(())
            })
        }
        Id::IfDuplex => {
            let words: Vec<String> = line.split_whitespace().map(str::to_ascii_lowercase).collect();
            let value = if words.first().is_some_and(|w| w == "no") { "auto".to_string() } else { arg(0).to_ascii_lowercase() };
            let duplex = match value.as_str() {
                "auto" => None,
                "full" => Some(packetcraft_model::Duplex::Full),
                "half" => Some(packetcraft_model::Duplex::Half),
                _ => return Err("% full, half or auto".into()),
            };
            c.each_if(|p, _| {
                if packetcraft_sim::port_max_speed(&p.name).is_none() {
                    return Err(format!("% {} has no duplex to set", p.name));
                }
                p.duplex = duplex;
                Ok(())
            })
        }
        Id::IfMdix | Id::NoIfMdix => {
            let off = spec.id == Id::NoIfMdix;
            c.each_if(|p, _| {
                p.mdix_off = off;
                Ok(())
            })
        }
        Id::ShowProtocols => Ok(render::protocols(c.t, c.d()?)),
        Id::ShowIntDescription => Ok(render::int_descriptions(c.t, c.d()?)),
        Id::ShowLogin => Ok(render::login_state(c.d()?, c.sim.login_blocked(c.dev))),
        Id::ShowPrivilege => Ok(format!("Current privilege level is {}", if c.cli.mode == Mode::User { 1 } else { 15 })),
        Id::ShowTechSupport => {
            let d = c.d()?;
            Ok(format!(
                "------------------ show version ------------------\n{}\n\n------------------ show running-config ------------------\n{}\n\n------------------ show ip interface brief ------------------\n{}\n\n------------------ show interfaces ------------------\n{}",
                render::version(c.sim, d),
                render::running_config(d),
                render::ip_int_brief(c.t, d),
                render::interfaces(c.t, d, None)
            ))
        }
        Id::ShowBoot => {
            let d = c.d()?;
            Ok(format!(
                "Boot image: flash:packetcraft-{}-{}.bin\nConfiguration at start-up: {}",
                d.kind.label().to_lowercase(),
                env!("CARGO_PKG_VERSION"),
                if d.startup.is_some() { "startup-config (saved)" } else { "none saved: factory settings" }
            ))
        }
        Id::ShowEnv => Ok("Power supply: OK (simulated)\nFans: OK\nTemperature: normal".into()),
        Id::ShowInventory => {
            let d = c.d()?;
            let m = d.catalogue_model();
            Ok(format!("NAME: \"{}\", DESCR: \"{}\"\nPID: {}, SN: PC{:08X}", d.name, m.description, m.label, d.id.0))
        }
        Id::DeleteVlanDat => {
            let file = arg(0).trim().to_ascii_lowercase();
            if !matches!(file.as_str(), "vlan.dat" | "flash:vlan.dat" | "flash:/vlan.dat") {
                return Err("% Only flash:vlan.dat (the VLAN database) can be deleted here".into());
            }
            c.dm()?.vlan_dat_deleted = true;
            Ok("vlan.dat deleted: the VLANs go back to factory at the next reload.".into())
        }
        Id::ShowIpv6Interface => {
            let r = c.iface_ref(arg(0))?;
            render::ipv6_interface(c.t, c.d()?, r)
        }
        Id::ClearCounters => Ok("Interface counters cleared.".into()),
        Id::ShowSessions => Ok(match &c.cli.remote {
            Some(r) => format!(
                "Conn Host                Protocol\n*  1 {:<19} {}",
                c.t.device(r.target).map_or("?".to_string(), |d| d.name.clone()),
                r.proto.name()
            ),
            None => "No open sessions.".into(),
        }),
        Id::ShowHosts => {
            let a = &c.d()?.access;
            Ok(format!(
                "Default domain: {}\nName lookup: {}",
                a.domain_name.clone().unwrap_or_else(|| "not set".into()),
                if a.no_domain_lookup { "off (no ip domain-lookup)" } else { "on (no name servers set)" }
            ))
        }
        Id::NatSideCmd | Id::NoNatSide => {
            let side = match arg(0).to_ascii_lowercase().as_str() {
                "inside" => NatSide::Inside,
                "outside" => NatSide::Outside,
                _ => return Err("% `inside` or `outside`".into()),
            };
            let set = spec.id == Id::NatSideCmd;
            if let Mode::Svi(v) = c.cli.mode {
                let svi = c.dm()?.svi_mut(v).ok_or("% Too many VLAN interfaces")?;
                if set {
                    svi.nat = Some(side);
                } else if svi.nat == Some(side) {
                    svi.nat = None;
                }
                return Ok(String::new());
            }
            c.each_if(|p, sub| {
                let slot = match sub {
                    Some(s) => &mut s.nat,
                    None => &mut p.nat,
                };
                if set {
                    *slot = Some(side);
                } else if *slot == Some(side) {
                    *slot = None;
                }
                Ok(())
            })
        }
        Id::NatSource | Id::NoNatSource => {
            let words: Vec<&str> = arg(0).split_whitespace().collect();
            let add = spec.id == Id::NatSource;
            let lower: Vec<String> = words.iter().map(|w| w.to_ascii_lowercase()).collect();
            match lower.first().map(String::as_str) {
                Some("static") => {
                    let (Some(l), Some(g)) = (words.get(1), words.get(2)) else { return Err("% static <inside local> <inside global>".into()) };
                    let pair = (parse_ip(l).map_err(|e| format!("% {e}"))?, parse_ip(g).map_err(|e| format!("% {e}"))?);
                    let n = &mut c.dm()?.nat;
                    n.statics.retain(|s| *s != pair);
                    if add {
                        n.statics.push(pair);
                    }
                    Ok(String::new())
                }
                Some("list") => {
                    let acl = words.get(1).ok_or("% list <access list> ...")?.to_string();
                    let overload = lower.last().is_some_and(|w| w == "overload");
                    let end = if overload { words.len() - 1 } else { words.len() };
                    let target = match lower.get(2).map(String::as_str) {
                        Some("interface") => {
                            let name = words.get(3..end).map(|w| w.join(" ")).unwrap_or_default();
                            let r = c.iface_ref(&name)?;
                            let p = c.d()?.ports.get(r.port).ok_or("% No such port")?;
                            NatTarget::Interface(r.sub.map_or(p.name.clone(), |s| format!("{}.{s}", p.name)))
                        }
                        Some("pool") => NatTarget::Pool(words.get(3).ok_or("% pool <name>")?.to_string()),
                        _ => return Err("% list <acl> interface <name> overload | list <acl> pool <name> [overload]".into()),
                    };
                    let rule = NatRule { acl, target, overload };
                    let n = &mut c.dm()?.nat;
                    n.rules.retain(|r| r.acl != rule.acl);
                    if add {
                        n.rules.push(rule);
                    }
                    Ok(String::new())
                }
                _ => Err("% `static` or `list`".into()),
            }
        }
        Id::NatPool => {
            let w: Vec<&str> = arg(1).split_whitespace().collect();
            let (Some(a), Some(b)) = (w.first(), w.get(1)) else { return Err("% <start> <end> netmask <mask>".into()) };
            let (start, end) = (parse_ip(a).map_err(|e| format!("% {e}"))?, parse_ip(b).map_err(|e| format!("% {e}"))?);
            if u32::from(start) > u32::from(end) {
                return Err("% The start address must not be above the end".into());
            }
            let prefix = match (w.get(2).map(|s| s.to_ascii_lowercase()).as_deref(), w.get(3)) {
                (Some("netmask"), Some(m)) => {
                    packetcraft_core::mask_to_prefix(parse_ip(m).map_err(|e| format!("% {e}"))?).ok_or("% Not a contiguous mask")?
                }
                (Some("prefix-length"), Some(n)) => n.parse::<u8>().ok().filter(|n| *n <= 32).ok_or("% prefix-length is 0-32")?,
                _ => return Err("% Give `netmask <mask>` or `prefix-length <n>`".into()),
            };
            let name = arg(0).to_string();
            let n = &mut c.dm()?.nat;
            n.pools.retain(|p| p.name != name);
            n.pools.push(NatPool { name, start, end, prefix });
            Ok(String::new())
        }
        Id::NoNatPool => {
            let n = &mut c.dm()?.nat;
            let before = n.pools.len();
            n.pools.retain(|p| p.name != arg(0));
            if n.pools.len() == before { Err(format!("% No pool {}", arg(0))) } else { Ok(String::new()) }
        }
        Id::ShowNat => Ok(render::nat_translations(c.sim, c.d()?)),
        Id::ClearNat => {
            c.sim.clear_nat(c.dev);
            Ok(String::new())
        }
        Id::EncapSerial => {
            let enc = match arg(0).to_ascii_lowercase().as_str() {
                "hdlc" => packetcraft_model::Encapsulation::Hdlc,
                "ppp" => packetcraft_model::Encapsulation::Ppp,
                "dot1q" => return Err("% dot1Q is for subinterfaces: `interface g0/0.10` first".into()),
                _ => return Err("% hdlc or ppp".into()),
            };
            c.each_if(|p, _| {
                if !packetcraft_model::ifname::is_serial(&p.name) {
                    return Err(format!("% {} is not a serial interface", p.name));
                }
                p.serial.encapsulation = enc;
                // PPP's own settings go with it.
                if enc == packetcraft_model::Encapsulation::Hdlc {
                    p.serial.ppp_auth.clear();
                    p.serial.pap_sent = None;
                }
                Ok(())
            })
        }
        Id::ClockRate | Id::NoClockRate => {
            let rate = if spec.id == Id::ClockRate {
                let r: u32 = arg(0).parse().map_err(|_| "% The clock rate is a number of bit/s")?;
                if !packetcraft_model::CLOCK_RATES.contains(&r) {
                    let list: Vec<String> = packetcraft_model::CLOCK_RATES.iter().map(u32::to_string).collect();
                    return Err(format!("% Unsupported clock rate; use one of {}", list.join(", ")));
                }
                Some(r)
            } else {
                None
            };
            c.each_if(|p, _| {
                if !packetcraft_model::ifname::is_serial(&p.name) {
                    return Err(format!("% {} is not a serial interface", p.name));
                }
                p.serial.clock_rate = rate;
                Ok(())
            })?;
            // On the DTE end the setting is kept but has no effect; say so.
            let dte = match &c.cli.mode {
                Mode::Iface(ifs) => ifs.iter().any(|r| {
                    let e = packetcraft_model::End { device: c.dev, port: r.port };
                    c.t.dce_of(e).is_some_and(|dce| dce != e)
                }),
                _ => false,
            };
            Ok(if rate.is_some() && dte { "This end of the cable is DTE: the clock comes from the other (DCE) end.".into() } else { String::new() })
        }
        Id::ShowControllers => {
            let r = c.iface_ref(arg(0))?;
            let d = c.d()?;
            let p = d.ports.get(r.port).ok_or("% No such port")?;
            let e = packetcraft_model::End { device: d.id, port: r.port };
            Ok(if !packetcraft_model::ifname::is_serial(&p.name) {
                render::ethernet_controller(c.t, d, r.port)
            } else {
                match c.t.dce_of(e) {
                    None => format!("{}: no cable attached", p.name),
                    Some(dce) if dce == e => format!(
                        "{}: DCE end of a serial cable, clock rate {}",
                        p.name,
                        p.serial.clock_rate.map_or("not set (the link stays down)".to_string(), |r| r.to_string())
                    ),
                    Some(_) => format!("{}: DTE end of a serial cable (clocked by the other end)", p.name),
                }
            })
        }
        Id::RouterRip if c.d()?.kind == DeviceKind::Switch && !c.d()?.ip_routing => Err("% RIP needs `ip routing` (multilayer switches)".into()),
        Id::RouterRip => {
            let d = c.dm()?;
            if d.rip.is_none() {
                d.rip = Some(packetcraft_model::Rip::default());
            }
            c.cli.mode = Mode::Rip;
            Ok(String::new())
        }
        Id::NoRouterRip => {
            let d = c.dm()?;
            if d.rip.take().is_none() {
                return Err("% RIP is not running".into());
            }
            Ok(String::new())
        }
        Id::RipVersion => {
            let v: u8 = arg(0).parse().ok().filter(|v| (1..=2).contains(v)).ok_or("% Version 1 or 2")?;
            c.dm()?.rip.as_mut().ok_or("% Not in RIP mode")?.version = v;
            Ok(String::new())
        }
        Id::RipNetwork | Id::NoRipNetwork => {
            let a = parse_ip(arg(0)).map_err(|e| format!("% {e}"))?;
            let net = packetcraft_model::Rip::classful(a).addr;
            let r = c.dm()?.rip.as_mut().ok_or("% Not in RIP mode")?;
            if spec.id == Id::RipNetwork {
                if !r.networks.contains(&net) {
                    if r.networks.len() >= 256 {
                        return Err("% At most 256 network statements".into());
                    }
                    r.networks.push(net);
                    r.networks.sort();
                }
                // RIP takes the classful network, whatever was typed.
                Ok(if net == a { String::new() } else { format!("Taken as network {net} (RIP networks are classful).") })
            } else {
                let before = r.networks.len();
                r.networks.retain(|n| *n != net);
                if r.networks.len() == before { Err("% No such network statement".into()) } else { Ok(String::new()) }
            }
        }
        Id::RipPassive | Id::NoRipPassive => {
            let r = c.iface_ref(arg(0))?;
            let d = c.d()?;
            let port = d.ports.get(r.port).ok_or("% No such port")?;
            let name = r.sub.map_or(port.name.clone(), |s| format!("{}.{s}", port.name));
            let rip = c.dm()?.rip.as_mut().ok_or("% Not in RIP mode")?;
            rip.passive.retain(|p| *p != name);
            if spec.id == Id::RipPassive {
                rip.passive.push(name);
            }
            Ok(String::new())
        }
        Id::RipDefaultOriginate | Id::NoRipDefaultOriginate => {
            c.dm()?.rip.as_mut().ok_or("% Not in RIP mode")?.default_originate = spec.id == Id::RipDefaultOriginate;
            Ok(String::new())
        }
        Id::RipAutoSummary | Id::NoRipAutoSummary => {
            c.dm()?.rip.as_mut().ok_or("% Not in RIP mode")?.auto_summary = spec.id == Id::RipAutoSummary;
            Ok(String::new())
        }
        Id::ShowIpRipDatabase => {
            let st = c.sim.rip_state(c.t);
            Ok(render::rip_database(c.t, c.d()?, &st))
        }
        Id::RouterOspf if c.d()?.kind == DeviceKind::Switch && !c.d()?.ip_routing => Err("% OSPF needs `ip routing` (multilayer switches)".into()),
        Id::RouterOspf => {
            let n: u16 = arg(0).parse().ok().filter(|n| *n >= 1).ok_or("% The process id is 1-65535")?;
            let d = c.dm()?;
            match &mut d.ospf {
                Some(o) if o.process != n => {
                    return Err(format!("% OSPF process {} already runs here; PacketCraft runs one process per router", o.process));
                }
                Some(_) => {}
                None => d.ospf = Some(Ospf { process: n, ..Ospf::default() }),
            }
            c.cli.mode = Mode::Ospf;
            Ok(String::new())
        }
        Id::NoRouterOspf => {
            let d = c.dm()?;
            if d.ospf.as_ref().is_some_and(|o| o.process.to_string() == arg(0)) {
                d.ospf = None;
                Ok(String::new())
            } else {
                Err(format!("% No OSPF process {}", arg(0)))
            }
        }
        Id::OspfNetwork | Id::NoOspfNetwork => {
            let address = parse_ip(arg(0)).map_err(|e| format!("% {e}"))?;
            let wildcard = parse_ip(arg(1)).map_err(|e| format!("% {e}"))?;
            let area: u32 = arg(2).parse().map_err(|_| "% The area is a number")?;
            if packetcraft_core::mask_to_prefix(Ipv4Addr::from(!u32::from(wildcard))).is_none() {
                return Err(format!("% {wildcard} is not a wildcard mask (the inverse of a subnet mask, such as 0.0.0.255)"));
            }
            let o = c.dm()?.ospf.as_mut().ok_or("% Not in OSPF mode")?;
            let n = OspfNetwork { address, wildcard, area };
            if spec.id == Id::OspfNetwork {
                if !o.networks.contains(&n) {
                    if o.networks.len() >= 256 {
                        return Err("% At most 256 network statements".into());
                    }
                    o.networks.push(n);
                }
                Ok(String::new())
            } else {
                let before = o.networks.len();
                o.networks.retain(|x| *x != n);
                if o.networks.len() == before { Err("% No such network statement".into()) } else { Ok(String::new()) }
            }
        }
        Id::OspfRouterId | Id::NoOspfRouterId => {
            let id = if spec.id == Id::OspfRouterId { Some(parse_ip(arg(0)).map_err(|e| format!("% {e}"))?) } else { None };
            if c.cli.mode == Mode::Ospf6 {
                c.dm()?.ospf6.as_mut().ok_or("% Not in OSPFv3 mode")?.router_id = id;
            } else {
                c.dm()?.ospf.as_mut().ok_or("% Not in OSPF mode")?.router_id = id;
            }
            Ok(String::new())
        }
        Id::OspfPassive | Id::NoOspfPassive => {
            let r = c.iface_ref(arg(0))?;
            let d = c.d()?;
            let port = d.ports.get(r.port).ok_or("% No such port")?;
            let name = r.sub.map_or(port.name.clone(), |s| format!("{}.{s}", port.name));
            let add = spec.id == Id::OspfPassive;
            let six = c.cli.mode == Mode::Ospf6;
            let d = c.dm()?;
            let list = if six {
                &mut d.ospf6.as_mut().ok_or("% Not in OSPFv3 mode")?.passive
            } else {
                &mut d.ospf.as_mut().ok_or("% Not in OSPF mode")?.passive
            };
            list.retain(|p| *p != name);
            if add {
                list.push(name);
            }
            Ok(String::new())
        }
        Id::OspfDefaultOriginate | Id::NoOspfDefaultOriginate => {
            let on = spec.id == Id::OspfDefaultOriginate;
            if c.cli.mode == Mode::Ospf6 {
                c.dm()?.ospf6.as_mut().ok_or("% Not in OSPFv3 mode")?.default_originate = on;
            } else {
                c.dm()?.ospf.as_mut().ok_or("% Not in OSPF mode")?.default_originate = on;
            }
            Ok(String::new())
        }
        Id::Ipv6RouterOspf => {
            let n: u16 = arg(0).parse().ok().filter(|n| *n >= 1).ok_or("% The process id is 1-65535")?;
            let d = c.dm()?;
            match &d.ospf6 {
                Some(o) if o.process != n => {
                    return Err(format!("% OSPFv3 process {} already runs here; PacketCraft runs one process per router", o.process));
                }
                Some(_) => {}
                None => d.ospf6 = Some(packetcraft_model::Ospf6 { process: n, ..Default::default() }),
            }
            let note = if packetcraft_sim::ospf6::router_id(d).is_none() {
                "OSPFv3 needs a router ID: give one with `router-id A.B.C.D` (there's no IPv4 address to take it from)."
            } else {
                ""
            };
            c.cli.mode = Mode::Ospf6;
            Ok(note.into())
        }
        Id::NoIpv6RouterOspf => {
            let d = c.dm()?;
            if d.ospf6.as_ref().is_none_or(|o| o.process.to_string() != arg(0)) {
                return Err(format!("% No OSPFv3 process {}", arg(0)));
            }
            d.ospf6 = None;
            Ok(String::new())
        }
        Id::Ipv6OspfArea | Id::NoIpv6OspfArea => {
            let process: u16 = arg(0).parse().ok().filter(|n| *n >= 1).ok_or("% The process id is 1-65535")?;
            let area: u32 = arg(1).parse().map_err(|_| "% Area number 0-4294967295")?;
            let add = spec.id == Id::Ipv6OspfArea;
            c.each_v6(|v6, _, _| {
                if add {
                    v6.ospf = Some((process, area));
                } else if v6.ospf == Some((process, area)) {
                    v6.ospf = None;
                }
                Ok(())
            })
        }
        Id::ShowIpv6OspfNeighbor => {
            let st = c.sim.ospf6_state(c.t);
            Ok(render::ospf6_neighbors(c.t, c.d()?, &st))
        }
        Id::ShowIpv6Ospf => {
            let st = c.sim.ospf6_state(c.t);
            Ok(render::ospf6_summary(c.d()?, &st))
        }
        Id::ShowOspfNeighbor => {
            let st = c.sim.ospf_state(c.t);
            Ok(render::ospf_neighbors(c.t, c.d()?, &st))
        }
        Id::ShowIpProtocols => {
            let st = c.sim.ospf_state(c.t);
            let rs = c.sim.rip_state(c.t);
            Ok(render::ip_protocols(c.d()?, &st, &rs))
        }
        Id::HelperAddress | Id::NoHelperAddress => {
            let target =
                if args.first().is_some_and(|a| !a.trim().is_empty()) { Some(parse_ip(arg(0).trim()).map_err(|e| format!("% {e}"))?) } else { None };
            let add = spec.id == Id::HelperAddress;
            c.each_if(|p, sub| {
                let list = match sub {
                    Some(s) => &mut s.helpers,
                    None => &mut p.helpers,
                };
                match (add, target) {
                    (true, Some(a)) if !list.contains(&a) && list.len() < 8 => list.push(a),
                    (true, _) => {}
                    (false, Some(a)) => list.retain(|x| *x != a),
                    (false, None) => list.clear(),
                }
                Ok(())
            })
        }
        Id::NoInterfaceVlan => {
            let v: VlanId = arg(0).parse().map_err(|e| format!("% {e}"))?;
            let d = c.dm()?;
            let before = d.svis.len();
            d.svis.retain(|s| s.vlan != v);
            if d.svis.len() == before { Err(format!("% There is no interface vlan {v}")) } else { Ok(String::new()) }
        }
        Id::DefaultGateway => {
            let g = parse_ip(arg(0)).map_err(|e| format!("% {e}"))?;
            c.dm()?.gateway = Some(g);
            Ok(String::new())
        }
        Id::NoDefaultGateway => {
            c.dm()?.gateway = None;
            Ok(String::new())
        }
        Id::IpAddress => c.ip_address(arg(0), arg(1)),
        Id::NoIpAddress => c.each_if(|p, sub| {
            match sub {
                Some(s) => s.ip = None,
                None => {
                    p.ip = None;
                    p.dhcp = false;
                    p.dhcp_router = None;
                }
            }
            Ok(())
        }),
        Id::IpAddressDhcp => {
            let switch = c.d()?.kind == DeviceKind::Switch;
            c.each_if(|p, sub| {
                if sub.is_some() {
                    return Err("% `ip address dhcp` goes on the physical interface here".into());
                }
                if switch && !p.routed {
                    return Err(format!("% {} is a switch port: `no switchport` first", p.name));
                }
                p.dhcp = true;
                p.ip = None;
                p.dhcp_router = None;
                Ok(())
            })?;
            Ok(dhcp_leases(c.t, c.sim, c.dev).join("\n"))
        }
        Id::Shutdown | Id::NoShutdown => {
            let down = spec.id == Id::Shutdown;
            c.each_if(|p, sub| {
                match sub {
                    Some(s) => s.shutdown = down,
                    None => {
                        p.shutdown = down;
                        // shutdown / no shutdown is how an err-disabled port comes back.
                        p.err_disabled = false;
                    }
                }
                Ok(())
            })?;
            Ok(c.link_notice())
        }
        Id::Description => {
            let text: String = arg(0).chars().take(MAX_DESCRIPTION).collect();
            c.each_if(|p, sub| {
                match sub {
                    Some(s) => s.description = text.clone(),
                    None => p.description = text.clone(),
                }
                Ok(())
            })
        }
        Id::NoDescription => c.each_if(|p, sub| {
            match sub {
                Some(s) => s.description.clear(),
                None => p.description.clear(),
            }
            Ok(())
        }),
        Id::SwModeAccess | Id::SwModeTrunk | Id::SwModeDynAuto | Id::SwModeDynDesirable => {
            let mode = match spec.id {
                Id::SwModeTrunk => SwitchportMode::Trunk,
                Id::SwModeDynAuto => SwitchportMode::DynamicAuto,
                Id::SwModeDynDesirable => SwitchportMode::DynamicDesirable,
                _ => SwitchportMode::Access,
            };
            c.each_if(|p, _| {
                if mode != SwitchportMode::Access && p.port_security.is_some() {
                    return Err(format!("% {} has port security: it stays an access port", p.name));
                }
                p.switchport.mode = mode;
                Ok(())
            })
        }
        Id::SwNonegotiate | Id::NoSwNonegotiate => {
            let on = spec.id == Id::SwNonegotiate;
            c.each_if(|p, _| {
                if on && matches!(p.switchport.mode, SwitchportMode::DynamicAuto | SwitchportMode::DynamicDesirable) {
                    return Err(format!("% {} negotiates its mode: set `switchport mode access` or `trunk` first", p.name));
                }
                p.switchport.nonegotiate = on;
                Ok(())
            })
        }
        Id::SwAccessVlan => {
            let v: VlanId = arg(0).parse().map_err(|e| format!("% {e}"))?;
            let created = {
                let d = c.dm()?;
                let new = !d.vlans.contains_key(&v);
                d.vlans.entry(v).or_insert_with(|| Device::default_vlan_name(v));
                new
            };
            c.each_if(|p, _| {
                p.switchport.access_vlan = v;
                Ok(())
            })?;
            Ok(if created { format!("VLAN {v} did not exist; created it.") } else { String::new() })
        }
        Id::NoSwAccessVlan => c.each_if(|p, _| {
            p.switchport.access_vlan = VlanId::DEFAULT;
            Ok(())
        }),
        Id::SwNativeVlan | Id::NoSwNativeVlan => {
            let v = if spec.id == Id::SwNativeVlan { arg(0).parse::<VlanId>().map_err(|e| format!("% {e}"))? } else { VlanId::DEFAULT };
            c.each_if(|p, _| {
                p.switchport.native_vlan = v;
                Ok(())
            })
        }
        Id::SwAllowed => {
            let spec_text = arg(0).to_string();
            c.each_if(|p, _| {
                p.switchport.allowed = allowed_list(&p.switchport.allowed, &spec_text)?;
                Ok(())
            })
        }
        Id::NoSwAllowed => c.each_if(|p, _| {
            p.switchport.allowed = VlanSet::all();
            Ok(())
        }),
        Id::Encapsulation => {
            let v: VlanId = arg(0).parse().map_err(|e| format!("% {e}"))?;
            let Mode::Iface(ifs) = &c.cli.mode else { return Err("% Not in interface mode".into()) };
            let ifs = ifs.clone();
            let d = c.dm()?;
            for r in ifs {
                let port = d.ports.get_mut(r.port).ok_or("% No such port")?;
                if port.subifs.iter().any(|s| Some(s.number) != r.sub && s.vlan == Some(v)) {
                    return Err(format!("% VLAN {v} is already used by another subinterface of {}", port.name));
                }
                let s = r.sub.and_then(|n| port.subif_mut(n)).ok_or("% Encapsulation applies to subinterfaces")?;
                s.vlan = Some(v);
            }
            Ok(String::new())
        }
        Id::EnableSecret => {
            let hash = match password_words(arg(0)) {
                (Some("5"), Some(h)) if h.starts_with("$pc$") => h.to_string(),
                (Some("5"), _) => return Err("% Give the secret itself, or a hash PacketCraft wrote (`$pc$…`)".into()),
                (_, Some(plain)) => {
                    c.check_length(plain)?;
                    secret_hash(plain)
                }
                _ => return Err("% Give a password".into()),
            };
            c.dm()?.access.enable_secret = Some(hash);
            Ok(String::new())
        }
        Id::EnablePassword => {
            let p = line_password(arg(0))?;
            c.check_length(&p)?;
            c.dm()?.access.enable_password = Some(p);
            Ok(String::new())
        }
        Id::NoEnableSecret => {
            c.dm()?.access.enable_secret = None;
            Ok(String::new())
        }
        Id::NoEnablePassword => {
            c.dm()?.access.enable_password = None;
            Ok(String::new())
        }
        Id::LineConsole => {
            if arg(0) != "0" {
                return Err("% There is one console line: `line console 0`".into());
            }
            c.cli.mode = Mode::Line(false);
            Ok(String::new())
        }
        Id::LineVty => {
            let nums: Vec<u32> =
                arg(0).split_whitespace().map(|n| n.parse::<u32>().map_err(|_| format!("% `{n}` is not a line number"))).collect::<Result<_, _>>()?;
            if nums.is_empty() || nums.len() > 2 || nums.iter().any(|n| *n > 15) {
                return Err("% vty lines are 0-15: `line vty 0 4` or `line vty 0 15`".into());
            }
            c.cli.mode = Mode::Line(true);
            Ok(String::new())
        }
        Id::LinePassword | Id::NoLinePassword | Id::Login | Id::NoLogin => {
            let Mode::Line(vty) = c.cli.mode else { return Err("% Not in line mode".into()) };
            let pw = if spec.id == Id::LinePassword { Some(line_password(arg(0))?) } else { None };
            if let Some(p) = &pw {
                c.check_length(p)?;
            }
            let d = c.dm()?;
            let line = if vty { &mut d.access.vty } else { &mut d.access.console };
            match spec.id {
                Id::LinePassword => line.password = pw,
                Id::NoLinePassword => line.password = None,
                Id::Login => line.login = true,
                _ => line.login = false,
            }
            let warn = line.login && line.password.is_none();
            Ok(if warn { "Login is on but the line has no password yet: it won't ask for one until you set it.".into() } else { String::new() })
        }
        Id::Accepted => Ok("Accepted (not simulated yet).".into()),
        Id::BannerMotd => {
            let text = arg(0);
            let mut chars = text.chars();
            let Some(delim) = chars.next() else {
                return Err("% Give the message between two delimiters: banner motd #Authorised users only#".into());
            };
            let body: String = chars.collect();
            let msg = body.split(delim).next().unwrap_or("").chars().take(2_000).collect::<String>();
            c.dm()?.access.banner_motd = msg;
            Ok(String::new())
        }
        Id::NoBannerMotd => {
            c.dm()?.access.banner_motd.clear();
            Ok(String::new())
        }
        Id::ServicePasswordEncryption | Id::NoServicePasswordEncryption => {
            c.dm()?.access.password_encryption = spec.id == Id::ServicePasswordEncryption;
            Ok(String::new())
        }
        Id::DomainLookup | Id::NoDomainLookup => {
            c.dm()?.access.no_domain_lookup = spec.id == Id::NoDomainLookup;
            Ok(String::new())
        }
        Id::DhcpPool => {
            let name: String = arg(0).chars().take(32).collect();
            let d = c.dm()?;
            if !d.dhcp_pools.iter().any(|p| p.name == name) {
                if d.dhcp_pools.len() >= MAX_DHCP_POOLS {
                    return Err(format!("% At most {MAX_DHCP_POOLS} pools"));
                }
                d.dhcp_pools.push(DhcpPool { name: name.clone(), network: None, default_router: None, dns: None });
            }
            c.cli.mode = Mode::DhcpPool(name);
            Ok(String::new())
        }
        Id::NoDhcpPool => {
            let d = c.dm()?;
            let before = d.dhcp_pools.len();
            d.dhcp_pools.retain(|p| p.name != arg(0));
            if d.dhcp_pools.len() == before { Err(format!("% No pool named {}", arg(0))) } else { Ok(String::new()) }
        }
        Id::DhcpExcluded | Id::NoDhcpExcluded => {
            let ips: Vec<Ipv4Addr> = arg(0).split_whitespace().map(|w| parse_ip(w).map_err(|e| format!("% {e}"))).collect::<Result<_, _>>()?;
            let (a, b) = match ips.as_slice() {
                [a] => (*a, *a),
                [a, b] if u32::from(*a) <= u32::from(*b) => (*a, *b),
                [_, _] => return Err("% The first address must not be above the second".into()),
                _ => return Err("% Give one address, or the first and last of a range".into()),
            };
            let d = c.dm()?;
            if spec.id == Id::DhcpExcluded {
                if !d.dhcp_excluded.contains(&(a, b)) {
                    if d.dhcp_excluded.len() >= MAX_DHCP_POOLS {
                        return Err(format!("% At most {MAX_DHCP_POOLS} excluded ranges"));
                    }
                    d.dhcp_excluded.push((a, b));
                }
            } else {
                d.dhcp_excluded.retain(|r| *r != (a, b));
            }
            Ok(String::new())
        }
        Id::DhcpNetwork | Id::DhcpDefaultRouter | Id::DhcpDns => {
            let Mode::DhcpPool(name) = c.cli.mode.clone() else { return Err("% Not in DHCP pool mode".into()) };
            let first = parse_ip(arg(0)).map_err(|e| format!("% {e}"))?;
            let network = if spec.id == Id::DhcpNetwork {
                let n = Ipv4Net::with_mask(first, parse_ip(arg(1)).map_err(|e| format!("% {e}"))?).map_err(|e| format!("% {e}"))?;
                Some(n.network())
            } else {
                None
            };
            let d = c.dm()?;
            let pool = d.dhcp_pools.iter_mut().find(|p| p.name == name).ok_or("% The pool was deleted")?;
            match spec.id {
                Id::DhcpNetwork => pool.network = network,
                Id::DhcpDefaultRouter => pool.default_router = Some(first),
                _ => pool.dns = Some(first),
            }
            Ok(String::new())
        }
        Id::StpPriority | Id::NoStpPriority | Id::StpRoot => {
            let set = VlanSet::parse_list(arg(0)).map_err(|e| format!("% {e}"))?;
            let pri: Option<u16> = match spec.id {
                Id::StpPriority => {
                    let p: u16 = arg(1).parse().map_err(|_| "% The priority is a number, 0-61440")?;
                    if p > 61_440 || !p.is_multiple_of(4096) {
                        return Err("% The priority is a multiple of 4096 from 0 to 61440".into());
                    }
                    Some(p)
                }
                Id::StpRoot => match arg(1).to_ascii_lowercase().as_str() {
                    "primary" => Some(24_576),
                    "secondary" => Some(28_672),
                    _ => return Err("% `root primary` or `root secondary`".into()),
                },
                _ => None,
            };
            let d = c.dm()?;
            for v in set.iter() {
                match pri {
                    Some(p) => {
                        d.stp_priority.insert(v, p);
                    }
                    None => {
                        d.stp_priority.remove(&v);
                    }
                }
            }
            Ok(String::new())
        }
        Id::Portfast | Id::NoPortfast => {
            let on = spec.id == Id::Portfast;
            c.each_if(|p, _| {
                p.portfast = on;
                Ok(())
            })?;
            Ok(if on { "Portfast is for ports facing a single host: on a port to another switch it risks a loop.".into() } else { String::new() })
        }
        Id::ShowStp => {
            let d = c.d()?.clone();
            let vlans: Vec<VlanId> = d.vlans.keys().copied().collect();
            let mut out = Vec::new();
            for v in vlans {
                let tree = c.sim.spanning_tree(c.t, v).clone();
                if tree.bridges.contains_key(&d.id) {
                    out.push(render::spanning_tree(c.t, c.sim, &d, v, &tree));
                }
            }
            Ok(if out.is_empty() { "No VLAN has a port in a spanning tree yet.".into() } else { out.join("\n\n") })
        }
        Id::ShowStpVlan => {
            let v: VlanId = arg(0).parse().map_err(|e| format!("% {e}"))?;
            let d = c.d()?.clone();
            if !d.vlans.contains_key(&v) {
                return Err(format!("% VLAN {v} does not exist on this switch"));
            }
            let tree = c.sim.spanning_tree(c.t, v).clone();
            Ok(render::spanning_tree(c.t, c.sim, &d, v, &tree))
        }
        Id::ShowDhcpBinding => Ok(render::dhcp_bindings(c.sim, c.d()?)),
        Id::ShowDhcpPool => Ok(render::dhcp_pools(c.sim, c.d()?)),
        Id::ClearDhcpBinding => {
            c.sim.clear_leases(c.dev);
            Ok(String::new())
        }
        Id::VlanName | Id::NoVlanName => {
            let Mode::Vlan(ids) = &c.cli.mode else { return Err("% Not in VLAN mode".into()) };
            let ids = ids.clone();
            let name: String = arg(0).split_whitespace().next().unwrap_or("").chars().take(32).collect();
            let d = c.dm()?;
            vtp_may_edit(d, &ids)?;
            let before = d.vlans.clone();
            for v in ids {
                let n = if spec.id == Id::VlanName { name.clone() } else { Device::default_vlan_name(v) };
                d.vlans.insert(v, n);
            }
            vtp_bump(d, &before);
            Ok(String::new())
        }
    }
}

/// Parses an access-list line after the list name: `permit|deny <src>` (standard) or
/// `permit|deny <proto> <src> [<port>] <dst> [<port>]` (extended).
fn parse_acl_entry(words: &[&str], extended: bool) -> Result<acl::AclEntry, String> {
    let mut it = words.iter().copied().peekable();
    let permit = match it.next().map(str::to_ascii_lowercase).as_deref() {
        Some("permit") => true,
        Some("deny") => false,
        _ => return Err("% A line starts with `permit` or `deny`".into()),
    };
    let proto = if extended {
        let w = it.next().ok_or("% Give a protocol: ip, icmp, tcp or udp")?;
        acl::AclProto::from_name(w).ok_or_else(|| format!("% `{w}` is not a protocol PacketCraft knows (ip, icmp, tcp, udp)"))?
    } else {
        acl::AclProto::Ip
    };
    let addr = |it: &mut std::iter::Peekable<std::iter::Copied<std::slice::Iter<'_, &str>>>| -> Result<acl::AclAddr, String> {
        let w = it.next().ok_or("% Give an address: any, host A.B.C.D, or A.B.C.D wildcard")?;
        if w.eq_ignore_ascii_case("any") {
            return Ok(acl::AclAddr::ANY);
        }
        if w.eq_ignore_ascii_case("host") {
            let a = parse_ip(it.next().ok_or("% `host` needs an address")?).map_err(|e| format!("% {e}"))?;
            return Ok(acl::AclAddr::host(a));
        }
        let a = parse_ip(w).map_err(|e| format!("% {e}"))?;
        let wildcard = match it.peek().and_then(|n| parse_ip(n).ok()) {
            Some(wc) => {
                it.next();
                wc
            }
            None => Ipv4Addr::UNSPECIFIED,
        };
        Ok(acl::AclAddr { addr: a, wildcard })
    };
    let ports = |it: &mut std::iter::Peekable<std::iter::Copied<std::slice::Iter<'_, &str>>>| -> Result<Option<acl::PortMatch>, String> {
        let Some(op) = it.peek().map(|w| w.to_ascii_lowercase()) else { return Ok(None) };
        let num = |w: Option<&str>| -> Result<u16, String> {
            let w = w.ok_or("% Give a port")?;
            port_number(w).ok_or_else(|| format!("% `{w}` is not a port"))
        };
        let m = match op.as_str() {
            "eq" | "neq" | "lt" | "gt" => {
                it.next();
                let n = num(it.next())?;
                match op.as_str() {
                    "eq" => acl::PortMatch::Eq(n),
                    "neq" => acl::PortMatch::Neq(n),
                    "lt" => acl::PortMatch::Lt(n),
                    _ => acl::PortMatch::Gt(n),
                }
            }
            "range" => {
                it.next();
                let (a, b) = (num(it.next())?, num(it.next())?);
                acl::PortMatch::Range(a.min(b), a.max(b))
            }
            _ => return Ok(None),
        };
        Ok(Some(m))
    };
    let src = addr(&mut it)?;
    let with_ports = matches!(proto, acl::AclProto::Tcp | acl::AclProto::Udp);
    let src_port = if extended && with_ports { ports(&mut it)? } else { None };
    let (dst, dst_port) = if extended {
        let d = addr(&mut it)?;
        (d, if with_ports { ports(&mut it)? } else { None })
    } else {
        (acl::AclAddr::ANY, None)
    };
    match it.next() {
        None => {}
        Some(w) if w.eq_ignore_ascii_case("log") => {}
        Some(w) => return Err(format!("% Unexpected `{w}` at the end of the line")),
    }
    Ok(acl::AclEntry { permit, proto, src, src_port, dst, dst_port })
}

/// A port by number or by its usual name.
fn port_number(w: &str) -> Option<u16> {
    w.parse().ok().or(match w.to_ascii_lowercase().as_str() {
        "ftp" => Some(21),
        "ssh" => Some(22),
        "telnet" => Some(23),
        "smtp" => Some(25),
        "domain" => Some(53),
        "bootps" => Some(67),
        "bootpc" => Some(68),
        "tftp" => Some(69),
        "www" | "http" => Some(80),
        "pop3" => Some(110),
        "snmp" => Some(161),
        "https" => Some(443),
        _ => None,
    })
}

/// `vlan 10`, `vlan10`, `Vlan 10`: `Some(Ok(id))`; another interface name: `None`.
fn vlan_iface(text: &str) -> Option<Result<VlanId, String>> {
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    let lower = compact.to_ascii_lowercase();
    let num = lower.strip_prefix("vlan").or_else(|| lower.strip_prefix("vl"))?;
    Some(num.parse::<VlanId>().map_err(|e| format!("% {e}")))
}

/// `[type] password`: the type word (`0`, `5`, `7`) if present, and the password.
fn password_words(text: &str) -> (Option<&str>, Option<&str>) {
    let mut w = text.split_whitespace();
    match (w.next(), w.next()) {
        (Some(t @ ("0" | "5" | "7")), Some(p)) => (Some(t), Some(p)),
        (Some(p), _) => (None, Some(p)),
        (None, _) => (None, None),
    }
}

/// A line or enable password as typed: plain, `0 plain`, or `7 <obscured>` (as PacketCraft
/// writes it with `service password-encryption`).
fn line_password(text: &str) -> Result<String, String> {
    match password_words(text) {
        (Some("7"), Some(o)) => unobscure(o).ok_or_else(|| "% That is not an obscured password PacketCraft wrote".to_string()),
        (Some("5"), _) => Err("% Type-5 hashes are for `enable secret` only".into()),
        (_, Some(p)) => Ok(p.chars().take(64).collect()),
        _ => Err("% Give a password".into()),
    }
}

/// `switchport trunk allowed vlan` argument: `all`, `none`, `add L`, `remove L`, `except L` or `L`.
fn allowed_list(current: &VlanSet, text: &str) -> Result<VlanSet, String> {
    let mut words = text.split_whitespace();
    let first = words.next().unwrap_or("");
    let rest: String = words.collect::<Vec<_>>().join("");
    let parse = |s: &str| VlanSet::parse_list(s).map_err(|e| format!("% {e}"));
    Ok(match first.to_ascii_lowercase().as_str() {
        "add" => {
            let mut s = current.clone();
            s.union(&parse(&rest)?);
            s
        }
        "remove" => {
            let mut s = current.clone();
            s.subtract(&parse(&rest)?);
            s
        }
        "except" => {
            let mut s = VlanSet::all();
            s.subtract(&parse(&rest)?);
            s
        }
        _ => parse(&text.split_whitespace().collect::<String>())?,
    })
}

struct Ctx<'a> {
    t: &'a mut Topology,
    sim: &'a mut Sim,
    dev: DeviceId,
    cli: &'a mut Cli,
}

impl Ctx<'_> {
    fn d(&self) -> Result<&Device, String> {
        self.t.device(self.dev).ok_or_else(|| "% That device no longer exists".to_string())
    }

    fn dm(&mut self) -> Result<&mut Device, String> {
        self.t.device_mut(self.dev).ok_or_else(|| "% That device no longer exists".to_string())
    }

    fn acl_add(&mut self, name: &str, extended: bool, entry: acl::AclEntry) -> R {
        let d = self.dm()?;
        if !d.acls.iter().any(|a| a.name == name) {
            if d.acls.len() >= acl::MAX_ACLS {
                return Err(format!("% At most {} access lists", acl::MAX_ACLS));
            }
            d.acls.push(acl::Acl { name: name.to_string(), extended, entries: Vec::new() });
        }
        let list = d.acls.iter_mut().find(|a| a.name == name).ok_or("% No such list")?;
        if list.entries.len() >= acl::MAX_ENTRIES {
            return Err(format!("% At most {} lines per list", acl::MAX_ENTRIES));
        }
        if !list.entries.contains(&entry) {
            list.entries.push(entry);
        }
        Ok(String::new())
    }

    /// Ends the session: back to user mode, asking for the line password again if set.
    fn logout(&mut self) -> R {
        self.cli.mode = Mode::User;
        self.cli.logged_in = false;
        self.cli.enable_prompt = None;
        Ok(format!("Session ended.\n\n{}", greeting(self.t, self.dev)).trim_end().to_string())
    }

    fn set_mode(&mut self, m: Mode) -> R {
        self.cli.mode = m;
        Ok(String::new())
    }

    /// Resolves an interface name on this device.
    fn iface_ref(&self, name: &str) -> Result<IfRef, String> {
        let d = self.d()?;
        let parsed = ifname::parse(name).ok_or_else(|| format!("% `{name}` is not an interface name"))?;
        let port = d.port_index(&parsed.port_name()).ok_or_else(|| format!("% {} has no interface {}", d.name, parsed.port_name()))?;
        if parsed.sub.is_some() && d.kind != DeviceKind::Router {
            return Err("% Subinterfaces exist only on routers".into());
        }
        Ok(IfRef { port, sub: parsed.sub })
    }

    /// `f0/1 - 4, f0/7, g0/1-2`
    fn iface_range(&self, text: &str) -> Result<Vec<IfRef>, String> {
        let mut out = Vec::new();
        for item in text.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            let compact: String = item.chars().filter(|c| !c.is_whitespace()).collect();
            match compact.rsplit_once('-') {
                Some((start, end)) if !start.is_empty() => {
                    let first = ifname::parse(start).ok_or_else(|| format!("% `{start}` is not an interface name"))?;
                    let last: u32 = end.parse().map_err(|_| format!("% `{end}` is not a port number"))?;
                    let from = *first.numbers.last().ok_or("% Bad range")?;
                    if last < from || first.sub.is_some() {
                        return Err(format!("% Bad range `{item}`"));
                    }
                    for n in from..=last {
                        let mut name = first.clone();
                        if let Some(x) = name.numbers.last_mut() {
                            *x = n;
                        }
                        out.push(self.iface_ref(&name.port_name())?);
                        if out.len() > MAX_RANGE {
                            return Err(format!("% A range selects at most {MAX_RANGE} interfaces"));
                        }
                    }
                }
                _ => out.push(self.iface_ref(&compact)?),
            }
        }
        out.dedup();
        if out.is_empty() { Err("% No interfaces given".into()) } else { Ok(out) }
    }

    /// Applies `f` to every interface of the current interface mode.
    fn each_if(&mut self, mut f: impl FnMut(&mut packetcraft_model::Port, Option<&mut packetcraft_model::SubIf>) -> Result<(), String>) -> R {
        let Mode::Iface(ifs) = &self.cli.mode else { return Err("% Not in interface mode".into()) };
        let ifs = ifs.clone();
        let d = self.dm()?;
        for r in ifs {
            let port = d.ports.get_mut(r.port).ok_or("% No such port")?;
            match r.sub {
                Some(n) => {
                    let mut sub = port.subif(n).cloned().ok_or("% No such subinterface")?;
                    f(port, Some(&mut sub))?;
                    if let Some(s) = port.subif_mut(n) {
                        *s = sub;
                    }
                }
                None => f(port, None)?,
            }
        }
        Ok(String::new())
    }

    /// `security passwords min-length`: a new password must be at least that long.
    fn check_length(&self, password: &str) -> Result<(), String> {
        match self.d()?.access.min_password_len {
            Some(n) if password.chars().count() < usize::from(n) => {
                Err(format!("% Password too short: at least {n} characters (security passwords min-length)"))
            }
            _ => Ok(()),
        }
    }

    /// Applies `f` to the IPv6 settings of each interface the mode is on (ports, subinterfaces
    /// or a VLAN interface), with the MAC address and name of the interface.
    fn each_v6(&mut self, mut f: impl FnMut(&mut packetcraft_model::Ipv6Conf, packetcraft_core::MacAddr, &str) -> Result<(), String>) -> R {
        if let Mode::Svi(v) = self.cli.mode {
            let d = self.dm()?;
            let mac = packetcraft_sim::svi_mac(d);
            let svi = d.svi_mut(v).ok_or("% Too many VLAN interfaces")?;
            f(&mut svi.ipv6, mac, &format!("Vlan{v}"))?;
            return Ok(String::new());
        }
        let switch = self.d()?.kind == DeviceKind::Switch;
        self.each_if(|p, sub| {
            if switch && !p.routed {
                return Err(format!("% {} is a switch port: IPv6 goes on a VLAN interface, or `no switchport` first", p.name));
            }
            match sub {
                Some(s) => {
                    let name = format!("{}.{}", p.name, s.number);
                    f(&mut s.ipv6, p.mac, &name)
                }
                None => {
                    let name = p.name.clone();
                    f(&mut p.ipv6, p.mac, &name)
                }
            }
        })
    }

    /// The names of the interfaces the mode is on.
    fn v6_names(&self) -> Vec<String> {
        match &self.cli.mode {
            Mode::Svi(v) => vec![format!("Vlan{v}")],
            Mode::Iface(ifs) => self.d().map_or(Vec::new(), |d| {
                ifs.iter()
                    .filter_map(|r| {
                        let p = d.ports.get(r.port)?;
                        Some(r.sub.map_or(p.name.clone(), |n| format!("{}.{n}", p.name)))
                    })
                    .collect()
            }),
            _ => Vec::new(),
        }
    }

    /// After `shutdown` / `no shutdown`: the resulting state of each interface.
    fn link_notice(&self) -> String {
        let (Mode::Iface(ifs), Ok(d)) = (&self.cli.mode, self.d()) else { return String::new() };
        ifs.iter()
            .filter_map(|r| {
                let p = d.ports.get(r.port)?;
                let (name, admin_down) = match r.sub.and_then(|n| p.subif(n)) {
                    Some(s) => (format!("{}.{}", p.name, s.number), s.shutdown || p.shutdown),
                    None => (p.name.clone(), p.shutdown),
                };
                let state = if admin_down {
                    "administratively down"
                } else if port_up(self.t, packetcraft_model::End { device: d.id, port: r.port }) {
                    "up"
                } else if self.t.peer(packetcraft_model::End { device: d.id, port: r.port }).is_none() {
                    "down (no cable)"
                } else {
                    "down (the far end is shut down)"
                };
                Some(format!("{name} is now {state}"))
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn ip_address(&mut self, addr: &str, mask: &str) -> R {
        let net = Ipv4Net::with_mask(parse_ip(addr).map_err(|e| format!("% {e}"))?, parse_ip(mask).map_err(|e| format!("% {e}"))?)
            .map_err(|e| format!("% {e}"))?;
        if net.is_network_or_broadcast() {
            return Err(format!("% {} is the network or broadcast address of {}", net.addr, net.network()));
        }
        let Mode::Iface(ifs) = &self.cli.mode else { return Err("% Not in interface mode".into()) };
        let [r] = ifs.as_slice() else { return Err("% Set addresses one interface at a time".into()) };
        let r = *r;
        let d = self.d()?;
        if d.kind == DeviceKind::Switch && d.ports.get(r.port).is_some_and(|p| !p.routed) {
            return Err("% This is a switch port: `no switchport` first (multilayer switches), or address a VLAN interface".into());
        }
        // VLAN interfaces count too on a switch.
        if let Some(s) = d.svis.iter().find(|s| s.ip.is_some_and(|ip| ip.overlaps(net))) {
            return Err(format!("% {} overlaps Vlan{}", net.network(), s.vlan));
        }
        // No two interfaces of one router on overlapping subnets.
        for (i, p) in d.ports.iter().enumerate() {
            let others = std::iter::once((None, p.ip)).chain(p.subifs.iter().map(|s| (Some(s.number), s.ip)));
            for (sub, ip) in others {
                if let Some(ip) = ip
                    && !(i == r.port && sub == r.sub)
                    && ip.overlaps(net)
                {
                    let name = sub.map_or(p.name.clone(), |n| format!("{}.{n}", p.name));
                    return Err(format!("% {} overlaps {} on {name}", net.network(), ip.network()));
                }
            }
        }
        if let Some(n) = r.sub
            && d.ports.get(r.port).and_then(|p| p.subif(n)).is_none_or(|s| s.vlan.is_none())
        {
            return Err("% Set `encapsulation dot1Q <vlan>` on the subinterface first".into());
        }
        self.each_if(|p, sub| {
            match sub {
                Some(s) => s.ip = Some(net),
                None => p.ip = Some(net),
            }
            Ok(())
        })
    }

    fn ip_route(&mut self, net: &str, mask: &str, target: Option<&str>, add: bool) -> R {
        if self.d()?.kind == DeviceKind::Switch && !self.d()?.ip_routing {
            return Err("% Static routes need `ip routing` (multilayer switches); a plain switch uses `ip default-gateway`".into());
        }
        let n = Ipv4Net::with_mask(parse_ip(net).map_err(|e| format!("% {e}"))?, parse_ip(mask).map_err(|e| format!("% {e}"))?)
            .map_err(|e| format!("% {e}"))?;
        if n.network() != n {
            return Err(format!("% {n} has host bits set; the network is {}", n.network()));
        }
        let via = match target {
            None => None,
            Some(t) => Some(match t.parse::<Ipv4Addr>() {
                Ok(a) => NextHop::Address(a),
                Err(_) => {
                    let r = self.iface_ref(t)?;
                    let d = self.d()?;
                    let p = d.ports.get(r.port).ok_or("% No such port")?;
                    NextHop::Interface(r.sub.map_or(p.name.clone(), |s| format!("{}.{s}", p.name)))
                }
            }),
        };
        let d = self.dm()?;
        if add {
            let Some(via) = via else { return Err("% Give a next-hop address or an exit interface".into()) };
            let route = StaticRoute { net: n, via };
            if !d.routes.contains(&route) {
                if d.routes.len() >= MAX_ROUTES {
                    return Err(format!("% At most {MAX_ROUTES} static routes"));
                }
                d.routes.push(route);
                d.routes.sort();
            }
        } else {
            let before = d.routes.len();
            d.routes.retain(|r| !(r.net == n && via.as_ref().is_none_or(|v| *v == r.via)));
            if d.routes.len() == before {
                return Err("% No such static route".into());
            }
        }
        Ok(String::new())
    }

    /// `copy … ftp: <address> [file]` and `copy ftp: … <address> <file>`, as the `ip ftp` user.
    fn ftp_copy(&mut self, upload: bool, line: &str, rest: &str) -> R {
        use packetcraft_sim::ftp::FtpOp;
        let words: Vec<&str> = rest.split_whitespace().collect();
        let (addr, file) = match words.as_slice() {
            [a] => (*a, None),
            [a, f] => (*a, Some(f.to_string())),
            _ => return Err("% Give the server's address and the file name".into()),
        };
        let server = parse_ip(addr).map_err(|e| format!("% {e}"))?;
        let d = self.d()?.clone();
        let (user, pass) = d.ftp_login.clone().ok_or("% Set the account first: ip ftp username <name>, ip ftp password <password>")?;
        let startup = line.contains("startup-config");
        if upload {
            let name = file.unwrap_or_else(|| format!("{}-confg", d.name.to_lowercase()));
            let text = if startup { d.startup.clone().ok_or("% No startup-config to copy")? } else { render::running_config(&d) };
            let r = self.sim.ftp(self.t, self.dev, server, &user, &pass, &FtpOp::Put(name.clone(), text.clone().into_bytes()));
            crate::host::apply_ftp_changes(self.t, self.sim);
            r.map_err(|e| format!("% {e}"))?;
            Ok(format!("Copied {} bytes to ftp://{server}/{name}", text.len()))
        } else {
            let name = file.ok_or("% Give the file name to copy")?;
            let (_, bytes) = self.sim.ftp(self.t, self.dev, server, &user, &pass, &FtpOp::Get(name.clone())).map_err(|e| format!("% {e}"))?;
            let text = String::from_utf8_lossy(&bytes).into_owned();
            if startup {
                self.dm()?.startup = Some(text.clone());
                Ok(format!("Copied {} bytes from ftp://{server}/{name} to startup-config", text.len()))
            } else {
                let failed = apply_config(self.t, self.sim, self.dev, &text);
                let mut out = format!("Merged {} bytes from ftp://{server}/{name} into the running configuration", text.len());
                for f in failed.iter().take(10) {
                    out.push_str(&format!("\n% {f}"));
                }
                Ok(out)
            }
        }
    }

    /// `copy … tftp: <address> [file]` and `copy tftp: … <address> <file>` (or `tftp://a/file`).
    fn tftp_copy(&mut self, upload: bool, line: &str, rest: &str) -> R {
        let words: Vec<&str> = rest.split_whitespace().collect();
        let (addr, file) = match words.as_slice() {
            [url] if url.to_ascii_lowercase().starts_with("tftp://") => {
                let u = url.get(7..).unwrap_or("");
                let (a, f) = u.split_once('/').unwrap_or((u, ""));
                (a.to_string(), (!f.is_empty()).then(|| f.to_string()))
            }
            [a] => (a.to_string(), None),
            [a, f] => (a.to_string(), Some(f.to_string())),
            _ => return Err("% Give the server's address and the file name".into()),
        };
        let server = parse_ip(&addr).map_err(|e| format!("% {e}"))?;
        let startup = line.contains("startup-config");
        let d = self.d()?.clone();
        if upload {
            let name = file.unwrap_or_else(|| format!("{}-confg", d.name.to_lowercase()));
            let text = if startup { d.startup.clone().ok_or("% No startup-config to copy")? } else { render::running_config(&d) };
            self.sim.tftp_put(self.t, self.dev, server, &name, text.as_bytes()).map_err(|e| format!("% {e}"))?;
            // The server keeps the file.
            if let Some(sd) =
                self.t.devices.iter_mut().find(|x| x.kind == DeviceKind::Server && x.ports.iter().any(|p| p.ip.is_some_and(|n| n.addr == server)))
            {
                let files = &mut sd.services.tftp.files;
                files.retain(|f| f.name != name);
                if files.len() < packetcraft_model::services::MAX_ITEMS {
                    files.push(packetcraft_model::services::WebFile {
                        name: name.clone(),
                        content: text.chars().take(packetcraft_model::services::MAX_FILE).collect(),
                    });
                }
            }
            Ok(format!("Copied {} bytes to tftp://{server}/{name}", text.len()))
        } else {
            let name = file.ok_or("% Give the file name to copy")?;
            let bytes = self.sim.tftp_get(self.t, self.dev, server, &name).map_err(|e| format!("% {e}"))?;
            let text = String::from_utf8_lossy(&bytes).into_owned();
            if startup {
                self.dm()?.startup = Some(text.clone());
                Ok(format!("Copied {} bytes from tftp://{server}/{name} to startup-config", text.len()))
            } else {
                let failed = apply_config(self.t, self.sim, self.dev, &text);
                let mut out = format!("Merged {} bytes from tftp://{server}/{name} into the running configuration", text.len());
                for f in failed.iter().take(10) {
                    out.push_str(&format!("\n% {f}"));
                }
                Ok(out)
            }
        }
    }

    /// The `<next hop and/or interface>` part of `ipv6 route`.
    fn route6_via(&self, rest: &str) -> Result<(Option<String>, Option<std::net::Ipv6Addr>), String> {
        let words: Vec<&str> = rest.split_whitespace().collect();
        let addr = |w: &str| w.parse::<std::net::Ipv6Addr>().map_err(|_| format!("% `{w}` is not an IPv6 address"));
        match words.as_slice() {
            [w] if w.contains(':') => {
                let nh = addr(w)?;
                if packetcraft_core::Ipv6Net::is_link_local(nh) {
                    return Err("% A link-local next hop needs its interface: `ipv6 route P/L <interface> FE80::X`".into());
                }
                Ok((None, Some(nh)))
            }
            [i] => Ok((Some(self.port_name(i)?), None)),
            [i, w] => Ok((Some(self.port_name(i)?), Some(addr(w)?))),
            _ => Err("% Give a next hop, an interface, or an interface and a next hop".into()),
        }
    }

    fn port_name(&self, name: &str) -> Result<String, String> {
        let r = self.iface_ref(name)?;
        if r.sub.is_some() {
            return Err("% IPv6 on subinterfaces is not simulated yet".into());
        }
        self.d()?.ports.get(r.port).map(|p| p.name.clone()).ok_or_else(|| "% No such port".into())
    }

    fn ping(&mut self, target: &str, count: u16, data: Option<usize>) -> R {
        let mut opts = PingOptions { count, ..PingOptions::default() };
        if let Some(n) = data {
            opts.size = n;
        }
        if let Ok(dst6) = target.parse::<std::net::Ipv6Addr>() {
            let probes = self.sim.ping6(self.t, self.dev, dst6, opts).map_err(|e| format!("% {e}"))?;
            return Ok(render::ping(dst6.into(), &probes, opts));
        }
        let dst = parse_ip(target).map_err(|e| format!("% {e}"))?;
        let probes = self.sim.ping(self.t, self.dev, dst, opts).map_err(|e| format!("% {e}"))?;
        Ok(render::ping(dst.into(), &probes, opts))
    }

    fn traceroute(&mut self, target: &str) -> R {
        if let Ok(dst6) = target.parse::<std::net::Ipv6Addr>() {
            let hops = self.sim.traceroute6(self.t, self.dev, dst6, 30, 2_000_000).map_err(|e| format!("% {e}"))?;
            return Ok(render::traceroute(dst6.into(), &hops));
        }
        let dst = parse_ip(target).map_err(|e| format!("% {e}"))?;
        let hops = self.sim.traceroute(self.t, self.dev, dst, 30, 2_000_000).map_err(|e| format!("% {e}"))?;
        Ok(render::traceroute(dst.into(), &hops))
    }

    fn reload(&mut self) -> R {
        let startup = self.d()?.startup.clone();
        let d = self.dm()?;
        if d.vlan_dat_deleted {
            d.vlans.clear();
            d.vlan_dat_deleted = false;
            d.ensure_default_vlan();
        }
        self.dm()?.reset_config();
        self.sim.reset_device(self.dev);
        self.cli.mode = Mode::User;
        self.cli.logged_in = false;
        self.cli.enable_prompt = None;
        match startup {
            Some(cfg) => {
                let failed = apply_config(self.t, self.sim, self.dev, &cfg);
                let mut out = "Restarted with the saved configuration.".to_string();
                for f in failed {
                    out.push_str(&format!("\n% startup-config: {f}"));
                }
                Ok(out)
            }
            None => {
                self.sim.topology_changed(self.t);
                Ok("Restarted with factory settings (no saved configuration).".into())
            }
        }
    }
}

/// `port-channel 1`, `Port-channel1`, `po1`: the group number, if the name is a port channel.
fn port_channel(name: &str) -> Option<Result<u16, String>> {
    let compact: String = name.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_lowercase();
    let digits = ["port-channel", "port-chan", "port-ch", "po"].iter().find_map(|p| compact.strip_prefix(p))?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(
        digits
            .parse::<u16>()
            .ok()
            .filter(|n| (1..=packetcraft_model::MAX_CHANNEL_GROUP).contains(n))
            .ok_or_else(|| "% Port channels are 1-48".to_string()),
    )
}

/// Whether this switch may change these VLANs under its VTP mode.
fn vtp_may_edit(d: &Device, ids: &[VlanId]) -> Result<(), String> {
    use packetcraft_model::VtpMode;
    if d.vtp.mode == VtpMode::Client {
        return Err("% This switch is a VTP client: VLANs are changed on a server of the domain".into());
    }
    if d.vtp.mode == VtpMode::Server && ids.iter().any(|v| v.get() > packetcraft_model::VTP_MAX_VLAN) {
        return Err("% VLANs 1006-4094 need `vtp mode transparent`".into());
    }
    Ok(())
}

/// A server's VLAN database changed: one revision more, so the domain takes it.
fn vtp_bump(d: &mut Device, before: &std::collections::BTreeMap<VlanId, String>) {
    if d.vtp.mode == packetcraft_model::VtpMode::Server && d.vlans != *before {
        d.vtp.revision = d.vtp.revision.saturating_add(1);
    }
}
