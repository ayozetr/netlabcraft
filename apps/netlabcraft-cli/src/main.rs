//! `netlabcraft-cli`: NetLabCraft without a window. See `docs/cli.md`.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::io::{BufRead, Write};
use std::process::ExitCode;

use netlabcraft_engine::{CATALOG, Session, sample};
use serde_json::{Value, json};

const USAGE: &str = "\
usage: netlabcraft-cli <command>

commands:
  run [--in LAB] [--sample ID] --cmd 'id={json}'... [--out LAB]
        run engine commands in order, printing each result
  console [--in LAB] [--sample ID] DEVICE [--out LAB]
        type at a device's console (stdin lines; prints the session)
  exec [--in LAB] [--sample ID] DEVICE 'line'... [--out LAB]
        run console lines at a device and print their output
  pcap [--in LAB] [--sample ID] --ping FROM:ADDRESS... --out FILE.pcap
        run pings and write the event list as a capture
  mcp [--in LAB] [--sample ID]
        serve the Model Context Protocol on stdin/stdout
  commands        list the command catalog
  samples         list the sample labs
  version         print the version
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

struct Opts {
    input: Option<String>,
    sample: Option<String>,
    out: Option<String>,
    cmds: Vec<String>,
    pings: Vec<String>,
    rest: Vec<String>,
}

fn parse(args: &[String]) -> Result<Opts, String> {
    let mut o = Opts { input: None, sample: None, out: None, cmds: Vec::new(), pings: Vec::new(), rest: Vec::new() };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut val = |name: &str| it.next().cloned().ok_or_else(|| format!("{name} needs a value"));
        match a.as_str() {
            "--in" => o.input = Some(val("--in")?),
            "--sample" => o.sample = Some(val("--sample")?),
            "--out" => o.out = Some(val("--out")?),
            "--cmd" => o.cmds.push(val("--cmd")?),
            "--ping" => o.pings.push(val("--ping")?),
            _ => o.rest.push(a.clone()),
        }
    }
    Ok(o)
}

fn open(o: &Opts) -> Result<Session, String> {
    let mut s = Session::new();
    if let Some(p) = &o.input {
        s.execute("file.open", json!({"path": p})).map_err(|e| e.to_string())?;
    } else if let Some(id) = &o.sample {
        s.execute("file.new", json!({"sample": id})).map_err(|e| e.to_string())?;
    }
    Ok(s)
}

fn save(s: &mut Session, o: &Opts) -> Result<(), String> {
    if let Some(p) = &o.out {
        s.execute("file.save", json!({"path": p})).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn run(args: &[String]) -> Result<(), String> {
    let Some(cmd) = args.first() else {
        print!("{USAGE}");
        return Ok(());
    };
    let o = parse(args.get(1..).unwrap_or_default())?;
    match cmd.as_str() {
        "run" => {
            let mut s = open(&o)?;
            for c in &o.cmds {
                let (id, params) = match c.split_once('=') {
                    Some((id, p)) => (id, serde_json::from_str::<Value>(p).map_err(|e| format!("{id}: bad JSON: {e}"))?),
                    None => (c.as_str(), json!({})),
                };
                let r = s.execute(id, params).map_err(|e| format!("{id}: {e}"))?;
                println!("{}", serde_json::to_string_pretty(&r).unwrap_or_default());
            }
            save(&mut s, &o)
        }
        "exec" | "console" => {
            let mut s = open(&o)?;
            let dev = o.rest.first().ok_or("give a device name")?.clone();
            let lines: Vec<String> = if cmd == "exec" {
                o.rest.get(1..).unwrap_or_default().to_vec()
            } else {
                std::io::stdin().lock().lines().map_while(Result::ok).collect()
            };
            let prompt = s.execute("cli.prompt", json!({"device": dev})).map_err(|e| e.to_string())?;
            let mut prompt = prompt["prompt"].as_str().unwrap_or("").to_string();
            let mut stdout = std::io::stdout().lock();
            for l in lines {
                let r = s.execute("cli.exec", json!({"device": dev, "line": l})).map_err(|e| e.to_string())?;
                let _ = writeln!(stdout, "{prompt}{l}");
                let out = r["output"].as_str().unwrap_or("");
                if !out.is_empty() {
                    let _ = writeln!(stdout, "{out}");
                }
                prompt = r["prompt"].as_str().unwrap_or("").to_string();
            }
            save(&mut s, &o)
        }
        "pcap" => {
            let mut s = open(&o)?;
            for p in &o.pings {
                let (from, to) = p.split_once(':').ok_or_else(|| format!("--ping {p}: use FROM:ADDRESS"))?;
                let r = s.execute("net.ping", json!({"from": from, "to": to})).map_err(|e| e.to_string())?;
                eprintln!("{from} → {to}: {}", r["marks"].as_str().unwrap_or(""));
            }
            let out = o.out.as_deref().ok_or("--out FILE.pcap is required")?;
            let r = s.execute("file.export_pcap", json!({"path": out})).map_err(|e| e.to_string())?;
            eprintln!("{} frames written to {out}", r["frames"]);
            Ok(())
        }
        "mcp" => {
            let mut s = open(&o)?;
            serve_mcp(&mut s)
        }
        "commands" => {
            for c in CATALOG {
                println!("{:<20} {:<28} {}", c.id, c.label, c.params);
            }
            Ok(())
        }
        "samples" => {
            for x in sample::SAMPLES {
                println!("{:<16} {} — {}", x.id, x.title, x.description);
            }
            Ok(())
        }
        "version" | "--version" | "-V" => {
            println!("netlabcraft-cli {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        "help" | "--help" | "-h" => {
            print!("{USAGE}");
            Ok(())
        }
        other => Err(format!("unknown command `{other}`\n\n{USAGE}")),
    }
}

fn serve_mcp(s: &mut Session) -> Result<(), String> {
    let stdin = std::io::stdin();
    netlabcraft_mcp::serve(s, stdin.lock(), std::io::stdout()).map_err(|e| e.to_string())
}
