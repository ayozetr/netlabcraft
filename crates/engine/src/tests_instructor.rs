//! Instructor tools: answer network, assessment tree, initial network, languages, results
//! files and locks.

use serde_json::json;

use crate::Session;
use crate::instructor::{seal, unseal};

const SOLVE: &str = "en\nconf t\nhostname R1\nenable secret class\nint g0/0\nip add 192.168.10.1 255.255.255.0\nno shut\nint g0/1\nip add 192.168.20.1 255.255.255.0\nno shut\nend";

fn sample() -> Session {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "activity-first-router"})).unwrap();
    s
}

fn reopen(s: &mut Session) -> Session {
    let file = s.execute("file.save", json!({})).unwrap()["json"].as_str().unwrap().to_string();
    let mut student = Session::new();
    student.execute("file.open", json!({"json": file})).unwrap();
    student
}

#[test]
fn sealing_round_trips_and_refuses_the_wrong_key() {
    let json = r#"{"secret": "hostname R1"}"#;
    for scheme in ["plain", "salt", "password"] {
        let sealed = seal(json, scheme, "salty", Some("teacher")).unwrap();
        assert_eq!(unseal(&sealed, "salty", Some("teacher")).unwrap(), json);
        if scheme != "plain" {
            assert!(!sealed.data.contains("hostname"), "{scheme}: sealed data is not readable");
            assert!(sealed.data.chars().all(|c| c.is_ascii_hexdigit()));
            assert!(unseal(&sealed, "other salt", Some("teacher")).is_err());
            // A flipped bit is caught by the tag.
            let mut bad = sealed.clone();
            let last = if bad.data.ends_with('0') { '1' } else { '0' };
            bad.data.pop();
            bad.data.push(last);
            assert!(unseal(&bad, "salty", Some("teacher")).is_err());
        }
    }
    let locked = seal(json, "password", "salty", Some("teacher")).unwrap();
    assert!(unseal(&locked, "salty", Some("guess")).is_err());
    assert!(unseal(&locked, "salty", None).is_err());
    assert!(seal(json, "password", "salty", None).is_err());
    // Hostile sealed data never panics.
    for data in ["", "zz", "abc", &"00".repeat(40)] {
        let s = netlabcraft_model::activity::Sealed { scheme: "salt".into(), data: data.into() };
        assert!(unseal(&s, "x", None).is_err());
    }
}

#[test]
fn the_answer_network_is_saved_sealed_viewed_and_locked_with_the_password() {
    let mut s = sample();
    // The sample carries its answer, hidden: the file shows no configuration of it.
    let shown = s.execute("activity.show", json!({})).unwrap();
    assert_eq!(shown["answer"], "salt");
    assert_eq!(shown["initial"], true);
    let lab = s.execute("activity.answer", json!({})).unwrap();
    let answer = netlabcraft_model::file::from_json(lab["lab"].as_str().unwrap()).unwrap();
    assert!(answer.devices.iter().any(|d| d.name == "R1"), "the answer has the router configured");
    // A fresh answer from the solved lab.
    s.execute("cli.script", json!({"device": "Router0", "lines": SOLVE})).unwrap();
    assert_eq!(s.execute("activity.save_answer", json!({})).unwrap()["scheme"], "salt");
    let file = s.execute("file.save", json!({})).unwrap()["json"].as_str().unwrap().to_string();
    assert!(!file.contains("enable secret") && !file.contains("192.168.20.1 255"), "sealed in the file");
    // Locking puts it under the password; nobody reads it until it is unlocked.
    s.execute("activity.lock", json!({"password": "teacher"})).unwrap();
    assert!(s.topology.activity.as_ref().unwrap().lock.starts_with("p2$"), "a slow hash for new locks");
    assert_eq!(s.topology.activity.as_ref().unwrap().answer.as_ref().unwrap().scheme, "password");
    let mut student = reopen(&mut s);
    assert!(student.execute("activity.answer", json!({})).is_err());
    assert!(student.execute("activity.tree", json!({})).is_err());
    assert!(student.execute("activity.load_answer", json!({})).is_err());
    // Even with the salt from the file, the sealed answer doesn't open.
    let a = student.topology.activity.clone().unwrap();
    assert!(unseal(a.answer.as_ref().unwrap(), &a.salt, None).is_err());
    let shown = student.execute("activity.show", json!({})).unwrap();
    assert!(shown["activity"]["answer"].is_null() && shown["activity"]["assessment"].is_null());
    student.execute("activity.unlock", json!({"password": "teacher"})).unwrap();
    assert_eq!(student.topology.activity.as_ref().unwrap().answer.as_ref().unwrap().scheme, "salt");
    let lab = student.execute("activity.answer", json!({})).unwrap();
    assert!(lab["lab"].as_str().unwrap().contains("enable secret") || lab["lab"].as_str().unwrap().contains("\"name\": \"R1\""));
    // Answers shown: the answer network is kept in the clear.
    student.execute("activity.set", json!({"hidden": false})).unwrap_err();
    let mut open = Session::new();
    open.execute("file.new", json!({"sample": "two-subnets"})).unwrap();
    open.execute("activity.set", json!({"title": "t"})).unwrap();
    assert_eq!(open.execute("activity.save_answer", json!({})).unwrap()["scheme"], "plain");
    open.execute("activity.set", json!({"hidden": true})).unwrap();
    assert_eq!(open.topology.activity.as_ref().unwrap().answer.as_ref().unwrap().scheme, "salt");
    // Editing it: into the workspace, keeping the activity.
    open.execute("device.add", json!({"kind": "pc", "name": "Extra"})).unwrap();
    open.execute("activity.load_answer", json!({})).unwrap();
    assert!(open.topology.resolve("Extra").is_err() && open.topology.activity.is_some());
    open.execute("activity.clear_answer", json!({})).unwrap();
    assert!(open.execute("activity.answer", json!({})).is_err());
}

#[test]
fn a_legacy_lock_still_opens() {
    let mut s = sample();
    let a = s.topology.activity.as_mut().unwrap();
    a.lock = a.password_hash("old");
    s.execute("activity.unlock", json!({"password": "nope"})).unwrap_err();
    s.execute("activity.unlock", json!({"password": "old"})).unwrap();
    assert!(!s.topology.activity.as_ref().unwrap().locked());
}

#[test]
fn the_assessment_tree_weighs_items_and_becomes_checks() {
    let mut s = sample();
    let tree = s.execute("activity.tree", json!({})).unwrap();
    let items = tree["items"].as_array().unwrap().clone();
    let kinds = |k: &str| items.iter().filter(|i| i["kind"] == k).count();
    assert!(kinds("config") >= 5 && kinds("address") == 2 && kinds("cable") == 4, "{tree}");
    assert!(items.iter().any(|i| i["device"] == "R1" && i["text"] == "hostname r1"), "{tree}");
    assert!(items.iter().all(|i| i["key"].as_str().unwrap().len() == 16), "keys are opaque hashes");
    let total = tree["points"].as_u64().unwrap();
    assert_eq!(total, items.len() as u64);
    // Not graded, three points with a hint.
    let host = items.iter().find(|i| i["text"] == "hostname r1").unwrap()["key"].clone();
    let cable = items.iter().find(|i| i["kind"] == "cable").unwrap()["key"].clone();
    s.execute("activity.weight", json!({"key": host, "points": 0})).unwrap();
    s.execute("activity.weight", json!({"key": cable, "points": 3, "feedback": "Cable the router to S1"})).unwrap();
    assert!(s.execute("activity.weight", json!({"key": "nope", "points": 2})).is_err());
    assert!(s.execute("activity.weight", json!({"key": host, "points": "x"})).is_err());
    let tree = s.execute("activity.tree", json!({})).unwrap();
    assert_eq!(tree["points"].as_u64().unwrap(), total - 1 + 2);
    // The weights are kept by hash: the file says nothing about what they weigh.
    let file = s.execute("file.save", json!({})).unwrap()["json"].as_str().unwrap().to_string();
    assert!(file.contains("Cable the router to S1") && !file.contains("hostname r1"));
    let r = s.execute("activity.tree_checks", json!({})).unwrap();
    let a = s.topology.activity.clone().unwrap();
    let pings = a.checks.iter().filter(|c| matches!(c.kind, netlabcraft_model::activity::CheckKind::Ping { .. })).count();
    assert_eq!(r["checks"].as_u64().unwrap() as usize, items.len() - 1 + pings);
    assert!(a.checks.iter().any(|c| matches!(c.kind, netlabcraft_model::activity::CheckKind::LinkHashed { .. })
        && c.points == 3
        && c.feedback == "Cable the router to S1"));
    // Cables are given, so they pass from the start; solving the router passes everything.
    let start = s.execute("activity.check", json!({})).unwrap();
    assert!(start["percent"].as_u64().unwrap() < 60, "{start}");
    s.execute("cli.script", json!({"device": "Router0", "lines": SOLVE})).unwrap();
    let done = s.execute("activity.check", json!({})).unwrap();
    assert_eq!(done["percent"], 100, "{done}");
    // A cable taken away fails its check.
    s.execute("link.remove", json!({"device": "R1", "port": "g0/0"})).unwrap();
    assert!(s.execute("activity.check", json!({})).unwrap()["percent"].as_u64().unwrap() < 100);
    // Shown answers: cable checks name the ports.
    let mut open = Session::new();
    open.execute("file.new", json!({"sample": "two-subnets"})).unwrap();
    open.execute("activity.set", json!({"title": "t"})).unwrap();
    open.execute("activity.save_answer", json!({})).unwrap();
    open.execute("activity.tree_checks", json!({})).unwrap();
    let checks = &open.topology.activity.as_ref().unwrap().checks;
    assert!(
        checks.iter().any(|c| matches!(c.kind, netlabcraft_model::activity::CheckKind::Link { .. }) && c.label.ends_with("cabled")),
        "{checks:?}"
    );
    assert_eq!(open.execute("activity.check", json!({})).unwrap()["percent"], 100);
    // Hiding turns them into hashes, still passing.
    open.execute("activity.set", json!({"hidden": true})).unwrap();
    assert!(!open.topology.activity.as_ref().unwrap().checks.iter().any(|c| matches!(c.kind, netlabcraft_model::activity::CheckKind::Link { .. })));
    assert_eq!(open.execute("activity.check", json!({})).unwrap()["percent"], 100);
}

#[test]
fn reset_activity_returns_to_the_initial_network() {
    let mut s = sample();
    let mut student = reopen(&mut s);
    student.execute("cli.script", json!({"device": "Router0", "lines": SOLVE})).unwrap();
    assert_eq!(student.execute("activity.reset", json!({})).unwrap()["from"], "initial");
    assert!(student.topology.resolve("Router0").is_ok() && student.topology.activity.is_some());
    // The instructor edits the initial network: a note for the student, saved again.
    let mut s = sample();
    s.execute("activity.save_initial", json!({})).unwrap();
    s.execute("cli.script", json!({"device": "Router0", "lines": "en\nconf t\nhostname Start\nend"})).unwrap();
    s.execute("activity.save_initial", json!({})).unwrap();
    s.execute("cli.script", json!({"device": "Start", "lines": SOLVE})).unwrap();
    s.execute("activity.load_initial", json!({})).unwrap();
    assert!(s.topology.resolve("Start").is_ok());
    s.execute("cli.script", json!({"device": "Start", "lines": SOLVE})).unwrap();
    s.execute("activity.reset", json!({})).unwrap();
    assert!(s.topology.resolve("Start").is_ok(), "reset goes to the initial network");
    // Without one, reset returns to the lab as opened.
    s.execute("activity.clear_initial", json!({})).unwrap();
    assert!(s.execute("activity.load_initial", json!({})).is_err());
    let mut t = reopen(&mut s);
    t.execute("cli.script", json!({"device": "Start", "lines": SOLVE})).unwrap();
    assert_eq!(t.execute("activity.reset", json!({})).unwrap()["from"], "opened");
    assert!(t.topology.resolve("Start").is_ok());
}

#[test]
fn instructions_come_in_the_students_language() {
    let mut s = sample();
    let shown = s.execute("activity.show", json!({"lang": "es"})).unwrap();
    assert!(shown["text"].as_str().unwrap().starts_with("R1 une dos LAN"), "{shown}");
    assert_eq!(shown["languages"], json!(["en", "es"]));
    let fr = s.execute("activity.show", json!({"lang": "fr"})).unwrap();
    assert!(fr["text"].as_str().unwrap().starts_with("R1 joins two LANs"));
    s.execute("activity.translate", json!({"lang": "fr", "text": "R1 relie deux LAN."})).unwrap();
    assert_eq!(s.execute("activity.show", json!({"lang": "fr"})).unwrap()["text"], "R1 relie deux LAN.");
    // Spanish becomes the main language: English becomes a translation.
    let r = s.execute("activity.translate", json!({"lang": "es", "default": true})).unwrap();
    assert_eq!(r["default"], "es");
    let a = s.topology.activity.clone().unwrap();
    assert!(a.instructions.starts_with("R1 une") && a.translations["en"].starts_with("R1 joins"));
    assert!(s.execute("activity.show", json!({"lang": "de"})).unwrap()["text"].as_str().unwrap().starts_with("R1 une"));
    // Empty text removes a translation; bad codes are refused.
    s.execute("activity.translate", json!({"lang": "fr", "text": ""})).unwrap();
    assert!(!s.topology.activity.as_ref().unwrap().translations.contains_key("fr"));
    assert!(s.execute("activity.translate", json!({"lang": "../x", "text": "y"})).is_err());
    // A file from before (one text) reads as the main language.
    let old: netlabcraft_model::activity::Activity = serde_json::from_str(r#"{"title": "t", "instructions": "hello"}"#).unwrap();
    assert_eq!(old.instructions_in("es"), "hello");
    assert_eq!(old.languages(), ["en"]);
}

#[test]
fn connectivity_tests_run_the_pings() {
    let mut s = sample();
    let r = s.execute("activity.connectivity", json!({})).unwrap();
    assert_eq!(r["total"], 2, "{r}");
    assert_eq!(r["passed"], 0);
    assert_eq!(r["tests"][0]["from"], "PC1");
    s.execute("cli.script", json!({"device": "Router0", "lines": SOLVE})).unwrap();
    let r = s.execute("activity.connectivity", json!({})).unwrap();
    assert_eq!(r["passed"], 2, "{r}");
    // Locked with the score only: no pass or fail per test.
    s.execute("activity.set", json!({"score_only": true})).unwrap();
    s.execute("activity.lock", json!({"password": "p"})).unwrap();
    let r = s.execute("activity.connectivity", json!({})).unwrap();
    assert!(r["passed"].is_null() && r["tests"][0]["passed"].is_null(), "{r}");
}

/// Re-signs a results file after the test changed it, as a forger who read the code would.
fn resign(s: &Session, v: &serde_json::Value) -> String {
    let mut m = v.as_object().unwrap().clone();
    m.remove("hmac");
    let body = serde_json::Value::Object(m.clone()).to_string();
    m.insert("hmac".into(), crate::instructor::sign(s.topology.activity.as_ref().unwrap(), &body).into());
    serde_json::to_string(&serde_json::Value::Object(m)).unwrap()
}

fn export_json(student: &mut Session) -> serde_json::Value {
    student.topology.activity.as_mut().unwrap().student = Some("Ana".into());
    let r = student.execute("activity.export_results", json!({"format": "json", "time": "2026-11-13T10:00:00Z"})).unwrap();
    serde_json::from_str(r["text"].as_str().unwrap()).unwrap()
}

#[test]
fn a_replayed_results_file_matches_and_a_forged_score_fails() {
    let mut teacher = sample();
    let mut honest = teacher.clone();
    honest.execute("cli.script", json!({"device": "Router0", "lines": SOLVE})).unwrap();
    let file = export_json(&mut honest);
    assert_eq!(file["version"], 2);
    assert!(file["network"].is_object());
    let v = teacher.execute("activity.verify_results", json!({"text": file.to_string()})).unwrap();
    assert_eq!((v["valid"].clone(), v["replayed"].clone(), v["replay_matches"].clone()), (true.into(), true.into(), true.into()), "{v}");
    assert_eq!(v["replay_score"], v["score"]);

    // A student who did nothing forges a perfect score and re-signs it.
    let mut lazy = teacher.clone();
    let honest_lazy = export_json(&mut lazy);
    assert!(honest_lazy["percent"].as_u64().unwrap() < 100);
    let mut forged = honest_lazy.clone();
    forged["score"] = file["score"].clone();
    forged["total"] = file["total"].clone();
    forged["percent"] = 100.into();
    let items = forged["results"].as_array_mut().unwrap();
    for i in items.iter_mut() {
        i["passed"] = true.into();
    }
    let text = resign(&teacher, &forged);
    let v = teacher.execute("activity.verify_results", json!({"text": text})).unwrap();
    assert_eq!(v["signature_ok"], true, "the forger's signature is good: {v}");
    assert_eq!(v["replayed"], true);
    assert_eq!(v["valid"], false, "{v}");
    assert_eq!(v["replay_matches"], false);
    assert!(!v["mismatches"].as_array().unwrap().is_empty());
    assert!(v["replay_score"].as_u64().unwrap() < 100 || v["replay_score"] != forged["score"]);

    // The network swapped for the honest one without re-signing: the signature catches it.
    let mut swapped = honest_lazy.clone();
    swapped["network"] = file["network"].clone();
    let v = teacher.execute("activity.verify_results", json!({"text": swapped.to_string()})).unwrap();
    assert_eq!(v["valid"], false);
    assert_eq!(v["signature_ok"], false);
    // Re-signed, the honest network earns more than the file claims: still a mismatch.
    let v = teacher.execute("activity.verify_results", json!({"text": resign(&teacher, &swapped)})).unwrap();
    assert_eq!(v["valid"], false, "{v}");
    // A network edited to earn the points is just solving it: it verifies.
    let mut earned = file.clone();
    earned["student"] = "Someone".into();
    let v = teacher.execute("activity.verify_results", json!({"text": resign(&teacher, &earned)})).unwrap();
    assert_eq!(v["valid"], true, "{v}");
}

#[test]
fn csv_and_old_format_files_say_they_were_not_replayed() {
    let mut teacher = sample();
    let mut student = teacher.clone();
    student.topology.activity.as_mut().unwrap().student = Some("Ana".into());
    let csv = student.execute("activity.export_results", json!({"format": "csv"})).unwrap();
    assert_eq!(csv["replayable"], false);
    let v = teacher.execute("activity.verify_results", json!({"text": csv["text"]})).unwrap();
    assert_eq!((v["valid"].clone(), v["replayed"].clone(), v["replay_matches"].clone()), (true.into(), false.into(), serde_json::Value::Null), "{v}");
    assert!(v["message"].as_str().unwrap().contains("CSV"), "{v}");
    // A version 1 file: no network.
    let mut old = export_json(&mut student);
    old.as_object_mut().unwrap().remove("network");
    old["version"] = 1.into();
    let v = teacher.execute("activity.verify_results", json!({"text": resign(&teacher, &old)})).unwrap();
    assert_eq!((v["valid"].clone(), v["replayed"].clone()), (true.into(), false.into()), "{v}");
    assert!(v["message"].as_str().unwrap().contains("older export"), "{v}");
    // A newer version is refused.
    old["version"] = 99.into();
    assert!(teacher.execute("activity.verify_results", json!({"text": resign(&teacher, &old)})).is_err());
}

#[test]
fn a_hostile_embedded_network_is_refused_never_a_crash() {
    let mut teacher = sample();
    let mut student = teacher.clone();
    let file = export_json(&mut student);
    let big = "x".repeat(5 * 1024 * 1024);
    let cases = [
        json!("garbage"),
        json!(12),
        json!([1, 2, 3]),
        json!({"format": "netlabcraft", "version": 1, "topology": 5}),
        json!({"format": "other", "version": 1, "topology": {}}),
        json!({"pad": big}),
    ];
    for n in cases {
        let mut f = file.clone();
        f["network"] = n;
        let v = teacher.execute("activity.verify_results", json!({"text": resign(&teacher, &f)})).unwrap();
        assert_eq!(v["valid"], false, "{v}");
        assert_eq!(v["replayed"], false);
        assert_eq!(v["replay_matches"], false);
        assert!(!v["mismatches"].as_array().unwrap().is_empty());
    }
}

#[test]
fn results_files_are_signed_and_tampering_shows() {
    let mut s = sample();
    s.execute("cli.script", json!({"device": "Router0", "lines": SOLVE})).unwrap();
    let mut student = s.clone();
    student.topology.activity.as_mut().unwrap().student = Some("Ana, B.".into());
    for format in ["json", "csv"] {
        let r = student.execute("activity.export_results", json!({"format": format, "time": "2026-10-30T10:00:00Z", "elapsed_s": 600})).unwrap();
        let text = r["text"].as_str().unwrap().to_string();
        assert_eq!(r["percent"], 100);
        assert!(text.contains("2026-10-30T10:00:00Z") && text.contains("Ana, B."), "{text}");
        let v = s.execute("activity.verify_results", json!({"text": text})).unwrap();
        assert_eq!(v["valid"], true, "{format}: {v}");
        assert_eq!(v["student"], "Ana, B.");
        assert_eq!(v["percent"], 100);
        // A better score typed in by hand.
        let forged = if format == "json" { text.replace("\"percent\": 100", "\"percent\": 99") } else { text.replace("percent,100", "percent,99") };
        assert_ne!(forged, text);
        let v = s.execute("activity.verify_results", json!({"text": forged})).unwrap();
        assert_eq!(v["valid"], false, "{format}: {v}");
        assert_eq!(v["same_activity"], true);
        // Another activity's results.
        let mut other = Session::new();
        other.execute("file.new", json!({"sample": "two-subnets"})).unwrap();
        other.execute("activity.set", json!({"title": "other"})).unwrap();
        let v = other.execute("activity.verify_results", json!({"text": text})).unwrap();
        assert_eq!(v["valid"], false);
        assert_eq!(v["same_activity"], false);
    }
    assert!(s.execute("activity.export_results", json!({"format": "xml"})).is_err());
    assert!(s.execute("activity.verify_results", json!({"text": "hello"})).is_err());
    assert!(s.execute("activity.verify_results", json!({"text": "{\"a\": 1}"})).unwrap()["valid"] == false);
    // The clock writes a plain UTC time.
    let now = crate::instructor::now_utc();
    assert!(now.len() == 20 && now.ends_with('Z') && now.starts_with("20"), "{now}");
}

#[test]
fn locks_refuse_what_the_student_may_not_change() {
    let mut s = sample();
    // The sample locks devices, cables and the PCs; they hold once the activity is locked.
    s.execute("device.add", json!({"kind": "pc", "name": "Mine"})).unwrap();
    s.execute("edit.undo", json!({})).unwrap();
    s.execute("activity.lock", json!({"password": "teacher"})).unwrap();
    let mut st = reopen(&mut s);
    assert_eq!(st.execute("activity.show", json!({})).unwrap()["locks_active"], true);
    let e = st.execute("device.add", json!({"kind": "pc"})).unwrap_err().to_string();
    assert!(e.contains("add or remove devices"), "{e}");
    assert!(st.execute("device.remove", json!({"device": "PC1"})).is_err());
    assert_eq!(st.topology.devices.len(), 5);
    let e = st.execute("link.remove", json!({"device": "Router0", "port": "g0/0"})).unwrap_err().to_string();
    assert!(e.contains("cables"), "{e}");
    assert_eq!(st.topology.links.len(), 4);
    let before = st.topology.device(st.topology.resolve("PC1").unwrap()).unwrap().clone();
    let e = st.execute("cli.exec", json!({"device": "PC1", "line": "ipconfig 10.9.9.9 255.255.255.0 10.9.9.1"})).unwrap_err().to_string();
    assert!(e.contains("PC1's configuration"), "{e}");
    assert_eq!(st.topology.device(before.id).unwrap().ports[0].ip, before.ports[0].ip);
    // Moving a locked device isn't configuring it; the router is the student's.
    st.execute("device.move", json!({"device": "PC1", "x": 10, "y": 10})).unwrap();
    st.execute("cli.script", json!({"device": "Router0", "lines": SOLVE})).unwrap();
    assert_eq!(st.execute("activity.check", json!({})).unwrap()["percent"], 100);
    // The refusal leaves no undo step behind, and nothing to redo.
    assert!(st.execute("activity.locks", json!({"devices": false})).is_err(), "locks can't be changed while locked");
    // Unlocked, the instructor changes anything.
    st.execute("activity.unlock", json!({"password": "teacher"})).unwrap();
    st.execute("device.add", json!({"kind": "pc", "name": "Mine"})).unwrap();
    st.execute("activity.locks", json!({"devices": false, "cables": false, "config": ["R1"]})).unwrap();
    let a = st.topology.activity.clone().unwrap();
    assert!(!a.locks.devices && !a.locks.cables && a.locks.config.len() == 1);
    assert!(st.execute("activity.locks", json!({"config": ["nope"]})).is_err());
    assert!(st.execute("activity.locks", json!({"config": "R1"})).is_err());
    // File and reset commands are never refused.
    st.execute("activity.lock", json!({"password": "x"})).unwrap();
    st.execute("activity.reset", json!({})).unwrap();
    st.execute("file.new", json!({})).unwrap();
}

#[test]
fn edit_check_changes_points_and_hints() {
    let mut s = sample();
    let r = s.execute("activity.edit_check", json!({"index": 0, "points": 5, "feedback": "Look at the hostname"})).unwrap();
    assert_eq!(r["points"], 5);
    let a = s.topology.activity.clone().unwrap();
    assert_eq!(a.checks[0].feedback, "Look at the hostname");
    assert!(s.execute("activity.edit_check", json!({"index": 999})).is_err());
    assert!(s.execute("activity.edit_check", json!({})).is_err());
}

#[test]
fn instructor_commands_survive_hostile_params() {
    let hostile = [
        json!({}),
        json!(null),
        json!({"key": 5, "points": -1, "lang": "", "text": 7, "config": [1, "x", null], "format": {}, "index": -3}),
        json!({"key": "", "lang": "x".repeat(100), "text": "y".repeat(100_000), "config": [4_294_967_296u64], "path": "/nonexistent/x"}),
    ];
    let base = sample();
    let mut locked = sample();
    locked.execute("activity.lock", json!({"password": "p"})).unwrap();
    for c in crate::catalog::CATALOG.iter().filter(|c| c.id.starts_with("activity.") && c.id != "activity.lock" && c.id != "activity.unlock") {
        for p in &hostile {
            for mut s in [base.clone(), Session::new(), locked.clone()] {
                let _ = s.execute(c.id, p.clone());
            }
        }
    }
    // A hostile activity in a file: oversized and malformed parts are dropped, not trusted.
    let mut s = sample();
    let a = s.topology.activity.as_mut().unwrap();
    a.initial = Some(json!("not a lab"));
    a.answer = Some(netlabcraft_model::activity::Sealed { scheme: "weird".into(), data: "zz".into() });
    a.translations.insert(String::new(), "x".into());
    let mut t = reopen(&mut s);
    assert!(t.topology.activity.as_ref().unwrap().initial.is_none());
    assert!(t.execute("activity.answer", json!({})).is_err());
    assert!(t.execute("activity.tree", json!({})).is_err());
    let _ = t.execute("activity.reset", json!({}));
}
