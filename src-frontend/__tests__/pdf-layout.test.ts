import { describe, it, expect } from "vitest";
import { calculateColumnCount, buildRows } from "../pdf-viewer";

describe("calculateColumnCount", () => {
  it("returns 1 when only one page fits (800px container, 600px page)", () => {
    expect(calculateColumnCount(800, 600, 12)).toBe(1);
  });

  it("returns 2 when two pages fit (1300px container, 600px page)", () => {
    expect(calculateColumnCount(1300, 600, 12)).toBe(2);
  });

  it("returns 3 when three pages fit (1900px container, 600px page)", () => {
    expect(calculateColumnCount(1900, 600, 12)).toBe(3);
  });

  it("caps at 3 even when more pages could fit (3000px container)", () => {
    expect(calculateColumnCount(3000, 600, 12)).toBe(3);
  });

  it("returns 1 as minimum when container is smaller than page (400px container)", () => {
    expect(calculateColumnCount(400, 600, 12)).toBe(1);
  });

  it("returns 1 when container equals page width plus gap minus 1", () => {
    // 600 + 12 - 1 = 611 container; floor(623/612) = 1
    expect(calculateColumnCount(611, 600, 12)).toBe(1);
  });

  it("returns 2 when container exactly fits two pages with one gap", () => {
    // 2 pages: (2 * 600) + (1 * 12) = 1212
    expect(calculateColumnCount(1212, 600, 12)).toBe(2);
  });
});

describe("buildRows", () => {
  it("groups 5 pages into pairs with a trailing single page", () => {
    expect(buildRows(5, 2)).toEqual([[1, 2], [3, 4], [5]]);
  });

  it("groups 6 pages into triplets evenly", () => {
    expect(buildRows(6, 3)).toEqual([[1, 2, 3], [4, 5, 6]]);
  });

  it("handles single page with 2-column layout", () => {
    expect(buildRows(1, 2)).toEqual([[1]]);
  });

  it("stacks 4 pages in a single column", () => {
    expect(buildRows(4, 1)).toEqual([[1], [2], [3], [4]]);
  });

  it("handles 0 pages", () => {
    expect(buildRows(0, 2)).toEqual([]);
  });

  it("handles exactly matching page count for column count (4 pages, 2 columns)", () => {
    expect(buildRows(4, 2)).toEqual([[1, 2], [3, 4]]);
  });
});
