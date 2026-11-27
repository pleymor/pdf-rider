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
