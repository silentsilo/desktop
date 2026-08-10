//! What this machine calls itself, for the Devices list. Read from the OS
//! rather than asked of the user; a rename in the app wins. Both values
//! enter the operation log, which is encrypted before it leaves the
//! machine, and a computer name often carries a person's name, so this
//! reads only what the machine already advertises on its own network: no
//! serial numbers, no user account, no hardware ids.

/// The machine's name, as close to what the user sees in system settings as
/// can be had without asking the OS for anything privileged.
pub fn system_name() -> Option<String> {
    #[cfg(windows)]
    {
        // Set on every Windows session, and it is the name shown under
        // "Device name" in Settings.
        if let Ok(name) = std::env::var("COMPUTERNAME") {
            return non_empty(name);
        }
    }

    #[cfg(not(windows))]
    {
        // `HOSTNAME` is a shell variable rather than an exported one on most
        // systems, so the file is the reliable half of this pair.
        if let Ok(name) = std::fs::read_to_string("/etc/hostname") {
            if let Some(name) = non_empty(name.trim().to_string()) {
                return Some(name);
            }
        }
        if let Ok(name) = std::env::var("HOSTNAME") {
            return non_empty(name);
        }
    }

    None
}

/// A short description of the system: "Windows 11 Pro", "Linux", "macOS".
pub fn platform() -> String {
    #[cfg(windows)]
    {
        windows_product_name().unwrap_or_else(|| "Windows".to_string())
    }

    #[cfg(target_os = "macos")]
    {
        "macOS".to_string()
    }

    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        // Distribution rather than "Linux" where the file says so: telling a
        // Fedora box from a Debian one is most of the point of this line.
        std::fs::read_to_string("/etc/os-release")
            .ok()
            .and_then(|release| {
                release
                    .lines()
                    .find_map(|line| line.strip_prefix("PRETTY_NAME=").map(str::to_string))
            })
            .map(|value| value.trim_matches('"').trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "Linux".to_string())
    }
}

#[cfg(windows)]
fn windows_product_name() -> Option<String> {
    use winreg::RegKey;
    use winreg::enums::HKEY_LOCAL_MACHINE;

    let key = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")
        .ok()?;
    // The registry still says "Windows 10" on 11; the build number is what
    // tells them apart, and 22000 is where 11 starts.
    let product: String = key.get_value("ProductName").ok()?;
    let build: String = key.get_value("CurrentBuildNumber").unwrap_or_default();
    let product = match build.parse::<u32>() {
        Ok(build) if build >= 22000 => product.replacen("Windows 10", "Windows 11", 1),
        _ => product,
    };
    non_empty(product)
}

fn non_empty(value: String) -> Option<String> {
    let value = value.trim().to_string();
    if value.is_empty() { None } else { Some(value) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_platform_is_never_blank() {
        assert!(!platform().trim().is_empty());
    }

    #[test]
    fn a_name_that_is_only_whitespace_is_no_name() {
        assert_eq!(non_empty("   ".to_string()), None);
        assert_eq!(
            non_empty(" DESK-1 ".to_string()),
            Some("DESK-1".to_string())
        );
    }
}
