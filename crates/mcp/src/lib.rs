//! NetLabCraft L6: an MCP server over stdio (newline-delimited JSON-RPC 2.0).
//!
//! Agents get four tools: `netlabcraft_run` (any catalog command), `netlabcraft_cli` (type
//! lines at a device), `netlabcraft_lab` (devices, ports, cables) and `netlabcraft_commands`
//! (the catalog). See `docs/mcp.md`.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::io::{BufRead, Write};

use netlabcraft_engine::{CATALOG, Session};
use serde_json::{Value, json};

pub const PROTOCOL_VERSION: &str = "2025-06-18";
/// Longest request line accepted.
const MAX_LINE: usize = 16 * 1024 * 1024;

/// Serves requests from `input` until it closes.
pub fn serve(session: &mut Session, input: impl BufRead, mut output: impl Write) -> std::io::Result<()> {
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = if line.len() > MAX_LINE {
            Some(error(Value::Null, -32600, "request too large"))
        } else {
            match serde_json::from_str::<Value>(&line) {
                Ok(req) => handle(session, &req),
                Err(e) => Some(error(Value::Null, -32700, &format!("parse error: {e}"))),
            }
        };
        if let Some(r) = reply {
            writeln!(output, "{r}")?;
            output.flush()?;
        }
    }
    Ok(())
}

fn error(id: Value, code: i64, msg: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": msg}})
}

fn tools() -> Value {
    json!([
        {
            "name": "netlabcraft_run",
            "description": "Run a NetLabCraft command by id (see netlabcraft_commands): build the lab (device.add, link.add), configure it, test it (net.ping, net.traceroute), read the event list (sim.events), save it (file.save).",
            "inputSchema": {"type": "object", "properties": {"command": {"type": "string"}, "params": {"type": "object"}}, "required": ["command"]}
        },
        {
            "name": "netlabcraft_cli",
            "description": "Type command lines at a device's console, as a person would (routers and switches: enable, configure terminal, interface …, show …; PCs: ipconfig, ping, tracert). Returns each line's output and the final prompt.",
            "inputSchema": {"type": "object", "properties": {"device": {"type": "string"}, "lines": {"type": "array", "items": {"type": "string"}}}, "required": ["device", "lines"]}
        },
        {
            "name": "netlabcraft_lab",
            "description": "The lab: every device with its ports, addresses, link state and cables.",
            "inputSchema": {"type": "object", "properties": {}}
        },
        {
            "name": "netlabcraft_commands",
            "description": "The command catalog: ids, labels and parameters.",
            "inputSchema": {"type": "object", "properties": {}}
        }
    ])
}

/// Handles one request; `None` for notifications.
pub fn handle(session: &mut Session, req: &Value) -> Option<Value> {
    let id = req.get("id").cloned();
    let method = req.get("method").and_then(Value::as_str).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or(Value::Null);
    let id = id?;
    let result = match method {
        "initialize" => json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "netlabcraft", "version": env!("CARGO_PKG_VERSION")},
            "instructions": "NetLabCraft is a network simulator. Build labs with netlabcraft_run (device.add, link.add), configure devices with netlabcraft_cli, and verify with net.ping."
        }),
        "ping" => json!({}),
        "tools/list" => json!({"tools": tools()}),
        "tools/call" => call(session, &params),
        _ => return Some(error(id, -32601, &format!("method `{method}` not found"))),
    };
    Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

fn text_result(v: Result<Value, String>) -> Value {
    match v {
        Ok(v) => json!({"content": [{"type": "text", "text": serde_json::to_string_pretty(&v).unwrap_or_default()}], "isError": false}),
        Err(e) => json!({"content": [{"type": "text", "text": e}], "isError": true}),
    }
}

fn call(session: &mut Session, params: &Value) -> Value {
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
    let r = match name {
        "netlabcraft_run" => match args.get("command").and_then(Value::as_str) {
            Some(cmd) => session.execute(cmd, args.get("params").cloned().unwrap_or_else(|| json!({}))).map_err(|e| e.to_string()),
            None => Err("`command` is required".into()),
        },
        "netlabcraft_cli" => {
            session.execute("cli.script", json!({"device": args.get("device"), "lines": args.get("lines")})).map_err(|e| e.to_string())
        }
        "netlabcraft_lab" => session.execute("device.list", json!({})).map_err(|e| e.to_string()),
        "netlabcraft_commands" => Ok(Value::Array(CATALOG.iter().map(|c| json!({"id": c.id, "label": c.label, "params": c.params})).collect())),
        other => Err(format!("unknown tool `{other}`")),
    };
    text_result(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_over_stdio() {
        let input = [
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"netlabcraft_run","arguments":{"command":"file.new","params":{"sample":"lan"}}}}"#,
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"netlabcraft_cli","arguments":{"device":"PC1","lines":["ping 192.168.1.13"]}}}"#,
            r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"netlabcraft_run","arguments":{"command":"bogus"}}}"#,
            "not json",
            r#"{"jsonrpc":"2.0","id":6,"method":"nope"}"#,
        ]
        .join("\n");
        let mut out = Vec::new();
        let mut s = Session::new();
        serve(&mut s, input.as_bytes(), &mut out).unwrap();
        let replies: Vec<Value> = String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(replies.len(), 7);
        assert_eq!(replies[0]["result"]["serverInfo"]["name"], "netlabcraft");
        assert_eq!(replies[1]["result"]["tools"].as_array().unwrap().len(), 4);
        assert_eq!(replies[2]["result"]["isError"], false);
        let text = replies[3]["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("4 answered"), "{text}");
        assert_eq!(replies[4]["result"]["isError"], true);
        assert_eq!(replies[5]["error"]["code"], -32700);
        assert_eq!(replies[6]["error"]["code"], -32601);
    }
}
