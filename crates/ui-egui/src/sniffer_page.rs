//! The Capture tab of a sniffer: the frames that reached its port, read from the event list.
//! It is read-only; the sniffer itself answers nothing (`netlabcraft-sim`).

use egui::{RichText, Ui};
use netlabcraft_model::Device;

use crate::PacketApp;
use crate::i18n::{tl, tr_event, trf};
use crate::theme;

/// The most frames the table lists (the newest); the counter says how many were recorded.
const SHOWN: usize = 200;

pub(crate) fn tab(app: &mut PacketApp, ui: &mut Ui, dev: &Device) {
    let tk = theme::tokens(app.ui.dark);
    let own = dev.ports.first().map(|p| p.mac.0);
    let t = &app.session.topology;
    let seen: Vec<_> = app.session.sim.trace().iter().filter(|e| e.to.device == dev.id).collect();
    ui.label(tl(
        "A sniffer records every frame that reaches its port and answers none of them. Behind a hub it hears all the traffic; behind a switch, only broadcasts, multicasts and the frames the switch floods.",
    ));
    ui.label(RichText::new(trf("Frames captured: {}", &[&seen.len()])).strong());
    if seen.is_empty() {
        ui.label(
            RichText::new(tl("Nothing captured yet. Cable the sniffer to a hub or a switch and send some traffic (a ping, or a simple PDU)."))
                .color(tk.muted),
        );
        return;
    }
    let skip = seen.len().saturating_sub(SHOWN);
    if skip > 0 {
        ui.label(RichText::new(trf("Showing the last {} frames", &[&SHOWN])).color(tk.muted));
    }
    egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
        egui::Grid::new(("sniffer-capture", dev.id.0)).striped(true).num_columns(6).show(ui, |ui| {
            for head in ["No.", "Time (s)", "From", "Protocol", "Addressed to", "Summary"] {
                ui.label(RichText::new(tl(head)).strong());
            }
            ui.end_row();
            for e in seen.iter().skip(skip) {
                let from = t.device(e.from.device).map_or_else(|| "--".to_string(), |d| d.display_name().to_string());
                let dst = e.frame.get(0..6);
                let to = match dst {
                    Some(d) if own.is_some_and(|m| m.as_slice() == d) => tl("This port"),
                    Some(d) if d.first().is_some_and(|b| b & 1 == 1) => tl("Everyone (broadcast or multicast)"),
                    _ => tl("Another device (overheard)"),
                };
                let name = RichText::new(e.protocol.name()).color(theme::protocol_color(e.protocol));
                ui.label(e.id.to_string());
                ui.label(format!("{:.3}", e.at_us as f64 / 1e6));
                ui.label(from);
                ui.label(name);
                ui.label(to);
                // The arrow in a summary has no glyph in the built-in fonts.
                ui.label(tr_event(&e.summary).replace('→', "->"));
                ui.end_row();
            }
        });
    });
}
