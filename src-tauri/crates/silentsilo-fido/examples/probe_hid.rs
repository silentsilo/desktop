use ctap_hid_fido2::HidParam;
use ctap_hid_fido2::fidokey::FidoKeyHid;
use ctap_hid_fido2::{Cfg, FidoKeyHidFactory, get_fidokey_devices, get_hid_devices};

fn yubikey_fido_path_candidates() -> Vec<String> {
    let mut paths = Vec::new();
    for d in get_hid_devices().into_iter().filter(|d| d.vid == 0x1050) {
        if let HidParam::Path(path) = &d.param {
            // Windows often lists only MI_00 (keyboard); FIDO is typically MI_01 on the same key.
            if path.contains("MI_00") {
                let fido = path
                    .replace("MI_00", "MI_01")
                    .trim_end_matches("\\KBD")
                    .to_string();
                paths.push(fido);
            }
            if path.contains("MI_01") && !path.ends_with("\\KBD") {
                paths.push(path.clone());
            }
        }
    }
    paths.sort();
    paths.dedup();
    paths
}

fn main() {
    let status = silentsilo_fido::status();
    println!("status: {status:?}");
    println!("=== FIDO devices (usage_page 0xf1d0) ===");
    for d in get_fidokey_devices() {
        println!(
            "  vid={:04x} pid={:04x} {} | {}",
            d.vid, d.pid, d.product_string, d.info
        );
    }
    println!("=== Yubico HID ===");
    for d in get_hid_devices().into_iter().filter(|d| d.vid == 0x1050) {
        println!(
            "  vid={:04x} pid={:04x} {} | {}",
            d.vid, d.pid, d.product_string, d.info
        );
    }
    let cfg = Cfg::init();
    println!("=== Path candidates ===");
    let extra = [
        r"\\?\HID#VID_1050&PID_0407&MI_01#8&30C02E57&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}",
    ];
    let mut all_paths = yubikey_fido_path_candidates();
    all_paths.extend(extra.iter().map(|s| s.to_string()));
    for path in all_paths {
        println!("  try {path}");
        let params = [HidParam::Path(path.clone())];
        match FidoKeyHid::new(&params, &cfg) {
            Ok(dev) => match dev.get_info() {
                Ok(info) => println!("    OPEN OK: {:?}", info.versions),
                Err(e) => println!("    OPEN OK but get_info failed: {e}"),
            },
            Err(e) => println!("    OPEN FAIL: {e}"),
        }
    }
    match FidoKeyHidFactory::create(&cfg) {
        Ok(_) => println!("factory create: OK"),
        Err(e) => println!("factory create: ERR {e}"),
    }
}
