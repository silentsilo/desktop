//! The host must start on a Windows that never installed the Visual C++
//! redistributable: build.rs links its runtime statically, for every
//! profile, so the debug binary this test sees is linked the same way as
//! the shipped one. Read from the PE import tables, plain and delay-loaded.
#![cfg(all(windows, target_env = "msvc"))]

const HOST: &str = env!("CARGO_BIN_EXE_silentsilo-browser-host");

fn u16_at(b: &[u8], at: usize) -> usize {
    u16::from_le_bytes([b[at], b[at + 1]]) as usize
}

fn u32_at(b: &[u8], at: usize) -> usize {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]) as usize
}

/// The names of the DLLs a PE file imports.
fn imported_dlls(b: &[u8]) -> Vec<String> {
    assert_eq!(&b[..2], b"MZ");
    let pe = u32_at(b, 0x3c);
    assert_eq!(&b[pe..pe + 4], b"PE\0\0");
    let coff = pe + 4;
    let sections = u16_at(b, coff + 2);
    let optional = coff + 20;
    let optional_size = u16_at(b, coff + 16);
    // PE32+ keeps its data directories 16 bytes further than PE32.
    let directories = match u16_at(b, optional) {
        0x20b => optional + 112,
        0x10b => optional + 96,
        magic => panic!("not a PE optional header: {magic:#x}"),
    };
    let table = optional + optional_size;
    let offset = |rva: usize| -> usize {
        (0..sections)
            .map(|i| table + i * 40)
            .find_map(|s| {
                let (size, at, raw) = (u32_at(b, s + 8), u32_at(b, s + 12), u32_at(b, s + 20));
                (at..at + size).contains(&rva).then(|| rva - at + raw)
            })
            .expect("an RVA inside a section")
    };
    let name_at = |rva: usize| {
        let start = offset(rva);
        let end = start + b[start..].iter().position(|&c| c == 0).unwrap();
        String::from_utf8_lossy(&b[start..end]).to_lowercase()
    };

    let mut names = Vec::new();
    // Directory 1, imports: 20-byte descriptors, the name's RVA at 12.
    // Directory 13, delay imports: 32-byte descriptors, the name's RVA at 4.
    for (index, stride, name_field) in [(1, 20, 12), (13, 32, 4)] {
        let rva = u32_at(b, directories + index * 8);
        if rva == 0 {
            continue;
        }
        let mut at = offset(rva);
        while b[at..at + stride].iter().any(|&c| c != 0) {
            names.push(name_at(u32_at(b, at + name_field)));
            at += stride;
        }
    }
    names
}

#[test]
fn the_host_needs_no_visual_cpp_runtime() {
    let names = imported_dlls(&std::fs::read(HOST).unwrap());
    assert!(
        names.iter().any(|n| n == "kernel32.dll"),
        "the import table was not read: {names:?}"
    );
    let runtime: Vec<&String> = names
        .iter()
        .filter(|n| n.starts_with("vcruntime") || n.starts_with("msvcp"))
        .collect();
    assert!(
        runtime.is_empty(),
        "{HOST} imports {runtime:?}, which a clean Windows does not have"
    );
}
