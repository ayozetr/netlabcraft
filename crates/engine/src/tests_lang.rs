//! The board languages through the engine: choosing one, running a Python and a JavaScript
//! program in simulated time, the refusals, and the lab file.

use serde_json::{Value, json};

use crate::Session;

fn lab(board: &str) -> Session {
    let mut s = Session::new();
    s.execute("device.add", json!({"model": board, "name": "Board1"})).unwrap();
    s.execute("device.add", json!({"model": "IoT-Sensor", "name": "Temp1"})).unwrap();
    s.execute("device.add", json!({"model": "IoT-Device", "name": "Fan1"})).unwrap();
    s.execute("board.wire", json!({"device": "Board1", "pin": "A0", "thing": "Temp1"})).unwrap();
    s.execute("board.wire", json!({"device": "Board1", "pin": "D0", "thing": "Fan1"})).unwrap();
    s.execute("iot.configure", json!({"device": "Temp1", "value": 22})).unwrap();
    s
}

fn log(st: &Value) -> Vec<String> {
    st["log"].as_array().unwrap().iter().map(|l| l.as_str().unwrap().to_string()).collect()
}

const PY: &str = "import time\nlimit = 28\nwhile True:\n    t = analogRead(\"A0\")\n    digitalWrite(\"D0\", 1 if t > limit else 0)\n    print(\"temperature\", t)\n    time.sleep(2)\n";
const JS: &str = "const limit = 28;\nlet count = 0;\nfunction loop() {\n  const t = analogRead(\"A0\");\n  digitalWrite(\"D0\", t > limit ? 1 : 0);\n  count++;\n  console.log(`reading ${t}, run ${count}`);\n  delay(2000);\n}\n";

#[test]
fn a_python_program_runs_on_the_single_board_computer_in_simulated_time() {
    let mut s = lab("Board-SBC");
    let st = s.execute("board.status", json!({"device": "Board1"})).unwrap();
    assert_eq!((st["language"].as_str(), st["languages"].clone()), (Some("boardscript"), json!(["boardscript", "python"])));
    let st = s.execute("board.program", json!({"device": "Board1", "language": "python", "source": PY, "run": true})).unwrap();
    assert_eq!(st["language"], "python", "{st}");
    s.execute("sim.advance", json!({"seconds": 3})).unwrap();
    let st = s.execute("board.status", json!({"device": "Board1"})).unwrap();
    assert_eq!(st["state"], "running", "{st}");
    assert!(log(&st).contains(&"temperature 22".to_string()), "{st}");
    assert_eq!(st["vars"]["limit"], 28, "{st}");
    let fan = s.topology.resolve("Fan1").unwrap();
    assert!(!s.sim.iot_thing(fan).is_some_and(|x| x.on));
    s.execute("iot.configure", json!({"device": "Temp1", "value": 35})).unwrap();
    s.execute("sim.advance", json!({"seconds": 4})).unwrap();
    assert!(s.sim.iot_thing(fan).is_some_and(|x| x.on), "35 degrees: the Python program switches the fan on");
    let st = s.execute("board.status", json!({"device": "Board1"})).unwrap();
    assert!(log(&st).contains(&"temperature 35".to_string()), "{st}");
    // The language and the program survive save and open; BoardScript is left out of the file.
    let saved = s.execute("file.save", json!({})).unwrap();
    assert!(saved["json"].to_string().contains("python"));
    let mut s2 = Session::new();
    s2.execute("file.open", json!({"json": saved["json"]})).unwrap();
    assert_eq!(s2.topology, s.topology);
    assert_eq!(s2.execute("board.status", json!({"device": "Board1"})).unwrap()["language"], "python");
}

#[test]
fn a_javascript_program_with_a_loop_function_runs_on_the_microcontroller_board() {
    let mut s = lab("Board-MCU");
    s.execute("board.program", json!({"device": "Board1", "language": "JavaScript", "source": JS, "run": true})).unwrap();
    s.execute("sim.advance", json!({"seconds": 7})).unwrap();
    let st = s.execute("board.status", json!({"device": "Board1"})).unwrap();
    let lines = log(&st);
    assert!(lines.contains(&"reading 22, run 1".to_string()) && lines.contains(&"reading 22, run 3".to_string()), "{st}");
    assert_eq!(st["state"], "running");
}

#[test]
fn a_language_is_refused_where_the_board_does_not_run_it_and_errors_name_a_line() {
    let mut s = lab("Board-SBC");
    let e = s.execute("board.program", json!({"device": "Board1", "language": "javascript"})).unwrap_err().to_string();
    assert!(e.contains("boardscript or python"), "{e}");
    assert!(s.execute("board.program", json!({"device": "Board1", "language": "ruby"})).is_err());
    assert!(s.execute("board.program", json!({"device": "Board1", "language": 3})).is_err());
    s.execute("board.program", json!({"device": "Board1", "language": "python"})).unwrap();
    let e = s.execute("board.program", json!({"device": "Board1", "source": "x = 1\nprint("})).unwrap_err().to_string();
    assert!(e.contains("line 2"), "{e}");
    // BoardScript text in a Python board is a Python error, found at run time, with its line.
    let st = s.execute("board.program", json!({"device": "Board1", "source": "x = 1\nprint(y)\n", "run": true})).unwrap();
    assert_eq!(st["state"], "stopped", "{st}");
    assert!(st["message"].as_str().unwrap().starts_with("line 2: NameError"), "{st}");
    assert!(log(&st).iter().any(|l| l.starts_with("stopped: line 2")), "{st}");
    // A runaway loop stops with a message instead of holding the simulator up.
    let st = s.execute("board.program", json!({"device": "Board1", "source": "while True:\n    pass\n", "run": true})).unwrap();
    assert!(st["message"].as_str().unwrap().contains("steps without a pause"), "{st}");
    s.execute("sim.advance", json!({"seconds": 30})).unwrap();
    // Back to BoardScript.
    let st = s.execute("board.program", json!({"device": "Board1", "language": "boardscript", "source": "x = 1\nsleep 1\n", "run": true})).unwrap();
    assert_eq!(st["language"], "boardscript");
    let saved = s.execute("file.save", json!({})).unwrap();
    assert!(!saved["json"].to_string().contains("\"language\""), "BoardScript is left out of the file");
}
