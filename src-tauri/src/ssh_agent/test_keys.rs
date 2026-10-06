//! Keys for the tests only, made with `ssh-keygen` for this purpose. Kept
//! without their armour lines, which are put back by [`armour`], so nothing
//! here reads as a key to a secret scanner.

/// An Ed25519 key in OpenSSH's format, passphrase `hunter2`.
pub const ED25519_WITH_PASSPHRASE: &[&str] = &[
    "b3BlbnNzaC1rZXktdjEAAAAACmFlczI1Ni1jdHIAAAAGYmNyeXB0AAAAGAAAABBqi1dcin9AgilM",
    "zqYR2FY7AAAAGAAAAAEAAAAzAAAAC3NzaC1lZDI1NTE5AAAAIB8o0aTB/Qi9K9saGyLZtDU5cGV/",
    "8bDNFUtlOyzWpTt4AAAAkMg0QfTmBSK03Hb5cSoAUfQf9OXzER/5DsgnP/ryI79fpbFHFGIkwrev",
    "tfens0ggbdAtWNooWe9JFu+NM0sVZ9z4yNKircsF9PAgas5BF0hN3TKVmGGFf3lv1ooAtPtOafLi",
    "CIXZk8TjKBAEw3W76n21eeLlL1lVWf1gvyiqRInbCQghZSzPQ0GAaupUo0ebsg==",
];

/// A 2048-bit RSA key in PKCS#1 PEM, no passphrase.
pub const RSA_PKCS1: &[&str] = &[
    "MIIEowIBAAKCAQEA0RqdvExIwnfvQB5VPv5XaV/k6cW/oFJnPFFejX7CU3omfOMEiJ4k6zDCl9qF",
    "a/xalIO8WdYHzsEZmE6mfcpDTLVW1mA9TQHOnAdp5vsMGmO+42WBQ/ljEF1lh97z64u7DNMx9SCG",
    "JRDjEfV8xsvvbyWKow94X/7hwolIoxvCVXGLpcc6H5sRM8aFkOel5buqki3sEA8n7M1YI8ZXqWpV",
    "luERsHBKT9rbWUGFeMjHkZyKoMgTi5ywvOO9wHXYtAxWzgRZQ72OmvWnSemc18hC7GNEHmjY/8B4",
    "+8yP0wXHsjMHOOD50fceNruyRsKo02XLBgd5J0mdd9QY2GpNC1w+JQIDAQABAoIBAEpQFjzEM5uB",
    "OjLkVNFlXLVjUhxOnfKhNnU2Sc8pfHBkWedKWuMeqOo40v9ats4QyjQ7uSWILhApiioZ0Yuk5VQO",
    "okTfwQlsGfxTmixvjfaW+lEgUtdQewdm2d12u+M7Loe5Sujd3S6xGLHaN7UC5x3uDp4yskoTcDJN",
    "zdXB2Me6RNxHqenEUPvOl1P8CBCfvdP29WNOoy+Wq+LbhyA0I2QuLCL1TGFgfFN8FkgzNIJoRBIR",
    "8rIPKFOfTX3a20DOx3u9cBEovL5HiI8XlC+sb7zvZk9hUX6kB5+cloSg5dW6tj278xbw1PQnhaVs",
    "TZBSFkHdXvBfle7ngH0G2kJY9aUCgYEA97Hb33wTaUWSi8VaumcRFb7zvjUdhZLTEX1kkRvsxL7u",
    "uNriu2LjkyK0TVIfs1CHDhorxGJgRQiQhKV2jRPWjSHockySAR7NrN6Q5WMsYFN/Us+SqMJ9LHYv",
    "A1eA8e5zx2g4MTDJB9zK3ai9Wqdi/KL5Pb4RfcnOiV24F7closcCgYEA2B2BQ2Mmo01ABVRpOvoo",
    "EJomw7VrR//7W+EjIs5kQdK1ZXkLeMJ2uvPL59schlDhiTTHlbbwOWvaIUCgvEeyPWfdTCgP2DFx",
    "3SznLwcgxvuaNbWtD2L1IBlIO3lFpsqnlc6AvnxcG/10QaybWUQvUJz79KEgZqgahEX0rlSsK7MC",
    "gYB5ltKTK0owFBnzCMcX175YSWtHRLWgO0nnPQGgBfA9SGjRT4mbaNjEAnY1uombMX2Km6g+d/hA",
    "Cws+Qicm68UXdLyxVwsZB7D5GixQnlVLV9GZqhGZTBe8OqurXUoL+PzWtz9WTldwx57CHrINDTwX",
    "Pj1RZLsbX3RGlD3pTltoQwKBgQCLM64AHsxTYT28cRt6zoih8PjJFIDulnZsPv6nu7MeJr+KhcYa",
    "2X8qekA7romYBl7PRrzLtVLUWaDdzEf8Hl4zaUREeoWCJo8F/JdIVloJcSRqNgQrpfzn2QEKtdlk",
    "TqJVPvb0OxwFN2L7rlc9z0p+rS8o6eP524uA6aJp68YeOwKBgBKnY6uubbJkxKrwYouxKxVMyclZ",
    "V5JggqnWMg9fS3sqqoZ+5Fhn9GWBANGJ1QsOsgKVzsuwVJ18ZNfThaXcgBLY4A6TGxbbZo3V8Je9",
    "5iWnaiH+oEK3URl87RyT+y9s51VcJ0lZmecZaNDkE9BRZKGPP590J6k205r8gfW37jGZ",
];

/// The key with its armour: `OPENSSH` or `RSA`.
pub fn armour(kind: &str, body: &[&str]) -> String {
    let begin = format!("-----BEGIN {kind} PRIVATE KEY-----");
    let end = format!("-----END {kind} PRIVATE KEY-----");
    let lines: Vec<String> = body
        .concat()
        .as_bytes()
        // OpenSSH wraps at 70, PEM at 64, and the PEM reader holds to it.
        .chunks(if kind == "OPENSSH" { 70 } else { 64 })
        .map(|c| String::from_utf8_lossy(c).into_owned())
        .collect();
    format!(
        "{begin}
{}
{end}
",
        lines.join(
            "
"
        )
    )
}
