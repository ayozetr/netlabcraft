//! Labs of the lab book (`docs/labs/`) driven through the MCP server, as an agent would: the
//! test starts `netlabcraft-cli mcp` and speaks JSON-RPC to it over stdin/stdout.

// The test starts the CLI binary under test and speaks to it over pipes, in a test only.
#![allow(clippy::disallowed_methods)]

#[path = "../../../crates/engine/tests/labbook/mod.rs"]
mod labbook;

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

struct Mcp {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl Mcp {
    fn start() -> Mcp {
        let mut child = Command::new(env!("CARGO_BIN_EXE_netlabcraft-cli"))
            .arg("mcp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("netlabcraft-cli mcp starts");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = BufReader::new(child.stdout.take().expect("stdout"));
        let mut m = Mcp { child, stdin, stdout, next_id: 0 };
        let init =
            m.request("initialize", json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "lab-book", "version": "1"}}));
        assert_eq!(init["result"]["serverInfo"]["name"], "netlabcraft", "{init}");
        writeln!(m.stdin, "{}", json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).expect("write");
        let tools = m.request("tools/list", json!({}));
        assert_eq!(tools["result"]["tools"].as_array().map(Vec::len), Some(4), "{tools}");
        m
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        writeln!(self.stdin, "{}", json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})).expect("write");
        self.stdin.flush().expect("flush");
        let mut line = String::new();
        self.stdout.read_line(&mut line).expect("read");
        let reply: Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("{e}: {line}"));
        assert_eq!(reply["id"], id, "{reply}");
        reply
    }

    /// A tool's answer: the JSON in its text, or the error text.
    fn tool(&mut self, name: &str, arguments: Value) -> Result<Value, String> {
        let r = self.request("tools/call", json!({"name": name, "arguments": arguments}));
        let text = r["result"]["content"][0]["text"].as_str().unwrap_or_default().to_string();
        if r["result"]["isError"] == true {
            return Err(text);
        }
        serde_json::from_str(&text).map_err(|e| format!("{e}: {text}"))
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl labbook::Driver for Mcp {
    fn run(&mut self, id: &str, params: Value) -> Result<Value, String> {
        self.tool("netlabcraft_run", json!({"command": id, "params": params}))
    }

    fn type_lines(&mut self, device: &str, lines: &[String]) -> Result<Vec<String>, String> {
        let r = self.tool("netlabcraft_cli", json!({"device": device, "lines": lines}))?;
        Ok(r["outputs"].as_array().map(|a| a.iter().map(|o| o.as_str().unwrap_or_default().to_string()).collect()).unwrap_or_default())
    }
}

fn check(file: &str) {
    let lab = labbook::load(file);
    let mut mcp = Mcp::start();
    let fails = labbook::run(&lab, &mut mcp);
    assert!(fails.is_empty(), "{file} through MCP ({}):\n{}", lab.title, fails.join("\n"));
    // The lab the agent built is all there.
    let devices = mcp.tool("netlabcraft_lab", json!({})).expect("netlabcraft_lab");
    assert!(devices.as_array().is_some_and(|d| !d.is_empty()));
}

#[test]
fn itn_device_basics_through_mcp() {
    check("01-device-basics.md");
}

#[test]
fn srwe_vlans_and_trunks_through_mcp() {
    check("05-vlans-and-trunks.md");
}

#[test]
fn srwe_dhcp_through_mcp() {
    check("09-dhcpv4-dhcpv6.md");
}

#[test]
fn srwe_hsrp_through_mcp() {
    check("10-hsrp.md");
}

#[test]
fn ensa_ospf_through_mcp() {
    check("14-ospf.md");
}

#[test]
fn ensa_acls_through_mcp() {
    check("15-acls.md");
}

#[test]
fn ensa_nat_through_mcp() {
    check("16-nat-pat.md");
}

#[test]
fn ensa_restconf_through_mcp() {
    check("20-restconf.md");
}

#[test]
fn srwe_troubleshooting_routing_through_mcp() {
    check("22-troubleshoot-routing.md");
}

#[test]
fn ensa_troubleshooting_acls_through_mcp() {
    check("24-troubleshoot-acls.md");
}

#[test]
fn ensa_troubleshooting_nat_through_mcp() {
    check("26-troubleshoot-nat.md");
}

#[test]
fn ensa_troubleshooting_ospf_authentication_through_mcp() {
    check("28-troubleshoot-ospf-authentication.md");
}

#[test]
fn ensa_troubleshooting_acl_icmp_through_mcp() {
    check("29-troubleshoot-acl-icmp.md");
}

#[test]
fn srwe_inspecting_spanning_tree_through_mcp() {
    check("30-inspect-spanning-tree.md");
}
