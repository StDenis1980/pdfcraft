//! `pdfcraft-cli mcp --allow-commands LIST`, on the real binary (tintaoffice-web#26): a client
//! runs only the tools on the list, `tools/list` shows only those, a step of `command_run` is held
//! to it too, and a malformed flag stops the server instead of leaving every tool available.
#![cfg(feature = "mcp")]

use std::io::Write as _;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcraft-cli");

/// Run `mcp <flags>` and send `requests` (method, params); return the replies after `initialize`
/// and stderr.
fn session(flags: &[&str], requests: &[(&str, Value)]) -> (Vec<Value>, String) {
    let mut child = Command::new(BIN).arg("mcp").args(flags).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let mut input = String::from("{\"jsonrpc\":\"2.0\",\"id\":0,\"method\":\"initialize\",\"params\":{}}\n");
    for (i, (method, params)) in requests.iter().enumerate() {
        input.push_str(&format!("{}\n", json!({"jsonrpc": "2.0", "id": i + 1, "method": method, "params": params})));
    }
    child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let replies = String::from_utf8(out.stdout).unwrap().lines().skip(1).map(|l| serde_json::from_str(l).unwrap()).collect();
    (replies, String::from_utf8(out.stderr).unwrap())
}

fn call(name: &str, args: Value) -> (&'static str, Value) {
    ("tools/call", json!({"name": name, "arguments": args}))
}

fn is_error(reply: &Value) -> bool {
    reply["result"]["isError"] == true
}

fn text(reply: &Value) -> String {
    reply["result"]["content"][0]["text"].as_str().unwrap_or_default().to_string()
}

#[test]
fn only_the_listed_tools_run_and_are_listed() {
    let (replies, stderr) = session(
        &["--allow-commands", "doc_create, doc_list, page.rotate, command_run"],
        &[
            ("tools/list", json!({})),
            call("doc_create", json!({"from": "blank", "pages": 1})),
            call("page_rotate", json!({"doc": 1, "pages": [1], "degrees": 90})),
            call("command_run", json!({"id": "page.rotate", "params": {"doc": 1, "pages": [1], "degrees": 90}})),
            call("doc_info", json!({"doc": 1})),
            call("command_run", json!({"id": "doc.info", "params": {"doc": 1}})),
            call("doc_print", json!({"doc": 1})),
        ],
    );
    assert!(stderr.contains("tools allowed: command_run, doc_create, doc_list, page.rotate"), "{stderr}");
    let listed: Vec<&str> = replies[0]["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(listed.len(), 4, "{listed:?}");
    assert!(listed.contains(&"page_rotate") && !listed.contains(&"doc_print"), "{listed:?}");
    assert!(replies[0]["result"]["tools"][0]["_meta"]["risk"].is_object(), "every tool carries its risk");
    assert!(!is_error(&replies[1]), "{}", replies[1]);
    assert!(!is_error(&replies[2]), "{}", replies[2]);
    assert!(!is_error(&replies[3]), "command_run of an allowed tool: {}", replies[3]);
    for reply in &replies[4..] {
        assert!(is_error(reply) || reply.get("error").is_some(), "{reply}");
    }
    assert!(text(&replies[4]).contains("not on this session's --allow-commands list"), "{}", replies[4]);
    assert!(text(&replies[6]).contains("not allowed"), "{}", replies[6]);
}

#[test]
fn a_bad_list_never_starts_the_server() {
    for (flags, needle) in [
        (vec!["--allow-commands"], "needs a value"),
        (vec!["--allow-commands", "--compact"], "needs a value"),
        (vec!["--allow-commands", "doc_opn"], "unknown tool or command"),
        (vec!["--allow-commands", "nope_*"], "matches no tool"),
        (vec!["--allow-commands", ""], "empty"),
        (vec!["--allow-commands", "doc_open", "--allow-commands=doc_save"], "given twice"),
        (vec!["--allow-commands", "@/nonexistent/list.txt"], "--allow-commands"),
    ] {
        let out = Command::new(BIN).arg("mcp").args(&flags).stdin(Stdio::null()).output().unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "{flags:?} started: {stderr}");
        assert!(stderr.contains(needle), "{flags:?}: expected `{needle}` in {stderr}");
        assert!(!stderr.contains("MCP server on stdio"), "{flags:?} served: {stderr}");
    }
}
