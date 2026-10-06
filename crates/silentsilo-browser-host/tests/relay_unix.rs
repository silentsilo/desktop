//! The host as a browser runs it on Linux: a child process with the
//! extension's origin as its argument, frames on stdin and stdout, and the
//! app's socket in the runtime directory it is given.
#![cfg(unix)]

use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use silentsilo_browser_host::{NOT_OURS, NOT_RUNNING};
use silentsilo_shell::browser_pipe::{ClientCheck, Frame, PipeServer};

const DEV: &str = "chrome-extension://nomggfahfnppbkognojcibjhmlpbjgbl/";
const HOST: &str = env!("CARGO_BIN_EXE_silentsilo-browser-host");

/// A runtime directory of the test's own, removed on drop.
struct Runtime(PathBuf);

impl Runtime {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("silentsilo-host-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        Self(dir)
    }

    fn socket(&self) -> PathBuf {
        self.0.join("silentsilo").join("browser.sock")
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn host(runtime: &Path) -> Child {
    Command::new(HOST)
        .env("XDG_RUNTIME_DIR", runtime)
        .arg(DEV)
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

#[test]
fn manifests_go_where_this_user_s_browsers_look_and_come_away_again() {
    let home = Runtime::new("home");
    let chrome = home.0.join(".config").join("google-chrome");
    let firefox = home.0.join(".mozilla");
    std::fs::create_dir_all(&chrome).unwrap();
    std::fs::create_dir_all(&firefox).unwrap();
    let run = |flag: &str| {
        Command::new(HOST)
            .env("HOME", &home.0)
            .arg(flag)
            .status()
            .unwrap()
    };
    assert!(run("--install-manifests").success());
    let written = chrome
        .join("NativeMessagingHosts")
        .join("com.silentsilo.desktop.json");
    let body: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&written).unwrap()).unwrap();
    assert_eq!(body["path"], HOST);
    assert!(body["allowed_origins"].is_array());
    let firefox_manifest = firefox
        .join("native-messaging-hosts")
        .join("com.silentsilo.desktop.json");
    let body: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&firefox_manifest).unwrap()).unwrap();
    assert!(body["allowed_extensions"].is_array());
    assert!(
        !home.0.join(".config").join("chromium").exists(),
        "nothing made for a browser that is not there"
    );

    assert!(run("--remove-manifests").success());
    assert!(!written.exists());
    assert!(!firefox_manifest.exists());
}

#[test]
fn without_the_app_it_answers_alone() {
    let runtime = Runtime::new("alone");
    let mut child = host(&runtime.0);
    send(&mut child, br#"{"id":"1","type":"status"}"#);
    let answer = receive(&mut child);
    assert_eq!(answer["id"], "1");
    assert_eq!(answer["code"], "app-not-running");
    assert_eq!(answer["message"], NOT_RUNNING);
    drop(child.stdin.take());
    assert!(child.wait().unwrap().success());
}

#[test]
fn with_the_app_it_relays_and_a_shared_directory_is_refused() {
    let runtime = Runtime::new("relay");
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (stop, stop_rx) = tokio::sync::watch::channel(false);
    let only_the_host = ClientCheck { image: HOST.into() };
    let server = rt
        .block_on(PipeServer::bind_at(
            runtime.socket(),
            stop_rx,
            only_the_host,
        ))
        .unwrap();
    rt.spawn(server.run::<(), _, _, _>(
        |_state, frame| async move {
            match frame {
                Frame::Message(_) => br#"{"id":"1","type":"status","state":"locked"}"#.to_vec(),
                Frame::TooLarge => Vec::new(),
            }
        },
        |_warning| {},
    ));

    let mut child = host(&runtime.0);
    send(&mut child, br#"{"id":"1","type":"status"}"#);
    assert_eq!(receive(&mut child)["state"], "locked");
    drop(child.stdin.take());
    assert!(child.wait().unwrap().success());

    // The socket's directory opened to others: anyone could have made the
    // socket, so the host sends nothing to it.
    let dir = runtime.socket().parent().unwrap().to_path_buf();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut child = host(&runtime.0);
    send(&mut child, br#"{"id":"2","type":"status"}"#);
    let answer = receive(&mut child);
    assert_eq!(answer["code"], "app-not-running");
    assert_eq!(answer["message"], NOT_OURS);
    drop(child.stdin.take());
    assert!(child.wait().unwrap().success());
    stop.send(true).unwrap();
}
