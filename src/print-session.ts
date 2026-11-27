/**
 * Batch printing for `pdf-rider-portable.exe --print <file>...`, i.e. the
 * Explorer "Print" verb. Explorer launches one process per selected file; the
 * first one becomes the print session and the others hand their file over to
 * it (see `PrintQueue` in Rust), so files are printed one after another by a
 * single hidden window. Kept free of DOM/Tauri imports so it can be unit-tested.
 */

import { errorText, isPrintCancelled } from "./print-errors";

export interface PrintFailure {
  path: string;
  error: string;
}

export interface PrintSessionDeps {
  /** Removes and returns the files waiting to be printed. */
  takeJobs(): Promise<string[]>;
  /**
   * Returns files that arrived meanwhile, or closes the queue when there are
   * none (files arriving after that are printed by a new process).
   */
  finish(): Promise<string[]>;
  /** Prints one file to the default printer; throws with a readable message. */
  printFile(path: string): Promise<void>;
  /**
   * Shows failures not reported yet. Called while the queue is still open, so
   * files handed over meanwhile (the dialog may stay up for minutes) are
   * printed by this session instead of bouncing between processes.
   */
  report?(failures: PrintFailure[]): Promise<void>;
  /** Called before each file. `total` grows as files are handed over. */
  onProgress?(done: number, total: number, path: string): void;
  /** Waits before closing the queue, so that late Explorer launches join this session. */
  wait?(ms: number): Promise<void>;
  graceMs?: number;
}

/**
 * Prints every queued file and returns the ones that failed; a cancelled job
 * (e.g. the file prompt of a virtual printer) is not a failure. Never throws
 * unless `report` does.
 */
export async function runPrintSession(deps: PrintSessionDeps): Promise<PrintFailure[]> {
  const failures: PrintFailure[] = [];
  const wait = deps.wait ?? ((ms: number) => new Promise<void>(r => setTimeout(r, ms)));
  const graceMs = deps.graceMs ?? 1500;
  let done = 0;
  let total = 0;

  const printAll = async (jobs: string[]) => {
    total += jobs.length;
    for (const path of jobs) {
      deps.onProgress?.(done, total, path);
      try {
        await deps.printFile(path);
      } catch (err) {
        if (!isPrintCancelled(err)) failures.push({ path, error: errorText(err) });
      }
      done++;
      // Pick up files handed over while this one was printing.
      const more = await safe(deps.takeJobs, failures);
      if (more.length) { total += more.length; jobs.push(...more); }
    }
  };

  let reported = 0;
  await printAll(await safe(deps.takeJobs, failures));
  for (;;) {
    await wait(graceMs);
    await printAll(await safe(deps.takeJobs, failures));
    if (deps.report && failures.length > reported) {
      const fresh = failures.slice(reported);
      reported = failures.length;
      await deps.report(fresh);
      continue; // print what arrived during the report before closing
    }
    const late = await safe(deps.finish, failures);
    if (late.length === 0) break;
    await printAll(late);
  }
  return failures;
}

async function safe(fn: () => Promise<string[]>, failures: PrintFailure[]): Promise<string[]> {
  try {
    return await fn();
  } catch (err) {
    failures.push({ path: "", error: errorText(err) });
    return [];
  }
}

export function fileName(path: string): string {
  return path.split(/[\\/]/).pop() || path;
}

/** Text of the single dialog shown at the end when some files did not print. */
export function failureReport(failures: PrintFailure[], printed: number): string {
  const lines = failures.map(f => (f.path ? `• ${fileName(f.path)}: ${f.error}` : `• ${f.error}`));
  const head = failures.length === 1 && printed === 0
    ? "The document could not be printed."
    : `${failures.length} document(s) could not be printed${printed ? ` (${printed} printed)` : ""}.`;
  return `${head}\n\n${lines.join("\n")}`;
}
