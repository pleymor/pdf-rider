use std::process::Command;
use tauri::State;

use crate::AppState;

#[tauri::command]
pub fn get_startup_args(state: State<AppState>) -> crate::StartupArgs {
    state.startup_args.lock().unwrap().clone()
}

#[tauri::command]
pub fn check_pdf_association() -> bool {
    #[cfg(target_os = "windows")]
    {
        use winreg::enums::HKEY_CURRENT_USER;
        use winreg::RegKey;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        hkcu.open_subkey(r"Software\Classes\com.pdfrider.app\shell\open\command")
            .is_ok()
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

#[tauri::command]
pub fn register_pdf_handler() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};
        use winreg::RegKey;

        let exe = std::env::current_exe()
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .to_string();

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let classes = hkcu
            .open_subkey_with_flags(r"Software\Classes", KEY_SET_VALUE)
            .map_err(|e| e.to_string())?;

        // ProgID root
        let (progid, _) = classes
            .create_subkey("com.pdfrider.app")
            .map_err(|e| e.to_string())?;
        progid
            .set_value("", &"PDF Rider Document")
            .map_err(|e| e.to_string())?;
        progid
            .set_value("FriendlyTypeName", &"PDF Rider Document")
            .map_err(|e| e.to_string())?;

        // DefaultIcon
        let (icon_key, _) = progid
            .create_subkey("DefaultIcon")
            .map_err(|e| e.to_string())?;
        icon_key
            .set_value("", &format!("\"{exe}\",0"))
            .map_err(|e| e.to_string())?;

        // shell\open\command
        let (open_cmd, _) = progid
            .create_subkey(r"shell\open\command")
            .map_err(|e| e.to_string())?;
        open_cmd
            .set_value("", &format!("\"{exe}\" \"%1\""))
            .map_err(|e| e.to_string())?;

        // shell\print\command
        let (print_cmd, _) = progid
            .create_subkey(r"shell\print\command")
            .map_err(|e| e.to_string())?;
        print_cmd
            .set_value("", &format!("\"{exe}\" --print \"%1\""))
            .map_err(|e| e.to_string())?;

        // .pdf → com.pdfrider.app
        let (pdf_ext, _) = classes
            .create_subkey(".pdf")
            .map_err(|e| e.to_string())?;
        pdf_ext
            .set_value("", &"com.pdfrider.app")
            .map_err(|e| e.to_string())?;

        // Also register print verb under Applications\pdf-rider.exe (used when
        // Windows sets the default via Settings → Default Apps, which ignores our
        // ProgID and uses Applications\<exe-name> instead).
        let exe_name = std::path::Path::new(&exe)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "pdf-rider.exe".to_string());
        let app_key_path = format!(r"Software\Classes\Applications\{exe_name}");
        let (app_key, _) = hkcu
            .create_subkey(&app_key_path)
            .map_err(|e| e.to_string())?;
        // Hide from "Open with" — this key is only needed for the print verb
        app_key
            .set_value("NoOpenWith", &"")
            .map_err(|e| e.to_string())?;
        let (app_print_cmd, _) = app_key
            .create_subkey(r"shell\print\command")
            .map_err(|e| e.to_string())?;
        app_print_cmd
            .set_value("", &format!("\"{exe}\" --print \"%1\""))
            .map_err(|e| e.to_string())?;

        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("Registration is only supported on Windows".to_string())
    }
}

// ── Printer helpers ──────────────────────────────────────────────────────────

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrinterList {
    pub printers: Vec<String>,
    pub default_printer: String,
}

#[tauri::command]
pub fn list_printers() -> Result<PrinterList, String> {
    #[cfg(target_os = "windows")]
    {
        list_printers_impl()
    }
    #[cfg(not(target_os = "windows"))]
    {
        Ok(PrinterList { printers: vec![], default_printer: String::new() })
    }
}

#[cfg(target_os = "windows")]
fn get_default_printer_name() -> Result<String, String> {
    use winapi::um::winspool::GetDefaultPrinterW;

    let mut size = 256u32;
    let mut name_buf = vec![0u16; size as usize];
    let ok = unsafe { GetDefaultPrinterW(name_buf.as_mut_ptr(), &mut size) };
    if ok == 0 {
        return Err("No default printer found".to_string());
    }
    Ok(String::from_utf16_lossy(&name_buf[..size as usize]).trim_end_matches('\0').to_string())
}

#[cfg(target_os = "windows")]
fn list_printers_impl() -> Result<PrinterList, String> {
    use std::ptr;
    use winapi::um::winspool::EnumPrintersW;

    const PRINTER_ENUM_LOCAL: u32 = 0x2;
    const PRINTER_ENUM_CONNECTIONS: u32 = 0x4;
    let flags = PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS;

    // First call: get required buffer size
    let mut needed: u32 = 0;
    let mut returned: u32 = 0;
    unsafe {
        EnumPrintersW(
            flags,
            ptr::null_mut(),
            2, // PRINTER_INFO_2
            ptr::null_mut(),
            0,
            &mut needed,
            &mut returned,
        );
    }

    if needed == 0 {
        let default = get_default_printer_name().unwrap_or_default();
        return Ok(PrinterList { printers: vec![], default_printer: default });
    }

    // Second call: fill buffer
    let mut buf = vec![0u8; needed as usize];
    let ok = unsafe {
        EnumPrintersW(
            flags,
            ptr::null_mut(),
            2,
            buf.as_mut_ptr(),
            needed,
            &mut needed,
            &mut returned,
        )
    };
    if ok == 0 {
        let default = get_default_printer_name().unwrap_or_default();
        return Ok(PrinterList { printers: vec![], default_printer: default });
    }

    // Parse PRINTER_INFO_2W structs
    #[repr(C)]
    #[allow(non_snake_case)]
    struct PrinterInfo2W {
        pServerName: *mut u16,
        pPrinterName: *mut u16,
        pShareName: *mut u16,
        pPortName: *mut u16,
        pDriverName: *mut u16,
        pComment: *mut u16,
        pLocation: *mut u16,
        pDevMode: *mut u8,
        pSepFile: *mut u16,
        pPrintProcessor: *mut u16,
        pDatatype: *mut u16,
        pParameters: *mut u16,
        pSecurityDescriptor: *mut u8,
        Attributes: u32,
        Priority: u32,
        DefaultPriority: u32,
        StartTime: u32,
        UntilTime: u32,
        Status: u32,
        cJobs: u32,
        AveragePPM: u32,
    }

    let info_ptr = buf.as_ptr() as *const PrinterInfo2W;
    let mut printers = Vec::with_capacity(returned as usize);
    for i in 0..returned as isize {
        let info = unsafe { &*info_ptr.offset(i) };
        if !info.pPrinterName.is_null() {
            let name_len = unsafe {
                let mut len = 0;
                while *info.pPrinterName.add(len) != 0 { len += 1; }
                len
            };
            let name_slice = unsafe { std::slice::from_raw_parts(info.pPrinterName, name_len) };
            printers.push(String::from_utf16_lossy(name_slice));
        }
    }

    let default = get_default_printer_name().unwrap_or_default();
    Ok(PrinterList { printers, default_printer: default })
}

// ── Print pages ──────────────────────────────────────────────────────────────

#[tauri::command]
pub fn print_pages(
    pages_b64: Vec<String>,
    printer_name: Option<String>,
    copies: Option<u32>,
    orientation: Option<String>,
    fit_mode: Option<String>,
) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        print_pages_impl(pages_b64, printer_name, copies, orientation, fit_mode)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (pages_b64, printer_name, copies, orientation, fit_mode);
        Err("Printing is only supported on Windows".to_string())
    }
}

#[cfg(target_os = "windows")]
fn print_pages_impl(
    pages_b64: Vec<String>,
    printer_name: Option<String>,
    copies: Option<u32>,
    orientation: Option<String>,
    fit_mode: Option<String>,
) -> Result<(), String> {
    use base64::Engine;
    use std::ptr;
    use winapi::shared::windef::HDC;
    use winapi::um::wingdi::{
        CreateDCW, DeleteDC, GetDeviceCaps, StretchDIBits, BITMAPINFO,
        BITMAPINFOHEADER, DIB_RGB_COLORS, DOCINFOW, HORZRES, LOGPIXELSX,
        LOGPIXELSY, SRCCOPY, VERTRES,
    };

    // Resolve printer name
    let printer_wide: Vec<u16> = match &printer_name {
        Some(name) => {
            let mut v: Vec<u16> = name.encode_utf16().collect();
            v.push(0);
            v
        }
        None => {
            let name = get_default_printer_name()?;
            let mut v: Vec<u16> = name.encode_utf16().collect();
            v.push(0);
            v
        }
    };

    // Ask the driver for its full DEVMODE (public part + driver-private data)
    // and set the orientation on it. A hand-built, zeroed DEVMODE is rejected
    // by some drivers, so fall back to the driver defaults if this fails.
    let devmode_buf = orientation
        .as_deref()
        .and_then(|o| printer_devmode(&printer_wide, o == "landscape"));
    let devmode_ptr = devmode_buf
        .as_ref()
        .map_or(ptr::null(), |b| b.as_ptr() as *const winapi::um::wingdi::DEVMODEW);

    let hdc: HDC = unsafe {
        CreateDCW(
            ptr::null(),
            printer_wide.as_ptr(),
            ptr::null(),
            devmode_ptr,
        )
    };

    if hdc.is_null() {
        return Err("Failed to create printer DC".to_string());
    }

    let page_w = unsafe { GetDeviceCaps(hdc, HORZRES) };
    let page_h = unsafe { GetDeviceCaps(hdc, VERTRES) };
    let dpi_x = unsafe { GetDeviceCaps(hdc, LOGPIXELSX) };
    let dpi_y = unsafe { GetDeviceCaps(hdc, LOGPIXELSY) };
    let actual_size = fit_mode.as_deref() == Some("actual");

    let doc_name: Vec<u16> = "PDF Document\0".encode_utf16().collect();
    let doc_info = DOCINFOW {
        cbSize: std::mem::size_of::<DOCINFOW>() as i32,
        lpszDocName: doc_name.as_ptr(),
        lpszOutput: ptr::null(),
        lpszDatatype: ptr::null(),
        fwType: 0,
    };
    let job = unsafe { winapi::um::wingdi::StartDocW(hdc, &doc_info) };
    if job <= 0 {
        unsafe { DeleteDC(hdc) };
        return Err("StartDocW failed".to_string());
    }

    let num_copies = copies.unwrap_or(1).max(1);

    for _ in 0..num_copies {
        for page_b64 in &pages_b64 {
            let jpeg_bytes = base64::engine::general_purpose::STANDARD
                .decode(page_b64.trim())
                .map_err(|e| format!("base64: {e}"))?;

            let img = image::load_from_memory(&jpeg_bytes)
                .map_err(|e| format!("image decode: {e}"))?
                .into_rgb8();
            let (img_w, img_h) = img.dimensions();
            let rgb = img.as_raw();

            // Convert RGB → BGR with 4-byte row alignment (required by GDI)
            let row_stride = (img_w as usize * 3 + 3) & !3;
            let mut dib = vec![0u8; row_stride * img_h as usize];
            for y in 0..img_h as usize {
                for x in 0..img_w as usize {
                    let s = y * img_w as usize * 3 + x * 3;
                    let d = y * row_stride + x * 3;
                    dib[d] = rgb[s + 2]; // B
                    dib[d + 1] = rgb[s + 1]; // G
                    dib[d + 2] = rgb[s]; // R
                }
            }

            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: img_w as i32,
                    biHeight: -(img_h as i32), // top-down
                    biPlanes: 1,
                    biBitCount: 24,
                    biCompression: 0, // BI_RGB
                    biSizeImage: 0,
                    biXPelsPerMeter: 0,
                    biYPelsPerMeter: 0,
                    biClrUsed: 0,
                    biClrImportant: 0,
                },
                bmiColors: [winapi::um::wingdi::RGBQUAD {
                    rgbBlue: 0,
                    rgbGreen: 0,
                    rgbRed: 0,
                    rgbReserved: 0,
                }],
            };

            // Compute destination rectangle
            let (dest_x, dest_y, dest_w, dest_h) = if actual_size {
                // Map image pixels to printer units at native DPI
                let w = (img_w as i64 * dpi_x as i64 / 200) as i32; // 200 = render DPI
                let h = (img_h as i64 * dpi_y as i64 / 200) as i32;
                let x = (page_w - w) / 2;
                let y = (page_h - h) / 2;
                (x.max(0), y.max(0), w.min(page_w), h.min(page_h))
            } else {
                // Scale to fit the printable area, keeping the aspect ratio
                let (w, h) = fit_within(img_w, img_h, page_w, page_h);
                ((page_w - w) / 2, (page_h - h) / 2, w, h)
            };

            unsafe { winapi::um::wingdi::StartPage(hdc) };
            unsafe {
                StretchDIBits(
                    hdc,
                    dest_x, dest_y, dest_w, dest_h,
                    0, 0, img_w as i32, img_h as i32,
                    dib.as_ptr() as *const _,
                    &bmi,
                    DIB_RGB_COLORS,
                    SRCCOPY,
                )
            };
            unsafe { winapi::um::wingdi::EndPage(hdc) };
        }
    }

    unsafe { winapi::um::wingdi::EndDoc(hdc) };
    unsafe { DeleteDC(hdc) };

    Ok(())
}

/// Largest `(w, h)` with the image's aspect ratio that fits in `max_w × max_h`.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn fit_within(img_w: u32, img_h: u32, max_w: i32, max_h: i32) -> (i32, i32) {
    if img_w == 0 || img_h == 0 || max_w <= 0 || max_h <= 0 {
        return (max_w.max(0), max_h.max(0));
    }
    // Compare max_w / img_w against max_h / img_h without floating point
    if max_w as i64 * img_h as i64 <= max_h as i64 * img_w as i64 {
        (max_w, (img_h as i64 * max_w as i64 / img_w as i64) as i32)
    } else {
        ((img_w as i64 * max_h as i64 / img_h as i64) as i32, max_h)
    }
}

/// Returns the printer driver's DEVMODE with the requested orientation set,
/// as a raw buffer sized by the driver. `None` if the driver can't supply one.
#[cfg(target_os = "windows")]
fn printer_devmode(printer_wide: &[u16], landscape: bool) -> Option<Vec<u8>> {
    use std::ptr;
    use winapi::um::wingdi::{DEVMODEW, DMORIENT_LANDSCAPE, DMORIENT_PORTRAIT, DM_ORIENTATION};
    use winapi::um::winspool::{ClosePrinter, DocumentPropertiesW, OpenPrinterW};

    const DM_OUT_BUFFER: u32 = 2;
    const DM_IN_BUFFER: u32 = 8;

    let mut name = printer_wide.to_vec();
    let mut handle = ptr::null_mut();
    if unsafe { OpenPrinterW(name.as_mut_ptr(), &mut handle, ptr::null_mut()) } == 0 {
        return None;
    }

    let result = (|| {
        let size = unsafe {
            DocumentPropertiesW(ptr::null_mut(), handle, name.as_mut_ptr(), ptr::null_mut(), ptr::null_mut(), 0)
        };
        if size < std::mem::size_of::<DEVMODEW>() as i32 {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        let dm = buf.as_mut_ptr() as *mut DEVMODEW;
        let ok = unsafe {
            DocumentPropertiesW(ptr::null_mut(), handle, name.as_mut_ptr(), dm, ptr::null_mut(), DM_OUT_BUFFER)
        };
        if ok < 0 {
            return None;
        }
        unsafe {
            let dm = &mut *dm;
            dm.dmFields |= DM_ORIENTATION;
            dm.u1.s1_mut().dmOrientation =
                if landscape { DMORIENT_LANDSCAPE } else { DMORIENT_PORTRAIT } as i16;
        }
        // Let the driver validate and merge the change
        let ok = unsafe {
            DocumentPropertiesW(ptr::null_mut(), handle, name.as_mut_ptr(), dm, dm, DM_IN_BUFFER | DM_OUT_BUFFER)
        };
        if ok < 0 {
            return None;
        }
        Some(buf)
    })();

    unsafe { ClosePrinter(handle) };
    result
}

#[cfg(test)]
mod tests {
    use super::fit_within;

    #[test]
    fn fit_within_keeps_aspect_ratio() {
        // Portrait A4-ish image on a portrait page: height-bound
        assert_eq!(fit_within(1000, 1414, 4800, 6600), (4667, 6600));
        // Landscape image on a portrait page: width-bound, letterboxed
        assert_eq!(fit_within(1414, 1000, 4800, 6600), (4800, 3394));
        // Exact match
        assert_eq!(fit_within(100, 200, 50, 100), (50, 100));
    }

    #[test]
    fn fit_within_degenerate_inputs() {
        assert_eq!(fit_within(0, 100, 50, 100), (50, 100));
        assert_eq!(fit_within(100, 100, 0, 100), (0, 100));
    }
}

/// Removes all registry entries created by register_pdf_handler.
#[tauri::command]
pub fn unregister_pdf_handler() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use winreg::enums::HKEY_CURRENT_USER;
        use winreg::RegKey;

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);

        // Remove ProgID
        hkcu.delete_subkey_all(r"Software\Classes\com.pdfrider.app")
            .ok();

        // Remove .pdf extension mapping
        hkcu.delete_subkey_all(r"Software\Classes\.pdf").ok();

        // Remove Applications\pdf-rider.exe
        let exe = std::env::current_exe()
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .to_string();
        let exe_name = std::path::Path::new(&exe)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "pdf-rider.exe".to_string());
        hkcu.delete_subkey_all(format!(r"Software\Classes\Applications\{exe_name}"))
            .ok();

        // Remove from FileExts OpenWithProgids
        if let Ok(key) = hkcu.open_subkey_with_flags(
            r"Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\.pdf\OpenWithProgids",
            winreg::enums::KEY_SET_VALUE,
        ) {
            key.delete_value("com.pdfrider.app").ok();
        }

        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("Only supported on Windows".to_string())
    }
}

/// Registers only the print verb (silent, no prompt needed).
#[tauri::command]
pub fn register_print_verb() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use winreg::enums::HKEY_CURRENT_USER;
        use winreg::RegKey;

        let exe = std::env::current_exe()
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .to_string();

        let exe_name = std::path::Path::new(&exe)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "pdf-rider.exe".to_string());

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);

        // Under our ProgID
        let (progid_print, _) = hkcu
            .create_subkey(format!(
                r"Software\Classes\com.pdfrider.app\shell\print\command"
            ))
            .map_err(|e| e.to_string())?;
        progid_print
            .set_value("", &format!("\"{exe}\" --print \"%1\""))
            .map_err(|e| e.to_string())?;

        // Under Applications\<exe> (used when default set via Windows Settings).
        // NoOpenWith prevents this key from appearing in "Open with" — it only needs the print verb.
        let (app_key, _) = hkcu
            .create_subkey(format!(r"Software\Classes\Applications\{exe_name}"))
            .map_err(|e| e.to_string())?;
        app_key
            .set_value("NoOpenWith", &"")
            .map_err(|e| e.to_string())?;
        let (app_print, _) = app_key
            .create_subkey(r"shell\print\command")
            .map_err(|e| e.to_string())?;
        app_print
            .set_value("", &format!("\"{exe}\" --print \"%1\""))
            .map_err(|e| e.to_string())?;

        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Ok(())
    }
}

#[tauri::command]
pub fn open_url(url: String) -> Result<(), String> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("Only http/https URLs are supported".into());
    }
    // Use explorer.exe directly — avoids cmd.exe shell interpretation
    Command::new("explorer.exe")
        .arg(&url)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn open_default_apps_settings() -> Result<(), String> {
    Command::new("cmd")
        .args(["/C", "start", "", "ms-settings:defaultapps"])
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}
