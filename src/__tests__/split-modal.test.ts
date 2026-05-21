import { describe, it, expect } from "vitest";
import { parseRanges } from "../split-modal";

describe("parseRanges", () => {
  it("returns [] for empty input", () => {
    expect(parseRanges("", 10)).toEqual([]);
    expect(parseRanges("   ", 10)).toEqual([]);
  });

  it("parses a single page", () => {
    expect(parseRanges("3", 10)).toEqual([{ start: 3, end: 3 }]);
  });

  it("parses a single range", () => {
    expect(parseRanges("1-5", 10)).toEqual([{ start: 1, end: 5 }]);
  });

  it("parses multiple ranges with spaces", () => {
    expect(parseRanges(" 1-3 , 5 , 7-9 ", 10)).toEqual([
      { start: 1, end: 3 },
      { start: 5, end: 5 },
      { start: 7, end: 9 },
    ]);
  });

  it("rejects out-of-bounds pages", () => {
    expect(parseRanges("0-3", 10)).toBeNull();
    expect(parseRanges("8-12", 10)).toBeNull();
    expect(parseRanges("11", 10)).toBeNull();
  });

  it("rejects reversed ranges", () => {
    expect(parseRanges("5-3", 10)).toBeNull();
  });

  it("rejects non-numeric tokens", () => {
    expect(parseRanges("foo", 10)).toBeNull();
    expect(parseRanges("1-bar", 10)).toBeNull();
    expect(parseRanges("1,,3", 10)).toBeNull();
  });

  it("rejects non-integer values", () => {
    expect(parseRanges("1.5-3", 10)).toBeNull();
  });
});
