//! The VPN Client app of an end device's Desktop: the settings it keeps (the concentrator's
//! address, the group and its key, the account). The connection itself is dynamic state of the
//! simulator (its keys are never saved); see `netlabcraft_sim::ravpn`.

use serde::{Deserialize, Serialize};

/// Longest group name, key, user name, password or address text.
pub const MAX_VPN_TEXT: usize = 64;

/// What a PC's VPN Client was last given.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct VpnClientSettings {
    /// The VPN concentrator's address (the router's outside address), as typed.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub host: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub group: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub key: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub username: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub password: String,
}

impl VpnClientSettings {
    pub fn is_default(&self) -> bool {
        *self == VpnClientSettings::default()
    }

    /// Cuts every field to [`MAX_VPN_TEXT`] characters (hostile files).
    pub fn sanitize(&mut self) {
        for f in [&mut self.host, &mut self.group, &mut self.key, &mut self.username, &mut self.password] {
            if f.chars().count() > MAX_VPN_TEXT {
                *f = f.chars().take(MAX_VPN_TEXT).collect();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_are_left_out_of_a_file_and_long_text_is_cut() {
        assert!(VpnClientSettings::default().is_default());
        let mut s = VpnClientSettings { group: "g".repeat(500), username: "ann".into(), ..VpnClientSettings::default() };
        assert!(!s.is_default());
        s.sanitize();
        assert_eq!(s.group.chars().count(), MAX_VPN_TEXT);
        assert_eq!(s.username, "ann");
    }
}
