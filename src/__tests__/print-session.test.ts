import { describe, it, expect } from "vitest";
import { runPrintSession, failureReport, type PrintSessionDeps } from "../print-session";
import { errorText, isPrintCancelled, describeLoadError } from "../print-errors";

/** In-memory stand-in for the Rust PrintQueue. */
function fakeQueue(initial: string[]) {
  const pending = [...initial];
  let closed = false;
  return {
    handOver(files: string[]) {
      if (closed) throw new Error("handed over after close");
      pending.push(...files);
    },
    get closed() { return closed; },
    takeJobs: async () => pending.splice(0),
    finish: async () => {
      const rest = pending.splice(0);
      if (rest.length === 0) closed = true;
      return rest;
    },
  };
}

function deps(q: ReturnType<typeof fakeQueue>, printFile: (p: string) => Promise<void>): PrintSessionDeps {
  return { takeJobs: q.takeJobs, finish: q.finish, printFile, wait: async () => {} };
}

describe("runPrintSession", () => {
  it("prints every selected file in order", async () => {
    const q = fakeQueue(["a.pdf", "b.pdf", "c.pdf"]);
    const printed: string[] = [];
    const failures = await runPrintSession(deps(q, async p => { printed.push(p); }));
    expect(printed).toEqual(["a.pdf", "b.pdf", "c.pdf"]);
    expect(failures).toEqual([]);
    expect(q.closed).toBe(true);
  });

  it("prints files handed over by other Explorer launches while printing", async () => {
    // Explorer starts one process per selected file; later ones hand their
    // file to the first while it is already printing.
    const q = fakeQueue(["1.pdf"]);
    const printed: string[] = [];
    const launches = ["2.pdf", "3.pdf", "4.pdf"];
    const failures = await runPrintSession({
      ...deps(q, async p => {
        printed.push(p);
        const next = launches.shift();
        if (next) q.handOver([next]);
      }),
    });
    expect(printed).toEqual(["1.pdf", "2.pdf", "3.pdf", "4.pdf"]);
    expect(failures).toEqual([]);
  });

  it("picks up late launches during the grace period before closing", async () => {
    const q = fakeQueue(["1.pdf"]);
    const printed: string[] = [];
    let waited = 0;
    await runPrintSession({
      ...deps(q, async p => { printed.push(p); }),
      wait: async () => { if (waited++ === 0) q.handOver(["late.pdf"]); },
    });
    expect(printed).toEqual(["1.pdf", "late.pdf"]);
    expect(q.closed).toBe(true);
  });

  it("keeps printing after a failure and reports each failed file with its cause", async () => {
    const q = fakeQueue(["ok.pdf", "C:\\docs\\broken.pdf", "ok2.pdf"]);
    const printed: string[] = [];
    const failures = await runPrintSession(deps(q, async p => {
      if (p.includes("broken")) {
        // Shape of a Rust PrintError rejected by invoke()
        throw { kind: "printerNotFound", message: "The printer \"HP\" is not installed on this PC." };
      }
      printed.push(p);
    }));
    expect(printed).toEqual(["ok.pdf", "ok2.pdf"]);
    expect(failures).toEqual([{ path: "C:\\docs\\broken.pdf", error: "The printer \"HP\" is not installed on this PC." }]);
    expect(failureReport(failures, 2)).toBe(
      "1 document(s) could not be printed (2 printed).\n\n• broken.pdf: The printer \"HP\" is not installed on this PC.",
    );
  });

  it("keeps the queue open while failures are reported, so a Print started meanwhile joins", async () => {
    // The failure dialog can stay open for minutes; a closed queue would make
    // every later Explorer launch bounce between processes until it is dismissed.
    const q = fakeQueue(["bad.pdf"]);
    const printed: string[] = [];
    const reports: string[][] = [];
    const failures = await runPrintSession({
      ...deps(q, async p => { if (p === "bad.pdf") throw new Error("broken"); printed.push(p); }),
      report: async f => {
        expect(q.closed).toBe(false);
        reports.push(f.map(x => x.path));
        if (reports.length === 1) q.handOver(["during-dialog.pdf"]);
      },
    });
    expect(printed).toEqual(["during-dialog.pdf"]);
    expect(reports).toEqual([["bad.pdf"]]); // each failure reported once
    expect(failures.map(f => f.path)).toEqual(["bad.pdf"]);
    expect(q.closed).toBe(true);
  });

  it("does not report a cancelled print as a failure", async () => {
    const q = fakeQueue(["a.pdf"]);
    const failures = await runPrintSession(deps(q, async () => {
      throw { kind: "cancelled", message: "Printing was cancelled." };
    }));
    expect(failures).toEqual([]);
  });

  it("reports a broken queue instead of silently printing nothing", async () => {
    const failures = await runPrintSession({
      takeJobs: async () => { throw "command take_print_jobs not found"; },
      finish: async () => [],
      printFile: async () => {},
      wait: async () => {},
    });
    expect(failures[0].error).toBe("command take_print_jobs not found");
  });
});

describe("error helpers", () => {
  it("extracts text from strings, Errors and PrintError objects", () => {
    expect(errorText("boom")).toBe("boom");
    expect(errorText(new Error("bad"))).toBe("bad");
    expect(errorText({ kind: "failed", message: "Printing to \"HP\" failed" })).toBe("Printing to \"HP\" failed");
  });

  it("recognises a cancelled print job", () => {
    expect(isPrintCancelled({ kind: "cancelled", message: "Printing was cancelled." })).toBe(true);
    expect(isPrintCancelled({ kind: "failed", message: "x" })).toBe(false);
    expect(isPrintCancelled("cancelled")).toBe(false);
  });

  it("words a single-file failure plainly", () => {
    expect(failureReport([{ path: "a.pdf", error: "No default printer is set on this PC." }], 0))
      .toBe("The document could not be printed.\n\n• a.pdf: No default printer is set on this PC.");
  });

  it("explains why a file could not be opened, without internal URLs", () => {
    const missing = Object.assign(new Error('Missing PDF "http://asset.localhost/C%3A%5Cx.pdf?t=1".'), { name: "MissingPDFException" });
    expect(describeLoadError(missing)).toBe("The file was not found. It may have been moved, renamed or deleted.");
    const invalid = Object.assign(new Error("Invalid PDF structure."), { name: "InvalidPDFException" });
    expect(describeLoadError(invalid)).toBe("The file is not a valid PDF or is damaged.");
    expect(describeLoadError({ name: "PasswordException", message: "No password given" }))
      .toBe("The PDF is password-protected. Open it in PDF Rider and print it from there.");
    expect(describeLoadError("disk error")).toBe("The file could not be opened (disk error).");
  });
});
