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
    #[cfg(target_os = "linux")]
    {
        // Considered registered when our .desktop file exists in the user's
        // application directory.
        if let Some(home) = std::env::var_os("HOME") {
            let desktop = std::path::PathBuf::from(home)
                .join(".local/share/applications/pdf-rider.desktop");
            return desktop.exists();
        }
        false
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
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
    #[cfg(target_os = "linux")]
    {
        register_pdf_handler_linux()
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        Err("Registration is only supported on Windows and Linux".to_string())
    }
}

#[cfg(target_os = "linux")]
fn register_pdf_handler_linux() -> Result<(), String> {
    use std::fs;

    let exe = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .to_string_lossy()
        .to_string();

    let home = std::env::var("HOME").map_err(|e| e.to_string())?;
    let apps_dir = std::path::PathBuf::from(&home).join(".local/share/applications");
    fs::create_dir_all(&apps_dir).map_err(|e| e.to_string())?;

    let desktop_path = apps_dir.join("pdf-rider.desktop");
    let desktop = format!(
        "[Desktop Entry]\n\
Type=Application\n\
Name=PDF Rider\n\
GenericName=PDF Reader\n\
Comment=View, annotate and sign PDF documents\n\
Exec={exe} %f\n\
Icon=pdf-rider\n\
Terminal=false\n\
Categories=Office;Viewer;\n\
MimeType=application/pdf;\n\
StartupWMClass=PDF Rider\n\
Actions=Print;\n\
\n\
[Desktop Action Print]\n\
Name=Print\n\
Exec={exe} --print %f\n",
    );
    fs::write(&desktop_path, desktop).map_err(|e| e.to_string())?;

    // Best-effort: refresh the desktop database and set default handler.
    let _ = std::process::Command::new("update-desktop-database")
        .arg(&apps_dir)
        .status();
    let _ = std::process::Command::new("xdg-mime")
        .args(["default", "pdf-rider.desktop", "application/pdf"])
        .status();

    Ok(())
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
    #[cfg(target_os = "linux")]
    {
        list_printers_linux()
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        Ok(PrinterList { printers: vec![], default_printer: String::new() })
    }
}

#[cfg(target_os = "linux")]
fn list_printers_linux() -> Result<PrinterList, String> {
    use std::process::Command;

    // `lpstat -a` lists accepting destinations: "<name> accepting requests since ..."
    let mut printers: Vec<String> = Vec::new();
    if let Ok(out) = Command::new("lpstat").arg("-a").output() {
        if out.status.success() {
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                if let Some(name) = line.split_whitespace().next() {
                    printers.push(name.to_string());
                }
            }
        }
    }

    // `lpstat -d` outputs either "system default destination: <name>" or
    // "no system default destination".
    let mut default_printer = String::new();
    if let Ok(out) = Command::new("lpstat").arg("-d").output() {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout);
            if let Some(idx) = s.find(':') {
                default_printer = s[idx + 1..].trim().to_string();
            }
        }
    }

    Ok(PrinterList { printers, default_printer })
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
    #[cfg(target_os = "linux")]
    {
        print_pages_linux(pages_b64, printer_name, copies, orientation, fit_mode)
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        let _ = (pages_b64, printer_name, copies, orientation, fit_mode);
        Err("Printing is only supported on Windows and Linux".to_string())
    }
}

#[cfg(target_os = "linux")]
fn print_pages_linux(
    pages_b64: Vec<String>,
    printer_name: Option<String>,
    copies: Option<u32>,
    orientation: Option<String>,
    fit_mode: Option<String>,
) -> Result<(), String> {
    use base64::Engine;
    use std::io::Write;
    use std::process::Command;

    let num_copies = copies.unwrap_or(1).max(1);
    let landscape = orientation.as_deref() == Some("landscape");
    let fit_to_page = fit_mode.as_deref() != Some("actual");

    // Write each JPEG page to a temp file, then submit via `lp`.
    let tmpdir = std::env::temp_dir();
    for (i, page_b64) in pages_b64.iter().enumerate() {
        let jpeg_bytes = base64::engine::general_purpose::STANDARD
            .decode(page_b64.trim())
            .map_err(|e| format!("base64: {e}"))?;

        let path = tmpdir.join(format!(
            "pdf-rider-print-{}-{}.jpg",
            std::process::id(),
            i
        ));
        {
            let mut f = std::fs::File::create(&path).map_err(|e| e.to_string())?;
            f.write_all(&jpeg_bytes).map_err(|e| e.to_string())?;
        }

        let mut cmd = Command::new("lp");
        if let Some(p) = printer_name.as_deref() {
            cmd.args(["-d", p]);
        }
        cmd.args(["-n", &num_copies.to_string()]);
        if landscape {
            cmd.args(["-o", "landscape"]);
        }
        if fit_to_page {
            cmd.args(["-o", "fit-to-page"]);
        }
        cmd.arg(&path);

        let out = cmd
            .output()
            .map_err(|e| format!("failed to invoke lp: {e}"))?;
        // Remove the temp file regardless of outcome.
        let _ = std::fs::remove_file(&path);
        if !out.status.success() {
            return Err(format!(
                "lp failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
    }
    Ok(())
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

    // Build DEVMODEW if orientation is specified
    let landscape = orientation.as_deref() == Some("landscape");
    let devmode_ptr = if landscape {
        use winapi::um::wingdi::DEVMODEW;
        let mut dm: DEVMODEW = unsafe { std::mem::zeroed() };
        dm.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
        dm.dmFields = 0x1; // DM_ORIENTATION
        unsafe { *dm.u1.s1_mut() }.dmOrientation = 2; // DMORIENT_LANDSCAPE
        Box::into_raw(Box::new(dm))
    } else {
        ptr::null_mut()
    };

    let hdc: HDC = unsafe {
        CreateDCW(
            ptr::null(),
            printer_wide.as_ptr(),
            ptr::null(),
            devmode_ptr as *const _,
        )
    };

    // Clean up devmode allocation
    if !devmode_ptr.is_null() {
        unsafe { drop(Box::from_raw(devmode_ptr)); }
    }

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
                (0, 0, page_w, page_h)
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
    #[cfg(target_os = "linux")]
    {
        let home = std::env::var("HOME").map_err(|e| e.to_string())?;
        let desktop = std::path::PathBuf::from(&home)
            .join(".local/share/applications/pdf-rider.desktop");
        if desktop.exists() {
            std::fs::remove_file(&desktop).map_err(|e| e.to_string())?;
        }
        let _ = std::process::Command::new("update-desktop-database")
            .arg(std::path::PathBuf::from(&home).join(".local/share/applications"))
            .status();
        Ok(())
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        Err("Only supported on Windows and Linux".to_string())
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
        // On Linux the print action is part of the .desktop file written by
        // register_pdf_handler, so there is nothing extra to do here.
        Ok(())
    }
}

/// Prints a PDF file to a named printer using the system's native PDF handler.
#[tauri::command]
pub fn print_pdf_file(file_path: String, printer_name: String, copies: Option<u32>) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        print_pdf_file_impl(file_path, printer_name, copies)
    }
    #[cfg(target_os = "linux")]
    {
        let num_copies = copies.unwrap_or(1).max(1);
        let out = std::process::Command::new("lp")
            .args(["-d", &printer_name, "-n", &num_copies.to_string()])
            .arg(&file_path)
            .output()
            .map_err(|e| format!("failed to invoke lp: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "lp failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Ok(())
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        let _ = (file_path, printer_name, copies);
        Err("Printing is only supported on Windows and Linux".to_string())
    }
}

#[cfg(target_os = "windows")]
fn print_pdf_file_impl(file_path: String, printer_name: String, copies: Option<u32>) -> Result<(), String> {
    use std::ptr;

    let verb: Vec<u16> = "printto\0".encode_utf16().collect();
    let file: Vec<u16> = format!("{file_path}\0").encode_utf16().collect();
    let printer: Vec<u16> = format!("{printer_name}\0").encode_utf16().collect();

    let num_copies = copies.unwrap_or(1).max(1);
    for _ in 0..num_copies {
        let result = unsafe {
            winapi::um::shellapi::ShellExecuteW(
                ptr::null_mut(),
                verb.as_ptr(),
                file.as_ptr(),
                printer.as_ptr(),
                ptr::null(),
                0, // SW_HIDE
            )
        };
        if (result as usize) <= 32 {
            return Err(format!("ShellExecuteW failed with code {}", result as usize));
        }
    }
    Ok(())
}

#[tauri::command]
pub fn open_url(url: String) -> Result<(), String> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("Only http/https URLs are supported".into());
    }
    #[cfg(target_os = "windows")]
    {
        // Use explorer.exe directly — avoids cmd.exe shell interpretation
        Command::new("explorer.exe")
            .arg(&url)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        Command::new("xdg-open")
            .arg(&url)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn open_default_apps_settings() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(["/C", "start", "", "ms-settings:defaultapps"])
            .spawn()
            .map_err(|e| e.to_string())?;
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    {
        // Try to open the desktop's default-apps panel; fall back to running
        // xdg-mime which at least surfaces the current PDF handler.
        let candidates: &[&[&str]] = &[
            &["gnome-control-center", "default-apps"],
            &["systemsettings5", "kcm_componentchooser"],
            &["systemsettings", "kcm_componentchooser"],
        ];
        for argv in candidates {
            if Command::new(argv[0]).args(&argv[1..]).spawn().is_ok() {
                return Ok(());
            }
        }
        Err("Could not open default-apps settings panel".to_string())
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        Err("Not supported on this platform".to_string())
    }
}
