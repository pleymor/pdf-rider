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
        hkcu.open_subkey(r"Software\Classes\com.pdfreader.app\shell\open\command")
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
            .create_subkey("com.pdfreader.app")
            .map_err(|e| e.to_string())?;
        progid
            .set_value("", &"PDF Reader Document")
            .map_err(|e| e.to_string())?;
        progid
            .set_value("FriendlyTypeName", &"PDF Reader Document")
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

        // .pdf → com.pdfreader.app
        let (pdf_ext, _) = classes
            .create_subkey(".pdf")
            .map_err(|e| e.to_string())?;
        pdf_ext
            .set_value("", &"com.pdfreader.app")
            .map_err(|e| e.to_string())?;

        // Also register print verb under Applications\pdf-reader.exe (used when
        // Windows sets the default via Settings → Default Apps, which ignores our
        // ProgID and uses Applications\<exe-name> instead).
        let exe_name = std::path::Path::new(&exe)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "pdf-reader.exe".to_string());
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

/// Renders PDF pages (JPEG bytes, base64-encoded) to the default Windows printer
/// without showing any dialog.
#[tauri::command]
pub fn print_pages(pages_b64: Vec<String>) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        print_pages_impl(pages_b64)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = pages_b64;
        Err("Silent printing is only supported on Windows".to_string())
    }
}

#[cfg(target_os = "windows")]
fn print_pages_impl(pages_b64: Vec<String>) -> Result<(), String> {
    use base64::Engine;
    use std::ptr;
    use winapi::shared::windef::HDC;
    use winapi::um::wingdi::{
        CreateDCW, DeleteDC, GetDeviceCaps, StretchDIBits, BITMAPINFO,
        BITMAPINFOHEADER, DIB_RGB_COLORS, DOCINFOW, HORZRES, SRCCOPY, VERTRES,
    };
    use winapi::um::winspool::GetDefaultPrinterW;

    // Get default printer name
    let mut size = 256u32;
    let mut name_buf = vec![0u16; size as usize];
    let ok = unsafe { GetDefaultPrinterW(name_buf.as_mut_ptr(), &mut size) };
    if ok == 0 {
        return Err("No default printer found".to_string());
    }

    // Create a DC for the default printer
    let hdc: HDC = unsafe {
        CreateDCW(
            ptr::null(),
            name_buf.as_ptr(),
            ptr::null(),
            ptr::null(),
        )
    };
    if hdc.is_null() {
        return Err("Failed to create printer DC".to_string());
    }

    let page_w = unsafe { GetDeviceCaps(hdc, HORZRES) };
    let page_h = unsafe { GetDeviceCaps(hdc, VERTRES) };

    // Start print job
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

        unsafe { winapi::um::wingdi::StartPage(hdc) };
        unsafe {
            StretchDIBits(
                hdc,
                0, 0, page_w, page_h,
                0, 0, img_w as i32, img_h as i32,
                dib.as_ptr() as *const _,
                &bmi,
                DIB_RGB_COLORS,
                SRCCOPY,
            )
        };
        unsafe { winapi::um::wingdi::EndPage(hdc) };
    }

    unsafe { winapi::um::wingdi::EndDoc(hdc) };
    unsafe { DeleteDC(hdc) };

    Ok(())
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
        hkcu.delete_subkey_all(r"Software\Classes\com.pdfreader.app")
            .ok();

        // Remove .pdf extension mapping
        hkcu.delete_subkey_all(r"Software\Classes\.pdf").ok();

        // Remove Applications\pdf-reader.exe
        let exe = std::env::current_exe()
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .to_string();
        let exe_name = std::path::Path::new(&exe)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "pdf-reader.exe".to_string());
        hkcu.delete_subkey_all(format!(r"Software\Classes\Applications\{exe_name}"))
            .ok();

        // Remove from FileExts OpenWithProgids
        if let Ok(key) = hkcu.open_subkey_with_flags(
            r"Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\.pdf\OpenWithProgids",
            winreg::enums::KEY_SET_VALUE,
        ) {
            key.delete_value("com.pdfreader.app").ok();
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
            .unwrap_or_else(|| "pdf-reader.exe".to_string());

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);

        // Under our ProgID
        let (progid_print, _) = hkcu
            .create_subkey(format!(
                r"Software\Classes\com.pdfreader.app\shell\print\command"
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
