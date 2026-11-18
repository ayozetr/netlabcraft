//! The device catalogue: generic models (our own names), each a kind plus a port layout.

use crate::DeviceKind;
use crate::cable::Medium;

/// A run of ports: `prefix` followed by numbers `first..=last` (`FastEthernet0/` 1–24).
#[derive(Debug, Clone, Copy)]
pub struct PortRun {
    pub prefix: &'static str,
    pub first: u32,
    pub last: u32,
    /// What the ports take; `None`: what their name says (serial, radio, else copper).
    pub medium: Option<Medium>,
}

#[derive(Debug, Clone, Copy)]
pub struct Model {
    /// Stable id, saved in lab files (`Router-3`).
    pub id: &'static str,
    pub kind: DeviceKind,
    /// What the workspace prints under the icon: our own descriptive name (ports at a glance:
    /// `GE` Gigabit Ethernet, `FE` Fast Ethernet, `S` serial). docs/model-equivalents.md maps the
    /// models courses name to these.
    pub label: &'static str,
    pub description: &'static str,
    pub ports: &'static [PortRun],
}

/// The groups of the device box (its subcategories): where a model's tile is listed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Routers,
    Switches,
    Hubs,
    /// Access points, controllers, home routers and the cell tower and central office.
    Wireless,
    /// Firewalls.
    Security,
    /// The clouds and the modems.
    Wan,
    EndDevices,
    Home,
    SmartCity,
    Industrial,
    PowerGrid,
    Boards,
    Actuators,
    Sensors,
}

impl Model {
    /// The device box group the model is listed in.
    pub fn group(&self) -> Group {
        match self.id {
            "AccessPoint" | "LAP" | "WLC" | "Home-Router" | "Home-Gateway" | CELL_TOWER | CENTRAL_OFFICE => Group::Wireless,
            "Firewall-5" | "Firewall-3" => Group::Security,
            "Board-SBC" | "Board-MCU" | "IoT-Thing" => Group::Boards,
            "IoT-Streetlight" | "IoT-Traffic" | "IoT-Parking" | "IoT-Drain" | "IoT-Air" => Group::SmartCity,
            "IoT-Pressure" | "IoT-Beacon" | "IoT-CO2" | "IoT-Fire" | "IoT-FireSprinkler" | "IoT-Car" | "IoT-RFIDCard" | "IoT-RFIDReader"
            | "IoT-LED" | "IoT-Trip" | "IoT-WindDet" => Group::SmartCity,
            "PLC" | "Data-Historian" | "HMI-Panel" | "IoT-Blower" | "IoT-Signal" => Group::Industrial,
            "IoT-Battery" | "IoT-Meter" | "IoT-Solar" | "IoT-Wind" => Group::PowerGrid,
            "Part-LED" | "Part-Buzzer" | "Part-Motor" | "Part-Relay" | "Part-LCD" => Group::Actuators,
            "Part-AirCooler"
            | "Part-Alarm"
            | "Part-CeilingSprinkler"
            | "Part-FloorSprinkler"
            | "Part-DimLED"
            | "Part-HeatingElement"
            | "Part-RGBLED"
            | "Part-Servo"
            | "Part-Speaker" => Group::Actuators,
            "Part-Button" | "Part-Pot" | "Part-Ultrasonic" | "Part-Light" | "Part-Switch" => Group::Sensors,
            "Part-Flex" | "Part-EnvSensor" | "Part-Generic" | "Part-Humidity" | "Part-Humiture" | "Part-Membrane" | "Part-Metal" | "Part-Motion"
            | "Part-LatchSwitch" | "Part-Rocker" | "Part-PushToggle" | "Part-Smoke" | "Part-Temp" | "Part-Trip" | "Part-WaterDetector"
            | "Part-WaterLevel" | "Part-Wind" => Group::Sensors,
            _ if self.wan() => Group::Wan,
            _ => match self.kind {
                DeviceKind::Router => Group::Routers,
                DeviceKind::Switch => Group::Switches,
                DeviceKind::Hub => Group::Hubs,
                DeviceKind::Iot => Group::Home,
                _ => Group::EndDevices,
            },
        }
    }
}

impl Model {
    /// The other groups the model is also listed in (a model is one model, shown in more than one
    /// subgroup of the device box where the reference app lists the same thing in several): the
    /// smart LED is a city, an industrial and an actuator thing; the temperature, humidity and
    /// carbon monoxide monitors are city things too.
    pub fn extra_groups(&self) -> &'static [Group] {
        match self.id {
            "IoT-LED" => &[Group::Industrial, Group::Actuators],
            "IoT-Sensor" | "IoT-Humidity" | "IoT-CO" => &[Group::SmartCity],
            _ => &[],
        }
    }

    /// Every group the model is listed in: its own first, then the extra ones.
    pub fn groups(&self) -> Vec<Group> {
        std::iter::once(self.group()).chain(self.extra_groups().iter().copied()).collect()
    }

    /// A multilayer switch: can route (`ip routing`) and have routed ports (`no switchport`).
    pub fn multilayer(&self) -> bool {
        self.id.starts_with("Switch-ML") || self.home_router()
    }

    /// A home wireless router: a multilayer switch with an Internet port, four LAN ports and a
    /// radio, set up at the factory to route, translate and hand out addresses.
    pub fn home_router(&self) -> bool {
        matches!(self.id, "Home-Router" | "Home-Gateway")
    }
}

/// The cell tower and central office model ids.
pub const CELL_TOWER: &str = "Cell-Tower";
pub const CENTRAL_OFFICE: &str = "Central-Office";

const fn run(prefix: &'static str, first: u32, last: u32) -> PortRun {
    PortRun { prefix, first, last, medium: None }
}

/// Uplink ports with two sockets, copper and fiber (SFP): either cable fits.
const fn combo(prefix: &'static str, first: u32, last: u32) -> PortRun {
    PortRun { prefix, first, last, medium: Some(Medium::Combo) }
}

/// Fiber-only ports (an SFP cage with no copper socket).
const fn fiber(prefix: &'static str, first: u32, last: u32) -> PortRun {
    PortRun { prefix, first, last, medium: Some(Medium::Fiber) }
}

pub static MODELS: &[Model] = &[
    Model {
        id: "Router-2",
        kind: DeviceKind::Router,
        label: "Router 2GE",
        description: "Router with 2 Gigabit Ethernet ports",
        ports: &[run("GigabitEthernet0/", 0, 1)],
    },
    Model {
        id: "Router-3",
        kind: DeviceKind::Router,
        label: "Router 3GE",
        description: "Router with 3 Gigabit Ethernet ports",
        ports: &[run("GigabitEthernet0/", 0, 2)],
    },
    Model {
        id: "Router-4",
        kind: DeviceKind::Router,
        label: "Router 4GE",
        description: "Router with 4 Gigabit Ethernet ports",
        ports: &[run("GigabitEthernet0/", 0, 3)],
    },
    Model {
        id: "Router-WAN",
        kind: DeviceKind::Router,
        label: "Router 2GE+2S",
        description: "Router with 2 Gigabit Ethernet ports and 2 serial WAN ports",
        ports: &[run("GigabitEthernet0/", 0, 1), run("Serial0/0/", 0, 1)],
    },
    Model {
        id: "Router-FE2",
        kind: DeviceKind::Router,
        label: "Router 2FE",
        description: "Router with 2 Fast Ethernet ports",
        ports: &[run("FastEthernet0/", 0, 1)],
    },
    Model {
        id: "Router-FE2S",
        kind: DeviceKind::Router,
        label: "Router 2FE+2S",
        description: "Router with 2 Fast Ethernet ports and 2 serial WAN ports",
        ports: &[run("FastEthernet0/", 0, 1), run("Serial0/0/", 0, 1)],
    },
    Model {
        id: "Access-Server",
        kind: DeviceKind::Router,
        label: "Access Server",
        description: "Router that answers dial-up calls: 2 Fast Ethernet ports and 4 modem lines (Phone0-3) that take calls from the telephone network and run PPP with a user name and password",
        ports: &[run("FastEthernet0/", 0, 1), run("Phone", 0, 3)],
    },
    Model {
        id: "Router-WAN3",
        kind: DeviceKind::Router,
        label: "Router 3GE+2S",
        description: "Router with 3 Gigabit Ethernet ports and 2 serial WAN ports",
        ports: &[run("GigabitEthernet0/", 0, 2), run("Serial0/0/", 0, 1)],
    },
    Model {
        id: "Router-M2",
        kind: DeviceKind::Router,
        label: "Modular Router 2GE",
        description: "Modular router with 2 Gigabit Ethernet ports and 2 empty module bays (serial and Gigabit cards, fitted while powered off)",
        ports: &[run("GigabitEthernet0/", 0, 1)],
    },
    Model {
        id: "Router-MF2",
        kind: DeviceKind::Router,
        label: "Modular Router 2FE",
        description: "Modular router with 2 Fast Ethernet ports and 2 empty module bays (serial, Gigabit and Fast Ethernet cards, fitted while powered off)",
        ports: &[run("FastEthernet0/", 0, 1)],
    },
    Model {
        id: "Router-M3",
        kind: DeviceKind::Router,
        label: "Modular Router 3GE",
        description: "Modular router with 3 Gigabit Ethernet ports and 4 empty module bays (serial and Gigabit cards, fitted while powered off)",
        ports: &[run("GigabitEthernet0/", 0, 2)],
    },
    Model {
        id: "Firewall-5",
        kind: DeviceKind::Router,
        label: "Firewall 5GE",
        description: "Stateful firewall with 5 Gigabit Ethernet ports: named interfaces with security levels, access rules, object NAT and a DHCP server",
        ports: &[run("GigabitEthernet1/", 1, 5)],
    },
    Model {
        id: "Switch-8",
        kind: DeviceKind::Switch,
        label: "Switch 8FE",
        description: "Switch with 8 Fast Ethernet + 1 Gigabit Ethernet ports (the Gigabit uplink takes copper or fiber)",
        ports: &[run("FastEthernet0/", 1, 8), combo("GigabitEthernet0/", 1, 1)],
    },
    Model {
        id: "Switch-24",
        kind: DeviceKind::Switch,
        label: "Switch 24FE",
        description: "Switch with 24 Fast Ethernet + 2 Gigabit Ethernet ports (the Gigabit uplinks take copper or fiber)",
        ports: &[run("FastEthernet0/", 1, 24), combo("GigabitEthernet0/", 1, 2)],
    },
    Model {
        id: "Switch-ML-24",
        kind: DeviceKind::Switch,
        label: "L3 Switch 24",
        description: "Multilayer switch with 24 Fast Ethernet + 2 Gigabit Ethernet ports (copper or fiber): switches, and routes with `ip routing`",
        ports: &[run("FastEthernet0/", 1, 24), combo("GigabitEthernet0/", 1, 2)],
    },
    Model {
        id: "Switch-48",
        kind: DeviceKind::Switch,
        label: "Switch 48FE",
        description: "Switch with 48 Fast Ethernet + 2 Gigabit Ethernet ports (the Gigabit uplinks take copper or fiber)",
        ports: &[run("FastEthernet0/", 1, 48), combo("GigabitEthernet0/", 1, 2)],
    },
    Model {
        id: "Switch-24-SFP",
        kind: DeviceKind::Switch,
        label: "Switch 24FE+2SFP",
        description: "Switch with 24 Fast Ethernet ports and 2 Gigabit fiber (SFP) uplinks",
        ports: &[run("FastEthernet0/", 1, 24), fiber("GigabitEthernet0/", 1, 2)],
    },
    Model {
        id: "Switch-8-PoE",
        kind: DeviceKind::Switch,
        label: "Switch 8FE PoE",
        description: "Switch with 8 Fast Ethernet ports that give power over Ethernet (65 W in all) + 1 Gigabit Ethernet uplink: powers IP phones and lightweight access points",
        ports: &[run("FastEthernet0/", 1, 8), combo("GigabitEthernet0/", 1, 1)],
    },
    Model {
        id: "Switch-24-PoE",
        kind: DeviceKind::Switch,
        label: "Switch 24FE PoE",
        description: "Switch with 24 Fast Ethernet ports that give power over Ethernet (370 W in all) + 2 Gigabit Ethernet uplinks: powers IP phones and lightweight access points",
        ports: &[run("FastEthernet0/", 1, 24), combo("GigabitEthernet0/", 1, 2)],
    },
    Model {
        id: "AccessPoint",
        kind: DeviceKind::Switch,
        label: "Access Point",
        description: "Wi-Fi access point: bridges its radio and its Ethernet port (SSID, WPA2-PSK)",
        ports: &[run("GigabitEthernet", 0, 0), run("Wireless", 0, 0)],
    },
    Model {
        id: "WLC",
        kind: DeviceKind::Switch,
        label: "WLC",
        description: "Wireless LAN controller: holds the WLANs (SSID, WPA2-PSK or WPA2-Enterprise) and pushes them to the lightweight APs that join it",
        ports: &[run("GigabitEthernet0/", 1, 4)],
    },
    Model {
        id: "LAP",
        kind: DeviceKind::Switch,
        label: "Light AP",
        description: "Lightweight access point: joins a wireless LAN controller over its Ethernet port and offers the controller's WLANs",
        ports: &[run("GigabitEthernet", 0, 0), run("Wireless", 0, 0)],
    },
    Model {
        id: "Home-Router",
        kind: DeviceKind::Switch,
        label: "Home Router",
        description: "Home wireless router: Internet port, 4 LAN ports and Wi-Fi, with DHCP and NAT set up",
        ports: &[run("Internet", 0, 0), run("Ethernet", 1, 4), run("Wireless", 0, 0)],
    },
    Model {
        id: "Home-Gateway",
        kind: DeviceKind::Switch,
        label: "Home Gateway",
        description: "Home IoT gateway: Internet port, 4 LAN ports and Wi-Fi with DHCP and NAT like a home router, plus a built-in IoT service that smart things register with",
        ports: &[run("Internet", 0, 0), run("Ethernet", 1, 4), run("Wireless", 0, 0)],
    },
    Model {
        id: "Cloud",
        kind: DeviceKind::Hub,
        label: "Cloud",
        description: "A provider's network (WAN): 2 Ethernet, 2 coaxial and 2 phone (DSL) ports, each joined to the port it is mapped to (by default coaxial to phone: a cable modem reaches a DSL modem)",
        ports: &[run("Ethernet", 0, 1), run("Coaxial", 0, 1), run("Phone", 0, 1)],
    },
    Model {
        id: "PSTN",
        kind: DeviceKind::Hub,
        label: "Telephone Cloud",
        description: "The telephone network (PSTN): 8 phone ports, each with a telephone number you give it; a call joins the port that dials to the port whose number it dialled until one hangs up",
        ports: &[run("Phone", 0, 7)],
    },
    Model {
        id: "DSL-Modem",
        kind: DeviceKind::Hub,
        label: "DSL Modem",
        description: "DSL modem: bridges its phone line (Phone0, RJ-11) to its Ethernet port (Ethernet1)",
        ports: &[run("Phone", 0, 0), run("Ethernet", 1, 1)],
    },
    Model {
        id: "Cable-Modem",
        kind: DeviceKind::Hub,
        label: "Cable Modem",
        description: "Cable modem: bridges its coaxial port (Coaxial0) to its Ethernet port (Ethernet1)",
        ports: &[run("Coaxial", 0, 0), run("Ethernet", 1, 1)],
    },
    Model {
        id: "Cell-Tower",
        kind: DeviceKind::Hub,
        label: "Cell Tower",
        description: "Cell tower: a radio with six sectors (Cellular0-5) that mobile devices in range attach to, and one backhaul port (Ethernet0) cabled to the provider network",
        ports: &[run("Ethernet", 0, 0), run("Cellular", 0, 5)],
    },
    Model {
        id: "Central-Office",
        kind: DeviceKind::Router,
        label: "Central Office Server",
        description: "The provider's data service: admits mobile devices that attach through a cell tower, gives each an address from its pool and routes them to the lab (3 Gigabit Ethernet ports)",
        ports: &[run("GigabitEthernet0/", 0, 2)],
    },
    Model {
        id: "Hub-8",
        kind: DeviceKind::Hub,
        label: "Hub 8",
        description: "Hub: repeats every frame out of its other 8 ports",
        ports: &[run("Ethernet", 0, 7)],
    },
    Model { id: "PC", kind: DeviceKind::Pc, label: "PC", description: "Desktop PC with one Fast Ethernet port", ports: &[run("FastEthernet", 0, 0)] },
    Model {
        id: "PC-Modem",
        kind: DeviceKind::Pc,
        label: "Dial-up PC",
        description: "Desktop PC with a modem and no network card: its phone port (Phone0) dials an access server through the telephone network, from the Dial-up app",
        ports: &[run("Phone", 0, 0)],
    },
    Model {
        id: "Laptop",
        kind: DeviceKind::Laptop,
        label: "Laptop",
        description: "Laptop with one Fast Ethernet port",
        ports: &[run("FastEthernet", 0, 0)],
    },
    Model {
        id: "Laptop-WiFi",
        kind: DeviceKind::Laptop,
        label: "Wi-Fi Laptop",
        description: "Laptop with a Wi-Fi card instead of an Ethernet port",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "Tablet",
        kind: DeviceKind::Laptop,
        label: "Tablet",
        description: "Touch-screen tablet with Wi-Fi and a cellular radio: browser, email, command prompt and programming apps; no battery to look after",
        ports: &[run("Wireless", 0, 0), run("Cellular", 0, 0)],
    },
    Model {
        id: "Smartphone",
        kind: DeviceKind::Laptop,
        label: "Smartphone",
        description: "Touch-screen smartphone with Wi-Fi and a cellular radio: browser, email, command prompt and programming apps; no battery to look after",
        ports: &[run("Wireless", 0, 0), run("Cellular", 0, 0)],
    },
    Model {
        id: "Server",
        kind: DeviceKind::Server,
        label: "Server",
        description: "Server with one Gigabit Ethernet port",
        ports: &[run("GigabitEthernet", 0, 0)],
    },
    Model {
        id: "Printer",
        kind: DeviceKind::Printer,
        label: "Printer",
        description: "Network printer with one Fast Ethernet port",
        ports: &[run("FastEthernet", 0, 0)],
    },
    Model {
        id: "Sniffer",
        kind: DeviceKind::Pc,
        label: "Sniffer",
        description: "Sniffer with one Fast Ethernet port: records every frame that reaches its port (all the traffic behind a hub; behind a switch, broadcasts, multicasts and flooded frames, or whatever a monitor session copies to its port) and never answers",
        ports: &[run("FastEthernet", 0, 0)],
    },
    Model {
        id: "IP-Phone",
        kind: DeviceKind::Phone,
        label: "IP Phone",
        description: "IP phone with a built-in switch: FastEthernet0 goes to the network, FastEthernet1 to a computer behind it; takes its address and its call server by DHCP",
        ports: &[run("FastEthernet", 0, 1)],
    },
    Model {
        id: "IoT-Sensor",
        kind: DeviceKind::Iot,
        label: "Smart Sensor",
        description: "Smart temperature sensor with one Fast Ethernet port: reports its reading to an IoT server",
        ports: &[run("FastEthernet", 0, 0)],
    },
    Model {
        id: "IoT-Motion",
        kind: DeviceKind::Iot,
        label: "Motion Detector",
        description: "Smart motion detector with one Fast Ethernet port: reports 1 while something moves in front of it, else 0",
        ports: &[run("FastEthernet", 0, 0)],
    },
    Model {
        id: "IoT-Smoke",
        kind: DeviceKind::Iot,
        label: "Smoke Detector",
        description: "Smart smoke detector with one Fast Ethernet port: reports how much smoke it senses (0-100 %)",
        ports: &[run("FastEthernet", 0, 0)],
    },
    Model {
        id: "IoT-Light",
        kind: DeviceKind::Iot,
        label: "Smart Light",
        description: "Smart lamp with one Fast Ethernet port: the IoT server switches it on and off",
        ports: &[run("FastEthernet", 0, 0)],
    },
    Model {
        id: "IoT-Door",
        kind: DeviceKind::Iot,
        label: "Smart Door Lock",
        description: "Smart door lock with one Fast Ethernet port: the IoT server unlocks it (on) and locks it (off)",
        ports: &[run("FastEthernet", 0, 0)],
    },
    Model {
        id: "IoT-Siren",
        kind: DeviceKind::Iot,
        label: "Siren",
        description: "Smart siren with one Fast Ethernet port: sounds while the IoT server keeps it on",
        ports: &[run("FastEthernet", 0, 0)],
    },
    Model {
        id: "IoT-Webcam",
        kind: DeviceKind::Iot,
        label: "Webcam",
        description: "Smart webcam with one Fast Ethernet port: while on, it takes a still image each time it reports and the IoT server keeps the latest",
        ports: &[run("FastEthernet", 0, 0)],
    },
    Model {
        id: "IoT-Thermostat",
        kind: DeviceKind::Iot,
        label: "Thermostat",
        description: "Wi-Fi thermostat: reports the room temperature to its IoT service, so rules can switch a heater or an air conditioner",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "IoT-Heater",
        kind: DeviceKind::Iot,
        label: "Heater",
        description: "Wi-Fi heater: warms the room while the IoT service keeps it on",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "IoT-AC",
        kind: DeviceKind::Iot,
        label: "Air Conditioner",
        description: "Wi-Fi air conditioner: cools the room while the IoT service keeps it on",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "IoT-Humidity",
        kind: DeviceKind::Iot,
        label: "Humidity Monitor",
        description: "Wi-Fi humidity sensor: reports the air's relative humidity (0-100 %)",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "IoT-Window",
        kind: DeviceKind::Iot,
        label: "Smart Window",
        description: "Wi-Fi window opener: the IoT service opens the window (on) and closes it (off)",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "IoT-Soil",
        kind: DeviceKind::Iot,
        label: "Soil Moisture Sensor",
        description: "Wi-Fi soil moisture sensor: reports how wet the ground is (0-100 %)",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "IoT-Sprinkler",
        kind: DeviceKind::Iot,
        label: "Lawn Sprinkler",
        description: "Wi-Fi lawn sprinkler: waters the garden while the IoT service keeps it on",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "IoT-CO",
        kind: DeviceKind::Iot,
        label: "CO Detector",
        description: "Wi-Fi carbon-monoxide detector: reports the CO level in parts per million",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "IoT-Plug",
        kind: DeviceKind::Iot,
        label: "Smart Plug",
        description: "Wi-Fi smart plug: powers what is plugged into it while the IoT service keeps it on",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "IoT-Garage",
        kind: DeviceKind::Iot,
        label: "Garage Door",
        description: "Wi-Fi garage door opener: the IoT service opens the door (on) and closes it (off)",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "IoT-TV",
        kind: DeviceKind::Iot,
        label: "Smart TV",
        description: "Wi-Fi smart TV: registers with the IoT service, which switches it on (playing) and off (standby)",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "IoT-TVE",
        kind: DeviceKind::Iot,
        label: "Wired Smart TV",
        description: "Smart TV with one Fast Ethernet port: registers with an IoT server, which switches it on (playing) and off (standby)",
        ports: &[run("FastEthernet", 0, 0)],
    },
    Model {
        id: "IoT-Speaker",
        kind: DeviceKind::Iot,
        label: "Smart Speaker",
        description: "Wi-Fi smart speaker: plays while the IoT service keeps it on",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "IoT-Water",
        kind: DeviceKind::Iot,
        label: "Water Level Sensor",
        description: "Wi-Fi water level sensor: reports how full a tank is (0-100 %)",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "IoT-Appliance",
        kind: DeviceKind::Iot,
        label: "Appliance",
        description: "Wi-Fi appliance: any mains appliance with a power state, switched on and off by the IoT service",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "IoT-BTSpeaker",
        kind: DeviceKind::Iot,
        label: "Bluetooth Speaker",
        description: "Wi-Fi speaker with Bluetooth: plays while the IoT service keeps it on, or while a music player within 60 m is playing and paired with it",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "IoT-CeilingFan",
        kind: DeviceKind::Iot,
        label: "Ceiling Fan",
        description: "Wi-Fi ceiling fan: while on it stirs the air and cools the room toward the outside temperature",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "IoT-Furnace",
        kind: DeviceKind::Iot,
        label: "Furnace",
        description: "Wi-Fi furnace: heats the room faster than a heater while the IoT service keeps it on",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "IoT-Humidifier",
        kind: DeviceKind::Iot,
        label: "Humidifier",
        description: "Wi-Fi humidifier: raises the room's humidity while the IoT service keeps it on",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "IoT-Humiture",
        kind: DeviceKind::Iot,
        label: "Humiture Monitor",
        description: "Wi-Fi monitor that reports the temperature (its reading) and the relative humidity (a second reading)",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "IoT-Player",
        kind: DeviceKind::Iot,
        label: "Portable Music Player",
        description: "Wi-Fi music player: plays a tone while on, quietly alone and louder through a Bluetooth speaker within 60 m",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "IoT-SoundDet",
        kind: DeviceKind::Iot,
        label: "Sound Frequency Detector",
        description: "Wi-Fi sound detector: reports the pitch of the loudest sound in Hz (its reading) and its volume in dB (a second reading)",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "IoT-Pressure",
        kind: DeviceKind::Iot,
        label: "Atmospheric Pressure Monitor",
        description: "Wi-Fi barometer: reports the atmospheric pressure in kPa (the room is at sea-level pressure; an environment can set another)",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "IoT-Beacon",
        kind: DeviceKind::Iot,
        label: "Beacon",
        description: "Wi-Fi beacon: announces its id (its reading) to the smart things within 100 m, and reports how many hear it as a second reading",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "IoT-CO2",
        kind: DeviceKind::Iot,
        label: "Carbon Dioxide Detector",
        description: "Wi-Fi carbon dioxide detector: reports the CO2 level in parts per million (fresh air is about 420)",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "IoT-Fire",
        kind: DeviceKind::Iot,
        label: "Fire Monitor",
        description: "Wi-Fi fire monitor: reports how much of the room is in flames (0-100 %)",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "IoT-FireSprinkler",
        kind: DeviceKind::Iot,
        label: "Fire Sprinkler",
        description: "Wi-Fi fire sprinkler: sprays on its own while the room is in flames, and when the IoT service switches it on; the flames go down while it sprays",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "IoT-Car",
        kind: DeviceKind::Iot,
        label: "Old Car",
        description: "A car you can move on the map: while its engine is on (the IoT service switches it) it adds carbon monoxide and carbon dioxide to the room, and a trip sensor sees it pass",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "IoT-RFIDCard",
        kind: DeviceKind::Iot,
        label: "RFID Card",
        description: "Wi-Fi tag that announces its card id (its reading, set by hand); an RFID reader within 30 m reads it",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "IoT-RFIDReader",
        kind: DeviceKind::Iot,
        label: "RFID Reader",
        description: "Wi-Fi RFID reader: reports the id of the nearest RFID card within 30 m, or 0 when there is none",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "IoT-LED",
        kind: DeviceKind::Iot,
        label: "Smart LED",
        description: "Wi-Fi dimmable colour LED: shines while the IoT service keeps it on, at the brightness and colour set in its IoT tab",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "IoT-Trip",
        kind: DeviceKind::Iot,
        label: "Trip Sensor",
        description: "Wi-Fi tripwire: reports 1 while something that moves (a car, a card, a phone, a tablet, a laptop) is within 50 m, else 0",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "IoT-WindDet",
        kind: DeviceKind::Iot,
        label: "Wind Detector",
        description: "Wi-Fi anemometer: reports the wind speed in kph (the room is calm; an environment can set a wind)",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "IoT-Signal",
        kind: DeviceKind::Iot,
        label: "Signal Generator",
        description: "Wi-Fi signal generator: outputs a level (its reading, -1000 to 1000) at a frequency (a second reading, 0 to 20000 Hz) that other things read through the IoT service",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "Board-SBC",
        kind: DeviceKind::Iot,
        label: "Board SBC",
        description: "Single-board computer with one Fast Ethernet port, 8 digital and 4 analog pins: runs a BoardScript program that reads and switches the smart things wired to its pins",
        ports: &[run("FastEthernet", 0, 0)],
    },
    Model {
        id: "Board-MCU",
        kind: DeviceKind::Iot,
        label: "Board MCU",
        description: "Microcontroller board with Wi-Fi, 6 digital and 2 analog pins: runs a BoardScript program that reads and switches the smart things wired to its pins",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "IoT-Device",
        kind: DeviceKind::Iot,
        label: "Smart Device",
        description: "Smart fan with one Fast Ethernet port: registers with an IoT server, which switches it on and off",
        ports: &[run("FastEthernet", 0, 0)],
    },
    Model {
        id: "Repeater",
        kind: DeviceKind::Hub,
        label: "Repeater",
        description: "Repeater: regenerates every frame heard on one of its 2 Ethernet ports onto the other",
        ports: &[run("Ethernet", 0, 1)],
    },
    Model {
        id: "Coax-Splitter",
        kind: DeviceKind::Hub,
        label: "Coax Splitter",
        description: "Coaxial splitter: repeats every frame between its 3 coaxial ports (the coaxial cable of a cable modem line)",
        ports: &[run("Coaxial", 0, 2)],
    },
    Model {
        id: "Bridge-2",
        kind: DeviceKind::Switch,
        label: "Bridge 2FE",
        description: "Bridge with 2 Fast Ethernet ports: learns the addresses on each side and passes only the frames that must cross",
        ports: &[run("FastEthernet0/", 1, 2)],
    },
    Model {
        id: "Switch-IND-8",
        kind: DeviceKind::Switch,
        label: "Industrial Switch 8FE",
        description: "Industrial switch with 8 Fast Ethernet + 2 Gigabit Ethernet ports (the Gigabit uplinks take copper or fiber): switches like the other switches",
        ports: &[run("FastEthernet0/", 1, 8), combo("GigabitEthernet0/", 1, 2)],
    },
    Model {
        id: "Router-IND",
        kind: DeviceKind::Router,
        label: "Industrial Router 2GE+2S",
        description: "Industrial router with 2 Gigabit Ethernet ports and 2 serial WAN ports: routes like the other routers",
        ports: &[run("GigabitEthernet0/", 0, 1), run("Serial0/0/", 0, 1)],
    },
    Model {
        id: "Firewall-3",
        kind: DeviceKind::Router,
        label: "Firewall 3GE",
        description: "Stateful firewall with 3 Gigabit Ethernet ports: named interfaces with security levels, access rules, object NAT and a DHCP server",
        ports: &[run("GigabitEthernet1/", 1, 3)],
    },
    Model {
        id: "Net-Controller",
        kind: DeviceKind::Server,
        label: "Network Controller",
        description: "Network controller with one Gigabit Ethernet port: a server whose Programming tab holds the controller that discovers routers and pushes configuration templates",
        ports: &[run("GigabitEthernet", 0, 0)],
    },
    Model {
        id: "VoIP-Adapter",
        kind: DeviceKind::Phone,
        label: "VoIP Adapter",
        description: "VoIP adapter for analog phones: its Ethernet port (FastEthernet0) goes to the network and takes an address and a call server by DHCP like an IP phone; its phone port (Phone0, RJ-11) takes an analog phone",
        ports: &[run("FastEthernet", 0, 0), run("Phone", 0, 0)],
    },
    Model {
        id: "Analog-Phone",
        kind: DeviceKind::Phone,
        label: "Analog Phone",
        description: "Analog telephone with one phone port (Phone0, RJ-11): plugged into a VoIP adapter it rings and calls like an IP phone behind it",
        ports: &[run("Phone", 0, 0)],
    },
    Model {
        id: "Generic-Wired",
        kind: DeviceKind::Pc,
        label: "Generic Wired",
        description: "Generic wired end device with one Fast Ethernet port: behaves as a PC",
        ports: &[run("FastEthernet", 0, 0)],
    },
    Model {
        id: "Generic-Wireless",
        kind: DeviceKind::Laptop,
        label: "Generic Wireless",
        description: "Generic wireless end device with a Wi-Fi card and no Ethernet port: behaves as a Wi-Fi laptop",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "IoT-Streetlight",
        kind: DeviceKind::Iot,
        label: "Street Light",
        description: "Wi-Fi street light: lights the road while the IoT service keeps it on",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "IoT-Traffic",
        kind: DeviceKind::Iot,
        label: "Traffic Light",
        description: "Wi-Fi traffic light: shows green while the IoT service keeps it on, red otherwise",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "IoT-Parking",
        kind: DeviceKind::Iot,
        label: "Parking Sensor",
        description: "Wi-Fi parking sensor: reports 1 while a car stands on the space, else 0",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "IoT-Drain",
        kind: DeviceKind::Iot,
        label: "Water Drain Monitor",
        description: "Wi-Fi water drain monitor: reports how full a storm drain is (0-100 %)",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "IoT-Air",
        kind: DeviceKind::Iot,
        label: "Air Quality Sensor",
        description: "Wi-Fi air quality sensor: reports an air quality index (0-500, higher is worse)",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "IoT-Blower",
        kind: DeviceKind::Iot,
        label: "Blower Fan",
        description: "Blower fan with one Fast Ethernet port: the IoT server switches it on and off",
        ports: &[run("FastEthernet", 0, 0)],
    },
    Model {
        id: "PLC",
        kind: DeviceKind::Iot,
        label: "PLC",
        description: "Programmable logic controller with one Fast Ethernet port, 8 digital and 4 analog pins: runs a BoardScript program that reads and switches the things wired to its pins",
        ports: &[run("FastEthernet", 0, 0)],
    },
    Model {
        id: "Data-Historian",
        kind: DeviceKind::Server,
        label: "Data Historian",
        description: "Data historian with one Gigabit Ethernet port: a server that runs the IoT service; it lists the latest reading of each thing but keeps no history",
        ports: &[run("GigabitEthernet", 0, 0)],
    },
    Model {
        id: "HMI-Panel",
        kind: DeviceKind::Pc,
        label: "HMI Panel",
        description: "Operator panel (HMI) with one Fast Ethernet port: a PC with browser, command prompt and the IoT monitor to watch and switch things",
        ports: &[run("FastEthernet", 0, 0)],
    },
    Model {
        id: "IoT-Battery",
        kind: DeviceKind::Iot,
        label: "Battery",
        description: "Wi-Fi battery bank: reports its charge (0-100 %)",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "IoT-Meter",
        kind: DeviceKind::Iot,
        label: "Power Meter",
        description: "Wi-Fi power meter: reports the power drawn, in watts",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "IoT-Solar",
        kind: DeviceKind::Iot,
        label: "Solar Panel",
        description: "Wi-Fi solar panel: reports the power it produces, in watts",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "IoT-Wind",
        kind: DeviceKind::Iot,
        label: "Wind Turbine",
        description: "Wi-Fi wind turbine: reports how fast it turns, in revolutions per minute",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "IoT-Thing",
        kind: DeviceKind::Iot,
        label: "Thing",
        description: "Generic thing for a board's pin: a reading you set by hand (-1000 to 1000)",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "Part-LED",
        kind: DeviceKind::Iot,
        label: "LED",
        description: "LED wired to a board's pin with an IoT custom cable: lit while the pin is high",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "Part-Buzzer",
        kind: DeviceKind::Iot,
        label: "Buzzer",
        description: "Buzzer wired to a board's pin with an IoT custom cable: sounds while the pin is high",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "Part-Motor",
        kind: DeviceKind::Iot,
        label: "Motor",
        description: "Motor wired to a board's pin with an IoT custom cable: runs while the pin is high",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "Part-Relay",
        kind: DeviceKind::Iot,
        label: "Relay",
        description: "Relay wired to a board's pin with an IoT custom cable: closed while the pin is high",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "Part-LCD",
        kind: DeviceKind::Iot,
        label: "LCD",
        description: "LCD wired to a board's pin with an IoT custom cable: shows its text while the pin is high",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "Part-Button",
        kind: DeviceKind::Iot,
        label: "Button",
        description: "Push button wired to a board's pin with an IoT custom cable: reads 1 while pressed, else 0",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "Part-Pot",
        kind: DeviceKind::Iot,
        label: "Potentiometer",
        description: "Potentiometer wired to a board's pin with an IoT custom cable: reads how far the knob is turned (0-100 %)",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "Part-Ultrasonic",
        kind: DeviceKind::Iot,
        label: "Distance Sensor",
        description: "Ultrasonic distance sensor wired to a board's pin with an IoT custom cable: reads the distance to the nearest object, in centimetres",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "Part-Light",
        kind: DeviceKind::Iot,
        label: "Light Sensor",
        description: "Light sensor wired to a board's pin with an IoT custom cable: reads the brightness (0-100 %)",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "Part-Switch",
        kind: DeviceKind::Iot,
        label: "Toggle Switch",
        description: "Toggle switch wired to a board's pin with an IoT custom cable: reads 1 while on, else 0",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "Part-AirCooler",
        kind: DeviceKind::Iot,
        label: "Air Cooler",
        description: "Air cooler wired to a board's pin with an IoT custom cable: cools its environment while the pin is high",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "Part-Alarm",
        kind: DeviceKind::Iot,
        label: "Alarm",
        description: "Alarm wired to a board's pin with an IoT custom cable: sounds its siren and lights up while the pin is high",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "Part-CeilingSprinkler",
        kind: DeviceKind::Iot,
        label: "Ceiling Sprinkler",
        description: "Ceiling sprinkler wired to a board's pin with an IoT custom cable: sprays water from above while the pin is high and puts out flames",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "Part-FloorSprinkler",
        kind: DeviceKind::Iot,
        label: "Floor Sprinkler",
        description: "Floor sprinkler wired to a board's pin with an IoT custom cable: sprays water from the floor while the pin is high and puts out flames",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "Part-DimLED",
        kind: DeviceKind::Iot,
        label: "Dimmable LED",
        description: "Dimmable LED wired to a board's pin with an IoT custom cable: its brightness follows the level written to the pin (0-100)",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "Part-HeatingElement",
        kind: DeviceKind::Iot,
        label: "Heating Element",
        description: "Heating element wired to a board's pin with an IoT custom cable: warms its environment while the pin is high",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "Part-RGBLED",
        kind: DeviceKind::Iot,
        label: "RGB LED",
        description: "RGB LED wired to a board's pin with an IoT custom cable: shows the red, green and blue levels written to the pin as one number (0xRRGGBB)",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "Part-Servo",
        kind: DeviceKind::Iot,
        label: "Servo",
        description: "Servo wired to a board's pin with an IoT custom cable: turns to the angle written to the pin (0-180 degrees)",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "Part-Speaker",
        kind: DeviceKind::Iot,
        label: "Speaker",
        description: "Speaker wired to a board's pin with an IoT custom cable: plays a tone and raises the sound around it while the pin is high",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "Part-Flex",
        kind: DeviceKind::Iot,
        label: "Flex Sensor",
        description: "Flex sensor wired to a board's pin with an IoT custom cable: reads how far it is bent (0-100 %)",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "Part-EnvSensor",
        kind: DeviceKind::Iot,
        label: "Generic Environment Sensor",
        description: "Generic environment sensor wired to a board's pin with an IoT custom cable: reads the environment variable you pick, rounded to a whole number",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "Part-Generic",
        kind: DeviceKind::Iot,
        label: "Generic Sensor",
        description: "Generic sensor wired to a board's pin with an IoT custom cable: reads a raw analog value you set (-1000 to 1000)",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "Part-Humidity",
        kind: DeviceKind::Iot,
        label: "Humidity Sensor",
        description: "Humidity sensor wired to a board's pin with an IoT custom cable: reads the relative humidity of its environment (0-100 %)",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "Part-Humiture",
        kind: DeviceKind::Iot,
        label: "Humiture Sensor",
        description: "Humiture sensor wired to a board's pin with an IoT custom cable: reads the temperature, and the humidity with channel(PIN, 1)",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "Part-Membrane",
        kind: DeviceKind::Iot,
        label: "Membrane Potentiometer",
        description: "Membrane potentiometer wired to a board's pin with an IoT custom cable: reads where a finger presses along its strip (0-100 %)",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "Part-Metal",
        kind: DeviceKind::Iot,
        label: "Metal Sensor",
        description: "Metal sensor wired to a board's pin with an IoT custom cable: reads 1 while a metal object is near, else 0",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "Part-Motion",
        kind: DeviceKind::Iot,
        label: "Motion Sensor",
        description: "Motion sensor wired to a board's pin with an IoT custom cable: reads 1 while something moves in its environment, else 0",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "Part-LatchSwitch",
        kind: DeviceKind::Iot,
        label: "Push Button Toggle Switch",
        description: "Push button toggle switch wired to a board's pin with an IoT custom cable: latches on at one push and off at the next, reads 1 while on",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "Part-Rocker",
        kind: DeviceKind::Iot,
        label: "Rocker Switch",
        description: "Rocker switch wired to a board's pin with an IoT custom cable: reads 1 in its on position and 0 in its off position",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "Part-PushToggle",
        kind: DeviceKind::Iot,
        label: "Toggle Push Button",
        description: "Toggle push button wired to a board's pin with an IoT custom cable: each press flips its state, reads 1 while on",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "Part-Smoke",
        kind: DeviceKind::Iot,
        label: "Smoke Sensor",
        description: "Smoke sensor wired to a board's pin with an IoT custom cable: reads the smoke level of its environment (0-100 %)",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "Part-Temp",
        kind: DeviceKind::Iot,
        label: "Temperature Sensor",
        description: "Temperature sensor wired to a board's pin with an IoT custom cable: reads the temperature of its environment, in degrees",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "Part-Trip",
        kind: DeviceKind::Iot,
        label: "Trip Wire",
        description: "Trip wire wired to a board's pin with an IoT custom cable: reads 1 while something crosses the line, else 0",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "Part-WaterDetector",
        kind: DeviceKind::Iot,
        label: "Water Detector",
        description: "Water detector wired to a board's pin with an IoT custom cable: reads 1 while water is present, else 0",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "Part-WaterLevel",
        kind: DeviceKind::Iot,
        label: "Water Sensor",
        description: "Water sensor wired to a board's pin with an IoT custom cable: reads the water level, in centimetres",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "Part-Wind",
        kind: DeviceKind::Iot,
        label: "Wind Sensor",
        description: "Wind sensor wired to a board's pin with an IoT custom cable: reads the wind speed, in kilometres per hour",
        ports: &[run("Wireless", 0, 0)],
    },
];

/// A plug-in card for a modular router's bay: its ports are named after the bay
/// (`Serial` in bay `0/1` gives `Serial0/1/0`, `Serial0/1/1`).
#[derive(Debug, Clone, Copy)]
pub struct Module {
    /// Stable id, saved in lab files (`Card-2S`).
    pub id: &'static str,
    /// What the interface shows: our own descriptive name.
    pub label: &'static str,
    pub description: &'static str,
    /// The port type word (`Serial`, `GigabitEthernet`).
    pub port_type: &'static str,
    pub port_count: u32,
    /// What the ports take; `None`: what their name says.
    pub medium: Option<Medium>,
    /// Layer-2 switch ports (a switch card): VLAN members switched inside the router, routed
    /// through its VLAN interfaces.
    pub switched: bool,
}

/// The cards a modular router's bays take.
pub static MODULES: &[Module] = &[
    Module {
        id: "Card-2S",
        label: "2-port Serial card",
        description: "Two synchronous serial WAN ports",
        port_type: "Serial",
        port_count: 2,
        medium: None,
        switched: false,
    },
    Module {
        id: "Card-4S",
        label: "4-port Serial card",
        description: "Four synchronous serial WAN ports",
        port_type: "Serial",
        port_count: 4,
        medium: None,
        switched: false,
    },
    Module {
        id: "Card-1GE",
        label: "1-port Gigabit card",
        description: "One routed Gigabit Ethernet port",
        port_type: "GigabitEthernet",
        port_count: 1,
        medium: None,
        switched: false,
    },
    Module {
        id: "Card-2GE",
        label: "2-port Gigabit card",
        description: "Two routed Gigabit Ethernet ports",
        port_type: "GigabitEthernet",
        port_count: 2,
        medium: None,
        switched: false,
    },
    Module {
        id: "Card-1FE",
        label: "1-port Fast Ethernet card",
        description: "One routed Fast Ethernet port",
        port_type: "FastEthernet",
        port_count: 1,
        medium: None,
        switched: false,
    },
    Module {
        id: "Card-2FE",
        label: "2-port Fast Ethernet card",
        description: "Two routed Fast Ethernet ports",
        port_type: "FastEthernet",
        port_count: 2,
        medium: None,
        switched: false,
    },
    Module {
        id: "Card-1GF",
        label: "1-port Gigabit fiber card",
        description: "One routed Gigabit Ethernet fiber (SFP) port",
        port_type: "GigabitEthernet",
        port_count: 1,
        medium: Some(Medium::Fiber),
        switched: false,
    },
    Module {
        id: "Card-4SW",
        label: "4-port switch card",
        description: "Four layer-2 switch ports inside the router: VLAN membership and trunks as on a switch, routed through the router's VLAN interfaces",
        port_type: "FastEthernet",
        port_count: 4,
        medium: None,
        switched: true,
    },
    Module {
        id: "Card-8A",
        label: "8-line async card",
        description: "Eight asynchronous lines for an octal cable: each line reaches the Console port of another router or switch (they are lines, not interfaces)",
        port_type: "Async",
        port_count: 0,
        medium: None,
        switched: false,
    },
];

/// A card by its id or its shown name.
pub fn module(id: &str) -> Option<&'static Module> {
    MODULES.iter().find(|m| m.id.eq_ignore_ascii_case(id) || m.label.eq_ignore_ascii_case(id))
}

impl Module {
    /// The card's port names in bay `slot` (`0/1`).
    pub fn port_names(&self, slot: &str) -> Vec<String> {
        (0..self.port_count).map(|n| format!("{}{slot}/{n}", self.port_type)).collect()
    }
}

impl Model {
    /// The module bays of a modular router, in order (`0/0`, `0/1`…); empty for fixed models.
    pub fn slots(&self) -> &'static [&'static str] {
        match self.id {
            "Router-M2" | "Router-MF2" => &["0/0", "0/1"],
            "Router-M3" => &["0/0", "0/1", "0/2", "0/3"],
            _ => &[],
        }
    }
}

impl Model {
    /// The nominal draw of the model in watts when it is switched on. These are round figures
    /// for a lab, taken from the ranges the public product class draws (a desktop PC about
    /// 120 W, a laptop charger 45 W, a small switch 45 W, a router 60 W, a server 350 W); they
    /// are nominal, not measured, and exist so a power strip's breaker has something to add up.
    pub fn nominal_watts(&self) -> u32 {
        match self.kind {
            DeviceKind::Router if self.id.starts_with("Firewall") => 90,
            DeviceKind::Router if self.id.starts_with("Router-M") => 80,
            DeviceKind::Router => 60,
            DeviceKind::Switch if self.multilayer() => 150,
            DeviceKind::Switch => 45,
            DeviceKind::Hub => 10,
            DeviceKind::Pc => 120,
            DeviceKind::Laptop => 45,
            DeviceKind::Server => 350,
            DeviceKind::Printer => 40,
            DeviceKind::Phone => 6,
            DeviceKind::Iot => 3,
        }
    }

    /// A provider cloud: frames go only between the ports mapped to each other.
    pub fn cloud(&self) -> bool {
        matches!(self.id, "Cloud" | "PSTN")
    }

    /// The telephone network: a cloud whose ports have telephone numbers and whose calls join two of them.
    pub fn pstn(&self) -> bool {
        self.id == "PSTN"
    }

    /// A WAN device (the cloud and the modems), listed apart in the device list.
    pub fn wan(&self) -> bool {
        matches!(self.id, "Cloud" | "PSTN" | "DSL-Modem" | "Cable-Modem")
    }

    /// A cell tower: a hub-kind device that carries mobile devices' frames to its backhaul port.
    pub fn tower(&self) -> bool {
        self.id == CELL_TOWER
    }

    /// A central office server: the router that admits and addresses mobile devices.
    pub fn central_office(&self) -> bool {
        self.id == CENTRAL_OFFICE
    }

    /// A device of the cellular network, listed apart in the device list.
    pub fn cellular(&self) -> bool {
        self.tower() || self.central_office()
    }

    /// The name new devices of this model get (`Cloud1`, `Modem1`), when not the kind's.
    pub fn name_prefix(&self) -> Option<&'static str> {
        match self.id {
            "Cloud" => Some("Cloud"),
            "PSTN" => Some("PSTN"),
            "DSL-Modem" | "Cable-Modem" => Some("Modem"),
            "Home-Gateway" => Some("Gateway"),
            "Cell-Tower" => Some("Tower"),
            "Central-Office" => Some("CO"),
            "Sniffer" => Some("Sniffer"),
            "IoT-TV" | "IoT-TVE" => Some("TV"),
            "Tablet" => Some("Tablet"),
            "Smartphone" => Some("Smartphone"),
            "Board-SBC" | "Board-MCU" => Some("Board"),
            "Repeater" => Some("Repeater"),
            "Coax-Splitter" => Some("Splitter"),
            "Bridge-2" => Some("Bridge"),
            "Switch-IND-8" => Some("IndSwitch"),
            "Router-IND" => Some("IndRouter"),
            "Firewall-3" => Some("Firewall"),
            "Net-Controller" => Some("Controller"),
            "VoIP-Adapter" => Some("Adapter"),
            "Analog-Phone" => Some("Phone"),
            "Generic-Wired" | "Generic-Wireless" => Some("Generic"),
            "IoT-Appliance" => Some("Appliance"),
            "IoT-BTSpeaker" => Some("BTSpeaker"),
            "IoT-CeilingFan" => Some("CeilingFan"),
            "IoT-Furnace" => Some("Furnace"),
            "IoT-Humidifier" => Some("Humidifier"),
            "IoT-Humiture" => Some("Humiture"),
            "IoT-Player" => Some("Player"),
            "IoT-SoundDet" => Some("SoundDet"),
            "IoT-Pressure" => Some("Pressure"),
            "IoT-Beacon" => Some("Beacon"),
            "IoT-CO2" => Some("CO2"),
            "IoT-Fire" => Some("FireMonitor"),
            "IoT-FireSprinkler" => Some("FireSprinkler"),
            "IoT-Car" => Some("Car"),
            "IoT-RFIDCard" => Some("Card"),
            "IoT-RFIDReader" => Some("Reader"),
            "IoT-LED" => Some("SmartLED"),
            "IoT-Trip" => Some("Trip"),
            "IoT-WindDet" => Some("WindDet"),
            "IoT-Signal" => Some("Signal"),
            "IoT-Streetlight" => Some("Streetlight"),
            "IoT-Traffic" => Some("Traffic"),
            "IoT-Parking" => Some("Parking"),
            "IoT-Drain" => Some("Drain"),
            "IoT-Air" => Some("Air"),
            "IoT-Blower" => Some("Blower"),
            "PLC" => Some("PLC"),
            "Data-Historian" => Some("Historian"),
            "HMI-Panel" => Some("HMI"),
            "IoT-Battery" => Some("Battery"),
            "IoT-Meter" => Some("Meter"),
            "IoT-Solar" => Some("Solar"),
            "IoT-Wind" => Some("Turbine"),
            "IoT-Thing" => Some("Thing"),
            "Part-LED" => Some("LED"),
            "Part-Buzzer" => Some("Buzzer"),
            "Part-Motor" => Some("Motor"),
            "Part-Relay" => Some("Relay"),
            "Part-LCD" => Some("LCD"),
            "Part-Button" => Some("Button"),
            "Part-Pot" => Some("Pot"),
            "Part-Ultrasonic" => Some("Distance"),
            "Part-Light" => Some("LightSensor"),
            "Part-Switch" => Some("Toggle"),
            "Part-AirCooler" => Some("AirCooler"),
            "Part-Alarm" => Some("Alarm"),
            "Part-CeilingSprinkler" => Some("CeilingSprinkler"),
            "Part-FloorSprinkler" => Some("FloorSprinkler"),
            "Part-DimLED" => Some("DimLED"),
            "Part-HeatingElement" => Some("HeatingElement"),
            "Part-RGBLED" => Some("RGBLED"),
            "Part-Servo" => Some("Servo"),
            "Part-Speaker" => Some("Speaker"),
            "Part-Flex" => Some("Flex"),
            "Part-EnvSensor" => Some("EnvSensor"),
            "Part-Generic" => Some("GenericSensor"),
            "Part-Humidity" => Some("HumiditySensor"),
            "Part-Humiture" => Some("Humiture"),
            "Part-Membrane" => Some("Membrane"),
            "Part-Metal" => Some("MetalSensor"),
            "Part-Motion" => Some("MotionSensor"),
            "Part-LatchSwitch" => Some("PushSwitch"),
            "Part-Rocker" => Some("Rocker"),
            "Part-PushToggle" => Some("ToggleButton"),
            "Part-Smoke" => Some("SmokeSensor"),
            "Part-Temp" => Some("TempSensor"),
            "Part-Trip" => Some("TripWire"),
            "Part-WaterDetector" => Some("WaterDetector"),
            "Part-WaterLevel" => Some("WaterSensor"),
            "Part-Wind" => Some("WindSensor"),
            _ => None,
        }
    }
}

/// A model by its id (`Router-3`, what lab files keep) or by the name the interface shows
/// (`Router 3GE`).
pub fn find(id: &str) -> Option<&'static Model> {
    MODELS.iter().find(|m| m.id.eq_ignore_ascii_case(id)).or_else(|| MODELS.iter().find(|m| m.label.eq_ignore_ascii_case(id)))
}

/// The model a kind gets when none is named.
pub fn default_for(kind: DeviceKind) -> &'static Model {
    let id = match kind {
        DeviceKind::Router => "Router-3",
        DeviceKind::Switch => "Switch-24",
        DeviceKind::Hub => "Hub-8",
        DeviceKind::Pc => "PC",
        DeviceKind::Laptop => "Laptop",
        DeviceKind::Server => "Server",
        DeviceKind::Printer => "Printer",
        DeviceKind::Phone => "IP-Phone",
        DeviceKind::Iot => "IoT-Sensor",
    };
    MODELS.iter().find(|m| m.id == id).unwrap_or(&FALLBACK)
}

static FALLBACK: Model = Model { id: "PC", kind: DeviceKind::Pc, label: "PC", description: "", ports: &[run("FastEthernet", 0, 0)] };

/// What a port takes, by its name alone.
pub fn medium_by_name(name: &str) -> Medium {
    if name.starts_with("Coaxial") {
        Medium::Coaxial
    } else if name.starts_with("Phone") {
        Medium::Phone
    } else if crate::ifname::is_serial(name) {
        Medium::Serial
    } else if crate::ifname::is_wireless(name) || crate::ifname::is_cellular(name) {
        Medium::Wireless
    } else {
        Medium::Copper
    }
}

impl Model {
    /// What a built-in port of this model takes (`None`: not one of its ports).
    pub fn port_medium(&self, name: &str) -> Option<Medium> {
        let target = crate::ifname::parse(name)?;
        self.ports.iter().find_map(|r| {
            (r.first..=r.last)
                .any(|n| crate::ifname::parse(&format!("{}{n}", r.prefix)).is_some_and(|x| x.same_port(&target)))
                .then(|| r.medium.unwrap_or_else(|| medium_by_name(name)))
        })
    }

    /// Whether the model has a console port (routers and switches with a command line; not a
    /// home router, which is set up from its web pages here).
    pub fn has_console(&self) -> bool {
        matches!(self.kind, DeviceKind::Router | DeviceKind::Switch) && !self.home_router()
    }

    /// Whether the model has an RS-232 serial port for a console cable (PCs and laptops).
    pub fn has_rs232(&self) -> bool {
        matches!(self.kind, DeviceKind::Pc | DeviceKind::Laptop) && !self.touch() && !self.sniffer()
    }

    /// A sniffer: a host-kind device that only records the frames reaching its port.
    pub fn sniffer(&self) -> bool {
        self.id == "Sniffer"
    }

    /// A touch-screen device (a tablet or a smartphone): Wi-Fi only, a touch desktop, no serial
    /// port for a console cable.
    pub fn touch(&self) -> bool {
        matches!(self.id, "Tablet" | "Smartphone")
    }
}

impl Model {
    /// Port names, in order.
    pub fn port_names(&self) -> Vec<String> {
        // A single-port host is `FastEthernet0`, not `FastEthernet0/0`.
        self.ports.iter().flat_map(|r| (r.first..=r.last).map(move |n| format!("{}{n}", r.prefix))).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_shown_names_are_unique_and_both_find_the_model() {
        let mut seen = std::collections::BTreeSet::new();
        for m in MODELS {
            assert!(seen.insert(m.id.to_lowercase()), "{}", m.id);
            if !m.label.eq_ignore_ascii_case(m.id) {
                assert!(seen.insert(m.label.to_lowercase()), "{}", m.label);
            }
            assert_eq!(find(m.id).map(|x| x.id), Some(m.id));
            assert_eq!(find(m.label).map(|x| x.id), Some(m.id));
        }
        assert_eq!(find("router 3ge").map(|m| m.id), Some("Router-3"));
    }

    #[test]
    fn every_model_is_listed_in_the_group_that_fits_it() {
        let g = |id: &str| find(id).map(|m| m.group());
        assert_eq!(g("Router-3"), Some(Group::Routers));
        assert_eq!(g("Central-Office"), Some(Group::Wireless));
        assert_eq!(g("Cell-Tower"), Some(Group::Wireless));
        assert_eq!(g("Home-Router"), Some(Group::Wireless));
        assert_eq!(g("AccessPoint"), Some(Group::Wireless));
        assert_eq!(g("Switch-24"), Some(Group::Switches));
        assert_eq!(g("Firewall-5"), Some(Group::Security));
        assert_eq!(g("Cloud"), Some(Group::Wan));
        assert_eq!(g("Hub-8"), Some(Group::Hubs));
        assert_eq!(g("Board-MCU"), Some(Group::Boards));
        assert_eq!(g("IoT-Light"), Some(Group::Home));
        assert_eq!(g("PC"), Some(Group::EndDevices));
        assert_eq!(g("IP-Phone"), Some(Group::EndDevices));
        assert_eq!(g("Firewall-3"), Some(Group::Security));
        assert_eq!(g("Repeater"), Some(Group::Hubs));
        assert_eq!(g("Bridge-2"), Some(Group::Switches));
        assert_eq!(g("Switch-IND-8"), Some(Group::Switches));
        assert_eq!(g("Part-LED"), Some(Group::Actuators));
        assert_eq!(g("Part-Pot"), Some(Group::Sensors));
        assert_eq!(g("IoT-Street"), None);
        assert_eq!(g("IoT-Streetlight"), Some(Group::SmartCity));
        assert_eq!(g("PLC"), Some(Group::Industrial));
        assert_eq!(g("IoT-Solar"), Some(Group::PowerGrid));
        assert_eq!(g("IoT-Thing"), Some(Group::Boards));
        // Every group holds at least one model, and the count of models is the catalogue's.
        let groups = [
            Group::Routers,
            Group::Switches,
            Group::Hubs,
            Group::Wireless,
            Group::Security,
            Group::Wan,
            Group::EndDevices,
            Group::Home,
            Group::SmartCity,
            Group::Industrial,
            Group::PowerGrid,
            Group::Boards,
            Group::Actuators,
            Group::Sensors,
        ];
        for want in groups {
            assert!(MODELS.iter().any(|m| m.group() == want), "{want:?} is empty");
        }
        assert_eq!(MODELS.len(), 146);
    }

    #[test]
    fn a_model_can_be_listed_in_more_than_one_group_and_the_first_is_its_own() {
        let groups = |id: &str| find(id).unwrap().groups();
        assert_eq!(groups("IoT-LED"), [Group::SmartCity, Group::Industrial, Group::Actuators]);
        assert_eq!(groups("IoT-Sensor"), [Group::Home, Group::SmartCity]);
        assert_eq!(groups("IoT-Humidity"), [Group::Home, Group::SmartCity]);
        assert_eq!(groups("IoT-CO"), [Group::Home, Group::SmartCity]);
        assert_eq!(groups("IoT-Streetlight"), [Group::SmartCity]);
        assert_eq!(groups("Router-3"), [Group::Routers]);
        for m in MODELS {
            assert_eq!(m.groups().first(), Some(&m.group()), "{}", m.id);
            let mut seen = std::collections::BTreeSet::new();
            assert!(m.groups().iter().all(|g| seen.insert(format!("{g:?}"))), "{} lists a group twice", m.id);
        }
        // The city group holds the new things; the reading and switched ones are as the framework says.
        for id in ["IoT-Pressure", "IoT-Beacon", "IoT-CO2", "IoT-Fire", "IoT-RFIDCard", "IoT-RFIDReader", "IoT-Trip", "IoT-WindDet"] {
            assert_eq!(find(id).unwrap().group(), Group::SmartCity, "{id}");
            assert!(crate::iot::thing_of(id).is_some_and(|t| t.sensor), "{id} reports a reading");
        }
        for id in ["IoT-FireSprinkler", "IoT-Car", "IoT-LED"] {
            assert!(crate::iot::thing_of(id).is_some_and(|t| !t.sensor), "{id} is switched");
        }
        assert!(crate::iot::thing_of("IoT-Beacon").is_some_and(|t| t.also == "audience"));
        assert_eq!(find("IoT-Signal").unwrap().group(), Group::Industrial);
        assert!(crate::iot::thing_of("IoT-Signal").is_some_and(|t| t.sensor && t.also == "frequency"));
    }

    #[test]
    fn the_new_end_devices_and_network_devices_have_the_ports_a_lab_expects() {
        assert_eq!(find("Repeater").unwrap().port_names(), ["Ethernet0", "Ethernet1"]);
        assert_eq!(find("Coax-Splitter").unwrap().port_names(), ["Coaxial0", "Coaxial1", "Coaxial2"]);
        assert_eq!(find("Coax-Splitter").unwrap().port_medium("Coaxial1"), Some(Medium::Coaxial));
        assert_eq!(find("Bridge 2FE").unwrap().port_names(), ["FastEthernet0/1", "FastEthernet0/2"]);
        assert_eq!(find("Firewall 3GE").unwrap().port_names(), ["GigabitEthernet1/1", "GigabitEthernet1/2", "GigabitEthernet1/3"]);
        assert_eq!(find("VoIP-Adapter").unwrap().port_medium("Phone0"), Some(Medium::Phone));
        assert_eq!(find("Analog-Phone").unwrap().kind, DeviceKind::Phone);
        assert_eq!(find("Generic-Wireless").unwrap().port_medium("Wireless0"), Some(Medium::Wireless));
        assert_eq!(find("PLC").unwrap().kind, DeviceKind::Iot);
        assert_eq!(crate::board::pins(find("PLC").unwrap()).len(), 12);
    }

    #[test]
    fn every_part_and_thing_has_its_behaviour_and_every_new_name_is_free() {
        for m in MODELS.iter().filter(|m| m.kind == DeviceKind::Iot && crate::board::pins(m).is_empty()) {
            assert!(crate::iot::thing_of(m.id).is_some(), "{} has no behaviour", m.id);
        }
        for m in MODELS.iter().filter(|m| matches!(m.group(), Group::Actuators | Group::Sensors)) {
            assert!(m.id.starts_with("Part-"), "{}", m.id);
        }
        for m in MODELS.iter().filter(|m| m.group() == Group::Sensors) {
            assert!(crate::iot::thing_of(m.id).is_some_and(|t| t.sensor), "{} is read", m.id);
        }
        for m in MODELS.iter().filter(|m| m.group() == Group::Actuators) {
            assert!(crate::iot::thing_of(m.id).is_some_and(|t| !t.sensor), "{} is switched", m.id);
        }
        // The component list the device box shows: nine new actuators and seventeen new sensors.
        assert_eq!(MODELS.iter().filter(|m| m.group() == Group::Actuators).count(), 14);
        assert_eq!(MODELS.iter().filter(|m| m.group() == Group::Sensors).count(), 22);
        assert_eq!(find("Humidity Sensor").map(|m| m.id), Some("Part-Humidity"));
        assert_eq!(find("Humidity Monitor").map(|m| m.id), Some("IoT-Humidity"));
    }

    #[test]
    fn every_kind_has_a_default_and_names_parse() {
        for k in DeviceKind::ALL {
            assert_eq!(default_for(k).kind, k);
        }
        for m in MODELS {
            let names = m.port_names();
            assert!(!names.is_empty(), "{}", m.id);
            for n in &names {
                assert!(crate::ifname::parse(n).is_some(), "{n}");
            }
        }
        assert_eq!(find("switch-48").map(|m| m.port_names().len()), Some(50));
        assert_eq!(find("Hub-8").map(|m| m.port_names()[7].clone()).as_deref(), Some("Ethernet7"));
    }

    #[test]
    fn modules_name_their_ports_after_the_bay() {
        let m = find("Modular Router 3GE").unwrap();
        assert_eq!(m.slots().len(), 4);
        assert!(find("Router-3").unwrap().slots().is_empty());
        let mut seen = std::collections::BTreeSet::new();
        for c in MODULES {
            assert!(seen.insert(c.id) && module(c.label).is_some_and(|x| x.id == c.id));
            for slot in m.slots() {
                for n in c.port_names(slot) {
                    assert!(crate::ifname::parse(&n).is_some_and(|x| x.numbers.len() == 3), "{n}");
                }
            }
        }
        assert_eq!(module("card-2s").unwrap().port_names("0/1"), ["Serial0/1/0", "Serial0/1/1"]);
    }

    #[test]
    fn tablets_and_smartphones_are_wifi_touch_devices_without_a_serial_port() {
        for id in ["Tablet", "Smartphone"] {
            let m = find(id).unwrap();
            assert!(m.touch() && !m.has_rs232() && !m.has_console(), "{id}");
            assert_eq!(m.port_names(), ["Wireless0", "Cellular0"]);
            assert_eq!(m.port_medium("Wireless0"), Some(Medium::Wireless));
            assert_eq!(m.port_medium("Cellular0"), Some(Medium::Wireless));
            assert_eq!(m.name_prefix(), Some(id));
        }
        assert!(!find("Laptop-WiFi").unwrap().touch() && find("Laptop-WiFi").unwrap().has_rs232());
    }

    #[test]
    fn the_newer_routers_and_end_devices_have_the_ports_a_lab_expects() {
        assert_eq!(find("Router 2FE").unwrap().port_names(), ["FastEthernet0/0", "FastEthernet0/1"]);
        assert_eq!(find("Router-FE2S").unwrap().port_names(), ["FastEthernet0/0", "FastEthernet0/1", "Serial0/0/0", "Serial0/0/1"]);
        assert_eq!(find("Router 3GE+2S").unwrap().port_names().len(), 5);
        assert_eq!(find("Router-FE2S").unwrap().port_medium("Serial0/0/1"), Some(Medium::Serial));
        assert_eq!(find("Modular Router 2FE").unwrap().slots(), ["0/0", "0/1"]);
        assert_eq!(module("Card-2FE").unwrap().port_names("0/1"), ["FastEthernet0/1/0", "FastEthernet0/1/1"]);
        let sniffer = find("Sniffer").unwrap();
        assert!(sniffer.sniffer() && !sniffer.has_rs232() && !sniffer.has_console());
        assert_eq!(sniffer.kind, DeviceKind::Pc);
        assert_eq!(find("Smart TV").unwrap().port_medium("Wireless0"), Some(Medium::Wireless));
        assert_eq!(find("Wired Smart TV").unwrap().port_medium("FastEthernet0"), Some(Medium::Copper));
        for id in ["IoT-TV", "IoT-TVE", "IoT-Speaker"] {
            assert!(crate::iot::thing_of(id).is_some_and(|t| !t.sensor), "{id} is switched on and off");
            assert_eq!(find(id).map(|m| m.kind), Some(DeviceKind::Iot));
        }
        assert!(crate::iot::thing_of("IoT-Water").is_some_and(|t| t.sensor), "the water level sensor reports a reading");
    }
}
