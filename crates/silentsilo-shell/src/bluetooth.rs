//! Whether a phone can be reached for the QR-code flow. Windows checks
//! that the phone is nearby over Bluetooth LE before it talks to it, and a
//! computer without a radio, or with it off, fails inside Windows' own
//! dialog without saying why. So the app asks first and says it plainly.

/// What the radio is doing, as far as this computer can tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BluetoothState {
    On,
    /// There is a radio and it is switched off (or in airplane mode).
    Off,
    /// No Bluetooth LE radio at all: many desktops.
    Absent,
    /// Could not ask. Treated as "go ahead": Windows says the rest.
    Unknown,
}

pub fn state() -> BluetoothState {
    platform::state()
}

#[cfg(windows)]
mod platform {
    use super::BluetoothState;
    use windows::Devices::Bluetooth::BluetoothAdapter;
    use windows::Devices::Radios::RadioState;
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};

    pub fn state() -> BluetoothState {
        // WinRT needs COM on this thread. Already initialised, in either
        // model, is fine: the calls below work in both.
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        // No default adapter comes back as an error (a null result).
        let Ok(adapter) = BluetoothAdapter::GetDefaultAsync().and_then(|op| op.get()) else {
            return BluetoothState::Absent;
        };
        match adapter.IsLowEnergySupported() {
            Ok(false) => return BluetoothState::Absent,
            Ok(true) => {}
            Err(_) => return BluetoothState::Unknown,
        }
        match adapter
            .GetRadioAsync()
            .and_then(|op| op.get())
            .and_then(|radio| radio.State())
        {
            Ok(RadioState::On) => BluetoothState::On,
            Ok(RadioState::Off) | Ok(RadioState::Disabled) => BluetoothState::Off,
            _ => BluetoothState::Unknown,
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use super::BluetoothState;

    /// No phone flow off Windows, so nothing asks.
    pub fn state() -> BluetoothState {
        BluetoothState::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_states_reach_the_window_by_these_names() {
        let names: Vec<String> = [
            BluetoothState::On,
            BluetoothState::Off,
            BluetoothState::Absent,
            BluetoothState::Unknown,
        ]
        .iter()
        .map(|s| serde_json::to_string(s).unwrap())
        .collect();
        assert_eq!(
            names,
            [r#""on""#, r#""off""#, r#""absent""#, r#""unknown""#]
        );
    }

    #[test]
    fn asking_this_machine_answers_without_failing() {
        // Whatever the radio does here, the call must come back.
        let _ = state();
    }
}
