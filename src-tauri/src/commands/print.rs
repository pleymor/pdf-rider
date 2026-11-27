//! In-process printing: pages rasterised by pdf.js are sent to a Windows
//! printer through GDI, so printing depends neither on the default PDF app
//! nor on any tool installed on the machine — only on the printer itself.
//!
//! Every failure is turned into a [`PrintError`] that says what went wrong and
//! what the user can do, plus the Windows error code and system text so that a
//! user report is enough to diagnose it.

use std::fmt;

// ── Errors ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PrintErrorKind {
    /// The user cancelled (e.g. the file name prompt of a virtual printer).
    Cancelled,
    NoDefaultPrinter,
    PrinterNotFound,
    DriverMissing,
    PrinterUnreachable,
    SpoolerNotRunning,
    AccessDenied,
    OutOfResources,
    BadPageData,
    NothingToPrint,
    #[cfg_attr(target_os = "windows", allow(dead_code))]
    Unsupported,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrintError {
    pub kind: PrintErrorKind,
    pub message: String,
}

impl fmt::Display for PrintError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl PrintError {
    fn new(kind: PrintErrorKind, message: impl Into<String>) -> Self {
        Self { kind, message: message.into() }
    }
}

/// Step of the print job that failed, used to word the message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    FindDefaultPrinter,
    OpenPrinter,
    StartJob,
    Page(usize),
    FinishJob,
}

impl Stage {
    fn describe(self) -> String {
        match self {
            Stage::FindDefaultPrinter => "looking up the default printer".into(),
            Stage::OpenPrinter => "opening the printer".into(),
            Stage::StartJob => "starting the print job".into(),
            Stage::Page(n) => format!("sending page {n}"),
            Stage::FinishJob => "finishing the print job".into(),
        }
    }
}

// Win32 error codes (winerror.h) that have a specific, actionable cause.
const ERROR_FILE_NOT_FOUND: u32 = 2;
const ERROR_ACCESS_DENIED: u32 = 5;
const ERROR_NOT_ENOUGH_MEMORY: u32 = 8;
const ERROR_OUTOFMEMORY: u32 = 14;
const ERROR_PRINT_CANCELLED: u32 = 63;
const ERROR_DISK_FULL: u32 = 112;
const ERROR_CANCELLED: u32 = 1223;
const RPC_S_SERVER_UNAVAILABLE: u32 = 1722;
const EPT_S_NOT_REGISTERED: u32 = 1753;
const ERROR_UNKNOWN_PRINTER_DRIVER: u32 = 1797;
const ERROR_INVALID_PRINTER_NAME: u32 = 1801;
const ERROR_SPOOL_FILE_NOT_FOUND: u32 = 3002;
const ERROR_PRINTER_NOT_FOUND: u32 = 3012;
const ERROR_PRINTER_DRIVER_BLOCKED: u32 = 3014;
const ERROR_PRINTER_NOT_SHAREABLE: u32 = 3022;

const PRINTERS_SETTINGS: &str = "Windows Settings › Bluetooth & devices › Printers & scanners";

/// Builds the user-facing error for a failed Win32 call.
///
/// `code` is `GetLastError()` (0 when the API did not set one) and
/// `system_text` its `FormatMessage` text, in the user's Windows language.
pub fn describe_failure(stage: Stage, code: u32, printer: &str, system_text: &str) -> PrintError {
    use PrintErrorKind::*;
    let is_network = printer.starts_with("\\\\");
    let (kind, what) = match code {
        ERROR_CANCELLED | ERROR_PRINT_CANCELLED => {
            return PrintError::new(Cancelled, "Printing was cancelled.");
        }
        ERROR_FILE_NOT_FOUND if stage == Stage::FindDefaultPrinter => (
            NoDefaultPrinter,
            format!(
                "No default printer is set on this PC. Choose a printer in the Print dialog, \
                 or set a default printer in {PRINTERS_SETTINGS} (turn off \
                 \"Let Windows manage my default printer\" if it is on)."
            ),
        ),
        ERROR_INVALID_PRINTER_NAME | ERROR_PRINTER_NOT_FOUND => (
            PrinterNotFound,
            format!(
                "The printer \"{printer}\" is not installed on this PC (it may have been \
                 removed or renamed). Choose another printer, or add it in {PRINTERS_SETTINGS}."
            ),
        ),
        ERROR_UNKNOWN_PRINTER_DRIVER | ERROR_PRINTER_DRIVER_BLOCKED => (
            DriverMissing,
            format!(
                "The driver of the printer \"{printer}\" is missing or blocked. Reinstall the \
                 printer (or its driver from the manufacturer) in {PRINTERS_SETTINGS}."
            ),
        ),
        RPC_S_SERVER_UNAVAILABLE | EPT_S_NOT_REGISTERED if is_network => (
            PrinterUnreachable,
            format!(
                "The print server of \"{printer}\" cannot be reached. Check the network or VPN \
                 connection, or choose a local printer."
            ),
        ),
        RPC_S_SERVER_UNAVAILABLE | EPT_S_NOT_REGISTERED => (
            SpoolerNotRunning,
            "The Windows \"Print Spooler\" service is not running. Start it from Services \
             (services.msc) or restart the PC, then print again."
                .to_string(),
        ),
        ERROR_ACCESS_DENIED | ERROR_PRINTER_NOT_SHAREABLE => (
            AccessDenied,
            format!(
                "Windows denied access to the printer \"{printer}\". Ask your administrator \
                 for permission to print on it, or choose another printer."
            ),
        ),
        ERROR_NOT_ENOUGH_MEMORY | ERROR_OUTOFMEMORY | ERROR_DISK_FULL | ERROR_SPOOL_FILE_NOT_FOUND => (
            OutOfResources,
            "Windows ran out of memory or disk space while spooling the document. Free some \
             disk space, close other programs or print fewer pages at a time."
                .to_string(),
        ),
        _ => (
            Failed,
            format!(
                "Printing to \"{printer}\" failed while {}. Check that the printer is switched \
                 on and ready (see its queue in {PRINTERS_SETTINGS}), then try again.",
                stage.describe()
            ),
        ),
    };
    PrintError::new(kind, format!("{what}{}", windows_detail(code, system_text)))
}

fn windows_detail(code: u32, system_text: &str) -> String {
    let text = system_text.trim();
    match (code, text.is_empty()) {
        (0, _) => " (Windows reported no error code.)".to_string(),
        (c, true) => format!(" (Windows error {c}.)"),
        (c, false) => format!(" (Windows error {c}: {text})"),
    }
}

/// The printer to use: the one chosen, else the default printer.
pub fn resolve_printer(
    chosen: Option<String>,
    default_printer: impl FnOnce() -> Result<String, (u32, String)>,
) -> Result<String, PrintError> {
    match chosen.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()) {
        Some(name) => Ok(name),
        None => default_printer().map_err(|(code, text)| {
            describe_failure(Stage::FindDefaultPrinter, code, "", &text)
        }),
    }
}

/// Largest `(w, h)` with the image's aspect ratio that fits in `max_w × max_h`.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn fit_within(img_w: u32, img_h: u32, max_w: i32, max_h: i32) -> (i32, i32) {
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

/// Decodes one base64 JPEG page into a bottom-up-free, 4-byte aligned BGR DIB.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn decode_page(page_b64: &str, page_no: usize) -> Result<(u32, u32, Vec<u8>), PrintError> {
    use base64::Engine;
    let bad = |e: String| {
        PrintError::new(
            PrintErrorKind::BadPageData,
            format!("Page {page_no} could not be prepared for printing ({e}). Nothing was printed."),
        )
    };
    let jpeg = base64::engine::general_purpose::STANDARD
        .decode(page_b64.trim())
        .map_err(|e| bad(e.to_string()))?;
    let img = image::load_from_memory(&jpeg).map_err(|e| bad(e.to_string()))?.into_rgb8();
    let (w, h) = img.dimensions();
    let rgb = img.as_raw();
    let row_stride = (w as usize * 3 + 3) & !3;
    let mut dib = vec![0u8; row_stride * h as usize];
    for y in 0..h as usize {
        for x in 0..w as usize {
            let s = y * w as usize * 3 + x * 3;
            let d = y * row_stride + x * 3;
            dib[d] = rgb[s + 2];
            dib[d + 1] = rgb[s + 1];
            dib[d + 2] = rgb[s];
        }
    }
    Ok((w, h, dib))
}

// ── Commands ─────────────────────────────────────────────────────────────────

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrinterList {
    pub printers: Vec<String>,
    pub default_printer: String,
}

#[tauri::command]
pub fn list_printers() -> Result<PrinterList, PrintError> {
    #[cfg(target_os = "windows")]
    {
        win::list_printers()
    }
    #[cfg(not(target_os = "windows"))]
    {
        Ok(PrinterList { printers: vec![], default_printer: String::new() })
    }
}

#[tauri::command]
pub fn print_pages(
    pages_b64: Vec<String>,
    printer_name: Option<String>,
    copies: Option<u32>,
    orientation: Option<String>,
    fit_mode: Option<String>,
) -> Result<(), PrintError> {
    let job = PrintJob {
        pages_b64,
        printer_name,
        copies: copies.unwrap_or(1).max(1),
        landscape: orientation.as_deref().map(|o| o == "landscape"),
        actual_size: fit_mode.as_deref() == Some("actual"),
        output_file: None,
    };
    #[cfg(target_os = "windows")]
    {
        win::print(&job)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = job;
        Err(PrintError::new(PrintErrorKind::Unsupported, "Printing is only supported on Windows."))
    }
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub struct PrintJob {
    pub pages_b64: Vec<String>,
    pub printer_name: Option<String>,
    pub copies: u32,
    /// `None` keeps the driver's default orientation.
    pub landscape: Option<bool>,
    pub actual_size: bool,
    /// Spool to this file instead of the printer's port (tests only).
    pub output_file: Option<std::path::PathBuf>,
}

/// Resolution pdf.js renders pages at for printing (see `renderPagesForPrint`).
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
const RENDER_DPI: i64 = 200;

#[cfg(target_os = "windows")]
pub mod win {
    use super::*;
    use std::ptr;
    use winapi::shared::windef::HDC;
    use winapi::um::errhandlingapi::GetLastError;

    /// Last Win32 error of the calling thread, with its system text.
    fn last_error() -> (u32, String) {
        let code = unsafe { GetLastError() };
        (code, system_message(code))
    }

    pub fn system_message(code: u32) -> String {
        use winapi::um::winbase::{
            FormatMessageW, FORMAT_MESSAGE_FROM_SYSTEM, FORMAT_MESSAGE_IGNORE_INSERTS,
        };
        if code == 0 {
            return String::new();
        }
        let mut buf = [0u16; 512];
        let len = unsafe {
            FormatMessageW(
                FORMAT_MESSAGE_FROM_SYSTEM | FORMAT_MESSAGE_IGNORE_INSERTS,
                ptr::null(),
                code,
                0,
                buf.as_mut_ptr(),
                buf.len() as u32,
                ptr::null_mut(),
            )
        };
        String::from_utf16_lossy(&buf[..len as usize]).trim().to_string()
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn from_wide_ptr(p: *const u16) -> String {
        if p.is_null() {
            return String::new();
        }
        let len = unsafe { (0..).take_while(|&i| *p.add(i) != 0).count() };
        String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(p, len) })
    }

    pub fn default_printer_name() -> Result<String, (u32, String)> {
        use winapi::um::winspool::GetDefaultPrinterW;
        const ERROR_INSUFFICIENT_BUFFER: u32 = 122;
        let mut size = 0u32;
        if unsafe { GetDefaultPrinterW(ptr::null_mut(), &mut size) } == 0 {
            let err = last_error();
            if err.0 != ERROR_INSUFFICIENT_BUFFER {
                return Err(err);
            }
        }
        let mut buf = vec![0u16; size.max(1) as usize];
        if unsafe { GetDefaultPrinterW(buf.as_mut_ptr(), &mut size) } == 0 {
            return Err(last_error());
        }
        Ok(from_wide_ptr(buf.as_ptr()))
    }

    pub fn list_printers() -> Result<PrinterList, PrintError> {
        use winapi::um::winspool::{EnumPrintersW, PRINTER_INFO_4W};
        const PRINTER_ENUM_LOCAL: u32 = 0x2;
        const PRINTER_ENUM_CONNECTIONS: u32 = 0x4;
        // Level 4 only reads the local registry: unlike level 2 it does not
        // query each print server, so an unreachable network printer cannot
        // make the whole list fail (or hang).
        let flags = PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS;
        let mut needed = 0u32;
        let mut returned = 0u32;
        let mut buf: Vec<u8> = Vec::new();
        for _ in 0..3 {
            let ok = unsafe {
                EnumPrintersW(flags, ptr::null_mut(), 4, buf.as_mut_ptr(), buf.len() as u32, &mut needed, &mut returned)
            };
            if ok != 0 {
                break;
            }
            let (code, text) = last_error();
            if code != 122 || needed as usize <= buf.len() {
                return Err(describe_failure(Stage::OpenPrinter, code, "", &text));
            }
            buf = vec![0u8; needed as usize];
        }
        let infos = buf.as_ptr() as *const PRINTER_INFO_4W;
        let printers = (0..returned as usize)
            .map(|i| from_wide_ptr(unsafe { (*infos.add(i)).pPrinterName }))
            .filter(|n| !n.is_empty())
            .collect();
        Ok(PrinterList { printers, default_printer: default_printer_name().unwrap_or_default() })
    }

    /// Printer DC that aborts an unfinished job and is always deleted, so a
    /// failure never leaves a half-spooled job stuck in the queue.
    struct PrinterDc {
        hdc: HDC,
        in_job: bool,
    }

    impl Drop for PrinterDc {
        fn drop(&mut self) {
            use winapi::um::wingdi::{AbortDoc, DeleteDC};
            unsafe {
                if self.in_job {
                    AbortDoc(self.hdc);
                }
                DeleteDC(self.hdc);
            }
        }
    }

    pub fn print(job: &PrintJob) -> Result<(), PrintError> {
        use winapi::um::wingdi::{
            CreateDCW, EndDoc, EndPage, GetDeviceCaps, StartDocW, StartPage, StretchDIBits,
            BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DOCINFOW, HORZRES, LOGPIXELSX,
            LOGPIXELSY, RGBQUAD, SRCCOPY, VERTRES,
        };
        const GDI_ERROR: i32 = -1;

        if job.pages_b64.is_empty() {
            return Err(PrintError::new(PrintErrorKind::NothingToPrint, "There are no pages to print."));
        }
        let printer = resolve_printer(job.printer_name.clone(), default_printer_name)?;
        let fail = |stage: Stage| {
            let (code, text) = last_error();
            describe_failure(stage, code, &printer, &text)
        };
        let printer_w = wide(&printer);

        // Start from the driver's own DEVMODE so its private settings are kept;
        // fall back to the driver defaults if it cannot supply one.
        let devmode = job.landscape.and_then(|l| printer_devmode(&printer_w, l));
        let devmode_ptr =
            devmode.as_ref().map_or(ptr::null(), |b| b.as_ptr() as *const winapi::um::wingdi::DEVMODEW);

        let hdc = unsafe { CreateDCW(ptr::null(), printer_w.as_ptr(), ptr::null(), devmode_ptr) };
        if hdc.is_null() {
            return Err(fail(Stage::OpenPrinter));
        }
        let mut dc = PrinterDc { hdc, in_job: false };

        let page_w = unsafe { GetDeviceCaps(hdc, HORZRES) };
        let page_h = unsafe { GetDeviceCaps(hdc, VERTRES) };
        let dpi_x = unsafe { GetDeviceCaps(hdc, LOGPIXELSX) };
        let dpi_y = unsafe { GetDeviceCaps(hdc, LOGPIXELSY) };

        let doc_name = wide("PDF Rider document");
        let output = job.output_file.as_ref().map(|p| wide(&p.to_string_lossy()));
        let doc_info = DOCINFOW {
            cbSize: std::mem::size_of::<DOCINFOW>() as i32,
            lpszDocName: doc_name.as_ptr(),
            lpszOutput: output.as_ref().map_or(ptr::null(), |o| o.as_ptr()),
            lpszDatatype: ptr::null(),
            fwType: 0,
        };
        if unsafe { StartDocW(hdc, &doc_info) } <= 0 {
            return Err(fail(Stage::StartJob));
        }
        dc.in_job = true;

        let total = job.pages_b64.len();
        for copy in 0..job.copies as usize {
            for (i, page_b64) in job.pages_b64.iter().enumerate() {
                let page_no = i + 1;
                let stage = Stage::Page(copy * total + page_no);
                let (img_w, img_h, dib) = decode_page(page_b64, page_no)?;

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
                    bmiColors: [RGBQUAD { rgbBlue: 0, rgbGreen: 0, rgbRed: 0, rgbReserved: 0 }],
                };

                let (dest_x, dest_y, dest_w, dest_h) = if job.actual_size {
                    // Map image pixels to printer units at native DPI
                    let w = (img_w as i64 * dpi_x as i64 / RENDER_DPI) as i32;
                    let h = (img_h as i64 * dpi_y as i64 / RENDER_DPI) as i32;
                    (((page_w - w) / 2).max(0), ((page_h - h) / 2).max(0), w.min(page_w), h.min(page_h))
                } else {
                    let (w, h) = fit_within(img_w, img_h, page_w, page_h);
                    ((page_w - w) / 2, (page_h - h) / 2, w, h)
                };

                if unsafe { StartPage(hdc) } <= 0 {
                    return Err(fail(stage));
                }
                let lines = unsafe {
                    StretchDIBits(
                        hdc, dest_x, dest_y, dest_w, dest_h, 0, 0, img_w as i32, img_h as i32,
                        dib.as_ptr() as *const _, &bmi, DIB_RGB_COLORS, SRCCOPY,
                    )
                };
                if lines == 0 || lines == GDI_ERROR {
                    return Err(fail(stage));
                }
                if unsafe { EndPage(hdc) } <= 0 {
                    return Err(fail(stage));
                }
            }
        }

        if unsafe { EndDoc(hdc) } <= 0 {
            return Err(fail(Stage::FinishJob));
        }
        dc.in_job = false;
        Ok(())
    }

    /// The printer driver's DEVMODE with the requested orientation set, as a
    /// raw buffer sized by the driver. `None` if the driver can't supply one.
    fn printer_devmode(printer_w: &[u16], landscape: bool) -> Option<Vec<u8>> {
        use winapi::um::wingdi::{DEVMODEW, DMORIENT_LANDSCAPE, DMORIENT_PORTRAIT, DM_ORIENTATION};
        use winapi::um::winspool::{ClosePrinter, DocumentPropertiesW, OpenPrinterW};
        const DM_OUT_BUFFER: u32 = 2;
        const DM_IN_BUFFER: u32 = 8;

        let mut name = printer_w.to_vec();
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
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn no_default_printer_tells_the_user_where_to_set_one() {
        // GetDefaultPrinterW fails with ERROR_FILE_NOT_FOUND on a PC without a
        // default printer (e.g. a fresh install, or none installed yet).
        let err = resolve_printer(None, || Err((2, "Le fichier spécifié est introuvable.".into()))).unwrap_err();
        assert_eq!(err.kind, PrintErrorKind::NoDefaultPrinter);
        assert!(err.message.contains("No default printer is set"), "{err}");
        assert!(err.message.contains("Printers & scanners"), "{err}");
        assert!(err.message.contains("Windows error 2: Le fichier spécifié est introuvable."), "{err}");
    }

    #[test]
    fn chosen_printer_wins_and_blank_means_default() {
        assert_eq!(resolve_printer(Some("HP".into()), || unreachable!()).unwrap(), "HP");
        assert_eq!(resolve_printer(Some("  ".into()), || Ok("Default".into())).unwrap(), "Default");
    }

    #[test]
    fn known_causes_are_classified() {
        let k = |stage, code, printer: &str| describe_failure(stage, code, printer, "").kind;
        use PrintErrorKind::*;
        assert_eq!(k(Stage::OpenPrinter, 1801, "X"), PrinterNotFound);
        assert_eq!(k(Stage::OpenPrinter, 3012, "X"), PrinterNotFound);
        assert_eq!(k(Stage::OpenPrinter, 1797, "X"), DriverMissing);
        assert_eq!(k(Stage::StartJob, 1722, "X"), SpoolerNotRunning);
        assert_eq!(k(Stage::StartJob, 1722, "\\\\srv\\X"), PrinterUnreachable);
        assert_eq!(k(Stage::StartJob, 5, "X"), AccessDenied);
        assert_eq!(k(Stage::Page(3), 112, "X"), OutOfResources);
        assert_eq!(k(Stage::StartJob, 1223, "X"), Cancelled);
        assert_eq!(k(Stage::Page(2), 31, "X"), Failed);
        // ERROR_FILE_NOT_FOUND only means "no default printer" when looking it up
        assert_eq!(k(Stage::StartJob, 2, "X"), Failed);
    }

    #[test]
    fn generic_failure_names_printer_stage_and_code() {
        let err = describe_failure(Stage::Page(4), 31, "HP", "Un périphérique ne fonctionne pas.");
        assert_eq!(
            err.message,
            "Printing to \"HP\" failed while sending page 4. Check that the printer is switched on \
             and ready (see its queue in Windows Settings › Bluetooth & devices › Printers & \
             scanners), then try again. (Windows error 31: Un périphérique ne fonctionne pas.)"
        );
        let err = describe_failure(Stage::StartJob, 0, "HP", "");
        assert!(err.message.ends_with("(Windows reported no error code.)"), "{err}");
    }

    #[test]
    fn corrupt_page_data_is_reported_with_page_number() {
        let err = decode_page("not base64!", 3).unwrap_err();
        assert_eq!(err.kind, PrintErrorKind::BadPageData);
        assert!(err.message.starts_with("Page 3 could not be prepared"), "{err}");
    }

    #[test]
    fn decode_page_pads_rows_to_four_bytes() {
        use base64::Engine;
        let img = image::RgbImage::from_pixel(3, 2, image::Rgb([10, 20, 30]));
        let mut png = Vec::new();
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let b64 = base64::engine::general_purpose::STANDARD.encode(png);
        let (w, h, dib) = decode_page(&b64, 1).unwrap();
        assert_eq!((w, h), (3, 2));
        assert_eq!(dib.len(), 12 * 2); // 9 bytes of BGR padded to 12 per row
        assert_eq!(&dib[0..3], &[30, 20, 10]); // RGB → BGR
    }
}

/// Runs against the real Windows print spooler; no paper is used.
#[cfg(all(test, target_os = "windows"))]
mod windows_print_tests {
    use super::*;

    fn tiny_page_b64() -> String {
        use base64::Engine;
        let img = image::RgbImage::from_pixel(4, 4, image::Rgb([255, 255, 255]));
        let mut jpeg = Vec::new();
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut std::io::Cursor::new(&mut jpeg), image::ImageFormat::Jpeg)
            .unwrap();
        base64::engine::general_purpose::STANDARD.encode(jpeg)
    }

    fn job(printer: &str, pages: Vec<String>, output: Option<std::path::PathBuf>) -> PrintJob {
        PrintJob {
            pages_b64: pages,
            printer_name: Some(printer.into()),
            copies: 1,
            landscape: Some(false),
            actual_size: false,
            output_file: output,
        }
    }

    /// Reproducer: a printer that is not installed on this machine.
    #[test]
    fn missing_printer_error_is_actionable() {
        let printer = "PDF Rider Test Printer That Is Not Installed";
        let err = win::print(&job(printer, vec![tiny_page_b64()], None)).unwrap_err().to_string();
        assert!(err.contains(printer), "message should name the printer: {err}");
        assert!(err.contains("not installed"), "message should say what is wrong: {err}");
        assert!(err.contains("Windows error"), "message should carry the Windows error code: {err}");
    }

    /// End-to-end GDI path on the built-in "Microsoft Print to PDF" printer,
    /// spooled to a file so no dialog is shown.
    #[test]
    fn prints_to_microsoft_print_to_pdf() {
        let out = std::env::temp_dir().join(format!("pdf-rider-print-test-{}.pdf", std::process::id()));
        let _ = std::fs::remove_file(&out);
        match win::print(&job("Microsoft Print to PDF", vec![tiny_page_b64(), tiny_page_b64()], Some(out.clone()))) {
            Ok(()) => {}
            Err(e) if e.kind == PrintErrorKind::PrinterNotFound => {
                eprintln!("skipped: Microsoft Print to PDF is not installed here");
                return;
            }
            Err(e) => panic!("{e}"),
        }
        // The spooler writes the file asynchronously
        let mut bytes = Vec::new();
        for _ in 0..100 {
            bytes = std::fs::read(&out).unwrap_or_default();
            if bytes.ends_with(b"%%EOF\n") || bytes.ends_with(b"%%EOF") || bytes.ends_with(b"%%EOF\r\n") {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let _ = std::fs::remove_file(&out);
        assert!(bytes.starts_with(b"%PDF"), "no PDF produced ({} bytes)", bytes.len());
        let doc = lopdf::Document::load_mem(&bytes).expect("spooled output is a valid PDF");
        assert_eq!(doc.get_pages().len(), 2);
    }

    /// A page that cannot be decoded aborts the job instead of leaving a
    /// half-spooled job behind; the spooled file is never completed.
    #[test]
    fn bad_page_aborts_the_job() {
        let out = std::env::temp_dir().join(format!("pdf-rider-abort-test-{}.pdf", std::process::id()));
        let _ = std::fs::remove_file(&out);
        let err = win::print(&job("Microsoft Print to PDF", vec![tiny_page_b64(), "garbage".into()], Some(out.clone())))
            .unwrap_err();
        if err.kind == PrintErrorKind::PrinterNotFound {
            return;
        }
        assert_eq!(err.kind, PrintErrorKind::BadPageData, "{err}");
        assert!(err.message.contains("Page 2"), "{err}");
        std::thread::sleep(std::time::Duration::from_millis(1500));
        let bytes = std::fs::read(&out).unwrap_or_default();
        let _ = std::fs::remove_file(&out);
        assert!(!bytes.starts_with(b"%PDF"), "aborted job still produced a PDF");
    }

    #[test]
    fn printer_list_contains_the_default_printer() {
        let list = win::list_printers().unwrap();
        if !list.default_printer.is_empty() {
            assert!(list.printers.contains(&list.default_printer), "{:?}", list.printers);
        }
    }
}
