//! The command log, the profile and cable length effects.

use serde_json::json;

use crate::settings::{MAX_EMAIL, MAX_INFO, MAX_LOG, clean_email, clean_info, clean_name};
use crate::{Session, settings};

fn lab() -> Session {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "vlans"})).unwrap();
    s
}

#[test]
fn every_command_is_logged_with_its_result() {
    let mut s = lab();
    s.execute("sim.advance", json!({"seconds": 2})).unwrap();
    assert!(s.execute("device.remove", json!({"device": "nobody"})).is_err());
    let r = s.execute("log.list", json!({})).unwrap();
    let entries = r["entries"].as_array().unwrap();
    let names: Vec<&str> = entries.iter().map(|e| e["command"].as_str().unwrap()).collect();
    assert_eq!(names, ["file.new", "sim.advance", "device.remove"]);
    assert_eq!(entries[1]["ok"], true);
    assert_eq!(entries[2]["ok"], false);
    assert!(entries[2]["error"].is_string(), "{r}");
    // Listing the log does not add to it, and `since` skips what was seen.
    let again = s.execute("log.list", json!({"since": 2})).unwrap();
    assert_eq!(again["entries"].as_array().unwrap().len(), 1);
    assert_eq!(again["total"], 3);
}

#[test]
fn passwords_and_long_values_never_reach_the_log() {
    let mut s = lab();
    let _ = s.execute("pc.http_request", json!({"pc": "PC1", "url": "http://x", "user": "u", "password": "hunter2", "body": "b".repeat(5000)}));
    let text = s.cmd_log.text();
    assert!(!text.contains("hunter2"), "{text}");
    assert!(text.contains("***"), "{text}");
    assert!(text.len() < 2000, "{}", text.len());
}

#[test]
fn a_drag_is_one_entry() {
    let mut s = lab();
    let (a, b) = (s.topology.devices[0].name.clone(), s.topology.devices[1].name.clone());
    for x in [10, 20, 30] {
        s.execute("device.move", json!({"device": a, "x": x, "y": 5, "transient": true})).unwrap();
    }
    s.execute("device.move", json!({"device": b, "x": 1, "y": 1, "transient": true})).unwrap();
    let moves: Vec<_> = s.cmd_log.entries().filter(|e| e.command == "device.move").collect();
    assert_eq!(moves.len(), 2);
    assert_eq!(moves[0].count, 3);
    assert!(moves[0].params.contains("30"), "the last position is kept: {}", moves[0].params);
}

#[test]
fn the_log_survives_a_new_lab_and_can_be_switched_off_and_cleared() {
    let mut s = lab();
    s.execute("file.new", json!({})).unwrap();
    assert_eq!(s.cmd_log.len(), 2);
    assert_eq!(s.execute("log.enable", json!({"on": false})).unwrap()["on"], false);
    s.execute("sim.advance", json!({"seconds": 1})).unwrap();
    assert_eq!(s.cmd_log.len(), 2, "nothing recorded while off");
    assert!(s.execute("log.enable", json!({})).is_err());
    s.execute("log.enable", json!({"on": true})).unwrap();
    assert_eq!(s.execute("log.clear", json!({})).unwrap()["removed"], 2);
    assert!(s.cmd_log.is_empty());
}

#[test]
fn the_log_keeps_the_newest_entries_only() {
    let mut s = Session::new();
    for _ in 0..MAX_LOG + 5 {
        s.execute("sim.status", json!({})).unwrap();
    }
    assert_eq!(s.cmd_log.len(), MAX_LOG);
    assert_eq!(s.cmd_log.entries().next().unwrap().n, 6);
    let r = s.execute("log.list", json!({"limit": 3})).unwrap();
    assert_eq!(r["entries"].as_array().unwrap().len(), 3);
    assert!(s.execute("log.list", json!({"limit": 0})).is_err());
}

#[test]
fn the_exported_log_is_text_with_one_line_per_command() {
    let mut s = lab();
    let _ = s.execute("device.remove", json!({"device": "nobody"}));
    let r = s.execute("log.export", json!({})).unwrap();
    let text = r["text"].as_str().unwrap();
    assert_eq!(r["file_name"], settings::LOG_FILE_NAME);
    assert_eq!(text.lines().filter(|l| !l.starts_with('#')).count(), 2, "{text}");
    assert!(text.contains("device.remove") && text.contains("failed:"), "{text}");
}

#[test]
fn the_profile_name_is_cleaned_and_names_the_student() {
    assert_eq!(clean_name("  Ana \t  Ruiz\n"), "Ana Ruiz");
    assert_eq!(clean_name(&"x".repeat(200)).chars().count(), settings::MAX_NAME);
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "activity-first-router"})).unwrap();
    assert!(s.execute("activity.start", json!({})).is_err(), "no name anywhere");
    assert_eq!(s.execute("profile.set", json!({"name": " Ana "})).unwrap()["name"], "Ana");
    assert_eq!(s.execute("profile.get", json!({})).unwrap()["name"], "Ana");
    // The profile is the person's, not the lab's: a new lab keeps it.
    s.execute("file.new", json!({"sample": "activity-first-router"})).unwrap();
    assert_eq!(s.profile.display_name, "Ana");
    let out = s.execute("activity.export_results", json!({"format": "json"})).unwrap();
    assert!(out["text"].as_str().unwrap().contains("\"student\": \"Ana\""), "{out}");
    s.execute("activity.start", json!({})).unwrap();
    assert_eq!(s.topology.activity.as_ref().unwrap().student.as_deref(), Some("Ana"));
    assert!(s.execute("profile.set", json!({})).is_err());
}

#[test]
fn the_profile_holds_an_email_and_info_that_stay_local_and_out_of_results() {
    assert_eq!(clean_email("  user@example.test \n"), "user@example.test");
    assert_eq!(clean_email(&"x".repeat(500)).chars().count(), MAX_EMAIL);
    assert_eq!(clean_info("one\r\ntwo\u{7}\n\n"), "one\ntwo");
    assert_eq!(clean_info(&"y".repeat(5000)).chars().count(), MAX_INFO);
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "activity-first-router"})).unwrap();
    let got = s.execute("profile.get", json!({})).unwrap();
    assert_eq!((got["name"].as_str(), got["email"].as_str(), got["info"].as_str()), (Some(""), Some(""), Some("")));
    s.execute("profile.set", json!({"name": "Test User", "email": "user@example.test", "info": "Group A\nBench 3"})).unwrap();
    // A call that names one field leaves the others.
    s.execute("profile.set", json!({"email": "other@example.test"})).unwrap();
    let got = s.execute("profile.get", json!({})).unwrap();
    assert_eq!(got["name"], "Test User");
    assert_eq!(got["email"], "other@example.test");
    assert_eq!(got["info"], "Group A\nBench 3");
    assert!(s.execute("profile.set", json!({"email": 5})).is_err());
    assert!(s.execute("profile.set", json!({"info": ""})).is_ok());
    assert_eq!(s.profile.info, "");
    // Only the name reaches the results and the lab file.
    s.execute("profile.set", json!({"info": "Group A", "email": "user@example.test"})).unwrap();
    let results = s.execute("activity.export_results", json!({"format": "json"})).unwrap()["text"].as_str().unwrap().to_string();
    assert!(results.contains("Test User") && !results.contains("example.test") && !results.contains("Group A"), "{results}");
    let saved = netlabcraft_model::file::to_json(&s.topology).unwrap_or_default();
    assert!(!saved.contains("example.test") && !saved.contains("Group A"));
    // The command log hides them, so an exported log does not carry them.
    let log = s.cmd_log.text();
    assert!(log.contains("profile.set") && !log.contains("example.test") && !log.contains("Group A"), "{log}");
    // A new lab keeps them.
    s.execute("file.new", json!({"sample": "activity-first-router"})).unwrap();
    assert_eq!(s.profile.email, "user@example.test");
}

#[test]
fn cable_length_effects_can_be_switched_off_for_a_lab() {
    let mut s = lab();
    let l = s.topology.links[0];
    let port = s.topology.device(l.a.device).unwrap().ports[l.a.port].name.clone();
    s.execute("link.length", json!({"device": l.a.device.0, "port": port, "meters": 500})).unwrap();
    assert!(netlabcraft_sim::cable_problem(&s.topology, l.a).is_some());
    assert_eq!(s.execute("physical.length_effects", json!({})).unwrap()["on"], true);
    assert_eq!(s.execute("physical.length_effects", json!({"on": false})).unwrap()["on"], false);
    assert!(netlabcraft_sim::cable_problem(&s.topology, l.a).is_none());
    // Saved with the lab, and undone like any change.
    let file = s.execute("file.save", json!({})).unwrap()["json"].as_str().unwrap().to_string();
    let mut t = Session::new();
    t.execute("file.open", json!({"json": file})).unwrap();
    assert!(netlabcraft_sim::cable_problem(&t.topology, l.a).is_none());
    assert!(s.undo());
    assert!(netlabcraft_sim::cable_problem(&s.topology, l.a).is_some());
}
