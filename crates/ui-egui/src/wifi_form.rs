//! The Wi-Fi settings form of an access point, a home router and a Wi-Fi host: SSID, channel and
//! coverage range (access points), the authentication as radio buttons, and the fields each
//! authentication needs (a WEP key, a pass phrase, a RADIUS server for an access point or a
//! login for a host, and the encryption type of WPA). Everything goes out through
//! `device.wireless`.

use netlabcraft_model::Wifi;
use netlabcraft_model::wlan::{Cipher, DEFAULT_COVERAGE_M, WifiMode};
use serde_json::{Value, json};

use crate::console::{LABEL_W, field, form_label};
use crate::i18n::tl;

/// The channels of the 2.4 GHz band an access point offers.
const CHANNELS: std::ops::RangeInclusive<u8> = 1..=11;

/// The form's fields, kept between frames.
#[derive(Debug, Clone, Default)]
pub(crate) struct WifiForm {
    loaded: bool,
    ssid: String,
    /// The position of the authentication in `WifiMode::ALL`.
    mode: usize,
    wep_key: String,
    pass: String,
    tkip: bool,
    radius_ip: String,
    radius_secret: String,
    coverage: String,
    channel: String,
    user: String,
    password: String,
}

impl WifiForm {
    pub(crate) fn is_loaded(&self) -> bool {
        self.loaded
    }

    /// Fills the form from a radio's present settings (`None`: a radio with no network set).
    pub(crate) fn load(&mut self, wifi: Option<&Wifi>) {
        *self = WifiForm { loaded: true, channel: "6".into(), coverage: format!("{DEFAULT_COVERAGE_M:.2}"), ..WifiForm::default() };
        let Some(w) = wifi else { return };
        self.ssid.clone_from(&w.ssid);
        let mode = w.mode();
        self.mode = WifiMode::ALL.iter().position(|m| *m == mode).unwrap_or(0);
        self.wep_key.clone_from(&w.security.wep_key);
        self.pass = w.key.clone().unwrap_or_default();
        self.tkip = w.security.encryption == Cipher::Tkip;
        self.radius_ip = w.security.radius_ip.map(|a| a.to_string()).unwrap_or_default();
        self.radius_secret.clone_from(&w.security.radius_secret);
        self.coverage = format!("{:.2}", w.coverage_m());
        self.channel = w.channel.to_string();
        self.user = w.login.as_ref().map(|l| l.user.clone()).unwrap_or_default();
        self.password = w.login.as_ref().map(|l| l.password.clone()).unwrap_or_default();
    }

    fn mode(&self) -> WifiMode {
        WifiMode::ALL.get(self.mode).copied().unwrap_or(WifiMode::Open)
    }

    /// Draws the form. `ap`: an access point or home router (it has a channel, a coverage range
    /// and a RADIUS server), else a host (it has a login). `salt` tells this form's pickers
    /// from another window's.
    pub(crate) fn show(&mut self, ui: &mut egui::Ui, ap: bool, salt: u32) {
        field(ui, tl("SSID"), &mut self.ssid);
        if ap {
            ui.horizontal(|ui| {
                let l = form_label(ui, tl("Channel"), LABEL_W);
                egui::ComboBox::from_id_salt(("wifi_channel", salt))
                    .selected_text(self.channel.clone())
                    .show_ui(ui, |ui| {
                        for n in CHANNELS {
                            ui.selectable_value(&mut self.channel, n.to_string(), n.to_string());
                        }
                    })
                    .response
                    .labelled_by(l.id);
            });
            field(ui, tl("Coverage Range (meters)"), &mut self.coverage);
        }
        ui.horizontal(|ui| {
            let _ = form_label(ui, tl("Authentication"), LABEL_W);
        });
        ui.horizontal_wrapped(|ui| {
            for (i, m) in WifiMode::ALL.iter().enumerate() {
                let text = if *m == WifiMode::Open { tl("Disabled").to_string() } else { m.name().to_string() };
                ui.radio_value(&mut self.mode, i, text);
            }
        });
        let mode = self.mode();
        if mode == WifiMode::Wep {
            field(ui, tl("WEP Key"), &mut self.wep_key);
        } else if mode.is_psk() {
            field(ui, tl("PSK Pass Phrase"), &mut self.pass);
        } else if mode.is_enterprise() {
            if ap {
                ui.label(egui::RichText::new(tl("RADIUS Server Settings")).strong());
                field(ui, tl("Server address"), &mut self.radius_ip);
                field(ui, tl("Shared secret"), &mut self.radius_secret);
            } else {
                field(ui, tl("Username"), &mut self.user);
                field(ui, tl("Password"), &mut self.password);
            }
        }
        if mode.is_wpa() {
            ui.horizontal(|ui| {
                let l = form_label(ui, tl("Encryption Type"), LABEL_W);
                egui::ComboBox::from_id_salt(("wifi_cipher", salt))
                    .selected_text(if self.tkip { Cipher::Tkip.name() } else { Cipher::Aes.name() })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.tkip, false, Cipher::Aes.name());
                        ui.selectable_value(&mut self.tkip, true, Cipher::Tkip.name());
                    })
                    .response
                    .labelled_by(l.id);
            });
        }
    }

    /// The `device.wireless` call that applies the form to a radio, sending only what the
    /// chosen authentication uses.
    pub(crate) fn params(&self, device: u32, port: &str, ap: bool) -> Value {
        let mode = self.mode();
        let word = match mode {
            WifiMode::Open => "open",
            WifiMode::Wep => "wep",
            WifiMode::WpaPsk => "wpa-psk",
            WifiMode::Wpa2Psk => "wpa2-psk",
            WifiMode::WpaEnterprise => "wpa-enterprise",
            WifiMode::Wpa2Enterprise => "wpa2-enterprise",
        };
        let mut p = json!({"device": device, "ssid": self.ssid.trim(), "mode": word});
        if !port.is_empty() {
            p["port"] = json!(port);
        }
        if ap {
            p["channel"] = json!(self.channel.trim().parse::<u64>().unwrap_or(6));
            if let Ok(m) = self.coverage.trim().parse::<f64>() {
                p["coverage_m"] = json!(m);
            }
        }
        if mode == WifiMode::Wep {
            p["wep_key"] = json!(self.wep_key.trim());
        } else if mode.is_psk() {
            p["key"] = json!(self.pass);
        } else if mode.is_enterprise() {
            if ap {
                p["radius_ip"] = json!(self.radius_ip.trim());
                p["radius_secret"] = json!(self.radius_secret);
            } else {
                p["user"] = json!(self.user.trim());
                p["password"] = json!(self.password);
            }
        }
        if mode.is_wpa() {
            p["encryption"] = json!(if self.tkip { "tkip" } else { "aes" });
        }
        p
    }
}
