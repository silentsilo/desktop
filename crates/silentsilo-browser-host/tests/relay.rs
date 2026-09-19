//! The host as the browser runs it: a child process with the extension's
//! origin as its argument, frames on stdin and stdout.
#![cfg(windows)]

use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use silentsilo_browser_host::{NOT_OURS, NOT_RUNNING};
use silentsilo_shell::browser_pipe::{ClientCheck, Frame, MAX_FRAME, PipeServer, pipe_name};

const DEV: &str = "chrome-extension://acgmibddhpnmaegpegjcibekcnihpfic/";
const FIREFOX_ID: &str = "browser@silentsilo.com";
const FIREFOX_MANIFEST: &str = r"C:\SilentSilo\silentsilo-browser-host.firefox.json";
const HOST: &str = env!("CARGO_BIN_EXE_silentsilo-browser-host");

fn host(origin: &str) -> Child {
    host_expecting(origin, None)
}

/// The host, told (debug builds only) which executable serves the app's
/// pipe. Without that it expects SilentSilo.exe beside itself.
fn host_expecting(origin: &str, server: Option<&std::path::Path>) -> Child {
    let mut command = Command::new(HOST);
    command.env_remove("SILENTSILO_BROWSER_HOST_TEST_SERVER");
    if let Some(server) = server {
        command.env("SILENTSILO_BROWSER_HOST_TEST_SERVER", server);
    }
    command
        .arg(origin)
        .arg("--parent-window=0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

/// The host as Firefox starts it: the manifest's path, then the add-on id.
fn firefox_host(id: &str) -> Child {
    Command::new(HOST)
        .env_remove("SILENTSILO_BROWSER_HOST_TEST_SERVER")
        .arg(FIREFOX_MANIFEST)
        .arg(id)
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
    let mut other = firefox_host("other@silentsilo.com");
    assert_eq!(other.wait().unwrap().code(), Some(2));
    // A Chromium origin where Firefox puts the add-on id.
    let mut swapped = firefox_host(DEV);
    assert_eq!(swapped.wait().unwrap().code(), Some(2));
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
    assert_eq!(answer["message"], NOT_RUNNING);
    send(
        &mut child,
        br#"{"id":"2","type":"logins","origin":"https://a.example"}"#,
    );
    assert_eq!(receive(&mut child)["id"], "2");
    drop(child.stdin.take());
    assert!(child.wait().unwrap().success());

    // Started the way Firefox starts it, the host gets as far.
    let mut child = firefox_host(FIREFOX_ID);
    send(&mut child, br#"{"id":"6","type":"status"}"#);
    let answer = receive(&mut child);
    assert_eq!(answer["id"], "6");
    assert_eq!(answer["code"], "app-not-running");
    drop(child.stdin.take());
    assert!(child.wait().unwrap().success());

    // Listening, but the pipe is served by a program the host does not
    // expect (this test, not SilentSilo.exe beside the host): the host
    // refuses it and writes nothing to it.
    let (stop, stop_rx) = tokio::sync::watch::channel(false);
    let only_the_host = ClientCheck {
        image: HOST.into(),
        same_signer: false,
    };
    let server = runtime
        .block_on(PipeServer::bind(stop_rx, only_the_host))
        .unwrap();
    let handled = Arc::new(AtomicUsize::new(0));
    let count = handled.clone();
    runtime.spawn(server.run(
        move |_: Arc<()>, frame: Frame| {
            count.fetch_add(1, Ordering::SeqCst);
            async move {
                match frame {
                    Frame::Message(body) => {
                        let request: serde_json::Value = serde_json::from_slice(&body).unwrap();
                        serde_json::json!({ "id": request["id"], "type": "echo" })
                            .to_string()
                            .into_bytes()
                    }
                    Frame::TooLarge => b"{\"id\":\"\",\"type\":\"too-large\"}".to_vec(),
                }
            }
        },
        |_| {},
    ));

    let mut child = host(DEV);
    send(&mut child, br#"{"id":"5","type":"status"}"#);
    let answer = receive(&mut child);
    assert_eq!(answer["id"], "5");
    assert_eq!(answer["code"], "app-not-running");
    assert_eq!(answer["message"], NOT_OURS);
    drop(child.stdin.take());
    assert!(child.wait().unwrap().success());
    assert_eq!(
        handled.load(Ordering::SeqCst),
        0,
        "a request reached a pipe that is not the app's"
    );

    // Served by the program the host expects: frames go through unread and
    // come back, an oversized one is refused by the host, and the host
    // leaves when stdin closes.
    let this_test = std::env::current_exe().unwrap();
    let mut child = host_expecting(DEV, Some(&this_test));
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
