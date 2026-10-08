//! Bitwarden's ".zip (With Attachments)" export: `data.json`, the same
//! unencrypted JSON export the window already reads, and each item's files
//! under `attachments/<item name>/<file name>`.
//!
//! The JSON goes to the window as text and becomes entries there
//! (`lib/bitwardenJson.ts`). The files never cross: each is encrypted into
//! the silo as it is read, as a KeePass import does, and the window gets the
//! attachment records with the folder each came from, to match them to
//! items by name.

use std::io::{Read, Seek};

use serde::Serialize;
use tauri::{AppHandle, Manager};
use zeroize::Zeroizing;

use crate::commands::fido::run_blocking;
use crate::commands::vault::{PasswordAttachment, encrypt_attachment_bytes};
use crate::state::AppState;

/// The largest export read: the file on disk, and what it unpacks to.
const MAX_ZIP_BYTES: u64 = 512 * 1024 * 1024;
/// `data.json` alone, the same cap as a CSV or JSON import.
const MAX_JSON_BYTES: u64 = 32 * 1024 * 1024;
/// More entries than any export holds; a zip that says otherwise is not one.
const MAX_ENTRIES: usize = 100_000;
/// Most of a file's buffer set aside up front, from what the zip says it
/// holds: growing it leaves copies of the plain bytes behind, unwiped.
const PREALLOCATE: u64 = 32 * 1024 * 1024;

/// What an export may unpack to, in all and for `data.json` alone.
struct Limits {
    total: u64,
    json: u64,
}

const NOT_AN_EXPORT: &str = crate::err::coded!(
    "err.bitwarden_not_export",
    "This zip is not a Bitwarden export: it has no data.json. Export again with the .zip (With Attachments) format."
);

#[derive(Serialize)]
pub struct ZipFile {
    /// The folder under `attachments/`: the item's name as Bitwarden wrote it.
    folder: String,
    attachment: PasswordAttachment,
}

#[derive(Serialize)]
pub struct BitwardenZip {
    json: String,
    files: Vec<ZipFile>,
}

/// Where a zip entry sits relative to the export's root, which is the zip's
/// own root or, for an export unpacked and zipped again, one folder in it.
fn parts(name: &str) -> Vec<&str> {
    name.split(['/', '\\']).filter(|p| !p.is_empty()).collect()
}

/// Reads the export, handing each attached file to `file` as it is read and
/// keeping what it returns. Nothing is held beyond one file at a time.
pub fn read_export<R: Read + Seek, T>(
    reader: R,
    file: impl FnMut(&str, &str, &[u8]) -> Result<T, String>,
) -> Result<(String, Vec<T>), String> {
    let limits = Limits {
        total: MAX_ZIP_BYTES,
        json: MAX_JSON_BYTES,
    };
    read_export_within(reader, &limits, file)
}

fn read_export_within<R: Read + Seek, T>(
    reader: R,
    limits: &Limits,
    mut file: impl FnMut(&str, &str, &[u8]) -> Result<T, String>,
) -> Result<(String, Vec<T>), String> {
    let mut zip = zip::ZipArchive::new(reader).map_err(|_| {
        crate::err::coded!(
            "err.zip_unreadable",
            "That file is not a zip archive SilentSilo can read."
        )
        .to_string()
    })?;
    if zip.len() > MAX_ENTRIES {
        return Err(NOT_AN_EXPORT.into());
    }

    // `data.json` at the root, or in the one folder an unpacked export was
    // zipped again from.
    let mut root: Option<Option<String>> = None;
    for i in 0..zip.len() {
        let name = zip.name_for_index(i).unwrap_or_default().to_string();
        match parts(&name).as_slice() {
            ["data.json"] => {
                root = Some(None);
                break;
            }
            [dir, "data.json"] if root.is_none() => root = Some(Some((*dir).to_string())),
            _ => {}
        }
    }
    let Some(root) = root else {
        return Err(NOT_AN_EXPORT.into());
    };
    let prefix = |p: &[&str]| -> Option<usize> {
        match &root {
            None => Some(0),
            Some(dir) if p.first() == Some(&dir.as_str()) => Some(1),
            Some(_) => None,
        }
    };

    let mut unpacked: u64 = 0;
    let mut json = None;
    let mut kept = Vec::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| match e {
            zip::result::ZipError::UnsupportedArchive(_) => {
                "This zip is protected with a password or packed in a way SilentSilo cannot read. Export again from Bitwarden.".to_string()
            }
            other => crate::err::coded_with(
                "err.zip_read_failed",
                format!("The zip could not be read: {other}"),
                &[("detail", &other)],
            ),
        })?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_string();
        let all = parts(&name);
        let Some(skip) = prefix(&all) else { continue };
        let rest = &all[skip..];
        let wanted = match rest {
            ["data.json"] => true,
            ["attachments", folder, file] => !folder.is_empty() && !file.is_empty(),
            _ => false,
        };
        if !wanted {
            continue;
        }

        let is_json = rest == ["data.json"];
        if is_json && json.is_some() {
            return Err(
                "This zip holds more than one data.json, so it is not one Bitwarden export.".into(),
            );
        }
        // Counted as it is read, not as the zip claims: a zip can lie.
        // Everything counts against the whole, data.json included.
        let left = limits.total.saturating_sub(unpacked);
        let cap = if is_json { limits.json.min(left) } else { left };
        let mut bytes = Zeroizing::new(Vec::with_capacity(
            entry.size().min(cap).min(PREALLOCATE) as usize
        ));
        (&mut entry)
            .take(cap.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|e| {
                crate::err::coded_with(
                    "err.zip_read_failed",
                    format!("The zip could not be read: {e}"),
                    &[("detail", &e)],
                )
            })?;
        if bytes.len() as u64 > cap {
            return Err(if is_json && cap == limits.json {
                format!(
                    "The data.json in this zip is larger than SilentSilo reads ({} MB).",
                    limits.json / (1024 * 1024)
                )
            } else {
                format!(
                    "That export unpacks to more than SilentSilo reads at once ({} MB).",
                    limits.total / (1024 * 1024)
                )
            });
        }
        unpacked += bytes.len() as u64;

        match rest {
            ["data.json"] => {
                let text = String::from_utf8(std::mem::take(&mut *bytes)).map_err(|_| {
                    crate::err::coded!(
                        "err.bitwarden_data_not_text",
                        "The data.json in this zip is not text."
                    )
                    .to_string()
                })?;
                json = Some(text);
            }
            ["attachments", folder, name] => kept.push(file(folder, name, &bytes)?),
            _ => {}
        }
    }
    let json = json.ok_or_else(|| NOT_AN_EXPORT.to_string())?;
    Ok((json, kept))
}

/// The export at `path`: its JSON, and its files already encrypted into the
/// silo on screen. The window drops those it does not keep.
#[tauri::command]
pub async fn passwords_read_bitwarden_zip(
    app: AppHandle,
    path: String,
) -> Result<BitwardenZip, String> {
    run_blocking(move || {
        let meta = std::fs::metadata(&path).map_err(|e| e.to_string())?;
        if !meta.is_file() {
            return Err(crate::err::coded!("err.not_a_file", "Not a file.").into());
        }
        if meta.len() > MAX_ZIP_BYTES {
            return Err(crate::err::coded!(
                "err.export_too_large",
                "That export is larger than 512 MB."
            )
            .into());
        }
        let snapshot = crate::state::snapshot_focused_session(&app.state::<AppState>())?;
        let reader =
            std::io::BufReader::new(std::fs::File::open(&path).map_err(|e| e.to_string())?);
        let mut written = Vec::new();
        let result = read_export(reader, |folder, name, bytes| {
            let attachment = encrypt_attachment_bytes(&snapshot, name, bytes)?;
            written.push(attachment.blob_id().to_string());
            Ok(ZipFile {
                folder: folder.to_string(),
                attachment,
            })
        });
        match result {
            Ok((json, files)) => Ok(BitwardenZip { json, files }),
            Err(e) => {
                // Nothing refers to what was encrypted before the failure.
                for blob in written {
                    if let Ok(id) = uuid::Uuid::parse_str(&blob) {
                        let _ = silentsilo_vault::remove_blob_from_cache(&snapshot.root, id);
                    }
                }
                Err(e)
            }
        }
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    use zip::write::SimpleFileOptions;

    fn zip_of(files: &[(&str, &[u8])], method: zip::CompressionMethod) -> Cursor<Vec<u8>> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default().compression_method(method);
        for (name, bytes) in files {
            writer.start_file(*name, options).unwrap();
            writer.write_all(bytes).unwrap();
        }
        let mut out = writer.finish().unwrap();
        out.set_position(0);
        out
    }

    /// Folder, file name and content of each attachment read.
    type Files = Vec<(String, String, Vec<u8>)>;

    fn read(zip: Cursor<Vec<u8>>) -> Result<(String, Files), String> {
        read_export(zip, |folder, name, bytes| {
            Ok((folder.to_string(), name.to_string(), bytes.to_vec()))
        })
    }

    #[test]
    fn reads_the_json_and_every_attached_file() {
        for method in [
            zip::CompressionMethod::Stored,
            zip::CompressionMethod::Deflated,
        ] {
            let (json, files) = read(zip_of(
                &[
                    ("data.json", br#"{"items":[]}"#),
                    ("attachments/", b""),
                    ("attachments/Bank/codes.txt", b"123"),
                    ("attachments/Bank/scan.pdf", b"%PDF"),
                    ("attachments/Mail_1/key.asc", b"key"),
                    ("notes.txt", b"ignored"),
                ],
                method,
            ))
            .unwrap();
            assert_eq!(json, r#"{"items":[]}"#);
            assert_eq!(
                files,
                vec![
                    ("Bank".into(), "codes.txt".into(), b"123".to_vec()),
                    ("Bank".into(), "scan.pdf".into(), b"%PDF".to_vec()),
                    ("Mail_1".into(), "key.asc".into(), b"key".to_vec()),
                ]
            );
        }
    }

    #[test]
    fn an_export_zipped_again_from_its_folder_reads_the_same() {
        let (json, files) = read(zip_of(
            &[
                ("export/data.json", b"{}"),
                ("export/attachments/Bank/codes.txt", b"123"),
                ("other/attachments/Bank/x.txt", b"no"),
            ],
            zip::CompressionMethod::Stored,
        ))
        .unwrap();
        assert_eq!(json, "{}");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].1, "codes.txt");
    }

    #[test]
    fn a_zip_without_data_json_is_not_an_export() {
        let err = read(zip_of(
            &[("attachments/Bank/codes.txt", b"123")],
            zip::CompressionMethod::Stored,
        ))
        .unwrap_err();
        assert!(err.contains("data.json"), "{err}");
        assert!(read(Cursor::new(b"not a zip".to_vec())).is_err());
    }

    #[test]
    fn a_path_that_climbs_out_is_not_an_attachment() {
        let (_, files) = read(zip_of(
            &[
                ("data.json", b"{}"),
                ("attachments/../../evil.txt", b"x"),
                ("attachments/Bank/deeper/file.txt", b"x"),
            ],
            zip::CompressionMethod::Stored,
        ))
        .unwrap();
        // Only `attachments/<folder>/<file>`, and names stay text: nothing
        // here touches a path.
        assert!(files.is_empty());
    }

    #[test]
    fn a_failing_file_stops_the_read() {
        let zip = zip_of(
            &[("data.json", b"{}"), ("attachments/A/a.txt", b"x")],
            zip::CompressionMethod::Stored,
        );
        let err = read_export(zip, |_, _, _| Err::<(), _>("disk full".to_string())).unwrap_err();
        assert_eq!(err, "disk full");
    }

    fn read_within(zip: Cursor<Vec<u8>>, total: u64, json: u64) -> Result<(String, Files), String> {
        read_export_within(zip, &Limits { total, json }, |folder, name, bytes| {
            Ok((folder.to_string(), name.to_string(), bytes.to_vec()))
        })
    }

    #[test]
    fn data_json_counts_against_the_whole_and_nothing_wraps_past_it() {
        // Files fill the budget, data.json takes it past, and a file after
        // that must still be refused rather than read without a cap.
        let zip = zip_of(
            &[
                ("attachments/A/a.bin", &[0u8; 60]),
                ("data.json", &[b' '; 30]),
                ("attachments/B/b.bin", &[0u8; 5]),
            ],
            zip::CompressionMethod::Deflated,
        );
        let err = read_within(zip, 80, 50).unwrap_err();
        assert!(err.contains("unpacks to more"), "{err}");
    }

    #[test]
    fn a_data_json_over_its_own_cap_says_so() {
        let zip = zip_of(
            &[("data.json", &[b' '; 30])],
            zip::CompressionMethod::Stored,
        );
        let err = read_within(zip, 1000, 20).unwrap_err();
        assert!(err.contains("data.json in this zip is larger"), "{err}");
    }

    #[test]
    fn a_second_data_json_is_refused() {
        // Two names that are one place once separators are dropped.
        let zip = zip_of(
            &[("data.json", b"{}"), ("/data.json", b"{\"x\":1}")],
            zip::CompressionMethod::Stored,
        );
        let err = read(zip).unwrap_err();
        assert!(err.contains("more than one data.json"), "{err}");
    }
}
