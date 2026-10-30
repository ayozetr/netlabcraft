//! Interface names and their abbreviations: `GigabitEthernet0/1`, `g0/1`, `gi 0/1`,
//! `Fa0/1.10`. A name is a type word followed by slot/port numbers and an optional `.sub`.

/// The interface types PacketCraft devices have, with their full spelling.
pub const TYPES: [&str; 6] = ["GigabitEthernet", "FastEthernet", "Ethernet", "Serial", "Wireless", "Internet"];

/// A parsed interface name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IfName {
    /// Full type word, from [`TYPES`].
    pub kind: &'static str,
    /// Slot/port numbers (`0/1` → `[0, 1]`).
    pub numbers: Vec<u32>,
    /// Subinterface number.
    pub sub: Option<u32>,
}

impl IfName {
    pub fn same_port(&self, other: &IfName) -> bool {
        self.kind == other.kind && self.numbers == other.numbers
    }

    /// The physical part of the name, full spelling.
    pub fn port_name(&self) -> String {
        format!("{}{}", self.kind, self.numbers.iter().map(u32::to_string).collect::<Vec<_>>().join("/"))
    }
}

impl std::fmt::Display for IfName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.port_name())?;
        if let Some(s) = self.sub {
            write!(f, ".{s}")?;
        }
        Ok(())
    }
}

/// Parses a name in any accepted spelling; whitespace between the type and numbers is allowed.
/// Type words match by case-insensitive prefix (`g`, `gi`, `fa`, `f`); `e` alone means Ethernet.
pub fn parse(input: &str) -> Option<IfName> {
    let s: String = input.chars().filter(|c| !c.is_whitespace()).collect();
    let split = s.find(|c: char| c.is_ascii_digit())?;
    let (word, nums) = s.split_at(split);
    if word.is_empty() {
        return None;
    }
    let lower = word.to_ascii_lowercase();
    let kind = if lower == "e" || lower == "eth" {
        "Ethernet"
    } else {
        let mut hits = TYPES.iter().filter(|t| t.to_ascii_lowercase().starts_with(&lower));
        let first = *hits.next()?;
        if hits.next().is_some() {
            return None;
        }
        first
    };
    let (path, sub) = match nums.split_once('.') {
        Some((p, s)) => (p, Some(s.parse::<u32>().ok()?)),
        None => (nums, None),
    };
    let numbers = path.split('/').map(|n| n.parse::<u32>().ok()).collect::<Option<Vec<_>>>()?;
    if numbers.is_empty() || numbers.len() > 3 {
        return None;
    }
    Some(IfName { kind, numbers, sub })
}

/// Whether a port name is a radio (Wi-Fi) interface.
pub fn is_wireless(full: &str) -> bool {
    full.starts_with("Wireless")
}

/// Whether a port name is a serial interface.
pub fn is_serial(full: &str) -> bool {
    full.starts_with("Serial")
}

/// The short form shown in tables (`Gi0/1`, `Fa0/3.10`).
pub fn short(full: &str) -> String {
    match parse(full) {
        Some(n) => {
            let abbr = match n.kind {
                "GigabitEthernet" => "Gi",
                "FastEthernet" => "Fa",
                "Serial" => "Se",
                "Wireless" => "Wl",
                "Internet" => "In",
                _ => "Et",
            };
            let mut s = format!("{abbr}{}", n.numbers.iter().map(u32::to_string).collect::<Vec<_>>().join("/"));
            if let Some(sub) = n.sub {
                s.push_str(&format!(".{sub}"));
            }
            s
        }
        None => full.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abbreviations() {
        let g = parse("g0/1").unwrap();
        assert_eq!(g.to_string(), "GigabitEthernet0/1");
        assert_eq!(parse("GI 0/1").unwrap(), g);
        assert_eq!(parse("fa0/24.10").unwrap().to_string(), "FastEthernet0/24.10");
        assert_eq!(parse("FastEthernet0").unwrap().numbers, vec![0]);
        assert!(parse("x0/1").is_none());
        assert!(parse("0/1").is_none());
        assert!(parse("g0/").is_none());
        assert!(parse("g0/1.").is_none());
        assert!(parse("g0/1/2/3/4").is_none());
        assert!(parse("g99999999999").is_none());
        assert_eq!(short("GigabitEthernet0/0.20"), "Gi0/0.20");
        assert_eq!(short("FastEthernet0/3"), "Fa0/3");
    }
}
