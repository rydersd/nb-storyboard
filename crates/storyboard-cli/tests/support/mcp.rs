use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};

use assert_cmd::cargo::cargo_bin;
use serde_json::{Value, json};
use tempfile::TempDir;

pub struct Mcp {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    next_id: u64,
}

impl Mcp {
    pub fn start(workspace: &std::path::Path, client_name: &str) -> Self {
        Self::start_in(workspace, client_name, None)
    }

    pub fn start_in(
        workspace: &std::path::Path,
        client_name: &str,
        current_dir: Option<&std::path::Path>,
    ) -> Self {
        let mut command = Command::new(cargo_bin("storyboard"));
        command.args(["mcp", workspace.to_str().expect("UTF-8 workspace")]);
        if let Some(current_dir) = current_dir {
            command.current_dir(current_dir);
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start MCP server");
        let input = child.stdin.take().expect("MCP stdin");
        let output = BufReader::new(child.stdout.take().expect("MCP stdout"));
        let mut mcp = Self {
            child,
            input,
            output,
            next_id: 1,
        };
        let initialized = mcp.request("initialize", json!({"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":client_name,"version":"test"}}));
        assert!(
            initialized.get("result").is_some(),
            "initialize: {initialized}"
        );
        mcp.notify("notifications/initialized", json!({}));
        mcp
    }

    pub fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}));
        loop {
            let mut line = String::new();
            let bytes = self.output.read_line(&mut line).expect("read MCP response");
            assert!(bytes > 0, "MCP closed stdout while waiting for {method}");
            let response: Value = serde_json::from_str(&line).expect("MCP JSON response");
            if response.get("id") == Some(&json!(id)) {
                return response;
            }
        }
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.send(json!({"jsonrpc":"2.0","method":method,"params":params}));
    }
    pub fn tool(&mut self, name: &str, arguments: Value) -> Value {
        self.request("tools/call", json!({"name":name,"arguments":arguments}))
    }
    fn send(&mut self, value: Value) {
        writeln!(self.input, "{value}").expect("write MCP request");
        self.input.flush().expect("flush MCP request");
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub fn create_project() -> (TempDir, std::path::PathBuf, String) {
    let workspace = TempDir::new().expect("workspace");
    let project = workspace.path().join("story");
    let output = Command::new(cargo_bin("storyboard"))
        .args([
            "new",
            project.to_str().expect("UTF-8 project"),
            "--title",
            "MCP story",
        ])
        .output()
        .expect("create project");
    assert!(
        output.status.success(),
        "new: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let created: Value = serde_json::from_slice(&output.stdout).expect("new JSON");
    (
        workspace,
        project,
        created["version"].as_str().expect("version").into(),
    )
}

pub fn result(response: &Value) -> Value {
    serde_json::from_str(
        response
            .pointer("/result/content/0/text")
            .and_then(Value::as_str)
            .expect("tool text content"),
    )
    .expect("tool JSON content")
}

pub fn error(response: &Value) -> Value {
    assert_eq!(
        response.pointer("/result/isError"),
        Some(&json!(true)),
        "{response}"
    );
    result(response)
}
