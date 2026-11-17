//! View ▸ View Wireless Signals: what each radio covers, drawn faintly under the cables.
//!
//! Which radios exist and which host is associated with which access point is not decided here:
//! [`radios`] reads the offers of every access point and wireless router
//! (`Topology::offers`) and [`associations`] asks `Topology::wifi_ap_of`, the same call the
//! simulator uses, so a host the walls keep out of a building has no line. Only the size of the
//! drawn circle comes from the radio's range: metres in the Physical view (the scale of the cable
//! lengths, once the lab has a layout) and workspace units in the Logical one, as for the cell towers.
//!
//! The toggle is a UI setting (`Prefs::show_wireless_signals`), not part of the lab.

use egui::{Align2, Color32, FontId, Painter, Pos2, Stroke, vec2};
use netlabcraft_core::DeviceId;
use netlabcraft_model::wlan::DEFAULT_COVERAGE_M;
use netlabcraft_model::{DeviceKind, End, Topology, WIFI_RANGE, ifname};

/// One radio that offers a network.
#[derive(Debug, Clone, PartialEq)]
pub struct Radio {
    pub device: DeviceId,
    /// The first network it offers.
    pub ssid: String,
    pub channel: u8,
    /// How far it reaches: workspace units on the logical map, and metres.
    pub units: f32,
    pub metres: f32,
}

/// The radios in the lab: access points, wireless routers and home routers with a radio port
/// that is up and offers a network (the devices `Topology::wifi_in_range` treats as access points).
pub fn radios(t: &Topology) -> Vec<Radio> {
    let mut out = Vec::new();
    for d in t.devices.iter().filter(|d| d.kind == DeviceKind::Switch && !d.powered_off) {
        let Some(port) = d.ports.iter().position(|p| ifname::is_wireless(&p.name) && !p.shutdown) else { continue };
        let Some(offer) = t.offers(End { device: d.id, port }).into_iter().next() else { continue };
        // The first radio port, as `wifi_in_range` reads it.
        let wifi = d.ports.iter().find(|p| ifname::is_wireless(&p.name)).and_then(|p| p.wifi.as_ref());
        out.push(Radio {
            device: d.id,
            ssid: offer.ssid,
            channel: offer.channel,
            units: wifi.map_or(WIFI_RANGE, |w| w.range_units()),
            metres: wifi.map_or(DEFAULT_COVERAGE_M, |w| w.coverage_m()),
        });
    }
    out
}

/// Each Wi-Fi host that is associated with an access point, and that access point.
pub fn associations(t: &Topology) -> Vec<(DeviceId, DeviceId)> {
    let mut out = Vec::new();
    for d in t.devices.iter().filter(|d| d.kind.is_host()) {
        let ap = (0..d.ports.len()).find_map(|port| t.wifi_ap_of(End { device: d.id, port }));
        if let Some(ap) = ap {
            out.push((d.id, ap.device));
        }
    }
    out
}

/// The colour of a channel: the three usual ones (1, 6 and 11) are far apart.
pub fn tint(channel: u8) -> Color32 {
    const PALETTE: [Color32; 6] = [
        Color32::from_rgb(0x2e, 0x86, 0xc1),
        Color32::from_rgb(0x28, 0xa7, 0x45),
        Color32::from_rgb(0xd6, 0x8a, 0x10),
        Color32::from_rgb(0x8e, 0x44, 0xad),
        Color32::from_rgb(0xc0, 0x39, 0x2b),
        Color32::from_rgb(0x16, 0xa0, 0x85),
    ];
    PALETTE.get(usize::from(channel) % PALETTE.len()).copied().unwrap_or(Color32::GRAY)
}

/// How a coverage area is drawn (Preferences ▸ Show/Hide).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Look {
    /// An outline around the area.
    pub boundary: bool,
    /// The colour fades outwards (otherwise a flat disc).
    pub gradient: bool,
}

impl Look {
    pub fn of(prefs: &crate::prefs::Prefs) -> Look {
        Look { boundary: prefs.wireless_boundary, gradient: prefs.wireless_gradient }
    }
}

/// Rings the fade is drawn with: each adds a little colour, so the middle is the densest.
const FADE_RINGS: u32 = 8;

/// A faint disc (fading toward the edge, with an outline: as `look` says), and the network name
/// at the top when it fits.
pub fn draw_area(painter: &Painter, centre: Pos2, radius: f32, color: Color32, ssid: &str, text: Color32, look: Look) {
    if !radius.is_finite() || radius < 1.0 {
        return;
    }
    if look.gradient {
        for ring in 0..FADE_RINGS {
            let r = radius * (1.0 - ring as f32 / FADE_RINGS as f32);
            painter.circle_filled(centre, r, color.gamma_multiply(0.035));
        }
    } else {
        painter.circle_filled(centre, radius, color.gamma_multiply(0.09));
    }
    if look.boundary {
        painter.circle_stroke(centre, radius, Stroke::new(1.0, color.gamma_multiply(0.5)));
    }
    if radius >= 40.0 && !ssid.is_empty() {
        painter.text(centre - vec2(0.0, radius - 4.0), Align2::CENTER_TOP, ssid, FontId::proportional(12.0), text.gamma_multiply(0.7));
    }
}

/// The thin line from a host to its access point.
pub fn draw_line(painter: &Painter, from: Pos2, to: Pos2, color: Color32) {
    painter.line_segment([from, to], Stroke::new(1.0, color.gamma_multiply(0.7)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use netlabcraft_engine::Session;
    use serde_json::json;

    fn wifi_lab() -> Session {
        let mut s = Session::new();
        s.execute("file.new", json!({"sample": "wifi"})).unwrap();
        s
    }

    #[test]
    fn the_access_point_is_a_radio_and_the_laptops_in_range_are_associated_with_it() {
        let s = wifi_lab();
        let t = &s.topology;
        let radios = radios(t);
        assert_eq!(radios.len(), 1, "{radios:?}");
        let ap = t.resolve("AP1").unwrap();
        assert_eq!((radios[0].device, radios[0].ssid.as_str(), radios[0].channel), (ap, "Aula", 6));
        assert_eq!((radios[0].units, radios[0].metres), (WIFI_RANGE, DEFAULT_COVERAGE_M));
        let links = associations(t);
        for name in ["Laptop1", "Laptop2"] {
            let host = t.resolve(name).unwrap();
            assert!(links.contains(&(host, ap)), "{name}: {links:?}");
        }
    }

    #[test]
    fn a_host_out_of_range_has_no_line_and_a_shorter_range_shrinks_the_circle() {
        let mut s = wifi_lab();
        let (laptop, ap) = (s.topology.resolve("Laptop2").unwrap(), s.topology.resolve("AP1").unwrap());
        if let Some(d) = s.topology.device_mut(laptop) {
            d.x += 5000.0;
        }
        assert!(!associations(&s.topology).iter().any(|(h, _)| *h == laptop));
        s.execute("device.wireless", json!({"device": "AP1", "ssid": "Aula", "mode": "wpa2-psk", "key": "clase1234", "coverage_m": 50})).unwrap();
        let r = radios(&s.topology);
        assert_eq!((r[0].device, r[0].metres), (ap, 50.0));
        assert!(r[0].units < WIFI_RANGE);
    }

    #[test]
    fn the_usual_channels_get_three_different_colours() {
        assert!(tint(1) != tint(6) && tint(6) != tint(11) && tint(1) != tint(11));
    }
}
