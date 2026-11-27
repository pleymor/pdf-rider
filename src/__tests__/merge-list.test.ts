import { describe, it, expect } from "vitest";
import { MergeList, fileNameOf, defaultMergedName } from "../merge-list";

function listOf(...paths: string[]): MergeList {
  const l = new MergeList();
  l.add(paths);
  return l;
}

describe("MergeList", () => {
  it("keeps files in the order they were added, duplicates included", () => {
    const l = listOf("C:\\a.pdf", "C:\\b.pdf");
    l.add(["C:\\a.pdf"]);
    expect(l.paths()).toEqual(["C:\\a.pdf", "C:\\b.pdf", "C:\\a.pdf"]);
  });

  it("starts every new file as being checked", () => {
    const l = listOf("a.pdf");
    expect(l.items[0].status).toEqual({ state: "checking" });
  });

  it("moves a file up and down, ignoring moves past either end", () => {
    const l = listOf("a.pdf", "b.pdf", "c.pdf");
    const [a, , c] = l.items;
    l.move(c.id, -1);
    expect(l.paths()).toEqual(["a.pdf", "c.pdf", "b.pdf"]);
    l.move(a.id, -1);
    expect(l.paths()).toEqual(["a.pdf", "c.pdf", "b.pdf"]);
    l.move(a.id, 1);
    expect(l.paths()).toEqual(["c.pdf", "a.pdf", "b.pdf"]);
    l.move(l.items[2].id, 1);
    expect(l.paths()).toEqual(["c.pdf", "a.pdf", "b.pdf"]);
  });

  it("moves a file to a given position (drag)", () => {
    const l = listOf("a.pdf", "b.pdf", "c.pdf", "d.pdf");
    l.moveTo(l.items[0].id, 2);
    expect(l.paths()).toEqual(["b.pdf", "c.pdf", "a.pdf", "d.pdf"]);
    l.moveTo(l.items[3].id, 0);
    expect(l.paths()).toEqual(["d.pdf", "b.pdf", "c.pdf", "a.pdf"]);
    l.moveTo(l.items[1].id, 99);
    expect(l.paths()).toEqual(["d.pdf", "c.pdf", "a.pdf", "b.pdf"]);
  });

  it("removes a file", () => {
    const l = listOf("a.pdf", "b.pdf");
    l.remove(l.items[0].id);
    expect(l.paths()).toEqual(["b.pdf"]);
  });

  it("is ready only with two or more checked, valid files", () => {
    const l = listOf("a.pdf");
    expect(l.readiness()).toBe("too-few");
    l.add(["b.pdf"]);
    expect(l.readiness()).toBe("checking");
    l.setStatus(l.items[0].id, { state: "ok", pages: 3 });
    l.setStatus(l.items[1].id, { state: "error", message: "b.pdf is encrypted" });
    expect(l.readiness()).toBe("has-errors");
    l.setStatus(l.items[1].id, { state: "ok", pages: 2 });
    expect(l.readiness()).toBe("ready");
    expect(l.totalPages()).toBe(5);
  });

  it("ignores status updates for a removed file", () => {
    const l = listOf("a.pdf", "b.pdf");
    const id = l.items[0].id;
    l.remove(id);
    l.setStatus(id, { state: "ok", pages: 1 });
    expect(l.items.map(i => i.status.state)).toEqual(["checking"]);
  });
});

describe("file names", () => {
  it("takes the last segment of Windows and POSIX paths", () => {
    expect(fileNameOf("C:\\Users\\me\\Report.pdf")).toBe("Report.pdf");
    expect(fileNameOf("/home/me/report.pdf")).toBe("report.pdf");
    expect(fileNameOf("plain.pdf")).toBe("plain.pdf");
  });

  it("names the merged file after the first file", () => {
    expect(defaultMergedName(["C:\\docs\\Invoice 12.PDF", "b.pdf"])).toBe("Invoice 12-merged.pdf");
    expect(defaultMergedName([])).toBe("merged.pdf");
  });
});
