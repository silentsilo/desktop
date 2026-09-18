//! The host as the browser runs it: a child process with the extension's
//! origin as its argument, frames on stdin and stdout.
#![cfg(windows)]

use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};

use silentsilo_shell::browser_pipe::{Frame, MAX_FRAME, PipeServer, pipe_name};

const DEV: &str = "chrome-extension://acgmibddhpnmaegpegjcibekcnihpfic/";

fn host(origin: &str) -> Child {
    Command::new(env!("CARGO_BIN_EXE_silentsilo-browser-host"))
        .arg(origin)
        .arg("--parent-window=0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

fn send(child: &mut Child, body: &[u8]) {
    let stdin = child.stdin.as_mut().unwrap();
    stdin.write_all(&(body.len() as u32).to_le_bytes()).unwrap();
    stdin.write_all(body).unwrap();
    stdin.flush().unwrap();
}

fn receive(child: &mut Child) -> serde_json::Value {
    let stdout = child.stdout.as_mut().unwrap();
    let mut len = [0u8; 4];
    stdout.read_exact(&mut len).unwrap();
    let mut body = vec![0u8; u32::from_le_bytes(len) as usize];
    stdout.read_exact(&mut body).unwrap();
    serde_json::from_slice(&body).unwrap()
}

/// Whether the real app is serving on this machine, in which case the name
/// is taken and neither half can run.
fn pipe_exists() -> bool {
    match tokio::net::windows::named_pipe::ClientOptions::new().open(pipe_name().unwrap()) {
        Ok(_) => true,
        Err(e) => e.kind() != std::io::ErrorKind::NotFound,
    }
}

#[test]
fn an_extension_not_on_the_list_is_turned_away() {
    let mut child = host("chrome-extension://bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb/");
    assert_eq!(child.wait().unwrap().code(), Some(2));
    let mut none = host("");
    assert_eq!(none.wait().unwrap().code(), Some(2));
}

/// One test, in order: both halves use the real per-user pipe name.
#[test]
fn without_the_app_it_answers_alone_and_with_it_it_relays() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let _guard = runtime.enter();
    if pipe_exists() {
        eprintln!("skipped: the app's browser pipe is open on this machine");
        return;
    }

    // Nothing listening: every request gets its own refusal, and closing
    // stdin ends the host.
    let mut child = host(DEV);
    send(&mut child, br#"{"id":"1","type":"status"}"#);
    let answer = receive(&mut child);
    assert_eq!(answer["id"], "1");
    assert_eq!(answer["code"], "app-not-running");
    send(
        &mut child,
        br#"{"id":"2","type":"logins","origin":"https://a.example"}"#,
    );
    assert_eq!(receive(&mut child)["id"], "2");
    drop(child.stdin.take());
    assert!(child.wait().unwrap().success());

    // Listening: frames go through unread and come back, an oversized one
    // is refused by the host, and the host leaves when stdin closes.
    let (stop, stop_rx) = tokio::sync::watch::channel(false);
    let server = runtime.block_on(PipeServer::bind(stop_rx)).unwrap();
    runtime.spawn(server.run(|frame: Frame| async move {
        match frame {
            Frame::Message(body) => {
                let request: serde_json::Value = serde_json::from_slice(&body).unwrap();
                serde_json::json!({ "id": request["id"], "type": "echo" })
                    .to_string()
                    .into_bytes()
            }
            Frame::TooLarge => b"{\"id\":\"\",\"type\":\"too-large\"}".to_vec(),
        }
    }));

    let mut child = host(DEV);
    send(&mut child, br#"{"id":"3","type":"status"}"#);
    let answer = receive(&mut child);
    assert_eq!(
        (answer["id"].as_str(), answer["type"].as_str()),
        (Some("3"), Some("echo"))
    );

    send(&mut child, &vec![b' '; MAX_FRAME + 1]);
    let refusal = receive(&mut child);
    assert_eq!(refusal["code"], "bad-request");

    send(&mut child, br#"{"id":"4","type":"status"}"#);
    assert_eq!(receive(&mut child)["id"], "4");

    drop(child.stdin.take());
    assert!(child.wait().unwrap().success());
    let _ = stop.send(true);
}
