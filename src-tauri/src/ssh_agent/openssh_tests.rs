//! The agent's messages and signatures against OpenSSH's own clients:
//! `ssh-add -L` lists the keys, and `ssh-keygen -Y sign` signs through the
//! agent and checks what it got. Skipped where OpenSSH is not installed.

use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

use silentsilo_shell::ssh_agent_channel::{AgentServer, Peer};
use ssh_key::PrivateKey;
use ssh_key::private::{Ed25519Keypair, KeypairData};

use super::keys::{private_key, sign_with};
use super::proto::{self, Request};
use super::test_keys::{RSA_PKCS1, armour};

fn tool(name: &str) -> Option<PathBuf> {
    #[cfg(windows)]
    let path = PathBuf::from(format!(r"C:\Windows\System32\OpenSSH\{name}.exe"));
    #[cfg(not(windows))]
    let path = PathBuf::from(format!("/usr/bin/{name}"));
    path.is_file().then_some(path)
}

fn keys() -> Vec<PrivateKey> {
    vec![
        PrivateKey::new(
            KeypairData::Ed25519(Ed25519Keypair::from_seed(&[3; 32])),
            "",
        )
        .unwrap(),
        private_key(&armour("RSA", RSA_PKCS1)).unwrap(),
    ]
}

/// The agent's answers with the app taken out: these keys, always yes.
fn answer(message: &[u8]) -> Vec<u8> {
    let keys = keys();
    match proto::parse(message) {
        Request::Identities => proto::identities_answer(
            &keys
                .iter()
                .enumerate()
                .map(|(i, k)| (k.public_key().to_bytes().unwrap(), format!("test key {i}")))
                .collect::<Vec<_>>(),
        ),
        Request::Sign { key, data, flags } => keys
            .iter()
            .find(|k| k.public_key().to_bytes().unwrap() == key)
            .and_then(|k| sign_with(k, &data, flags).ok())
            .map(|s| proto::sign_answer(&s))
            .unwrap_or_else(proto::failure),
        _ => proto::failure(),
    }
}

#[tokio::test]
async fn openssh_lists_the_keys_and_signs_through_the_agent() {
    let (Some(ssh_add), Some(ssh_keygen)) = (tool("ssh-add"), tool("ssh-keygen")) else {
        eprintln!("OpenSSH is not installed; skipped");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    #[cfg(windows)]
    let address = format!(r"\\.\pipe\silentsilo-agent-openssh-{}", std::process::id());
    #[cfg(not(windows))]
    let address = dir.path().join("agent.sock").display().to_string();

    let (stop, stop_rx) = tokio::sync::watch::channel(false);
    #[cfg(windows)]
    let server = AgentServer::bind_at(address.clone(), stop_rx)
        .await
        .unwrap();
    #[cfg(not(windows))]
    let server = AgentServer::bind_at(PathBuf::from(&address), stop_rx)
        .await
        .unwrap();
    tokio::spawn(server.run(
        |_peer: Arc<Peer>, _state: Arc<()>, message: Vec<u8>| async move { answer(&message) },
        |_| {},
    ));

    let run = |program: &PathBuf, args: &[&str]| {
        let program = program.clone();
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let address = address.clone();
        tokio::task::spawn_blocking(move || {
            Command::new(program)
                .args(args)
                .env("SSH_AUTH_SOCK", address)
                .output()
                .unwrap()
        })
    };

    let listed = run(&ssh_add, &["-L"]).await.unwrap();
    let listed = String::from_utf8_lossy(&listed.stdout).into_owned();
    for key in keys() {
        let public = key.public_key().to_openssh().unwrap();
        let body = public.split(' ').nth(1).unwrap();
        assert!(listed.contains(body), "{listed}");
    }

    // Signed by the agent: ssh-keygen holds only the public half.
    for (i, key) in keys().iter().enumerate() {
        let public = dir.path().join(format!("key{i}.pub"));
        std::fs::write(&public, key.public_key().to_openssh().unwrap()).unwrap();
        let message = dir.path().join(format!("message{i}"));
        std::fs::write(&message, b"a commit").unwrap();
        let signed = run(
            &ssh_keygen,
            &[
                "-Y",
                "sign",
                "-f",
                public.to_str().unwrap(),
                "-n",
                "git",
                message.to_str().unwrap(),
            ],
        )
        .await
        .unwrap();
        assert!(
            signed.status.success(),
            "{}",
            String::from_utf8_lossy(&signed.stderr)
        );
        let sig = std::fs::read_to_string(dir.path().join(format!("message{i}.sig"))).unwrap();
        let sig: ssh_key::SshSig = sig.parse().unwrap();
        key.public_key()
            .verify("git", b"a commit", &sig)
            .expect("the agent's signature verifies");
    }
    stop.send(true).unwrap();
}
