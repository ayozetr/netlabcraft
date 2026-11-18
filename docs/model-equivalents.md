# Model equivalents

> **Last reviewed:** 2026-11-18 · **Last updated:** 2026-11-18 · **Change:** minor (final consistency pass: the review line names only the latest change) · **Target:** CCNA course labs

Course labs name specific vendor models. NetLabCraft's models are its own, named after their
ports (`GE` Gigabit Ethernet, `FE` Fast Ethernet, `S` serial), and never carry vendor names
([decisions.md](decisions.md)). This table says which NetLabCraft model to place when a lab
asks for a given model, so the same commands and port numbers work. It only describes
equivalence for teaching; the vendor names below are mentioned to identify the products a lab
refers to.

Lab files keep the stable model id (second column); the workspace shows the name (first
column). Commands accept either: `device.add {"model": "Router 3GE"}` or `{"model": "Router-3"}`.

Routers and switches have one or two power-supply bays at the back with one supply fitted from
the factory (`device.psu`; the Router 2GE, Router 2GE+2S, Router 2FE, Router 2FE+2S, Modular Router 2GE, Modular Router 2FE, Firewall 5GE,
Switch 8FE, Switch 8FE PoE, Firewall 3GE, Bridge 2FE and WLC have one bay, the other routers and switches two). Where a
lab has students fit a redundant supply, use a two-bay model.

| NetLabCraft model | Id in files | Use it where a lab asks for | Port names | Notes |
|---|---|---|---|---|
| Router 2GE | `Router-2` | 1941, 2901, ISR 4321 | `GigabitEthernet0/0–0/1` | A 4321 names its ports `0/0/0–0/0/1`. |
| Router 3GE | `Router-3` | 2911, ISR 4331 | `GigabitEthernet0/0–0/2` | The usual CCNA router. A 4331 names its ports `0/0/0–0/0/2`. |
| Router 4GE | `Router-4` | Routers with four Gigabit ports | `GigabitEthernet0/0–0/3` | |
| Router 2FE | `Router-FE2` | 2621XM; 1841 or 2811 with no card fitted | `FastEthernet0/0–0/1` | The Fast Ethernet routers of older CCNA labs. |
| Router 2FE+2S | `Router-FE2S` | 1841 or 2811 with a 2-port serial card (HWIC-2T) | `FastEthernet0/0–0/1`, `Serial0/0/0–0/0/1` | Serial ports are fitted from the start. |
| Router 3GE+2S | `Router-WAN3` | 2911 with a 2-port serial card (HWIC-2T) | `GigabitEthernet0/0–0/2`, `Serial0/0/0–0/0/1` | Serial ports are fitted from the start. |
| Firewall 5GE | `Firewall-5` | ASA 5506-X | `GigabitEthernet1/1–1/5` | Firewall commands (`nameif`, `security-level`, object NAT). A 5506-X has eight ports (`1/1–1/8`): use the first five. |
| Modular Router 2GE | `Router-M2` | 1941 with its module bays | `GigabitEthernet0/0–0/1`, cards in bays `0/0–0/1` | Fit cards in its Physical tab with the power off. |
| Modular Router 2FE | `Router-MF2` | 1841 with its card slots | `FastEthernet0/0–0/1`, cards in bays `0/0–0/1` | Fit cards in its Physical tab with the power off. |
| Modular Router 3GE | `Router-M3` | 2911 with its module bays | `GigabitEthernet0/0–0/2`, cards in bays `0/0–0/3` | |
| 2-port Serial card | `Card-2S` | HWIC-2T | `Serial0/B/0–0/B/1` (B = bay) | |
| 4-port Serial card | `Card-4S` | HWIC-4T | `Serial0/B/0–0/B/3` | |
| 1-port Gigabit card | `Card-1GE` | EHWIC-1GE-SFP-CU | `GigabitEthernet0/B/0` | |
| 2-port Fast Ethernet card | `Card-2FE` | HWIC-2FE | `FastEthernet0/B/0–0/B/1` | Two routed ports. |
| 1-port Gigabit fiber card | `Card-1GF` | SFP-based Gigabit cards | `GigabitEthernet0/B/0` (fiber) | |
| 1-port Fast Ethernet card | `Card-1FE` | HWIC-1FE | `FastEthernet0/B/0` | |
| 4-port switch card | `Card-4SW` | HWIC-4ESW, EHWIC-4ESG | `FastEthernet0/B/0–0/B/3` | Layer-2 ports inside the router: VLANs, `switchport access\|trunk`, routed through `interface vlan N`. |
| Router 2GE+2S | `Router-WAN` | 1941 or 2911 with a 2-port serial card (HWIC-2T), ISR 4321 with a serial NIM | `GigabitEthernet0/0–0/1`, `Serial0/0/0–0/0/1` | Serial ports are fitted from the start. |
| Switch 8FE | `Switch-8` | Small 8-port access switches | `FastEthernet0/1–0/8`, `GigabitEthernet0/1` | |
| Switch 24FE | `Switch-24` | 2960-24TT | `FastEthernet0/1–0/24`, `GigabitEthernet0/1–0/2` | The usual CCNA switch. |
| Switch 24FE+2SFP | `Switch-24-SFP` | 2960 models with SFP uplinks | `FastEthernet0/1–0/24`, `GigabitEthernet0/1–0/2` (fiber) | The other switches' Gigabit uplinks take copper or fiber. |
| Switch 48FE | `Switch-48` | 48-port 2960 models | `FastEthernet0/1–0/48`, `GigabitEthernet0/1–0/2` | |
| Switch 8FE PoE | `Switch-8-PoE` | Small PoE access switches | `FastEthernet0/1–0/8`, `GigabitEthernet0/1` | Power over Ethernet on the Fast Ethernet ports, 65 W in all. |
| Switch 24FE PoE | `Switch-24-PoE` | 2960-24LT-L; 3560-24PS used as a layer-2 switch | `FastEthernet0/1–0/24`, `GigabitEthernet0/1–0/2` | Power over Ethernet, 370 W in all. It doesn't route: for `ip routing` use L3 Switch 24 (which has no PoE). |
| L3 Switch 24 | `Switch-ML-24` | 3560-24PS, 3650-24PS | `FastEthernet0/1–0/24`, `GigabitEthernet0/1–0/2` | Routes with `ip routing`. A 3650 has Gigabit access ports (`GigabitEthernet1/0/1…`): use the same numbers on `FastEthernet0/…`. |
| Access Point | `AccessPoint` | AccessPoint-PT, autonomous access points | `GigabitEthernet0`, `Wireless0` | |
| WLC | `WLC` | WLC-2504, 3504 | `GigabitEthernet0/1–0/4` | WLANs are set with `wlan …`, not a web page. |
| Light AP | `LAP` | Lightweight APs (3702i and similar) | `GigabitEthernet0`, `Wireless0` | Joins a WLC. Needs a PoE switch port or its power adapter. |
| Home Router | `Home-Router` | WRT300N, HomeRouter-PT | `Internet0`, `Ethernet1–4`, `Wireless0` | |
| Home Gateway | `Home-Gateway` | The home gateway in the course's IoT labs | `Internet0`, `Ethernet1–4`, `Wireless0` | A home router with a built-in IoT service and an IoT tab. |
| Cloud | `Cloud` | Cloud-PT | `Ethernet0–1`, `Coaxial0–1`, `Phone0–1` | Ports joined in pairs (`cloud.map`; coaxial to phone by default), shown in its Physical tab. |
| Telephone Cloud | `PSTN` | the telephone network in dial-up labs | `Phone0–7` | A number per port (`cloud.number`); a call pairs two ports until it ends. |
| DSL Modem | `DSL-Modem` | DSL-Modem-PT | `Phone0`, `Ethernet1` | Bridges the phone line to Ethernet. |
| Cable Modem | `Cable-Modem` | Cable-Modem-PT | `Coaxial0`, `Ethernet1` | Bridges the coaxial cable to Ethernet. |
| Hub 8 | `Hub-8` | Hub-PT | `Ethernet0–7` | |
| PC | `PC` | PC-PT | `FastEthernet0` | |
| Dial-up PC | `PC-Modem` | a PC with a modem module | `Phone0` | The Dial-up app dials from it. |
| Access Server | `Access-Server` | an access server router with modem lines | `FastEthernet0/0–0/1`, `Phone0–3` | `modem inout` on a line answers calls. |
| Laptop | `Laptop` | Laptop-PT | `FastEthernet0` | |
| Tablet | `Tablet` | TabletPC-PT | `Wireless0`, `Cellular0` | Wi-Fi, or mobile data through a cell tower; a touch home screen with the Desktop's apps. |
| Smartphone | `Smartphone` | SMARTPHONE-PT | `Wireless0`, `Cellular0` | Wi-Fi, or mobile data through a cell tower (Wi-Fi is preferred when connected); a touch home screen with the Desktop's apps. |
| Cell Tower | `Cell-Tower` | Cell Tower | `Cellular0–5` (sectors), `Ethernet0` (backhaul) | A radio with a coverage range; mobile devices in range attach to it. |
| Central Office Server | `Central-Office` | Central Office Server | `GigabitEthernet0/0–2` | The provider's data service: admits attached mobile devices, gives each an address from its pool and routes them to the lab (`cellular pool\|apn\|data-plan`). |
| Sniffer | `Sniffer` | Sniffer | `FastEthernet0` | Records every frame that reaches its port and shows them in its Capture tab; answers nothing. Behind a hub it hears everything; behind a switch only broadcasts, multicasts and flooded frames, plus what a `monitor session` copies to its port. |
| Wi-Fi Laptop | `Laptop-WiFi` | Laptop-PT with a wireless card | `Wireless0` | |
| IP Phone | `IP-Phone` | 7960 | `FastEthernet0` (network), `FastEthernet1` (PC) | Registers with `telephony-service` on a router, or, set to SIP (Signal with SIP, `phone.configure`), with `voice register`. Needs a PoE switch port or its power adapter. |
| Smart Sensor | `IoT-Sensor` | The temperature monitor among the course's IoT things | `FastEthernet0` | Reports to a server's IoT service or a Home Gateway. |
| Smart Device | `IoT-Device` | The ceiling fan among the course's IoT things | `FastEthernet0` | Switched by the rules or by hand. |
| Motion Detector | `IoT-Motion` | The course's motion detector | `FastEthernet0` | Reports 1 while something moves, else 0. |
| Smoke Detector | `IoT-Smoke` | The course's smoke detector | `FastEthernet0` | Reports the smoke level (0–100 %). |
| Smart Light | `IoT-Light` | The course's smart lamp or light | `FastEthernet0` | Switched on and off. |
| Smart Door Lock | `IoT-Door` | The course's smart door | `FastEthernet0` | On unlocks, off locks. |
| Siren | `IoT-Siren` | The course's siren | `FastEthernet0` | Sounds while on. |
| Webcam | `IoT-Webcam` | The course's webcam | `FastEthernet0` | Takes a still image at each report while on. |
| Thermostat | `IoT-Thermostat` | The course's thermostat | `Wireless0` | Reports degrees. Wi-Fi things join the home gateway's `HomeNetwork` when placed. |
| Heater | `IoT-Heater` | The course's heater (furnace) | `Wireless0` | Switched on and off; doesn't change a thermostat's reading. |
| Air Conditioner | `IoT-AC` | The course's air conditioner | `Wireless0` | Switched on and off; doesn't change a thermostat's reading. |
| Humidity Monitor | `IoT-Humidity` | The course's humidity monitor | `Wireless0` | Reports % humidity. (Renamed from Humidity Sensor, the name now of the component below; lab files keep the id.) Also listed in Smart City. |
| Smart Window | `IoT-Window` | The course's window | `Wireless0` | On opens, off closes. |
| Soil Moisture Sensor | `IoT-Soil` | The course's soil moisture sensor | `Wireless0` | Reports % moisture. |
| Lawn Sprinkler | `IoT-Sprinkler` | The course's lawn sprinkler | `Wireless0` | Waters while on. |
| CO Detector | `IoT-CO` | The course's carbon monoxide detector | `Wireless0` | Reports ppm CO. Also listed in Smart City. |
| Smart TV | `IoT-TV` | TV-PT, over Wi-Fi | `Wireless0` | Registers with the IoT service, which switches it on (playing) and off (standby). Joins the home gateway's `HomeNetwork` when placed. |
| Wired Smart TV | `IoT-TVE` | TV-PT, on a cable | `FastEthernet0` | As the Smart TV, on Ethernet. |
| Smart Speaker | `IoT-Speaker` | The course's home speaker | `Wireless0` | Plays while on. |
| Water Level Sensor | `IoT-Water` | The course's water level monitor | `Wireless0` | Reports % of a tank filled. |
| Smart Plug | `IoT-Plug` | The course's power outlet | `Wireless0` | Powered while on. |
| Garage Door | `IoT-Garage` | The course's garage door | `Wireless0` | On opens, off closes. |
| Appliance | `IoT-Appliance` | A generic mains appliance | `Wireless0` | Switched on and off; its power state is all it has (no load, no energy use). |
| Bluetooth Speaker | `IoT-BTSpeaker` | A Bluetooth speaker | `Wireless0` | Plays while on, or while a Portable Music Player within 60 m is playing (it pairs with the nearest one by distance). |
| Ceiling Fan | `IoT-CeilingFan` | A ceiling fan | `Wireless0` | While on it cools the room toward the outside temperature (it never goes below it). |
| Furnace | `IoT-Furnace` | A furnace | `Wireless0` | While on it heats the room faster than a Heater (up to the same ceiling). |
| Humidifier | `IoT-Humidifier` | A humidifier | `Wireless0` | While on it raises the room's humidity. |
| Humiture Monitor | `IoT-Humiture` | A temperature and humidity monitor | `Wireless0` | Reports the temperature as its reading and the humidity as a second reading. |
| Portable Music Player | `IoT-Player` | A portable music player | `Wireless0` | Plays one tone (440 Hz) while on: 55 dB alone, 75 dB with a Bluetooth Speaker paired. |
| Sound Frequency Detector | `IoT-SoundDet` | A sound frequency detector | `Wireless0` | Reports the pitch (Hz) of what is playing as its reading and the volume (dB) as a second reading. |
| Board SBC | `Board-SBC` | SBC boards | `FastEthernet0`, pins `D0–D7`, `A0–A3` | Runs BoardScript, not Python or JavaScript: course programs have to be written again in it. IoT custom cables join pins to things. |
| Board MCU | `Board-MCU` | MCU boards | `Wireless0`, pins `D0–D5`, `A0–A1` | Runs BoardScript, as the Board SBC. |
| Server | `Server` | Server-PT | `GigabitEthernet0` | |
| Printer | `Printer` | Printer-PT | `FastEthernet0` | |
| Repeater | `Repeater` | A two-port repeater | `Ethernet0–1` | Regenerates every frame onto the other port, as a hub with two ports does. |
| Coax Splitter | `Coax-Splitter` | A coaxial splitter | `Coaxial0–2` | Repeats every frame between its coaxial ports; takes coaxial cables only. |
| Bridge 2FE | `Bridge-2` | A two-port learning bridge | `FastEthernet0/1–0/2` | Same behaviour as a switch with two ports (learning, VLANs, spanning tree). |
| Industrial Switch 8FE | `Switch-IND-8` | Rugged industrial switches | `FastEthernet0/1–0/8`, `GigabitEthernet0/1–0/2` | Same behaviour as Switch 8FE with two uplinks; no ring protocols. |
| Industrial Router 2GE+2S | `Router-IND` | Rugged industrial routers | `GigabitEthernet0/0–0/1`, `Serial0/0/0–0/0/1` | Same behaviour as Router 2GE+2S. |
| Firewall 3GE | `Firewall-3` | A smaller firewall appliance | `GigabitEthernet1/1–1/3` | Same commands as Firewall 5GE with three ports. |
| Network Controller | `Net-Controller` | The course's network controller server | `GigabitEthernet0` | A server: its Programming tab holds the controller (discover routers, push templates). |
| VoIP Adapter | `VoIP-Adapter` | An analog phone adapter | `FastEthernet0`, `Phone0` | Behaves as an IP phone (DHCP, option 150, calls); `Phone0` takes an Analog Phone with a phone cable. |
| Analog Phone | `Analog-Phone` | A plain telephone | `Phone0` | Behaves as an IP phone behind the adapter; no ring voltage or dial pulses, and no direct call over the Telephone Cloud. |
| Generic Wired | `Generic-Wired` | A generic wired end device | `FastEthernet0` | Behaves as a PC. |
| Generic Wireless | `Generic-Wireless` | A generic wireless end device | `Wireless0` | Behaves as a Wi-Fi laptop. |
| Street Light | `IoT-Streetlight` | A smart street light | `Wireless0` | Lit while on. |
| Traffic Light | `IoT-Traffic` | A smart traffic light | `Wireless0` | Green while on, red while off; no timed cycle. |
| Parking Sensor | `IoT-Parking` | A parking space sensor | `Wireless0` | Reports 1 or 0. |
| Water Drain Monitor | `IoT-Drain` | A storm drain level monitor | `Wireless0` | Reports % full. |
| Air Quality Sensor | `IoT-Air` | An air quality monitor | `Wireless0` | Reports an index (0–500). |
| Atmospheric Pressure Monitor | `IoT-Pressure` | An atmospheric pressure monitor | `Wireless0` | Reports kPa: the room is at sea-level pressure (101), or the place's `atmospheric_pressure`; a reading set by hand sets the room's. |
| Beacon | `IoT-Beacon` | A Bluetooth-like proximity beacon | `Wireless0` | Reports its id (a reading set by hand; its device number plus one until then) and, as a second reading, how many smart things are within 100 m. |
| Carbon Dioxide Detector | `IoT-CO2` | A carbon dioxide detector | `Wireless0` | Reports ppm CO2: 420 in fresh air, more while a car runs (the room's, not per spot). |
| Fire Monitor | `IoT-Fire` | A fire monitor | `Wireless0` | Reports the room's flames (0–100 %), or the place's `fire`. |
| Fire Sprinkler | `IoT-FireSprinkler` | A fire sprinkler | `Wireless0` | Sprays on its own while the room is in flames (and when the IoT service switches it on); each second it takes 3 points off the flames. |
| Old Car | `IoT-Car` | A car that can be moved on the map | `Wireless0` | While its engine is on it adds 0.5 ppm CO and 5 ppm CO2 a second to the room; it is a point that a trip sensor sees. |
| RFID Card | `IoT-RFIDCard` | An RFID card or tag | `Wireless0` | Announces its card id (a reading set by hand; its device number plus one until then) to a reader within 30 m. |
| RFID Reader | `IoT-RFIDReader` | An RFID reader | `Wireless0` | Reports the id of the nearest card within 30 m, or 0. |
| Smart LED | `IoT-LED` | A dimmable colour LED | `Wireless0` | Switched on and off; brightness (0–100 %) and colour are set in its IoT tab; one model shown in Smart City, Industrial and Actuators. |
| Trip Sensor | `IoT-Trip` | A tripwire or light-beam sensor | `Wireless0` | Reports 1 while a car, a card, a smartphone, a tablet or a laptop is within 50 m, else 0. |
| Wind Detector | `IoT-WindDet` | An anemometer | `Wireless0` | Reports kph: calm in the room, or the place's `wind_speed`; a reading set by hand sets the room's. |
| Blower Fan | `IoT-Blower` | An industrial blower | `FastEthernet0` | Running or stopped. |
| Signal Generator | `IoT-Signal` | A signal generator | `Wireless0` | Outputs a level (its reading, set by hand, -1000 to 1000) and a frequency (a second reading, 0 to 20000 Hz); other things read both through the IoT service, and a rule can act on the level. |
| PLC | `PLC` | A programmable logic controller | `FastEthernet0`, pins `D0–D7`, `A0–A3` | A board that runs BoardScript, not ladder logic. |
| Data Historian | `Data-Historian` | A data historian server | `GigabitEthernet0` | A server with the IoT service; keeps the latest reading only, no time series. |
| HMI Panel | `HMI-Panel` | An operator panel | `FastEthernet0` | A PC (browser, command prompt, IoT monitor); no screen designer. |
| Battery | `IoT-Battery` | A battery bank | `Wireless0` | Reports % charge; does not charge or discharge. |
| Power Meter | `IoT-Meter` | A power meter | `Wireless0` | Reports watts. |
| Solar Panel | `IoT-Solar` | A solar panel | `Wireless0` | Reports watts; no sun cycle. |
| Wind Turbine | `IoT-Wind` | A wind turbine | `Wireless0` | Reports rpm; no wind model. |
| Thing | `IoT-Thing` | A generic thing for a board | `Wireless0` | A reading set by hand. |
| LED, Motor, Relay, LCD | `Part-LED`, `Part-Motor`, `Part-Relay`, `Part-LCD` | Board actuators | `Wireless0` (unused) | Wired to a board pin with an IoT custom cable; on while the pin is high. |
| Buzzer | `Part-Buzzer` | Piezo Speaker | `Wireless0` (unused) | Our Buzzer is the reference app's Piezo Speaker. Sounds while the pin is high, and raises the sound volume (80 dB) and pitch (2700 Hz) of its place. |
| Air Cooler | `Part-AirCooler` | Air Cooler | `Wireless0` (unused) | Lowers the temperature (5 degrees) and the humidity (10 %) of its place while on; with the smart room's physics running it counts as a cooler. |
| Alarm | `Part-Alarm` | Alarm | `Wireless0` (unused) | A siren with a lit state: sounds and lights up while on; sound volume 100 dB, pitch 1000 Hz in its place. |
| Ceiling Sprinkler | `Part-CeilingSprinkler` | Ceiling Sprinkler | `Wireless0` (unused) | While on: humidity +30 %, rain +2 cm and water level +1 cm in its place, and the flames (`fire`) go out. |
| Floor Sprinkler | `Part-FloorSprinkler` | Floor Sprinkler | `Wireless0` (unused) | While on: humidity +20 %, water level +3 cm and soil moisture +30 %, and the flames go out. |
| Dimmable LED | `Part-DimLED` | Dimmable LED | `Wireless0` (unused) | Brightness 0-100: `write(PIN, 40)`; on with no level set is 100. |
| Heating Element | `Part-HeatingElement` | Heating Element | `Wireless0` (unused) | Raises the temperature (10 degrees) and lowers the humidity (5 %) of its place while on; counts as a heater in the smart room. |
| RGB LED | `Part-RGBLED` | RGB LED | `Wireless0` (unused) | Red, green and blue as one number: `write(PIN, rgb(255, 128, 0))`; on with no colour set is white. |
| Servo | `Part-Servo` | Servo | `Wireless0` (unused) | Angle 0-180 degrees: `write(PIN, 90)`; it always holds an angle. |
| Speaker | `Part-Speaker` | Speaker | `Wireless0` (unused) | Plays while on, and raises the sound volume (70 dB) and pitch (440 Hz) of its place. |
| Button, Potentiometer, Distance Sensor, Light Sensor, Toggle Switch | `Part-Button`, `Part-Pot`, `Part-Ultrasonic`, `Part-Light`, `Part-Switch` | Board sensors | `Wireless0` (unused) | Wired to a board pin with an IoT custom cable; the reading is set by hand in its IoT tab. |
| Flex Sensor | `Part-Flex` | Flex Sensor | `Wireless0` (unused) | How far it is bent, 0-100 %, set by hand. |
| Generic Environment Sensor | `Part-EnvSensor` | Generic Environment Sensor | `Wireless0` (unused) | Reads the environment variable you pick (any of the 57), rounded to a whole number within -1000 to 1000; a variable no place sets reads its default. |
| Generic Sensor | `Part-Generic` | Generic Sensor | `Wireless0` (unused) | A raw analog value you set, -1000 to 1000. |
| Humidity Sensor | `Part-Humidity` | Humidity Sensor | `Wireless0` (unused) | The humidity of its place (0-100 %): what the place sets, with the running parts' changes, else the room's, else its own number. |
| Humiture Sensor | `Part-Humiture` | Humiture Sensor | `Wireless0` (unused) | The temperature on its pin and the humidity with `channel(PIN, 1)`. |
| Membrane Potentiometer | `Part-Membrane` | Membrane Potentiometer | `Wireless0` (unused) | The position of a press along its strip, 0-100 %, set by hand. |
| Metal Sensor | `Part-Metal` | Metal Sensor | `Wireless0` (unused) | A proximity flag, 0 or 1, set by hand: it detects no object in the room. |
| Motion Sensor | `Part-Motion` | Motion Sensor | `Wireless0` (unused) | 1 while the room's motion is set, else its own flag. |
| Push Button Toggle Switch | `Part-LatchSwitch` | Push Button Toggle Switch | `Wireless0` (unused) | A latching push switch: each press (`iot.configure` with `press`, the Press button) flips it between 0 and 1. |
| Rocker Switch | `Part-Rocker` | Rocker Switch | `Wireless0` (unused) | Two positions, 0 and 1, set by hand. |
| Toggle Push Button | `Part-PushToggle` | Toggle Push Button | `Wireless0` (unused) | A push button that flips its state at each press, 0 or 1. |
| Smoke Sensor | `Part-Smoke` | Smoke Sensor | `Wireless0` (unused) | The smoke level of its place (0-100 %). |
| Temperature Sensor | `Part-Temp` | Temperature Sensor | `Wireless0` (unused) | The temperature of its place, in degrees. |
| Trip Wire | `Part-Trip` | Trip Wire | `Wireless0` (unused) | A line-crossing flag, 0 or 1; follows the room's motion while its physics runs. |
| Water Detector | `Part-WaterDetector` | Water Detector | `Wireless0` (unused) | 1 while its place has water (a water level or rain set, or a sprinkler on), else 0 or its own flag. |
| Water Sensor | `Part-WaterLevel` | Water Sensor | `Wireless0` (unused) | The water level of its place, in centimetres. |
| Wind Sensor | `Part-Wind` | Wind Sensor | `Wireless0` (unused) | The wind speed of its place, in kph. |

Models a lab names that have no equivalent yet (PDAs, routers with other port
mixes or built-in radios) are listed in [hardware-parity.md](hardware-parity.md).

Cisco, Catalyst, ISR and Packet Tracer are trademarks of Cisco Systems, Inc. NetLabCraft is an
independent project, not affiliated with or endorsed by Cisco.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-18 | minor | Final consistency pass: the review line names only the latest change; the revision rows are in date order, newest first. |
| 2026-11-17 | minor | Consistency pass: the Humidity Monitor is named as `MODELS` names it; the rows were checked against the 146 models. |
| 2026-11-17 | minor | Smart City and Industrial: Atmospheric Pressure Monitor, Beacon, Carbon Dioxide Detector, Fire Monitor, Fire Sprinkler, Old Car, RFID Card, RFID Reader, Smart LED, Trip Sensor and Wind Detector, and an Industrial Signal Generator; the equivalents of a temperature monitor, a humidity monitor and a CO detector are also listed in Smart City. |
| 2026-11-17 | minor | Smart Home: Appliance, Bluetooth Speaker, Ceiling Fan, Furnace, Humidifier, Humiture Monitor, Portable Music Player and Sound Frequency Detector. |
| 2026-11-17 | minor | Twenty-six components: Air Cooler, Alarm, Ceiling Sprinkler, Floor Sprinkler, Dimmable LED, Heating Element, RGB LED, Servo and Speaker (actuators) and Flex Sensor, Generic Environment Sensor, Generic Sensor, Humidity Sensor, Humiture Sensor, Membrane Potentiometer, Metal Sensor, Motion Sensor, Push Button Toggle Switch, Rocker Switch, Toggle Push Button, Smoke Sensor, Temperature Sensor, Trip Wire, Water Detector, Water Sensor and Wind Sensor (sensors); 126 models in all. The Buzzer is listed as the Piezo Speaker; the Wi-Fi Humidity Sensor is now the Humidity Monitor. Several are shallow: see [gaps.md](gaps.md) item 16. |
| 2026-11-17 | minor | The full device box: repeater, coaxial splitter, bridge, industrial switch and router, a second firewall, network controller, VoIP adapter and analog phone, generic wired and wireless devices, five smart city things, four industrial ones, four power grid ones, a generic thing, and five actuators and five sensors for boards (35 models, 100 in all). Several are shallow: see [gaps.md](gaps.md) item 16. |
| 2026-11-16 | minor | Telephone Cloud, Dial-up PC and Access Server rows. |
| 2026-11-15 | minor | The Sniffer row: a switch's monitor session can copy traffic to it. |
| 2026-11-15 | minor | Router 2FE, Router 2FE+2S, Router 3GE+2S, Modular Router 2FE and the 2-port Fast Ethernet card; Sniffer; Smart TV, Wired Smart TV, Smart Speaker and Water Level Sensor; the "no equivalent yet" list brought up to date. |
| 2026-11-13 | minor | Cellular radio on Tablet and Smartphone; Cell Tower and Central Office Server rows. |
| 2026-10-31 | minor | Tablet and Smartphone, ten Wi-Fi IoT things, Board SBC and Board MCU (BoardScript, not Python or JavaScript); power-supply bays per model; the "no equivalent yet" list now names cellular, TVs and the sniffer. |
| 2026-10-30 | minor | 4-port switch card, the two PoE switches, Home Gateway, Cloud, DSL and cable modems, six more IoT things; IP Phone with SIP and power; the "no equivalent yet" list brought up to date. |
| 2026-10-30 | minor | Switch 24FE+2SFP and the fiber card. |
| 2026-10-30 | minor | Modular routers and their cards, IP Phone, Smart Sensor and Smart Device. |
| 2026-10-30 | minor | Firewall 5GE. |
| 2026-10-30 | major | Created: which NetLabCraft model to place for each model course labs name. |
