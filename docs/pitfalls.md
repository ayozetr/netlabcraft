# Pitfalls

> **Last reviewed:** 2026-11-18 · **Last updated:** 2026-11-18 · **Change:** minor (the mode bits are used up; a conflicting pull request gets no CI) · also merged with minor (a model listed in more than one device-box group) · also merged with minor (Preferences: Administrative, Show/Hide, Font, Miscellaneous, Answer Tree, Custom Interfaces, Publishers and Image Cleanup tabs) · also merged with minor (a request path cannot hold a space; a raw string ends at `"#`) · also merged with minor (a file handed to the system's opener is not a way to print or keep it) · earlier: minor (a galley that carries its own colour ignores the painter's fallback) · earlier: minor (an interface derived from a session record cannot depend on state the device does not hold) · earlier: minor (a wait for frames must look at what already arrived) · earlier: minor (in BGP a confederation session is external for the session and internal for the path) · earlier: minor (a lost path does not end a BGP session at once) · earlier: minor (a rule that acts at a switch port must cover the timed control frames too) · earlier: minor (redistribution is part of every protocol's computation) · earlier: minor (policy edits must reach the routing fingerprint) · earlier: minor (BGP needs simulated time) · earlier: minor (a derived-state pitfall (M9.10)) · also merged with M9.9 (RSA, X.509, certificates in TLS and SSH) · earlier: minor (four pitfalls from M9.7: counters keyed by position, a stale checksum, a database that lists unreachable routers, a `Rest` that needs a word) · also merged with M9.5, M9.6, M9.8 and M9.11 · earlier: minor (two test pitfalls from M9.4) · also merged with M9.2 and M9.3 (routing timers; real DES, 3DES, AH and elliptic-curve Diffie-Hellman) · earlier: minor (first version)

Mistakes already made here, and the rule that stops each one. Read this before your first change; when you make a
new mistake worth not repeating, add it in the same change.

## Std mutex and std process

- **A poisoned `std::sync::Mutex` takes everything down.** One panic while locked makes every later `lock()` fail,
  and the app catches panics at `Session::execute` and carries on. Use `netlabcraft_core::sync::Mutex`, whose `lock()`
  recovers the data. Clippy refuses the std type (`clippy.toml`).
- **`std::process::Command` inherits stdin and has no deadline.** A program waiting at a prompt hangs the caller
  forever. Start programs through `netlabcraft_core::process` (stdin closed, wall-clock deadline, capped output,
  `Err` instead of a panic). Clippy refuses `Command::new` elsewhere; developer tooling (`xtask`) and tests that
  start our own binary opt out with a comment saying why.

## Tests

- **A new field on a model struct breaks other crates' tests, and `cargo test -p one-crate` hides it.** Adding
  `log` to `AclEntry` compiled the library and the `ios` tests, while struct literals in `model`'s and `sim`'s own
  tests no longer compiled. Run `cargo test --workspace --no-run` after changing a shared struct, and build test
  literals from the constructor (`AclEntry::new(..)`) where one exists.
- **A lab-book `expect:` is a substring of NetLabCraft's own wording.** A lab walked for the first time fails on
  text (`Gi0/1` against `GigabitEthernet0/1`, `DISCOVER` against `Discover`); read the trace with
  `LAB_BOOK_TRACE=1` and fix the lab unless the output is what is wrong.
- **`assert!(x.contains("…") || true)` asserts nothing.** It was written to "just run the command" and passed
  forever. If you only need the command to run, write `let _ = x;`; if the result matters, assert it.
- **A new toolchain adds lints.** Clippy gates run with `-D warnings`, so a compiler bump can turn a green tree red
  (`nonminimal_bool` did, and CI on a rolling `stable` failed on `needless_range_loop`). The compiler is pinned in
  `rust-toolchain.toml`; bump it together with `rust-version`, and fix the code, not the gate.

## Console grammar

- **A `Rest(..)` token needs at least one word.** `spec!(X, .., [K("ip"), K("ospf"), K("authentication"), Rest("[message-digest|null]")])`
  makes the bare `ip ospf authentication` an "Incomplete command". Give a command whose last word is optional two
  specs: one without the `Rest`, one with it (as `area N stub` and `area N stub no-summary` do), and read the optional
  word with `arg(i)`, which is empty when it was not typed.
- **State kept per line is keyed by what identifies the line, never by its position.** ACL hit counters were keyed by
  the index in the list, so inserting `5 deny …` above the others moved every count onto the wrong line. They are keyed
  by the line's sequence number (`Acl::hit_key`), and a command that renumbers (`resequence`) or removes lines has to
  move or drop the counters with them (`Sim::remap_acl_hits`, `Sim::forget_acl_hit`).
- **A new console mode is edited in five places, and the tests only find some of them.** A `Mode` variant needs its
  bit in `Modes` and in `ANY_MODE`, its prompt, both lists of sub-modes in `exec.rs` (the `exit` list and the
  fall-back to configuration mode), the `every_spec_is_reachable_by_its_full_words` mode list in `grammar.rs`, and a
  setup line in `every_command_survives_hostile_arguments` so the fuzzer reaches the commands inside it. A
  `try_run` takes one line: a `\n` inside it is not a second command, it is a parse error.

- **`Modes` is a `u64` and every bit is taken.** Bit 63 is the last one, and the tracked-object and IP SLA modes share it
  (`Modes::TRACK`, as `Object` and `ObjectGroup` share `OBJECT`): a command of one is offered in the other and its handler
  refuses it. The next new console mode has to widen `Modes` (to a `u128`) instead of looking for a free bit.
- **A pull request that conflicts with `main` gets no CI at all.** GitHub runs the checks on the merge result, so a push to
  a conflicting branch starts nothing and the check list stays empty or stale. Merge `origin/main` first, and put new
  lines (an `Id` arm, a `Mode` variant) where other work is not adding its own: the end of a list is where everyone adds.

## Simulator

- **Periodic control traffic is scheduled for the whole `run_for` window up front.** `schedule_control` walks every
  tick of the window at once, so a sender that reads a role or a state (a VRRP master, a held neighbour) sees it as of
  the start of the window, not as of the tick. A test that changes something and expects the new behaviour on the wire
  must run a first window for the change to settle, clear the trace, and look in a second window.
- **State derived from the lab goes stale unless every change refreshes it.** The cellular links
  (`Topology::cell_links`) are computed from positions and settings and read on every hop by `Topology::peer`.
  `device.move` does not call `Session::changed` (a drag would rebuild the simulator at every step), so a phone dragged
  out of range kept its tower and its address until the move command refreshed coverage itself. A command that changes
  where a device is or what it is set to must end in `cell_refresh` (`Session::changed` does it), and a test that
  edits a `Topology` directly must call it before it asks for a link.

- **A control packet whose content depends on state is built when it is due, not when the tick is scheduled.** OSPF hellos list
  the neighbours heard, and `schedule_control` queued a whole window of them up front with the list as of the window's start,
  so every hello of the window said "no neighbours" and the neighbours fell back to Init as fast as they came up. They are
  queued as due times (`Sim::ospf_hello_later`) and built by `ospf_fire` when the clock reaches them. A new timer of this kind
  goes in `Sim::advance`'s list of next expiries, and clears itself so it can never spin.
- **A state kept by packets needs a value for "no packet yet".** `show ip ospf neighbor` shows an adjacency no packet has gone over
  as Full (the state the lab converges to), because a console test that configures and looks at once has no simulated time;
  the record appears with the first hello, so a test of the states runs the clock first and one of the converged view does not.
- **A BGP session needs simulated time before it carries a route.** A session takes 3 simulated seconds to reach
  Established, and the clock does not move while commands are typed, so a test or a lab that configures BGP and looks
  at the routing table at once finds nothing. Run `sim.advance` (`Sim::run_for` in a test) first. Keepalives follow
  the window rule above: one due at the 60 s tick is sent only if the session was up when the window began.
- **A packet changed after it was encoded needs its checksum zeroed before the new one is computed.** Signing an OSPF
  packet with a clear-text password rewrote the authentication field and summed the packet again with the old checksum
  still in it, so the packet failed its own checksum, and no test noticed because nothing decoded it. A signer gets a
  test that decodes what it signed (`wire/src/control.rs`).
- **A view of "what this router knows" must follow what reaches it, not what is configured.** `show ip ospf database`
  listed the LSAs of every router with a segment in an area, so a router cut off by a passive interface kept its router,
  summary and external LSAs for ever. A database view starts from the viewer and follows adjacencies
  (`ospf_lsdb::flooding_domain`).
- **A setting the simulator reads must be part of the fingerprint it recomputes on.** BGP and OSPF results are cached
  against `ospf::fingerprint`, which hashes the routers' configuration. Prefix lists, route maps and the access lists they
  name decide what BGP accepts and OSPF redistributes, so they are hashed there (only when a router has policy); a new
  setting that changes a computed result needs its line in the fingerprint, or the old result is shown until something
  else changes.
- **A rule at a switch port has to cover the timed control frames, which skip `emit_hops_in`.** BPDUs, LLDP and the
  routing hellos are queued by `control::queue_at`, straight into `push_hop`. Port mirroring first hooked only
  `emit_hops_in`, so a monitor destination kept sending BPDUs and a source port never copied them. Anything that decides
  what leaves a port (a mirror, a block, a counter) goes in both `emit_hops_in` and `queue_at`.
- **A routing protocol is computed through its public `compute`, never from the seeded function.** RIP, EIGRP and OSPF
  take the routes other protocols hand them (`redistribute`) from `redist::seeds`, which itself computes the three with
  `compute_seeded` until the seeds settle. Calling `compute_seeded` anywhere else, or reading a protocol's routes
  without the seeds, shows a router that ignores its own `redistribute` lines. A seed has no next hop, so it never
  appears as a route of the router that redistributes it; that, and taking seeds from routing tables (where the
  administrative distances have chosen), is what stops a route from coming back to the protocol it came from.
- **A BGP session whose path is lost is not down yet.** With a hold timer, a test that cuts a link and looks at once
  finds the session still Established (and its routes while their next hops resolve); it drops when the hold time runs
  out, so run `Sim::run_for` past it. A directly connected external neighbour whose interface goes down, a shutdown, a
  removed neighbour and `clear ip bgp` still end the session at once. A test of the hold timer needs a session that is
  not directly connected (`ebgp-multihop`, or an internal one) so the cut is not a fast fall-over.
- **Community lists and AS-path lists are read by the simulator, so they are in the routing fingerprint too.**
  `ospf::fingerprint` hashes them next to the prefix lists and route maps; a new list-like setting needs its line there.

- **A wait for an answer must look at what already arrived before it runs the clock.** `Sim::advance` checks its stop
  condition only after it delivers an event, and moves the clock to the deadline when nothing is left. A dialer that
  waits for its second packet after the first wait already received both runs two simulated seconds for nothing, or
  seems to time out. `Sim::pppoe_wait` takes a match from the inbox first. A conversation that ends with a frame the
  other side has not handled yet also needs the clock run a little before the lab says it is done (`pppoe.connect`
  does), or the last acknowledgement is handled by whatever command advances time next.
- **A BGP confederation session is `Peering::ebgp` but its paths are not `Path::ebgp`.** Between two member ASes the
  session is external to the member AS (it needs a directly connected neighbour, or `ebgp-multihop`, and a TTL of the
  hop count), but a path learned over it keeps its local preference and MED, counts as internal in the decision and
  has distance 200. `Path::ebgp` means learned from a neighbour outside the confederation, `Path::confed` from another
  member AS and `Path::internal()` from the same member AS. Testing `!path.ebgp` for "internal" lets a route reflector
  or an iBGP rule treat a confederation path wrongly; use `internal()`.

- **A layer-3 interface that exists because of a PPPoE session must be derivable from the data that is asked.**
  `l3_ifaces(device)` has no topology, so a router's `Dialer` interface comes from the dialer's own session record,
  and a concentrator's `Virtual-Access` interface from its configuration (the template's address); what a concentrator
  holds per session is derived from the callers' records (`sim::pppoe::calls`) wherever the topology is in hand
  (`up_ifaces`, `route_with`, `routing_table_static`). Do not keep a second copy of a session on the concentrator: it
  would go stale on undo and on a PADT.

- **A value with a space cannot ride in a request path.** The simulator's web server reads the second word of the
  request line as the path, so `GET /iot/register?name=M1&type=power meter&value=5` arrived as `type=power` with
  the reading cut off, and no test noticed because no sensor with a two-word type had its reading listed. Encode a
  space as `+` when the request is built and decode it where the query is read (`iot_report`, `iot_http`).
- **A raw string ends at the first `"#`.** A command description written as `r#"... "#rrggbb" ..."#` stops inside the
  text and breaks the whole file. Say `hex rrggbb` in a catalog description, or use `r##"…"##`.

- **A route that follows a tracked object cannot ask the routing table about itself.** `track N ip route …` reads the routing
  table, whose static rows read the object: `netlabcraft_sim::track` sets a thread-local flag while an object reads the
  table, and tracked routes count as up then. A tracked state that has delays is not a pure function of the lab: it is
  settled by `track::tick` after every engine command (`Session::apply_track`), and the simulator reads the settled
  state (`track::is_up`); a test that only runs console lines sees the live state.

## Device box

- **A model with a tile in more than one group is found more than once by a search.** Tiles are made per (model, group), so
  a search that filters the tiles by name lists the Smart LED three times. Search results keep the first tile of each tool
  (`bottom::show`), and the group test compares each subgroup's tiles with the models that list it, not with the models
  whose first group it is.

## Instructor

- **A key the student's app holds is not a secret from the student.** The results HMAC key comes from the salt in the
  file, so a signature never proved the score; what does is running the checks again against the network the file
  embeds (`activity.verify_results`). Do not add another "secret" to a file the student has: replay or move the check
  to the instructor.

## Drawing

- **A galley laid out with a colour keeps it; the colour given to `Painter::galley` is only a fallback.** The name
  plate under a rack device was laid out with the theme's text colour and painted with white over a dark plate, so in
  the light theme it was dark text on a dark plate, an empty black rectangle. Lay the text out in the colour that
  reads over its background (`theme::readable_on`), and test the contrast of what is painted. Drawing work gets looked
  at through the `Snapshots` workflow (`docs/extending.md`), not only through tests.

- **A tooltip written with `on_hover_text` ignores the tooltip size and colours of Preferences ▸ Font.** egui's popups
  share one style with every window, so the choices cannot be set once. Write tooltips with `.hint(..)` and
  `.disabled_hint(..)` (`prefs_look::HintExt`), which read them; a new file that uses them imports the trait.

## Desktop shell

- **Handing a file to `xdg-open` is not a way to print or save it.** File ▸ Print… used to write a temporary PDF and
  open it; with no PDF viewer registered, the only program that claims `application/pdf` (an image editor) opened it and
  asked to import it, with no way to keep or print the file, and the temporary file stayed in the temp folder. Always
  offer a save dialog next to "open", fall back to it when the opener fails, put temporary copies in one folder that is
  deleted when the app closes, and say where a file went in the status line.

## Wasm

- **`Instant::now()` and `SystemTime::now()` trap on `wasm32-unknown-unknown`.** They are disallowed in
  `clippy.toml`; simulated time lives in `netlabcraft-sim`, and the wall clock is read only off wasm.
- **`cargo xtask wasm` before you claim L0–L5 still builds there.** `process` compiles on wasm but only returns `Err`.

## Git

- **Never commit to `main`.** Work on a branch (`feature/…`), one task id per commit, and merge through a pull
  request. Check `git branch --show-current` before every commit and push by name.
- **The secret scan only protects clones that enable it.** Run `git config core.hooksPath .githooks` once per clone.
- **Deleting a branch closes its pull request.** Check open PRs before deleting a remote branch.
