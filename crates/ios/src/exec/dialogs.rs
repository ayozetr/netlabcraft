//! The interactive `ping` and `traceroute` dialogs. Typed with no target, each command asks for
//! its options one question at a time and reads the answers from the next lines of the console
//! session; an empty answer takes the default shown in square brackets.

use super::{Ctx, R};

/// Which command the dialog belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogKind {
    Ping,
    Traceroute,
}

/// A command waiting for the answers to its questions.
#[derive(Debug, Clone)]
pub struct Dialog {
    pub kind: DialogKind,
    /// The answers so far, one per question asked (an empty one means the default).
    pub answers: Vec<String>,
}

fn yes(text: &str) -> bool {
    text.trim().to_ascii_lowercase().starts_with('y')
}

/// An optional whole number in a range: empty is fine, anything else must fit.
fn number(text: &str, lo: u64, hi: u64, what: &str) -> Result<(), String> {
    if text.is_empty() {
        return Ok(());
    }
    match text.parse::<u64>() {
        Ok(n) if (lo..=hi).contains(&n) => Ok(()),
        _ => Err(format!("% {what} is a number from {lo} to {hi}")),
    }
}

impl Dialog {
    pub fn new(kind: DialogKind) -> Dialog {
        Dialog { kind, answers: Vec::new() }
    }

    fn answer(&self, i: usize) -> &str {
        self.answers.get(i).map_or("", String::as_str)
    }

    /// The question to ask next, or `None` when every answer is in.
    pub fn question(&self) -> Option<&'static str> {
        let step = self.answers.len();
        let extended = yes(self.answer(5));
        let sweep = extended && yes(self.answer(10));
        match self.kind {
            DialogKind::Ping => match step {
                0 => Some("Protocol [ip]: "),
                1 => Some("Target address or name: "),
                2 => Some("Repeat count [5]: "),
                3 => Some("Datagram size [100]: "),
                4 => Some("Timeout in seconds [2]: "),
                5 => Some("Extended commands [n]: "),
                6 if extended => Some("Source address or interface: "),
                7 if extended => Some("Type of service [0]: "),
                8 if extended => Some("Set the DF bit [n]: "),
                9 if extended => Some("Data pattern [0xABCD]: "),
                10 if extended => Some("Sweep a range of sizes [n]: "),
                11 if sweep => Some("Sweep minimum size [36]: "),
                12 if sweep => Some("Sweep maximum size [100]: "),
                13 if sweep => Some("Sweep interval [18]: "),
                _ => None,
            },
            DialogKind::Traceroute => match step {
                0 => Some("Protocol [ip]: "),
                1 => Some("Target address or name: "),
                2 => Some("Maximum number of hops [30]: "),
                3 => Some("Timeout in seconds [2]: "),
                _ => None,
            },
        }
    }

    /// Takes the answer to the question being asked; an error leaves the question unanswered.
    pub fn accept(&mut self, line: &str) -> Result<(), String> {
        let text: String = line.chars().filter(|c| !c.is_control()).take(80).collect::<String>().trim().to_string();
        let step = self.answers.len();
        let yes_no = ["", "y", "n", "yes", "no"].iter().any(|w| text.eq_ignore_ascii_case(w));
        let ping = self.kind == DialogKind::Ping;
        if step == 0 && !(text.is_empty() || text.eq_ignore_ascii_case("ip") || text.eq_ignore_ascii_case("ipv6")) {
            return Err("% The protocol is ip or ipv6".into());
        }
        if step == 1 && text.is_empty() {
            return Err("% A target address or name is needed (type `abort` to give up)".into());
        }
        match (self.kind, step) {
            (DialogKind::Ping, 2) => number(&text, 1, 1000, "The repeat count")?,
            (DialogKind::Ping, 3) => number(&text, 36, 18_024, "The datagram size")?,
            (DialogKind::Ping, 4) | (DialogKind::Traceroute, 3) => number(&text, 0, 3_600, "The timeout")?,
            (DialogKind::Ping, 7) => number(&text, 0, 255, "The type of service")?,
            (DialogKind::Ping, 11) => number(&text, 36, 18_024, "The size")?,
            (DialogKind::Ping, 12) => {
                number(&text, 36, 18_024, "The size")?;
                let low = self.answer(11).parse::<u64>().unwrap_or(36);
                if text.parse::<u64>().is_ok_and(|n| n < low) {
                    return Err(format!("% The maximum size cannot be below the minimum ({low})"));
                }
            }
            (DialogKind::Ping, 13) => number(&text, 1, 1_000, "The interval")?,
            (DialogKind::Traceroute, 2) => number(&text, 1, 64, "The number of hops")?,
            _ => {}
        }
        if ping && matches!(step, 5 | 8 | 10) && !yes_no {
            return Err("% Answer yes or no".into());
        }
        self.answers.push(text);
        Ok(())
    }

    /// Runs the command with the answers.
    pub(super) fn finish(&self, c: &mut Ctx) -> R {
        let (target, said) = c.name_to_address(self.answer(1))?;
        let or = |i: usize, default: &str| if self.answer(i).is_empty() { default.to_string() } else { self.answer(i).to_string() };
        if self.kind == DialogKind::Traceroute {
            let hops: u8 = or(2, "30").parse().unwrap_or(30);
            let timeout: u64 = or(3, "2").parse().unwrap_or(2);
            let out = c.traceroute_with(&target, hops, timeout.max(1).saturating_mul(1_000_000))?;
            return Ok(format!("{said}{out}"));
        }
        let extended = yes(self.answer(5));
        let mut options = format!("repeat {} timeout {}", or(2, "5"), or(4, "2"));
        if extended {
            if !self.answer(6).is_empty() {
                options.push_str(&format!(" source {}", self.answer(6).replace(' ', "")));
            }
            if yes(self.answer(8)) {
                options.push_str(" df-bit");
            }
        }
        let sizes: Vec<u64> = if extended && yes(self.answer(10)) {
            let low: u64 = or(11, "36").parse().unwrap_or(36);
            let high: u64 = or(12, "100").parse().unwrap_or(100);
            let step: usize = or(13, "18").parse().unwrap_or(18).max(1);
            (low..=high.max(low)).step_by(step).take(64).collect()
        } else {
            vec![or(3, "100").parse().unwrap_or(100)]
        };
        let mut out = String::new();
        for size in sizes {
            let part = c.ping_extended(&target, &format!("{options} size {size}"))?;
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&part);
        }
        let tos = self.answer(7);
        if extended && ((!tos.is_empty() && tos != "0") || !self.answer(9).is_empty()) {
            out.push_str("\nNote: the type of service and the data pattern are read but do not change the echo requests here.");
        }
        Ok(format!("{said}{out}"))
    }
}
