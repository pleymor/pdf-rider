//! Command line parsing and the queue behind batch printing from Explorer.
//!
//! Explorer's "Print" verb (`--print "%1"`, MultiSelectModel=Player) starts
//! one process per selected file. In `--print` mode the single-instance plugin
//! forwards the files of every later process to the first one, which prints
//! them one after another, then closes the queue and exits.

use std::collections::VecDeque;
use std::path::Path;

#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupArgs {
    /// File to open (normal mode).
    pub file_path: Option<String>,
    pub should_print: bool,
    /// Files to print (`--print a.pdf b.pdf …`).
    #[serde(skip)]
    pub print_files: Vec<String>,
}

/// Parses the arguments after the executable name.
pub fn parse_args<I: IntoIterator<Item = String>>(args: I, cwd: &Path) -> StartupArgs {
    let mut out = StartupArgs::default();
    for arg in args {
        if arg == "--print" {
            out.should_print = true;
        } else if !arg.starts_with("--") && !arg.trim().is_empty() {
            let path = absolute(&arg, cwd);
            if out.file_path.is_none() {
                out.file_path = Some(path.clone());
            }
            out.print_files.push(path);
        }
    }
    if !out.should_print {
        out.print_files.clear();
    }
    out
}

fn absolute(arg: &str, cwd: &Path) -> String {
    let p = Path::new(arg);
    if p.is_absolute() || cwd.as_os_str().is_empty() {
        arg.to_string()
    } else {
        cwd.join(p).to_string_lossy().into_owned()
    }
}

#[derive(Debug, Default)]
pub struct PrintQueue {
    pending: VecDeque<String>,
    closed: bool,
}

/// What to do with files handed over by another `--print` process.
#[derive(Debug, PartialEq, Eq)]
pub enum Handover {
    Queued,
    /// This session already finished: print them in a new process.
    Relaunch(Vec<String>),
}

impl PrintQueue {
    pub fn new(files: Vec<String>) -> Self {
        Self { pending: files.into(), closed: false }
    }

    pub fn hand_over(&mut self, files: Vec<String>) -> Handover {
        if self.closed {
            Handover::Relaunch(files)
        } else {
            self.pending.extend(files);
            Handover::Queued
        }
    }

    pub fn take(&mut self) -> Vec<String> {
        self.pending.drain(..).collect()
    }

    /// Returns the files that arrived meanwhile; when there are none, closes
    /// the queue so nothing handed over later can be lost.
    pub fn finish(&mut self) -> Vec<String> {
        let rest = self.take();
        if rest.is_empty() {
            self.closed = true;
        }
        rest
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn open_mode_keeps_the_file() {
        let a = parse_args(args(&["C:\\a.pdf"]), Path::new(""));
        assert_eq!(a.file_path.as_deref(), Some("C:\\a.pdf"));
        assert!(!a.should_print);
        assert!(a.print_files.is_empty());
    }

    #[test]
    fn print_mode_accepts_one_or_many_files() {
        let one = parse_args(args(&["--print", "C:\\a.pdf"]), Path::new(""));
        assert!(one.should_print);
        assert_eq!(one.print_files, args(&["C:\\a.pdf"]));

        let many = parse_args(args(&["--print", "C:\\a.pdf", "C:\\b c.pdf", "--print", "C:\\d.pdf"]), Path::new(""));
        assert_eq!(many.print_files, args(&["C:\\a.pdf", "C:\\b c.pdf", "C:\\d.pdf"]));
    }

    #[test]
    fn relative_paths_resolve_against_the_launch_directory() {
        let cwd = std::env::temp_dir();
        let a = parse_args(args(&["--print", "doc.pdf"]), &cwd);
        assert_eq!(a.print_files, vec![cwd.join("doc.pdf").to_string_lossy().into_owned()]);
    }

    #[test]
    fn queue_collects_handovers_until_finished() {
        let mut q = PrintQueue::new(args(&["a"]));
        assert_eq!(q.take(), args(&["a"]));
        assert_eq!(q.hand_over(args(&["b"])), Handover::Queued);
        assert_eq!(q.hand_over(args(&["c"])), Handover::Queued);
        // Files that arrive before finishing are returned, the queue stays open
        assert_eq!(q.finish(), args(&["b", "c"]));
        assert_eq!(q.hand_over(args(&["d"])), Handover::Queued);
        assert_eq!(q.take(), args(&["d"]));
        // Nothing left: the queue closes, later files go to a new process
        assert!(q.finish().is_empty());
        assert_eq!(q.hand_over(args(&["e"])), Handover::Relaunch(args(&["e"])));
    }
}

// ── Commands ─────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn take_print_jobs(state: tauri::State<crate::AppState>) -> Vec<String> {
    state.print_queue.lock().unwrap().take()
}

#[tauri::command]
pub fn finish_print_session(state: tauri::State<crate::AppState>) -> Vec<String> {
    state.print_queue.lock().unwrap().finish()
}

/// Single-instance callback: `argv` is the full command line of a later
/// `--print` process (executable first), `cwd` its working directory.
pub fn on_handover<R: tauri::Runtime>(app: &tauri::AppHandle<R>, argv: Vec<String>, cwd: String) {
    use tauri::Manager;
    let files = parse_args(argv.into_iter().skip(1), Path::new(&cwd)).print_files;
    if files.is_empty() {
        return;
    }
    let state = app.state::<crate::AppState>();
    let handover = state.print_queue.lock().unwrap().hand_over(files);
    if let Handover::Relaunch(files) = handover {
        // This session is shutting down: print them in a fresh process
        if let Ok(exe) = std::env::current_exe() {
            let _ = std::process::Command::new(exe).arg("--print").args(files).spawn();
        }
    }
}
