// ── Colour ────────────────────────────────────────────────────────────────────

export interface RgbColor {
  r: number; // 0–255
  g: number;
  b: number;
}

export function hexToRgb(hex: string): RgbColor {
  const n = parseInt(hex.replace("#", ""), 16);
  return { r: (n >> 16) & 255, g: (n >> 8) & 255, b: n & 255 };
}

export function rgbToHex(c: RgbColor): string {
  return (
    "#" +
    [c.r, c.g, c.b]
      .map((v) => v.toString(16).padStart(2, "0"))
      .join("")
  );
}

export function rgbToCss(c: RgbColor): string {
  return `rgb(${c.r},${c.g},${c.b})`;
}

// ── Annotations ───────────────────────────────────────────────────────────────

interface BaseAnnotation {
  /** 1-indexed page number */
  page: number;
  /** Left edge in PDF points (bottom-left origin) */
  x: number;
  /** Bottom edge in PDF points */
  y: number;
}

export interface RectAnnotation extends BaseAnnotation {
  kind: "rect";
  width: number;
  height: number;
  color: RgbColor;
  strokeWidth: number;
}

export interface CircleAnnotation extends BaseAnnotation {
  kind: "circle";
  width: number;
  height: number;
  color: RgbColor;
  strokeWidth: number;
}

export type TextAlignmentValue = "left" | "center" | "right";

export interface TextAnnotation extends BaseAnnotation {
  kind: "text";
  /** Bounding box width in PDF points (used for alignment and word-wrap) */
  width: number;
  content: string;
  color: RgbColor;
  fontSize: number;
  bold: boolean;
  italic: boolean;
  underline: boolean;
  alignment: TextAlignmentValue;
}

export interface SignatureAnnotation extends BaseAnnotation {
  kind: "signature";
  width: number;
  height: number;
  /** Base64-encoded PNG (no data: URI prefix) */
  imageData: string;
}

export type Annotation =
  | RectAnnotation
  | CircleAnnotation
  | TextAnnotation
  | SignatureAnnotation;

// ── Form fields ───────────────────────────────────────────────────────────────

export interface FormFieldValue {
  /** Full PDF field name (dotted path for nested fields). */
  name: string;
  /** Text value, or "true"/"false" for checkboxes, or export value for radios. */
  value: string;
}

// ── Tool state ────────────────────────────────────────────────────────────────

export type ToolKind = "select" | "rect" | "circle" | "text" | "signature";

export interface ActiveToolState {
  tool: ToolKind;
  color: RgbColor;
  fontSize: number;
  bold: boolean;
  italic: boolean;
  underline: boolean;
  alignment: TextAlignmentValue;
  strokeWidth: number;
}

export function defaultToolState(): ActiveToolState {
  return {
    tool: "select",
    color: { r: 0, g: 0, b: 0 },
    fontSize: 14,
    bold: false,
    italic: false,
    underline: false,
    alignment: "left",
    strokeWidth: 1.5,
  };
}

// ── Document state ────────────────────────────────────────────────────────────

export interface DocumentState {
  filePath: string | null;
  outputPath: string | null;
  pageCount: number;
  currentPage: number;
  zoomLevel: number;
  isDirty: boolean;
}

// ── Coordinate conversion ─────────────────────────────────────────────────────

/**
 * Convert canvas pixel coordinates (top-left origin, Y down) to PDF user-space
 * points (bottom-left origin, Y up).
 */
export function canvasToPdf(
  canvasX: number,
  canvasY: number,
  scale: number,
  pageHeightPt: number
): { x: number; y: number } {
  return {
    x: canvasX / scale,
    y: pageHeightPt - canvasY / scale,
  };
}

/**
 * Convert PDF user-space points to canvas pixel coordinates.
 */
export function pdfToCanvas(
  pdfX: number,
  pdfY: number,
  scale: number,
  pageHeightPt: number
): { x: number; y: number } {
  return {
    x: pdfX * scale,
    y: (pageHeightPt - pdfY) * scale,
  };
}
