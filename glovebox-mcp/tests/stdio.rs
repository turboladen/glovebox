use std::{process::Stdio, time::Duration};

use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
    time::timeout,
};

// Anything but protocol frames on stdout corrupts the connection, so this
// test reads every line the process writes to stdout until it exits.
#[tokio::test]
async fn the_binary_speaks_only_protocol_on_stdout() {
    let dir = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_glovebox-mcp"))
        .env("GLOVEBOX_DB_PATH", dir.path().join("glovebox.db"))
        .env("RUST_LOG", "debug")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("spawn glovebox-mcp");

    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap()).lines();

    let initialize = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": { "name": "stdio-test", "version": "0" }
        }
    });
    stdin
        .write_all(format!("{initialize}\n").as_bytes())
        .await
        .unwrap();
    stdin.flush().await.unwrap();

    let first_line = timeout(Duration::from_secs(30), stdout.next_line())
        .await
        .expect("initialize response before the deadline")
        .unwrap()
        .expect("stdout closed before the initialize response");
    let v: serde_json::Value = serde_json::from_str(first_line.trim()).unwrap();
    assert_eq!(v["jsonrpc"], "2.0");
    assert!(v["result"]["serverInfo"]["name"].is_string());

    // Closing stdin ends the session; the process must exit, and everything
    // it still writes must be a JSON object.
    drop(stdin);
    timeout(Duration::from_secs(30), async {
        while let Some(line) = stdout.next_line().await.unwrap() {
            let v: serde_json::Value = serde_json::from_str(line.trim())
                .unwrap_or_else(|e| panic!("non-JSON line on stdout ({e}): {line}"));
            assert!(v.is_object(), "stdout line is not a JSON object: {line}");
        }
    })
    .await
    .expect("stdout closes after stdin closes");
    let status = timeout(Duration::from_secs(30), child.wait())
        .await
        .expect("process exits after stdin closes")
        .unwrap();
    assert!(status.success(), "exit status: {status}");
}
