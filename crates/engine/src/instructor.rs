//! Instructor tools for activities: the answer network (sealed in the file), the assessment
//! tree built from it, the initial network, instructions per language, connectivity tests,
//! signed results files and locks on what the student may change.

use netlabcraft_core::DeviceId;
use netlabcraft_model::activity::{Activity, Check, CheckKind, Sealed, Weight, line_hash, link_text};
use netlabcraft_model::{Device, DeviceKind, Topology};
use netlabcraft_wire::sha1::{hmac_sha1, pbkdf2_sha1, sha1};
use serde_json::{Value, json};

use crate::{EngineError, Result, Session};

/// PBKDF2 rounds for a key from the salt (it is in the file: this only slows a casual look)
/// and from the instructor's password.
const SALT_ROUNDS: u32 = 1_000;
const PASSWORD_ROUNDS: u32 = 20_000;

type Res<T> = std::result::Result<T, String>;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    let s = s.trim().as_bytes();
    if !s.len().is_multiple_of(2) {
        return None;
    }
    s.chunks(2).map(|c| std::str::from_utf8(c).ok().and_then(|x| u8::from_str_radix(x, 16).ok())).collect()
}

/// The encryption and MAC keys from a secret.
fn keys(secret: &[u8], salt: &str, rounds: u32) -> ([u8; 16], [u8; 20]) {
    let k = pbkdf2_sha1(secret, format!("netlabcraft answer\0{salt}").as_bytes(), rounds, 36);
    let (mut enc, mut mac) = ([0u8; 16], [0u8; 20]);
    for (o, x) in enc.iter_mut().zip(&k) {
        *o = *x;
    }
    for (o, x) in mac.iter_mut().zip(k.iter().skip(16)) {
        *o = *x;
    }
    (enc, mac)
}

/// AES-128 in counter mode (NIST SP 800-38A §6.5): the same call encrypts and decrypts.
fn ctr(key: &[u8; 16], nonce: &[u8; 16], data: &[u8]) -> Vec<u8> {
    let aes = netlabcraft_wire::aes::Aes128::new(key);
    let mut low = [0u8; 8];
    for (o, x) in low.iter_mut().zip(nonce.iter().skip(8)) {
        *o = *x;
    }
    let base = u64::from_be_bytes(low);
    let mut out = Vec::with_capacity(data.len());
    for (i, chunk) in data.chunks(16).enumerate() {
        let mut block = *nonce;
        for (o, x) in block.iter_mut().skip(8).zip(base.wrapping_add(i as u64).to_be_bytes()) {
            *o = x;
        }
        let stream = aes.encrypt(&block);
        out.extend(chunk.iter().zip(stream).map(|(a, b)| a ^ b));
    }
    out
}

/// How an activity keeps its answer network now: in the clear when it shows its answers,
/// under a key from the salt when it hides them, under the password while locked.
pub fn scheme_for(a: &Activity) -> &'static str {
    if a.locked() {
        "password"
    } else if a.hidden {
        "salt"
    } else {
        "plain"
    }
}

/// Seals a lab file's JSON. `password` is used by the `password` scheme.
///
/// Strength, honestly: `salt` keeps the lab out of sight of someone reading the file, but the
/// salt is in the same file and this code is open, so anyone who looks it up can open it.
/// `password` is AES-128-CTR with an HMAC-SHA1 tag under keys from PBKDF2-HMAC-SHA1 (20 000
/// rounds) of the instructor's password: as strong as the password is hard to guess.
pub fn seal(json: &str, scheme: &str, salt: &str, password: Option<&str>) -> Res<Sealed> {
    let (secret, rounds) = match scheme {
        "plain" => return Ok(Sealed { scheme: "plain".into(), data: json.to_string() }),
        "salt" => (salt.as_bytes().to_vec(), SALT_ROUNDS),
        "password" => (password.ok_or("the instructor's password is needed")?.as_bytes().to_vec(), PASSWORD_ROUNDS),
        other => return Err(format!("unknown sealing `{other}`")),
    };
    let (enc, mac) = keys(&secret, salt, rounds);
    let digest = sha1(&[salt.as_bytes(), b"\0", json.as_bytes()].concat());
    let mut nonce = [0u8; 16];
    for (o, x) in nonce.iter_mut().zip(digest) {
        *o = x;
    }
    let mut body = nonce.to_vec();
    body.extend(ctr(&enc, &nonce, json.as_bytes()));
    let tag = hmac_sha1(&mac, &body);
    body.extend_from_slice(&tag);
    Ok(Sealed { scheme: scheme.into(), data: hex(&body) })
}

/// Opens a sealed lab: its lab file's JSON.
pub fn unseal(s: &Sealed, salt: &str, password: Option<&str>) -> Res<String> {
    let (secret, rounds) = match s.scheme.as_str() {
        "plain" => return Ok(s.data.clone()),
        "salt" => (salt.as_bytes().to_vec(), SALT_ROUNDS),
        "password" => (password.ok_or("the answer network is locked with the instructor's password")?.as_bytes().to_vec(), PASSWORD_ROUNDS),
        other => return Err(format!("unknown sealing `{other}`")),
    };
    let bytes = unhex(&s.data).ok_or("the answer network is damaged")?;
    if bytes.len() < 36 {
        return Err("the answer network is damaged".into());
    }
    let (body, tag) = bytes.split_at(bytes.len() - 20);
    let (enc, mac) = keys(&secret, salt, rounds);
    if hmac_sha1(&mac, body).as_slice() != tag {
        return Err("the answer network doesn't open: wrong password or a damaged file".into());
    }
    let (n, ct) = body.split_at(16);
    let mut nonce = [0u8; 16];
    for (o, x) in nonce.iter_mut().zip(n) {
        *o = *x;
    }
    String::from_utf8(ctr(&enc, &nonce, ct)).map_err(|_| "the answer network is damaged".to_string())
}

/// Seals the answer again for how the activity is kept now (after hiding, locking, unlocking).
pub fn reseal(a: &mut Activity, old_password: Option<&str>, new_password: Option<&str>) -> Res<()> {
    let Some(sealed) = a.answer.clone() else { return Ok(()) };
    let json = unseal(&sealed, &a.salt, old_password)?;
    a.answer = Some(seal(&json, scheme_for(a), &a.salt, new_password)?);
    Ok(())
}

/// A lab without its activity, as a lab file's JSON.
pub fn lab_json(t: &Topology) -> Res<String> {
    let mut copy = t.clone();
    copy.activity = None;
    netlabcraft_model::file::to_json(&copy).map_err(|e| e.to_string())
}

/// A lab from a lab file's JSON (sanitized like any file).
pub fn lab_from_json(json: &str) -> Res<Topology> {
    let mut t = netlabcraft_model::file::from_json(json).map_err(|e| e.to_string())?;
    t.activity = None;
    Ok(t)
}

/// The answer network, opened.
pub fn answer_network(a: &Activity, password: Option<&str>) -> Res<Topology> {
    let sealed = a.answer.as_ref().ok_or("this activity has no answer network: `activity.save_answer` first")?;
    lab_from_json(&unseal(sealed, &a.salt, password)?)
}

/// The initial network, if the activity has one.
pub fn initial_network(a: &Activity) -> Option<Res<Topology>> {
    a.initial.as_ref().map(|v| lab_from_json(&v.to_string()))
}

/// The hash a lock password is kept as (`p2$` and PBKDF2-HMAC-SHA1).
pub fn lock_hash(a: &Activity, password: &str) -> String {
    format!("p2${}", hex(&pbkdf2_sha1(password.as_bytes(), format!("netlabcraft lock\0{}", a.salt).as_bytes(), PASSWORD_ROUNDS, 20)))
}

/// Whether a password opens the lock (also locks from before, kept as a short hash).
pub fn lock_matches(a: &Activity, password: &str) -> bool {
    if a.lock.starts_with("p2$") { lock_hash(a, password) == a.lock } else { a.password_hash(password) == a.lock }
}

/// One item of the assessment tree: something in the answer network worth points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeItem {
    /// Stable and opaque (a salted hash): the weights are kept under it.
    pub key: String,
    pub device: String,
    /// `config`, `address` or `cable`.
    pub kind: &'static str,
    /// What it is, as the instructor reads it.
    pub text: String,
    /// 0: not graded.
    pub points: u32,
    pub feedback: String,
    /// The check it becomes.
    pub check: Check,
}

/// The assessment tree from the answer network: per device, its configuration items, its
/// address and its cables, with the weights the activity keeps.
pub fn tree(answer: &Topology, a: &Activity) -> Vec<TreeItem> {
    let salt = &a.salt;
    let mut out = Vec::new();
    let mut push = |dev: &Device, kind: &'static str, key_text: &str, text: String, check_kind: CheckKind, label: String| {
        let key = line_hash(salt, dev.id, &format!("tree {key_text}"));
        let w = a.assessment.get(&key);
        let points = w.map_or(1, |w| w.points);
        let feedback = w.map(|w| w.feedback.clone()).unwrap_or_default();
        let check = Check { label, points: points.max(1), section: dev.name.clone(), feedback: feedback.clone(), kind: check_kind };
        out.push(TreeItem { key, device: dev.name.clone(), kind, text, points, feedback, check });
    };
    for d in &answer.devices {
        match d.kind {
            DeviceKind::Router | DeviceKind::Switch => {
                for (n, line) in crate::activity::answer_lines(d).into_iter().enumerate() {
                    let (label, kind) = if a.hidden {
                        (
                            format!("{}: configuration item {}", d.name, n + 1),
                            CheckKind::ConfigHashed { device: d.id, hash: line_hash(salt, d.id, &line) },
                        )
                    } else {
                        (format!("{}: {line}", d.name), CheckKind::Config { device: d.id, line: line.clone() })
                    };
                    push(d, "config", &line, line.clone(), kind, label);
                }
            }
            k if k.is_host() => {
                if let Some(ip) = d.ports.first().and_then(|p| p.ip) {
                    let gw = d.gateway.map(|g| g.to_string());
                    let text = match &gw {
                        Some(g) => format!("{ip} via {g}"),
                        None => ip.to_string(),
                    };
                    let label = if a.hidden { format!("{}: address", d.name) } else { format!("{}: address {ip}", d.name) };
                    let kind = CheckKind::HostAddress { device: d.id, address: ip.to_string(), gateway: gw };
                    push(d, "address", &format!("address {text}"), text.clone(), kind, label);
                }
            }
            _ => {}
        }
        let mut n = 0;
        for l in &answer.links {
            let (Some(da), Some(db)) = (answer.device(l.a.device), answer.device(l.b.device)) else { continue };
            let (Some(pa), Some(pb)) = (da.ports.get(l.a.port), db.ports.get(l.b.port)) else { continue };
            let (owner, text) = link_text(da.id, &pa.name, db.id, &pb.name);
            if owner != d.id {
                continue;
            }
            n += 1;
            let (here, here_port, there, there_port) = if da.id == d.id { (da, pa, db, pb) } else { (db, pb, da, pa) };
            let shown = format!("{} {} - {} {}", here.name, here_port.name, there.name, there_port.name);
            let (label, kind) = if a.hidden {
                (format!("{}: cable {n}", d.name), CheckKind::LinkHashed { device: d.id, hash: line_hash(salt, d.id, &text) })
            } else {
                (
                    format!("{shown} cabled"),
                    CheckKind::Link { a: here.id, a_port: here_port.name.clone(), b: there.id, b_port: there_port.name.clone() },
                )
            };
            push(d, "cable", &text, shown, kind, label);
        }
    }
    out
}

/// The checks the tree's graded items become.
pub fn tree_checks(items: &[TreeItem]) -> Vec<Check> {
    items.iter().filter(|i| i.points > 0).map(|i| i.check.clone()).collect()
}

/// Every cable of a lab in the compared form.
fn cable_texts(t: &Topology) -> Vec<(DeviceId, String)> {
    t.links
        .iter()
        .filter_map(|l| {
            let pa = t.device(l.a.device)?.ports.get(l.a.port)?;
            let pb = t.device(l.b.device)?.ports.get(l.b.port)?;
            Some(link_text(l.a.device, &pa.name, l.b.device, &pb.name))
        })
        .collect()
}

/// Whether a cable check passes in a lab: (passed, expected, what the lab has).
pub fn eval_cable(t: &Topology, kind: &CheckKind, salt: &str) -> Option<(bool, String, String)> {
    let cables = cable_texts(t);
    match kind {
        CheckKind::Link { a, a_port, b, b_port } => {
            let ok = cables.contains(&link_text(*a, a_port, *b, b_port));
            let name = |d: DeviceId| t.device(d).map_or_else(|| format!("#{}", d.0), |x| x.name.clone());
            let want = format!("{} {a_port} - {} {b_port}", name(*a), name(*b));
            Some((ok, want, if ok { "cabled".into() } else { "no such cable".into() }))
        }
        CheckKind::LinkHashed { device, hash } => {
            let ok = cables.iter().any(|(d, text)| d == device && line_hash(salt, *d, text) == *hash);
            let n = cables.iter().filter(|(d, _)| d == device).count();
            Some((ok, String::new(), format!("{n} cables")))
        }
        _ => None,
    }
}

/// An activity's id, as results files carry it.
pub fn activity_id(a: &Activity) -> String {
    hex(sha1(format!("netlabcraft activity\0{}", a.salt).as_bytes()).get(..8).unwrap_or_default())
}

/// The key results files are signed with (from the salt). Honestly: the salt is in the
/// activity file, so this catches a results file edited by hand, not a student who studies
/// the source and the activity file to forge one.
fn results_key(a: &Activity) -> [u8; 20] {
    sha1(format!("netlabcraft results\0{}", a.salt).as_bytes())
}

pub fn sign(a: &Activity, body: &str) -> String {
    hex(&hmac_sha1(&results_key(a), body.as_bytes()))
}

/// The time now as `2026-10-30T12:00:00Z` (empty where the platform has no clock).
// The wall clock is read only off wasm (see `clippy.toml`).
#[allow(clippy::disallowed_methods)]
pub fn now_utc() -> String {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let Ok(d) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) else { return String::new() };
        let secs = d.as_secs();
        let days = i64::try_from(secs / 86_400).unwrap_or(0);
        let rem = secs % 86_400;
        // Days to a civil date (proleptic Gregorian calendar).
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = yoe + era * 400 + i64::from(month <= 2);
        format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem / 60 % 60, rem % 60)
    }
    #[cfg(target_arch = "wasm32")]
    {
        String::new()
    }
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() }
}

/// The version of the results file (JSON). Version 2 embeds the student's final network so
/// `verify_results` can replay the checks; version 1 files (no network) still verify, without replay.
pub const RESULTS_VERSION: u64 = 2;

/// The largest embedded network a results file may carry, as compact JSON.
const MAX_NETWORK_BYTES: usize = 4 * 1024 * 1024;

/// The most mismatches a verification lists.
const MAX_MISMATCHES: usize = 20;

/// The student's lab as it goes into a results file: the lab file format, compact, without the
/// activity (the verifier already has that).
pub fn network_value(t: &Topology) -> Res<Value> {
    serde_json::from_str(&lab_json(t)?).map_err(|e| e.to_string())
}

/// A results file (`json` or `csv`) from `activity.check`'s answer, signed. A JSON file also
/// embeds `network`, the student's final lab, so the score can be replayed.
pub fn results_file(a: &Activity, report: &Value, format: &str, time: &str, elapsed: Option<u64>, network: Option<Value>) -> Res<String> {
    let student = a.student.clone().unwrap_or_default();
    let id = activity_id(a);
    let results = report.get("results").and_then(Value::as_array).cloned().unwrap_or_default();
    let sections = report.get("sections").and_then(Value::as_array).cloned().unwrap_or_default();
    match format {
        "json" => {
            let mut m = serde_json::Map::new();
            m.insert("format".into(), "netlabcraft-results".into());
            m.insert("version".into(), RESULTS_VERSION.into());
            m.insert("activity_id".into(), id.into());
            m.insert("title".into(), a.title.clone().into());
            m.insert("student".into(), student.into());
            m.insert("time".into(), time.into());
            m.insert("elapsed_s".into(), elapsed.map_or(Value::Null, Into::into));
            for k in ["score", "total", "percent"] {
                m.insert(k.into(), report.get(k).cloned().unwrap_or(Value::Null));
            }
            m.insert("sections".into(), sections.into());
            let results: Vec<Value> =
                results.iter().map(|r| json!({"label": r["label"], "section": r["section"], "points": r["points"], "passed": r["passed"]})).collect();
            m.insert("results".into(), results.into());
            if let Some(n) = network {
                m.insert("network".into(), n);
            }
            let body = Value::Object(m.clone()).to_string();
            // Last, so taking it out again leaves the rest in the same order.
            m.insert("hmac".into(), sign(a, &body).into());
            serde_json::to_string_pretty(&Value::Object(m)).map_err(|e| e.to_string())
        }
        "csv" => {
            let num = |k: &str| report.get(k).and_then(Value::as_u64).unwrap_or(0);
            let mut out = String::from("# NetLabCraft activity results\n");
            for (k, v) in [("activity_id", id), ("title", a.title.clone()), ("student", student), ("time", time.to_string())] {
                out.push_str(&format!("{k},{}\n", csv_field(&v)));
            }
            out.push_str(&format!("elapsed_s,{}\n", elapsed.map(|e| e.to_string()).unwrap_or_default()));
            out.push_str(&format!("score,{}\ntotal,{}\npercent,{}\n", num("score"), num("total"), num("percent")));
            out.push_str("section,name,score,total,percent\n");
            for s in &sections {
                out.push_str(&format!(
                    "section,{},{},{},{}\n",
                    csv_field(s["name"].as_str().unwrap_or("")),
                    s["score"].as_u64().unwrap_or(0),
                    s["total"].as_u64().unwrap_or(0),
                    s["percent"].as_u64().unwrap_or(0)
                ));
            }
            out.push_str("check,label,section,points,passed\n");
            for r in &results {
                out.push_str(&format!(
                    "check,{},{},{},{}\n",
                    csv_field(r["label"].as_str().unwrap_or("")),
                    csv_field(r["section"].as_str().unwrap_or("")),
                    r["points"].as_u64().unwrap_or(0),
                    r["passed"].as_bool().unwrap_or(false)
                ));
            }
            let mac = sign(a, &out);
            out.push_str(&format!("hmac,{mac}\n"));
            Ok(out)
        }
        other => Err(format!("format `{other}`: json or csv")),
    }
}

/// What a results file says, and whether it holds for this activity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verified {
    /// The file is for this activity, its signature holds and, when it was replayed, the replay
    /// agrees with what it claims.
    pub valid: bool,
    pub same_activity: bool,
    /// The HMAC holds (it catches hand edits only).
    pub signature_ok: bool,
    /// The checks were run again against the embedded network.
    pub replayed: bool,
    /// `Some(true)`: the replay agrees; `Some(false)`: it does not, or the network was refused;
    /// `None`: not replayed.
    pub replay_matches: Option<bool>,
    /// The score the replay earned (equal to `score` when the file is honest).
    pub replay_score: Option<u64>,
    pub replay_total: Option<u64>,
    /// What differed (or why the embedded network was refused), at most [`MAX_MISMATCHES`].
    pub mismatches: Vec<String>,
    /// One plain sentence on what was and was not checked.
    pub message: String,
    pub student: String,
    pub score: u64,
    pub total: u64,
    pub percent: u64,
}

/// Runs the activity's checks against an embedded network, on a fresh simulator as a reopened
/// lab has, and compares with what the file claims. Never trusts the file: the network is
/// size-capped, sanitized like any lab file, and the simulator's own caps bound the work.
fn replay(a: &Activity, network: &Value, claimed: &serde_json::Map<String, Value>, out: &mut Verified) {
    let text = network.to_string();
    if text.len() > MAX_NETWORK_BYTES {
        out.replay_matches = Some(false);
        out.mismatches.push(format!("the embedded network is too large ({} bytes, at most {MAX_NETWORK_BYTES})", text.len()));
        return;
    }
    let t = match lab_from_json(&text) {
        Ok(t) => t,
        Err(e) => {
            out.replay_matches = Some(false);
            out.mismatches.push(format!("the embedded network is not a lab: {e}"));
            return;
        }
    };
    let mut sim = netlabcraft_sim::Sim::new();
    sim.topology_changed(&t);
    let results = crate::activity::evaluate(&t, &sim, &a.resolved_checks(), &a.salt, a.hidden);
    let total: u64 = results.iter().map(|r| u64::from(r.points)).sum();
    let score: u64 = results.iter().filter(|r| r.passed).map(|r| u64::from(r.points)).sum();
    let percent = (score * 100).checked_div(total).unwrap_or(0);
    out.replayed = true;
    out.replay_score = Some(score);
    out.replay_total = Some(total);
    let mut diffs: Vec<String> = Vec::new();
    for (k, want) in [("score", score), ("total", total), ("percent", percent)] {
        let said = claimed.get(k).and_then(Value::as_u64);
        if said != Some(want) {
            diffs.push(format!(
                "{k}: the file says {}, the embedded network earns {want}",
                said.map_or_else(|| "nothing".to_string(), |v| v.to_string())
            ));
        }
    }
    // Points per item, when the file lists them (a score-only activity lists none).
    let listed = claimed.get("results").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    if !listed.is_empty() {
        if listed.len() != results.len() {
            diffs.push(format!("the file lists {} items, the activity has {}", listed.len(), results.len()));
        }
        for (i, (r, c)) in results.iter().zip(listed).enumerate() {
            let same = c.get("label").and_then(Value::as_str) == Some(r.label.as_str())
                && c.get("points").and_then(Value::as_u64) == Some(u64::from(r.points))
                && c.get("passed").and_then(Value::as_bool) == Some(r.passed);
            if !same {
                diffs.push(format!(
                    "item {} (`{}`): the file says {}, the embedded network {}",
                    i + 1,
                    r.label,
                    if c.get("passed").and_then(Value::as_bool) == Some(true) { "passed" } else { "not passed" },
                    if r.passed { "passes" } else { "does not pass" }
                ));
            }
        }
    }
    out.replay_matches = Some(diffs.is_empty());
    diffs.truncate(MAX_MISMATCHES);
    out.mismatches = diffs;
}

/// Checks a results file against this activity: that it is this activity's, that the signature
/// holds (hand edits), and, for a JSON file that embeds the student's network, that running the
/// checks again earns the claimed score (forgery). Time claims are not replayable.
pub fn verify_results(a: &Activity, text: &str) -> Res<Verified> {
    if text.len() > 8 * 1024 * 1024 {
        return Err("that is too large to be a results file".into());
    }
    let id = activity_id(a);
    let mut out = Verified {
        valid: false,
        same_activity: false,
        signature_ok: false,
        replayed: false,
        replay_matches: None,
        replay_score: None,
        replay_total: None,
        mismatches: Vec::new(),
        message: String::new(),
        student: String::new(),
        score: 0,
        total: 0,
        percent: 0,
    };
    let csv = !text.trim_start().starts_with('{');
    if csv {
        let at = text.rfind("hmac,").ok_or("not a results file: it has no signature")?;
        let (body, tail) = text.split_at(at);
        let mac = tail.trim_start_matches("hmac,").trim();
        for line in body.lines() {
            let (k, v) = line.split_once(',').unwrap_or((line, ""));
            match k {
                "student" => out.student = v.trim_matches('"').to_string(),
                "score" => out.score = v.trim().parse().unwrap_or(0),
                "total" => out.total = v.trim().parse().unwrap_or(0),
                "percent" => out.percent = v.trim().parse().unwrap_or(0),
                "activity_id" => out.same_activity = v.trim() == id,
                _ => {}
            }
        }
        out.signature_ok = out.same_activity && sign(a, body) == mac;
    } else {
        let v: Value = serde_json::from_str(text).map_err(|e| format!("not a results file: {e}"))?;
        let mut m = v.as_object().cloned().ok_or("not a results file")?;
        if m.get("version").and_then(Value::as_u64).is_some_and(|v| v > RESULTS_VERSION) {
            return Err("this results file was written by a newer NetLabCraft".into());
        }
        let mac = m.remove("hmac").and_then(|x| x.as_str().map(str::to_string)).unwrap_or_default();
        let body = Value::Object(m.clone()).to_string();
        out.same_activity = m.get("activity_id").and_then(Value::as_str) == Some(id.as_str());
        out.signature_ok = out.same_activity && sign(a, &body) == mac;
        let get = |k: &str| m.get(k).and_then(Value::as_u64).unwrap_or(0);
        out.student = m.get("student").and_then(Value::as_str).unwrap_or("").to_string();
        out.score = get("score");
        out.total = get("total");
        out.percent = get("percent");
        // A file of another activity would be replayed against the wrong checks.
        if out.same_activity
            && let Some(n) = m.get("network").filter(|n| !n.is_null())
        {
            replay(a, n, &m, &mut out);
        }
    }
    out.valid = out.signature_ok && out.replay_matches != Some(false);
    out.message = if !out.same_activity {
        "This file belongs to another activity."
    } else if !out.signature_ok {
        "The signature does not hold: the file was changed after it was saved."
    } else if out.replay_matches == Some(false) {
        "The score was replayed against the embedded network and does not match, or the network was refused: this file is forged or damaged."
    } else if out.replayed {
        "The signature holds and the score, replayed against the student's embedded network, matches. Time and elapsed-time claims are not checked."
    } else if csv {
        "Not replayed: a CSV file cannot carry the student's network, so it is only checked for hand edits, not forgery."
    } else {
        "Not replayed: this file has no embedded network (an older export), so it is only checked for hand edits, not forgery."
    }
    .to_string();
    Ok(out)
}

/// A device's settings as a lock compares them (where it stands is not configuration).
fn device_settings(d: &Device) -> Value {
    let mut v = serde_json::to_value(d).unwrap_or_default();
    if let Some(m) = v.as_object_mut() {
        m.remove("x");
        m.remove("y");
    }
    v
}

/// Commands a lock never refuses: they replace the whole lab.
pub const LOCK_FREE: &[&str] = &["file.new", "file.open", "activity.reset"];

/// What a command did that the activity's locks forbid, comparing the lab before and after.
pub fn lock_violation(before: &Topology, after: &Topology) -> Option<String> {
    let a = before.activity.as_ref().filter(|a| a.locks_active())?;
    after.activity.as_ref().filter(|x| x.locks_active())?;
    let ids = |t: &Topology| t.devices.iter().map(|d| d.id).collect::<std::collections::BTreeSet<_>>();
    if a.locks.devices && ids(before) != ids(after) {
        return Some("the activity doesn't let you add or remove devices".into());
    }
    if a.locks.cables {
        let set = |t: &Topology| {
            let mut v: Vec<String> = t.links.iter().map(|l| serde_json::to_string(l).unwrap_or_default()).collect();
            v.extend(t.consoles.iter().map(|l| serde_json::to_string(l).unwrap_or_default()));
            v.sort();
            v
        };
        if set(before) != set(after) {
            return Some("the activity doesn't let you change the cables".into());
        }
    }
    for id in &a.locks.config {
        let Some(x) = before.device(*id) else { continue };
        if after.device(*id).is_none_or(|y| device_settings(x) != device_settings(y)) {
            return Some(format!("the activity doesn't let you change {}'s configuration", x.name));
        }
    }
    None
}

// ---------------------------------------------------------------------------------------------
// The commands.
// ---------------------------------------------------------------------------------------------

/// The instructor's commands, refused while the activity is locked.
const INSTRUCTOR: &[&str] = &[
    "activity.save_answer",
    "activity.load_answer",
    "activity.clear_answer",
    "activity.answer",
    "activity.tree",
    "activity.weight",
    "activity.tree_checks",
    "activity.save_initial",
    "activity.load_initial",
    "activity.clear_initial",
    "activity.edit_check",
    "activity.locks",
    "activity.translate",
];

fn bad(msg: impl Into<String>) -> EngineError {
    EngineError::BadParams(msg.into())
}

fn failed(msg: String) -> EngineError {
    EngineError::Failed(msg)
}

fn activity(s: &Session) -> Result<&Activity> {
    s.topology.activity.as_ref().ok_or_else(|| bad("this lab has no activity"))
}

/// Puts a network in the workspace, keeping the activity (to edit the answer or the initial
/// network, and for Reset Activity).
fn show_network(s: &mut Session, mut t: Topology) {
    t.activity = s.topology.activity.clone();
    s.checkpoint();
    s.topology = t;
    s.changed();
}

/// Changes the activity on a copy of the lab, then keeps it as one undo step.
fn edit(s: &mut Session, f: impl FnOnce(&mut Activity) -> Result<Value>) -> Result<Value> {
    let mut t = s.topology.clone();
    let a = t.activity.as_mut().ok_or_else(|| bad("this lab has no activity"))?;
    let out = f(a)?;
    s.checkpoint();
    s.topology = t;
    s.changed();
    Ok(out)
}

fn tree_json(items: &[TreeItem]) -> Value {
    json!(
        items
            .iter()
            .map(|i| json!({"key": i.key, "device": i.device, "kind": i.kind, "text": i.text, "points": i.points, "feedback": i.feedback}))
            .collect::<Vec<_>>()
    )
}

/// Runs an instructor command; `None` for ids it doesn't own.
pub fn run(s: &mut Session, id: &str, p: &Value) -> Option<Result<Value>> {
    if !id.starts_with("activity.") {
        return None;
    }
    if INSTRUCTOR.contains(&id) && s.topology.activity.as_ref().is_some_and(Activity::locked) {
        return Some(Err(bad("the activity is locked: `activity.unlock` with the instructor's password first")));
    }
    let r = match id {
        "activity.save_answer" => save_answer(s),
        "activity.answer" => activity(s).and_then(|a| answer_network(a, None).map_err(failed)).and_then(|t| {
            let json = lab_json(&t).map_err(failed)?;
            Ok(json!({"lab": json, "devices": t.devices.len(), "links": t.links.len(), "scheme": a_scheme(s)}))
        }),
        "activity.load_answer" => activity(s).and_then(|a| answer_network(a, None).map_err(failed)).map(|t| {
            show_network(s, t);
            json!({"devices": s.topology.devices.len()})
        }),
        "activity.clear_answer" => edit(s, |a| {
            a.answer = None;
            Ok(json!({}))
        }),
        "activity.tree" => activity(s).and_then(|a| {
            let t = answer_network(a, None).map_err(failed)?;
            let items = tree(&t, a);
            let graded: u32 = items.iter().map(|i| i.points).fold(0, u32::saturating_add);
            Ok(json!({"items": tree_json(&items), "points": graded}))
        }),
        "activity.weight" => weight(s, p),
        "activity.tree_checks" => tree_to_checks(s),
        "activity.save_initial" => lab_json(&s.topology).map_err(failed).and_then(|json| {
            let v: Value = serde_json::from_str(&json).map_err(|e| failed(e.to_string()))?;
            let n = s.topology.devices.len();
            edit(s, |a| {
                a.initial = Some(v);
                Ok(json!({"devices": n}))
            })
        }),
        "activity.load_initial" => match activity(s).map(initial_network) {
            Ok(Some(Ok(t))) => {
                show_network(s, t);
                Ok(json!({"devices": s.topology.devices.len()}))
            }
            Ok(Some(Err(e))) => Err(failed(e)),
            Ok(None) => Err(bad("this activity has no initial network: `activity.save_initial` first")),
            Err(e) => Err(e),
        },
        "activity.clear_initial" => edit(s, |a| {
            a.initial = None;
            Ok(json!({}))
        }),
        "activity.edit_check" => edit_check(s, p),
        "activity.locks" => set_locks(s, p),
        "activity.connectivity" => connectivity(s),
        "activity.export_results" => export_results(s, p),
        "activity.verify_results" => verify(s, p),
        "activity.translate" => translate(s, p),
        _ => return None,
    };
    Some(r)
}

fn a_scheme(s: &Session) -> String {
    s.topology.activity.as_ref().and_then(|a| a.answer.as_ref()).map(|x| x.scheme.clone()).unwrap_or_default()
}

fn save_answer(s: &mut Session) -> Result<Value> {
    activity(s)?;
    let json = lab_json(&s.topology).map_err(failed)?;
    let n = s.topology.devices.len();
    edit(s, |a| {
        let sealed = seal(&json, scheme_for(a), &a.salt, None).map_err(failed)?;
        let scheme = sealed.scheme.clone();
        a.answer = Some(sealed);
        Ok(json!({"devices": n, "scheme": scheme}))
    })
}

fn weight(s: &mut Session, p: &Value) -> Result<Value> {
    let key = p.get("key").and_then(Value::as_str).ok_or_else(|| bad("`key` (an assessment tree item's key) is required"))?.to_string();
    let a = activity(s)?;
    let t = answer_network(a, None).map_err(failed)?;
    let item = tree(&t, a).into_iter().find(|i| i.key == key).ok_or_else(|| bad(format!("no assessment tree item `{key}`")))?;
    let points = match p.get("points") {
        Some(v) => v.as_u64().map(|n| n.min(1_000) as u32).ok_or_else(|| bad("`points` is a number (0: not graded)"))?,
        None => item.points,
    };
    let feedback = p.get("feedback").and_then(Value::as_str).map_or(item.feedback, |f| f.chars().take(1_000).collect());
    edit(s, |a| {
        if a.assessment.len() >= netlabcraft_model::activity::MAX_WEIGHTS && !a.assessment.contains_key(&key) {
            return Err(bad("too many assessment weights"));
        }
        if points == 1 && feedback.is_empty() {
            a.assessment.remove(&key);
        } else {
            a.assessment.insert(key.clone(), Weight { points, feedback: feedback.clone() });
        }
        Ok(json!({"key": key, "points": points, "feedback": feedback}))
    })
}

fn tree_to_checks(s: &mut Session) -> Result<Value> {
    let a = activity(s)?;
    let t = answer_network(a, None).map_err(failed)?;
    let mut checks = tree_checks(&tree(&t, a));
    // Pings and command checks aren't in the tree: they stay.
    checks.extend(a.checks.iter().filter(|c| matches!(c.kind, CheckKind::Ping { .. } | CheckKind::Output { .. })).cloned());
    checks.truncate(netlabcraft_model::activity::MAX_CHECKS);
    let n = checks.len();
    edit(s, |a| {
        a.checks = checks;
        Ok(json!({"checks": n, "points": a.total_points()}))
    })
}

fn edit_check(s: &mut Session, p: &Value) -> Result<Value> {
    let i = p.get("index").and_then(Value::as_u64).ok_or_else(|| bad("`index` is required"))?;
    edit(s, |a| {
        let c = usize::try_from(i).ok().and_then(|i| a.checks.get_mut(i)).ok_or_else(|| bad(format!("no check {i}")))?;
        if let Some(n) = p.get("points").and_then(Value::as_u64) {
            c.points = n.clamp(1, 1_000) as u32;
        }
        if let Some(f) = p.get("feedback").and_then(Value::as_str) {
            c.feedback = f.chars().take(1_000).collect();
        }
        if let Some(l) = p.get("label").and_then(Value::as_str).filter(|l| !l.trim().is_empty()) {
            c.label = l.chars().take(300).collect();
        }
        if let Some(sec) = p.get("section").and_then(Value::as_str) {
            c.section = sec.trim().chars().take(64).collect();
        }
        Ok(json!({"label": c.label, "points": c.points, "feedback": c.feedback, "section": c.section}))
    })
}

fn set_locks(s: &mut Session, p: &Value) -> Result<Value> {
    let config = match p.get("config") {
        Some(Value::Array(list)) => {
            let mut ids = Vec::new();
            for v in list.iter().take(netlabcraft_model::MAX_DEVICES) {
                let one = json!({"d": v});
                let id = crate::cmd::device_param(s, &one, "d")?;
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
            Some(ids)
        }
        Some(_) => return Err(bad("`config` is a list of devices")),
        None => None,
    };
    edit(s, |a| {
        if let Some(b) = p.get("devices").and_then(Value::as_bool) {
            a.locks.devices = b;
        }
        if let Some(b) = p.get("cables").and_then(Value::as_bool) {
            a.locks.cables = b;
        }
        if let Some(ids) = config {
            a.locks.config = ids;
        }
        Ok(json!({"devices": a.locks.devices, "cables": a.locks.cables, "config": a.locks.config.iter().map(|d| d.0).collect::<Vec<_>>()}))
    })
}

/// The activity's ping checks, run now.
fn connectivity(s: &Session) -> Result<Value> {
    let a = activity(s)?;
    if a.needs_student() {
        return Err(bad("start the activity first: `activity.start` with the student's name"));
    }
    let detail = !(a.locked() && a.score_only);
    let pings: Vec<Check> = a.resolved_checks().into_iter().filter(|c| matches!(c.kind, CheckKind::Ping { .. })).collect();
    let results = crate::activity::evaluate(&s.topology, &s.sim, &pings, &a.salt, a.hidden);
    let name = |d: DeviceId| s.topology.device(d).map_or_else(|| format!("#{}", d.0), |x| x.name.clone());
    let tests: Vec<Value> = pings
        .iter()
        .zip(&results)
        .map(|(c, r)| {
            let (from, to, succeed) = match &c.kind {
                CheckKind::Ping { from, to, succeed } => (name(*from), to.clone(), *succeed),
                _ => (String::new(), String::new(), true),
            };
            json!({"label": r.label, "from": from, "to": to, "expect": if succeed { "replies" } else { "no replies" }, "passed": if detail { json!(r.passed) } else { Value::Null }, "actual": if detail { json!(r.actual) } else { Value::Null }})
        })
        .collect();
    let passed = results.iter().filter(|r| r.passed).count();
    Ok(json!({"tests": tests, "passed": if detail { json!(passed) } else { Value::Null }, "total": tests.len(), "detail": detail}))
}

fn export_results(s: &mut Session, p: &Value) -> Result<Value> {
    let format = p.get("format").and_then(Value::as_str).unwrap_or("json");
    if !matches!(format, "json" | "csv") {
        return Err(bad(format!("format `{format}`: json or csv")));
    }
    let report = crate::cmd::run(s, "activity.check", &json!({}))?;
    let a = activity(s)?;
    let time = match p.get("time").and_then(Value::as_str) {
        Some(t) => t.chars().take(64).collect(),
        None => now_utc(),
    };
    let elapsed = p.get("elapsed_s").and_then(Value::as_u64);
    let network = if format == "json" { Some(network_value(&s.topology).map_err(failed)?) } else { None };
    let text = results_file(a, &report, format, &time, elapsed, network).map_err(failed)?;
    // Replayed as the verifier will, so the student hears now if the file would not match.
    let replay_agrees = verify_results(a, &text).ok().map(|v| v.replay_matches != Some(false));
    if let Some(path) = p.get("path").and_then(Value::as_str) {
        crate::cmd::write_file(path, text.as_bytes())?;
    }
    let name = format!("results-{}.{format}", a.student.as_deref().unwrap_or("student").replace(|c: char| !c.is_alphanumeric(), "_"));
    Ok(
        json!({"text": text, "file_name": name, "score": report["score"], "total": report["total"], "percent": report["percent"], "replayable": format == "json", "replay_agrees": replay_agrees}),
    )
}

fn verify(s: &Session, p: &Value) -> Result<Value> {
    let text = match (p.get("text").and_then(Value::as_str), p.get("path").and_then(Value::as_str)) {
        (Some(t), _) => t.to_string(),
        (None, Some(path)) => crate::cmd::read_file(path)?,
        _ => return Err(bad("give `text` (the results file) or `path`")),
    };
    let v = verify_results(activity(s)?, &text).map_err(failed)?;
    Ok(json!({
        "valid": v.valid, "same_activity": v.same_activity, "signature_ok": v.signature_ok, "replayed": v.replayed,
        "replay_matches": v.replay_matches, "replay_score": v.replay_score, "replay_total": v.replay_total,
        "mismatches": v.mismatches, "message": v.message,
        "student": v.student, "score": v.score, "total": v.total, "percent": v.percent
    }))
}

/// Instructions in one language: `lang` and `text` (empty removes the translation); `default`
/// true makes `lang` the language of the main text.
fn translate(s: &mut Session, p: &Value) -> Result<Value> {
    let lang: String =
        p.get("lang").and_then(Value::as_str).ok_or_else(|| bad("`lang` (a language code such as es) is required"))?.trim().to_lowercase();
    if lang.is_empty() || lang.len() > 16 || !lang.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(bad("`lang` is a language code such as en, es or pt-br"));
    }
    let text = p.get("text").and_then(Value::as_str).map(|t| t.chars().take(netlabcraft_model::activity::MAX_TEXT).collect::<String>());
    let make_default = p.get("default").and_then(Value::as_bool).unwrap_or(false);
    edit(s, |a| {
        if make_default && lang != a.default_language() {
            // The old main text becomes a translation; the new language's text (if any) the main.
            let old = a.default_language().to_string();
            let main = std::mem::take(&mut a.instructions);
            if !main.is_empty() {
                a.translations.insert(old, main);
            }
            a.instructions = a.translations.remove(&lang).unwrap_or_default();
            a.language = lang.clone();
        }
        if let Some(text) = text {
            if lang == a.default_language() {
                a.instructions = text;
            } else if text.trim().is_empty() {
                a.translations.remove(&lang);
            } else {
                if a.translations.len() >= netlabcraft_model::activity::MAX_LANGUAGES && !a.translations.contains_key(&lang) {
                    return Err(bad("too many languages"));
                }
                a.translations.insert(lang.clone(), text);
            }
        }
        Ok(json!({"languages": a.languages(), "default": a.default_language()}))
    })
}
