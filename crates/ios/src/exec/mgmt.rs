//! Network-management commands: log timestamps and severity filters, NTP master and
//! authentication, image files in flash (backed up to and restored from a TFTP server), the
//! boot image, the configuration register (password recovery), and `show ip nat statistics`.

use std::fmt::Write as _;

use netlabcraft_core::parse_ip;
use netlabcraft_model::mgmt::{CONFREG_NORMAL, FlashFile, MAX_BOOT, MAX_FLASH_FILES, MAX_NTP_KEYS, NtpKey, Timestamps, severity_from, severity_name};
use netlabcraft_model::{Device, DeviceKind};

use super::{Ctx, R, apply_config};
use crate::grammar::Id;

/// The size flash reports for the image a device ships with.
pub const IMAGE_SIZE: u64 = 33_591_768;
/// The first line of an image file copied to a TFTP server (the server keeps a small stand-in,
/// not megabytes).
const IMAGE_MARK: &str = "NETLABCRAFT-IMAGE";

/// The image a device ships with.
pub fn factory_image(d: &Device) -> String {
    format!("netlabcraft-{}-{}.bin", d.kind.label().to_lowercase(), env!("CARGO_PKG_VERSION"))
}

/// The image the device boots: the first `boot system` file present in flash, else the factory
/// image.
pub fn boot_image(d: &Device) -> String {
    d.mgmt.boot_system.iter().find(|n| flash_files(d).iter().any(|(f, _)| f == *n)).cloned().unwrap_or_else(|| factory_image(d))
}

/// Every file in flash: the factory image, `vlan.dat` on switches, then copied images.
pub fn flash_files(d: &Device) -> Vec<(String, u64)> {
    let mut files = vec![(factory_image(d), IMAGE_SIZE)];
    if d.kind == DeviceKind::Switch && d.vlans.len() > 1 && !d.vlan_dat_deleted {
        files.push(("vlan.dat".into(), 616 + 32 * d.vlans.len() as u64));
    }
    for f in &d.nvram.flash {
        if !files.iter().any(|(n, _)| *n == f.name) {
            files.push((f.name.clone(), f.size));
        }
    }
    files
}

/// Largest configuration text kept inside a flash file.
const MAX_FLASH_TEXT: usize = 60_000;

/// Keeps `text` in flash as `name` (replacing a file of that name); returns its size.
fn store_flash_text(c: &mut Ctx<'_>, name: &str, text: &str) -> Result<u64, String> {
    let d = c.dm()?;
    if name == factory_image(d) {
        return Err("% That name is the system image; choose another".into());
    }
    d.nvram.flash.retain(|f| f.name != name);
    if d.nvram.flash.len() >= MAX_FLASH_FILES {
        return Err(format!("% Flash holds at most {MAX_FLASH_FILES} copied files"));
    }
    let kept: String = text.chars().take(MAX_FLASH_TEXT).collect();
    let size = kept.len() as u64;
    d.nvram.flash.push(FlashFile { name: name.to_string(), size, text: Some(kept) });
    Ok(size)
}

/// What a flash file holds as text: a configuration kept there, or the stand-in of an image.
fn flash_content(d: &Device, name: &str) -> Result<(String, u64), String> {
    let size = flash_files(d).iter().find(|(n, _)| n == name).map(|f| f.1).ok_or_else(|| format!("% flash:{name}: no such file"))?;
    let text = d.nvram.flash.iter().find(|f| f.name == name).and_then(|f| f.text.clone());
    Ok((text.unwrap_or_else(|| format!("{IMAGE_MARK} {name} {size}\n")), size))
}

/// Keeps what was fetched in flash: an image stand-in keeps its real size, any other text its bytes.
fn store_fetched(c: &mut Ctx<'_>, name: &str, text: &str) -> Result<u64, String> {
    if let Some(size) = text.strip_prefix(IMAGE_MARK).and_then(|r| r.split_whitespace().nth(1)).and_then(|n| n.parse::<u64>().ok()) {
        let d = c.dm()?;
        if name == factory_image(d) {
            return Err("% That name is the system image; choose another".into());
        }
        d.nvram.flash.retain(|f| f.name != name);
        if d.nvram.flash.len() >= MAX_FLASH_FILES {
            return Err(format!("% Flash holds at most {MAX_FLASH_FILES} copied files"));
        }
        d.nvram.flash.push(FlashFile { name: name.to_string(), size, text: None });
        return Ok(size);
    }
    store_flash_text(c, name, text)
}

/// Applies a configuration text: merged into the running one, or kept as the startup one.
fn take_config(c: &mut Ctx<'_>, text: &str, startup: bool, from: &str) -> R {
    if startup {
        c.dm()?.startup = Some(text.to_string());
        return Ok(format!("Copied {} bytes from {from} to startup-config", text.len()));
    }
    let failed = apply_config(c.t, c.sim, c.dev, text);
    c.cli.mode = super::Mode::Exec;
    let mut out = format!("Merged {} bytes from {from} into the running configuration", text.len());
    for f in failed.iter().take(10) {
        out.push_str(&format!("\n% {f}"));
    }
    Ok(out)
}

/// The address, the file and the name to keep it under, from the words after `ftp:` or `http:`.
fn server_words(rest: &str) -> Result<(std::net::Ipv4Addr, String, Option<String>), String> {
    let words: Vec<&str> = rest.split_whitespace().collect();
    let (addr, file, local) = match words.as_slice() {
        [a, f] => (*a, *f, None),
        [a, f, l] => (*a, *f, Some(l.to_string())),
        _ => return Err("% Give the server's address, the file and, if you want one, a name to keep it under".into()),
    };
    Ok((parse_ip(addr).map_err(|e| format!("% {e}"))?, file.to_string(), local))
}

/// `flash:NAME` or `NAME` → `NAME`.
fn flash_name(w: &str) -> Result<String, String> {
    let n = w.strip_prefix("flash:").unwrap_or(w).trim_start_matches('/');
    if n.is_empty() || n.len() > 64 || !n.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)) {
        return Err(format!("% `{w}` is not a file name (letters, digits, - _ .)"));
    }
    Ok(n.to_string())
}

/// The USB stick this device has mounted, or why it has none.
fn mounted_stick(c: &Ctx<'_>) -> Result<netlabcraft_core::DeviceId, String> {
    if c.d()?.usb_host_ports() == 0 {
        return Err("% This device has no USB port".into());
    }
    let plugged = c.t.usbs_of_host(c.dev).into_iter().find(|u| c.t.device(u.device).is_some_and(Device::is_usb_stick));
    match plugged {
        None => Err("% No USB stick is plugged into this device".into()),
        Some(u) if u.ejected => Err("% The USB stick is ejected: mount it first (usb.mount)".into()),
        Some(u) => match c.t.usb_problem(&u) {
            Some(why) => Err(format!("% The USB stick does not answer: {why}")),
            None => Ok(u.device),
        },
    }
}

/// `show usb`: the port and what is plugged into it.
fn show_usb(t: &netlabcraft_model::Topology, dev: netlabcraft_core::DeviceId) -> String {
    let Some(d) = t.device(dev) else { return String::new() };
    let ports = d.usb_host_ports();
    if ports == 0 {
        return "% This device has no USB port".into();
    }
    let mut s = format!("USB ports: {ports}");
    for port in 0..ports {
        let n = u16::from(port) + 1;
        let Some(c) = t.usb_in_port(dev, port) else {
            let _ = write!(s, "\nPort {n}: empty");
            continue;
        };
        let Some(x) = t.device(c.device) else { continue };
        let state = match (t.usb_problem(c), c.ejected) {
            (Some(why), _) => format!("not working ({why})"),
            (None, true) => "ejected".to_string(),
            (None, false) => "mounted".to_string(),
        };
        let _ = write!(s, "\nPort {n}: {} ({}), {state}", x.name, x.catalogue_model().label);
        if x.is_usb_stick() {
            let _ = write!(s, ", {} files, {} bytes", x.usb.files.len(), x.usb.used());
        }
    }
    s
}

/// The configuration lines of these settings, for `show running-config`.
pub fn running_config(s: &mut String, d: &Device) {
    let m = &d.mgmt;
    for (what, ts) in [("log", m.timestamps_log), ("debug", m.timestamps_debug)] {
        if let Some(t) = ts {
            let _ = writeln!(s, "service timestamps {what} datetime{}", if t.msec { " msec" } else { "" });
        }
    }
    for b in &m.boot_system {
        let _ = writeln!(s, "boot system flash:{b}");
    }
    if let Some(l) = m.logging_buffered {
        let _ = writeln!(s, "logging buffered {}", severity_name(l));
    }
    if let Some(l) = m.logging_console {
        let _ = writeln!(s, "logging console {}", severity_name(l));
    }
    if let Some(l) = m.logging_monitor {
        let _ = writeln!(s, "logging monitor {}", severity_name(l));
    }
    if let Some(l) = m.logging_trap {
        let _ = writeln!(s, "logging trap {}", severity_name(l));
    }
    if m.ntp_authenticate {
        let _ = writeln!(s, "ntp authenticate");
    }
    for k in &m.ntp_keys {
        let _ = writeln!(s, "ntp authentication-key {} md5 {}", k.id, k.key);
    }
    for k in &m.ntp_trusted {
        let _ = writeln!(s, "ntp trusted-key {k}");
    }
    for (a, k) in &m.ntp_server_keys {
        let _ = writeln!(s, "ntp server {a} key {k}");
    }
    if let Some(n) = m.ntp_master {
        let _ = writeln!(s, "ntp master {n}");
    }
    if m.ntp_update_calendar {
        let _ = writeln!(s, "ntp update-calendar");
    }
    if d.nvram.register() != CONFREG_NORMAL {
        let _ = writeln!(s, "config-register 0x{:04x}", d.nvram.register());
    }
}

/// A key number (1-4294967295).
fn key_id(w: &str) -> Result<u32, String> {
    w.parse::<u32>().ok().filter(|n| *n > 0).ok_or_else(|| format!("% `{w}` is not a key number (1-4294967295)"))
}

fn level(w: &str) -> Result<u8, String> {
    severity_from(w).ok_or_else(|| format!("% `{w}` is not a severity: 0-7 or emergencies … debugging"))
}

pub(super) fn command(c: &mut Ctx<'_>, id: Id, args: &[String]) -> R {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    match id {
        Id::ServiceTimestamps | Id::NoServiceTimestamps => {
            let set = id == Id::ServiceTimestamps;
            let rest = arg(1).to_ascii_lowercase();
            if set && !rest.split_whitespace().all(|w| ["msec", "localtime", "show-timezone", "year"].contains(&w)) {
                return Err("% After `datetime`: msec, localtime, show-timezone".into());
            }
            let ts = set.then_some(Timestamps { msec: rest.split_whitespace().any(|w| w == "msec") });
            let m = &mut c.dm()?.mgmt;
            match arg(0).to_ascii_lowercase().as_str() {
                "log" => m.timestamps_log = ts,
                "debug" => m.timestamps_debug = ts,
                _ => return Err("% `log` or `debug`".into()),
            }
            Ok(String::new())
        }
        Id::LoggingTrap | Id::LoggingConsole | Id::LoggingMonitor => {
            let l = level(arg(0))?;
            let m = &mut c.dm()?.mgmt;
            match id {
                Id::LoggingTrap => m.logging_trap = Some(l),
                Id::LoggingMonitor => m.logging_monitor = Some(l),
                _ => m.logging_console = Some(l),
            }
            Ok(String::new())
        }
        Id::NoLoggingMonitor => {
            c.dm()?.mgmt.logging_monitor = None;
            Ok(String::new())
        }
        Id::NoLoggingBuffered => {
            c.dm()?.mgmt.logging_buffered = None;
            Ok(String::new())
        }
        Id::NoLoggingTrap => {
            c.dm()?.mgmt.logging_trap = None;
            Ok(String::new())
        }
        Id::NoLoggingConsole => {
            c.dm()?.mgmt.logging_console = None;
            Ok(String::new())
        }
        Id::LoggingBuffered => {
            // `[size] [level]`: a number above 7 is the buffer size (fixed here).
            let mut lv = None;
            for w in arg(0).split_whitespace() {
                match w.parse::<u32>() {
                    Ok(n) if n > 7 => {}
                    _ => lv = Some(level(w)?),
                }
            }
            c.dm()?.mgmt.logging_buffered = lv;
            Ok(String::new())
        }
        Id::NtpMaster => {
            let stratum =
                if arg(0).is_empty() { 8 } else { arg(0).parse::<u8>().ok().filter(|n| (1..=15).contains(n)).ok_or("% The stratum is 1-15")? };
            c.dm()?.mgmt.ntp_master = Some(stratum);
            Ok(String::new())
        }
        Id::NoNtpMaster => {
            c.dm()?.mgmt.ntp_master = None;
            Ok(String::new())
        }
        Id::NtpAuthenticate | Id::NoNtpAuthenticate => {
            c.dm()?.mgmt.ntp_authenticate = id == Id::NtpAuthenticate;
            Ok(String::new())
        }
        Id::NtpUpdateCalendar | Id::NoNtpUpdateCalendar => {
            c.dm()?.mgmt.ntp_update_calendar = id == Id::NtpUpdateCalendar;
            Ok(String::new())
        }
        Id::NtpAuthKey => {
            let n = key_id(arg(0))?;
            let key: String = arg(1).chars().take(32).collect();
            if key.is_empty() || key.chars().any(|ch| ch.is_whitespace() || ch.is_control()) {
                return Err("% Give the key as one word".into());
            }
            let m = &mut c.dm()?.mgmt;
            m.ntp_keys.retain(|k| k.id != n);
            if m.ntp_keys.len() >= MAX_NTP_KEYS {
                return Err(format!("% At most {MAX_NTP_KEYS} keys"));
            }
            m.ntp_keys.push(NtpKey { id: n, key });
            m.ntp_keys.sort_by_key(|k| k.id);
            Ok(String::new())
        }
        Id::NoNtpAuthKey => {
            let n = key_id(arg(0))?;
            c.dm()?.mgmt.ntp_keys.retain(|k| k.id != n);
            Ok(String::new())
        }
        Id::NtpTrustedKey | Id::NoNtpTrustedKey => {
            let n = key_id(arg(0))?;
            let m = &mut c.dm()?.mgmt;
            m.ntp_trusted.retain(|k| *k != n);
            if id == Id::NtpTrustedKey {
                if m.ntp_trusted.len() >= MAX_NTP_KEYS {
                    return Err(format!("% At most {MAX_NTP_KEYS} trusted keys"));
                }
                m.ntp_trusted.push(n);
                m.ntp_trusted.sort_unstable();
            }
            Ok(String::new())
        }
        Id::NtpServerKey => {
            let a = parse_ip(arg(0)).map_err(|e| format!("% {e}"))?;
            let n = key_id(arg(1))?;
            let d = c.dm()?;
            if !d.ntp_servers.contains(&a) {
                if d.ntp_servers.len() >= 4 {
                    return Err("% At most 4 NTP servers".into());
                }
                d.ntp_servers.push(a);
            }
            d.mgmt.ntp_server_keys.retain(|(x, _)| *x != a);
            d.mgmt.ntp_server_keys.push((a, n));
            Ok(match c.sim.ntp_sync(c.t, c.dev) {
                Some(s) => format!("Clock synchronised with {s}."),
                None => format!("{a} gave no time this device accepts yet; the clock will follow when it does."),
            })
        }
        Id::ShowIpNatStatistics => Ok(nat_statistics(c)),
        Id::ShowFileSystems => {
            let d = c.d()?;
            let used: u64 = flash_files(d).iter().map(|f| f.1).sum();
            let total = 268_435_456u64;
            let nv = d.startup.as_ref().map_or(0, |s| s.len() as u64);
            let usb =
                c.t.stick_of_host(c.dev).and_then(|s| c.t.device(s)).map(|s| netlabcraft_model::usb::STICK_CAPACITY.saturating_sub(s.usb.used()));
            let usb_row = usb.map_or(String::new(), |free| {
                format!("\n{:<10} {:>11} {:>11}  {:<7} {}", "usbflash0:", netlabcraft_model::usb::STICK_CAPACITY, free, "usbflash", "read/write")
            });
            Ok(format!(
                "{:<10} {:>11} {:>11}  {:<7} {}\n{:<10} {:>11} {:>11}  {:<7} {}\n{:<10} {:>11} {:>11}  {:<7} {}\n{:<10} {:>11} {:>11}  {:<7} {}",
                "Name",
                "Size",
                "Free",
                "Type",
                "Access",
                "flash:",
                total,
                total.saturating_sub(used),
                "disk",
                "read/write",
                "nvram:",
                262_144,
                262_144u64.saturating_sub(nv),
                "nvram",
                "read/write",
                "tftp:",
                "-",
                "-",
                "network",
                "read/write",
            ) + &usb_row)
        }
        Id::BootSystem => {
            let name = flash_name(arg(0).trim())?;
            let m = &mut c.dm()?.mgmt;
            if !m.boot_system.contains(&name) {
                if m.boot_system.len() >= MAX_BOOT {
                    return Err(format!("% At most {MAX_BOOT} boot system lines"));
                }
                m.boot_system.push(name.clone());
            }
            let present = flash_files(c.d()?).iter().any(|(n, _)| *n == name);
            Ok(if present { String::new() } else { format!("flash:{name} is not in flash: until it is, the device boots its factory image.") })
        }
        Id::NoBootSystem => {
            let w = arg(0).trim();
            let m = &mut c.dm()?.mgmt;
            if w.is_empty() {
                m.boot_system.clear();
            } else {
                let name = flash_name(w)?;
                m.boot_system.retain(|n| *n != name);
            }
            Ok(String::new())
        }
        Id::ConfigRegister => {
            let w = arg(0).to_ascii_lowercase();
            let v = u16::from_str_radix(w.strip_prefix("0x").unwrap_or(&w), 16)
                .map_err(|_| format!("% `{}` is not a register value (0x0-0xFFFF)", arg(0)))?;
            let d = c.dm()?;
            d.nvram.config_register = (v != CONFREG_NORMAL).then_some(v);
            Ok(format!("The configuration register will be 0x{v:04x} at the next reload."))
        }
        Id::CopyStartRun => {
            let text = c.d()?.startup.clone().ok_or("% No startup-config to copy")?;
            let failed = apply_config(c.t, c.sim, c.dev, &text);
            c.cli.mode = super::Mode::Exec;
            let mut out = format!("Merged {} bytes of startup-config into the running configuration", text.len());
            for f in failed.iter().take(10) {
                out.push_str(&format!("\n% {f}"));
            }
            Ok(out)
        }
        Id::CopyFlashTftp => {
            let words: Vec<&str> = arg(0).split_whitespace().collect();
            let (file, addr, remote) = match words.as_slice() {
                [f, a] => (*f, *a, *f),
                [f, a, r] => (*f, *a, *r),
                _ => return Err("% copy flash: tftp: <file> <server address> [name on the server]".into()),
            };
            let name = flash_name(file)?;
            let remote = flash_name(remote)?;
            let server = parse_ip(addr).map_err(|e| format!("% {e}"))?;
            let d = c.d()?.clone();
            let size = flash_files(&d).iter().find(|(n, _)| *n == name).map(|f| f.1).ok_or_else(|| format!("% flash:{name}: no such file"))?;
            let text = format!("{IMAGE_MARK} {name} {size}\n");
            c.sim.tftp_put(c.t, c.dev, server, &remote, text.as_bytes()).map_err(|e| format!("% {e}"))?;
            super::keep_on_tftp_server(c.t, server, &remote, &text);
            Ok(format!("Copied flash:{name} ({size} bytes) to tftp://{server}/{remote}"))
        }
        Id::CopyRunFlash | Id::CopyStartFlash => {
            let name = flash_name(arg(0).trim())?;
            let d = c.d()?;
            let text =
                if id == Id::CopyStartFlash { d.startup.clone().ok_or("% No startup-config to copy")? } else { crate::render::running_config(d) };
            let size = store_flash_text(c, &name, &text)?;
            Ok(format!("Wrote {size} bytes to flash:{name}"))
        }
        Id::CopyFlashRun | Id::CopyFlashStart => {
            let name = flash_name(arg(0).trim())?;
            let text = c
                .d()?
                .nvram
                .flash
                .iter()
                .find(|f| f.name == name)
                .and_then(|f| f.text.clone())
                .ok_or_else(|| format!("% flash:{name} holds no configuration text"))?;
            take_config(c, &text, id == Id::CopyFlashStart, &format!("flash:{name}"))
        }
        Id::CopyFlashFtp => {
            use netlabcraft_sim::ftp::FtpOp;
            let words: Vec<&str> = arg(0).split_whitespace().collect();
            let (file, addr, remote) = match words.as_slice() {
                [f, a] => (*f, *a, *f),
                [f, a, r] => (*f, *a, *r),
                _ => return Err("% copy flash: ftp: <file> <server address> [name on the server]".into()),
            };
            let name = flash_name(file)?;
            let remote = flash_name(remote)?;
            let server = parse_ip(addr).map_err(|e| format!("% {e}"))?;
            let (user, pass) = c.d()?.ftp_login.clone().ok_or("% Set the account first: ip ftp username <name>, ip ftp password <password>")?;
            let (text, size) = flash_content(c.d()?, &name)?;
            let r = c.sim.ftp(c.t, c.dev, server, &user, &pass, &FtpOp::Put(remote.clone(), text.into_bytes()));
            crate::host::apply_ftp_changes(c.t, c.sim);
            r.map_err(|e| format!("% {e}"))?;
            Ok(format!("Sent flash:{name} ({size} bytes) to ftp://{server}/{remote}"))
        }
        Id::CopyFtpFlash => {
            use netlabcraft_sim::ftp::FtpOp;
            let (server, file, local) = server_words(arg(0))?;
            let name = flash_name(&file)?;
            let local = flash_name(&local.unwrap_or_else(|| name.clone()))?;
            let (user, pass) = c.d()?.ftp_login.clone().ok_or("% Set the account first: ip ftp username <name>, ip ftp password <password>")?;
            let (_, bytes) = c.sim.ftp(c.t, c.dev, server, &user, &pass, &FtpOp::Get(name.clone())).map_err(|e| format!("% {e}"))?;
            let text = String::from_utf8_lossy(&bytes).into_owned();
            let size = store_fetched(c, &local, &text)?;
            Ok(format!("Fetched ftp://{server}/{name} into flash:{local} ({size} bytes)"))
        }
        Id::ShowUsb => Ok(show_usb(c.t, c.dev)),
        Id::DirUsb => {
            let stick = mounted_stick(c)?;
            let files = c.t.device(stick).map(|d| d.usb.clone()).unwrap_or_default();
            let mut s = format!("Directory of usbflash0:/\n{:<4} {:>10}  {}\n", "#", "Bytes", "Name");
            for (i, f) in files.files.iter().enumerate() {
                let _ = writeln!(s, "{:<4} {:>10}  {}", i + 1, f.size(), f.name);
            }
            let _ = write!(
                s,
                "{} bytes free of {}",
                netlabcraft_model::usb::STICK_CAPACITY.saturating_sub(files.used()),
                netlabcraft_model::usb::STICK_CAPACITY
            );
            Ok(s)
        }
        Id::CopyFlashUsb => {
            let words: Vec<&str> = arg(0).split_whitespace().collect();
            let (file, remote) = match words.as_slice() {
                [f] => (*f, *f),
                [f, r] => (*f, *r),
                _ => return Err("% copy flash: usbflash0: <file> [name on the stick]".into()),
            };
            let name = flash_name(file)?;
            let remote = flash_name(remote)?;
            let stick = mounted_stick(c)?;
            let (text, size) = flash_content(c.d()?, &name)?;
            let dev = c.t.device_mut(stick).ok_or("% The USB stick is gone")?;
            dev.usb.put(&remote, &text).map_err(|e| format!("% {e}"))?;
            Ok(format!("Copied flash:{name} ({size} bytes) to usbflash0:{remote}"))
        }
        Id::CopyUsbFlash => {
            let words: Vec<&str> = arg(0).split_whitespace().collect();
            let (file, local) = match words.as_slice() {
                [f] => (*f, *f),
                [f, l] => (*f, *l),
                _ => return Err("% copy usbflash0: flash: <file> [name in flash]".into()),
            };
            let stick = mounted_stick(c)?;
            let local = flash_name(local)?;
            let text =
                c.t.device(stick)
                    .and_then(|d| d.usb.find(file.strip_prefix("usbflash0:").unwrap_or(file)))
                    .map(|f| f.text.clone())
                    .ok_or_else(|| format!("% usbflash0:{file}: no such file"))?;
            let size = store_fetched(c, &local, &text)?;
            Ok(format!("Copied usbflash0:{file} into flash:{local} ({size} bytes)"))
        }
        Id::CopyHttpRun | Id::CopyHttpStart | Id::CopyHttpFlash => {
            let words: Vec<&str> = arg(0).split_whitespace().collect();
            let (addr, path) = match words.as_slice() {
                [a, p, ..] => (*a, p.trim_start_matches('/').to_string()),
                _ => return Err("% Give the server's address and the page's path".into()),
            };
            let server = parse_ip(addr).map_err(|e| format!("% {e}"))?;
            let (status, body) = c.sim.http_get(c.t, c.dev, server, addr, &format!("/{path}")).map_err(|e| format!("% {e}"))?;
            if !status.trim_start().starts_with("200") {
                return Err(format!("% The server answered: {status}"));
            }
            let from = format!("http://{server}/{path}");
            if id == Id::CopyHttpFlash {
                let default = path.rsplit('/').next().filter(|s| !s.is_empty()).unwrap_or("index.html").to_string();
                let local = flash_name(words.get(2).copied().unwrap_or(&default))?;
                let size = store_fetched(c, &local, &body)?;
                return Ok(format!("Fetched {from} into flash:{local} ({size} bytes)"));
            }
            take_config(c, &body, id == Id::CopyHttpStart, &from)
        }
        Id::CopyTftpFlash => {
            let words: Vec<&str> = arg(0).split_whitespace().collect();
            let (addr, file, local) = match words.as_slice() {
                [a, f] => (*a, *f, *f),
                [a, f, l] => (*a, *f, *l),
                _ => return Err("% copy tftp: flash: <server address> <file> [name in flash]".into()),
            };
            let server = parse_ip(addr).map_err(|e| format!("% {e}"))?;
            let name = flash_name(file)?;
            let local = flash_name(local)?;
            let bytes = c.sim.tftp_get(c.t, c.dev, server, &name).map_err(|e| format!("% {e}"))?;
            let text = String::from_utf8_lossy(&bytes);
            // An image backed up from a device carries its real size; any other file is its bytes.
            let size = text
                .strip_prefix(IMAGE_MARK)
                .and_then(|r| r.split_whitespace().nth(1))
                .and_then(|n| n.parse::<u64>().ok())
                .unwrap_or(bytes.len() as u64);
            let d = c.dm()?;
            d.nvram.flash.retain(|f| f.name != local);
            if d.nvram.flash.len() >= MAX_FLASH_FILES {
                return Err(format!("% Flash holds at most {MAX_FLASH_FILES} copied files"));
            }
            if local != factory_image(d) {
                d.nvram.flash.push(FlashFile { name: local.clone(), size, text: None });
            }
            Ok(format!("Copied tftp://{server}/{name} to flash:{local} ({size} bytes)"))
        }
        _ => Err("% Not a management command".into()),
    }
}

/// `show ip nat statistics`.
fn nat_statistics(c: &Ctx<'_>) -> String {
    let Ok(d) = c.d() else { return String::new() };
    let now = c.sim.now_us();
    let entries: Vec<netlabcraft_sim::NatEntry> =
        c.sim.state(c.dev).map(|st| st.nat.iter().filter(|e| e.alive(d, now)).cloned().collect::<Vec<_>>()).unwrap_or_default();
    let dynamic = entries.iter().filter(|e| !e.fixed).count();
    let extended = entries.iter().filter(|e| e.local_id.is_some()).count();
    let statics = d.nat.statics.len();
    let mut inside = Vec::new();
    let mut outside = Vec::new();
    for p in &d.ports {
        match p.nat {
            Some(netlabcraft_model::NatSide::Inside) => inside.push(p.name.clone()),
            Some(netlabcraft_model::NatSide::Outside) => outside.push(p.name.clone()),
            None => {}
        }
        for sub in &p.subifs {
            let n = format!("{}.{}", p.name, sub.number);
            match sub.nat {
                Some(netlabcraft_model::NatSide::Inside) => inside.push(n),
                Some(netlabcraft_model::NatSide::Outside) => outside.push(n),
                None => {}
            }
        }
    }
    for svi in &d.svis {
        match svi.nat {
            Some(netlabcraft_model::NatSide::Inside) => inside.push(format!("Vlan{}", svi.vlan)),
            Some(netlabcraft_model::NatSide::Outside) => outside.push(format!("Vlan{}", svi.vlan)),
            None => {}
        }
    }
    let list = |v: &[String]| if v.is_empty() { "none".to_string() } else { v.join(", ") };
    let mut s = format!(
        "Translations in use: {} ({statics} static, {dynamic} dynamic; {extended} with ports)\nInside interfaces:  {}\nOutside interfaces: {}\n",
        statics + dynamic,
        list(&inside),
        list(&outside)
    );
    let (hits, misses) = c.sim.state(c.dev).map_or((0, 0), |st| (st.nat_hits, st.nat_misses));
    let _ = writeln!(s, "Hits: {hits} (found a translation)  Misses: {misses} (needed a new one)");
    if d.nat.rules.is_empty() {
        s.push_str("Dynamic rules: none\n");
    }
    for r in &d.nat.rules {
        let target = match &r.target {
            netlabcraft_model::NatTarget::Interface(i) => format!("interface {i}"),
            netlabcraft_model::NatTarget::Pool(p) => format!("pool {p}"),
        };
        let vrf = r.vrf.as_ref().map_or(String::new(), |v| format!(" in VRF {v}"));
        let _ = writeln!(s, "Dynamic rule: access list {} → {target}{vrf}{}", r.acl, if r.overload { " (overload)" } else { "" });
    }
    for p in &d.nat.pools {
        let used = entries
            .iter()
            .filter(|e| !e.fixed && u32::from(e.inside_global) >= u32::from(p.start) && u32::from(e.inside_global) <= u32::from(p.end))
            .count();
        let _ = writeln!(s, "Pool {}: {} - {}, {used} address{} in use", p.name, p.start, p.end, if used == 1 { "" } else { "es" });
    }
    s.trim_end().to_string()
}
