//! Reads a QR code out of a picture, for the one-time code of a login: the
//! two-factor QR a site shows, pasted as a screenshot or picked as a file.
//! The webview decodes the picture (any format it can show) and sends its
//! grey pixels as the raw body, with the size in two headers. Nothing is
//! written anywhere and nothing is logged: the text inside is a secret.

use tauri::ipc::{InvokeBody, Request};

/// Bigger than any screen; the webview scales a larger picture down first.
const MAX_SIDE: usize = 4096;

fn header(request: &Request<'_>, name: &str) -> Result<usize, String> {
    request
        .headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| format!("Missing {name}."))
}

/// Every QR code found, in the order the detector meets them. Ones that do
/// not decode are skipped.
fn read_codes(width: usize, height: usize, grey: &[u8]) -> Vec<String> {
    let mut img =
        rqrr::PreparedImage::prepare_from_greyscale(width, height, |x, y| grey[y * width + x]);
    img.detect_grids()
        .into_iter()
        .filter_map(|grid| grid.decode().ok().map(|(_, text)| text))
        .collect()
}

/// The text of the QR code in the picture, the `otpauth://` one first when
/// there are several, or `None` when no code could be read.
#[tauri::command]
pub fn qr_from_pixels(request: Request<'_>) -> Result<Option<String>, String> {
    let InvokeBody::Raw(grey) = request.body() else {
        return Err("Expected the picture's pixels.".into());
    };
    let width = header(&request, "x-width")?;
    let height = header(&request, "x-height")?;
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return Err("The picture is empty or too large.".into());
    }
    if grey.len() != width * height {
        return Err("The picture's size does not match its pixels.".into());
    }
    Ok(pick(read_codes(width, height, grey)))
}

/// The `otpauth://` code when there is one, else the first.
fn pick(codes: Vec<String>) -> Option<String> {
    let otp = codes.iter().position(|c| {
        c.trim_start()
            .to_ascii_lowercase()
            .starts_with("otpauth://")
    });
    codes.into_iter().nth(otp.unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use qrcode::{Color, QrCode};

    const OTP: &str =
        "otpauth://totp/Example:you@example.com?secret=JBSWY3DPEHPK3PXP&issuer=Example";

    /// `text` as a QR code on a white page `stride` pixels wide, `scale`
    /// pixels a module, its top left corner at `(left, top)`.
    fn draw_on(grey: &mut [u8], stride: usize, text: &str, scale: usize, left: usize, top: usize) {
        let code = QrCode::new(text.as_bytes()).unwrap();
        let modules = code.width();
        for (i, color) in code.to_colors().into_iter().enumerate() {
            if color != Color::Dark {
                continue;
            }
            let (row, col) = (i / modules, i % modules);
            for dy in 0..scale {
                for dx in 0..scale {
                    grey[(top + row * scale + dy) * stride + left + col * scale + dx] = 0;
                }
            }
        }
    }

    #[test]
    fn a_drawn_code_reads_back() {
        let side = 400;
        let mut grey = vec![255u8; side * side];
        draw_on(&mut grey, side, OTP, 6, 40, 40);
        assert_eq!(read_codes(side, side, &grey), vec![OTP.to_string()]);
    }

    #[test]
    fn a_blank_picture_has_no_code() {
        let grey = vec![255u8; 64 * 64];
        assert!(read_codes(64, 64, &grey).is_empty());
    }

    #[test]
    fn noise_is_not_a_code() {
        let grey: Vec<u8> = (0..128 * 128).map(|i| ((i * 7919) % 251) as u8).collect();
        assert!(read_codes(128, 128, &grey).is_empty());
    }

    #[test]
    fn the_otpauth_code_wins_over_another_beside_it() {
        let (w, h) = (900, 400);
        let mut grey = vec![255u8; w * h];
        // Two codes side by side, the other one first from the left.
        draw_on(&mut grey, w, "https://example.com/help", 6, 30, 30);
        draw_on(&mut grey, w, OTP, 6, 480, 30);
        assert_eq!(pick(read_codes(w, h, &grey)).as_deref(), Some(OTP));
    }
}
