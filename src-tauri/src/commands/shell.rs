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
        if is_packaged() {
            return false; // see register_pdf_handler
        }
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
        if is_packaged() {
            // MSIX: the association and its Print verb come from the package
            // manifest; HKCU\Software\Classes writes from inside a package go
            // to a private per-package hive that Explorer never reads.
            return Ok(());
        }
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

        // shell\print — Explorer's Print, also for a multiple selection
        write_print_verb(&progid, &exe).map_err(|e| e.to_string())?;

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
        write_print_verb(&app_key, &exe).map_err(|e| e.to_string())?;

        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("Registration is only supported on Windows".to_string())
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
        if is_packaged() {
            return Ok(()); // declared in AppxManifest.xml
        }
        write_print_verbs(true).map_err(|e| e.to_string())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Ok(())
    }
}

/// At startup: if this copy was registered as PDF handler before, point the
/// print verbs at the current executable again (the portable exe may have
/// moved) and upgrade registrations made by older versions, which lacked
/// MultiSelectModel and so offered Print for at most 15 selected files.
pub fn refresh_print_verb() {
    #[cfg(target_os = "windows")]
    {
        // Debug builds (`tauri dev`) must not take over a real install's verbs
        if !cfg!(debug_assertions) && !is_packaged() {
            let _ = write_print_verbs(false);
        }
    }
}

/// Command line Explorer runs for each file of a Print.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn print_command(exe: &str) -> String {
    format!("\"{exe}\" --print \"%1\"")
}

/// Writes `shell\print` under `class`. `MultiSelectModel=Player` makes Explorer
/// offer Print for any number of selected files (the default, "Document",
/// hides it above 15) and run the command once per file; those processes
/// then hand their file to a single print session (see print_queue.rs).
#[cfg(target_os = "windows")]
fn write_print_verb(class: &winreg::RegKey, exe: &str) -> std::io::Result<()> {
    let (verb, _) = class.create_subkey(r"shell\print")?;
    verb.set_value("MultiSelectModel", &"Player")?;
    let (cmd, _) = verb.create_subkey("command")?;
    cmd.set_value("", &print_command(exe))
}

/// Print verbs under our ProgID and under Applications\<exe> (used when the
/// default is chosen in Windows Settings). With `create == false` only
/// existing registrations are updated.
#[cfg(target_os = "windows")]
fn write_print_verbs(create: bool) -> std::io::Result<()> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_ALL_ACCESS};
    use winreg::RegKey;

    let exe = std::env::current_exe()?.to_string_lossy().to_string();
    let exe_name = std::path::Path::new(&exe)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "pdf-rider.exe".to_string());
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);

    let progid = r"Software\Classes\com.pdfrider.app".to_string();
    let app = format!(r"Software\Classes\Applications\{exe_name}");
    for path in [progid, app] {
        let key = if create {
            hkcu.create_subkey(&path)?.0
        } else {
            match hkcu.open_subkey_with_flags(&path, KEY_ALL_ACCESS) {
                Ok(k) => k,
                Err(_) => continue,
            }
        };
        if path.contains("Applications") {
            // Only needed for the print verb: keep it out of "Open with"
            key.set_value("NoOpenWith", &"")?;
        }
        write_print_verb(&key, &exe)?;
    }
    Ok(())
}

/// True when running from an MSIX package (Microsoft Store build).
#[cfg(target_os = "windows")]
pub fn is_packaged() -> bool {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentPackageFullName(len: *mut u32, name: *mut u16) -> i32;
    }
    const APPMODEL_ERROR_NO_PACKAGE: i32 = 15700;
    let mut len = 0u32;
    // ERROR_INSUFFICIENT_BUFFER when packaged, APPMODEL_ERROR_NO_PACKAGE otherwise
    unsafe { GetCurrentPackageFullName(&mut len, std::ptr::null_mut()) != APPMODEL_ERROR_NO_PACKAGE }
}

#[cfg(test)]
mod tests {
    #[test]
    fn print_command_quotes_exe_and_file() {
        assert_eq!(
            super::print_command(r"C:\Program Files\PDF Rider\pdf-rider-portable.exe"),
            r#""C:\Program Files\PDF Rider\pdf-rider-portable.exe" --print "%1""#
        );
    }

    /// Writes the verb under a throwaway HKCU key (not a real file class) and reads it back.
    #[cfg(target_os = "windows")]
    #[test]
    fn print_verb_allows_any_number_of_selected_files() {
        use winreg::enums::HKEY_CURRENT_USER;
        let hkcu = winreg::RegKey::predef(HKEY_CURRENT_USER);
        let path = format!(r"Software\PdfRiderTest-{}", std::process::id());
        let (class, _) = hkcu.create_subkey(&path).unwrap();
        super::write_print_verb(&class, r"C:\x\pdf-rider-portable.exe").unwrap();
        let model: String = class.open_subkey(r"shell\print").unwrap().get_value("MultiSelectModel").unwrap();
        let cmd: String = class.open_subkey(r"shell\print\command").unwrap().get_value("").unwrap();
        hkcu.delete_subkey_all(&path).unwrap();
        assert_eq!(model, "Player");
        assert_eq!(cmd, r#""C:\x\pdf-rider-portable.exe" --print "%1""#);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_binary_is_not_packaged() {
        assert!(!super::is_packaged());
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
