import type { Translations } from "./i18n";
import { listPrinters } from "./tauri-bridge";

export interface PrintSettings {
  printerName: string;
  pageRange: number[];
  copies: number;
  orientation: "portrait" | "landscape";
  fitMode: "fit" | "actual";
}

type PrintHandler = (settings: PrintSettings) => void;

export class PrintModal {
  private backdrop: HTMLElement;
  private printerSelect: HTMLSelectElement;
  private rangeCustomInput: HTMLInputElement;
  private copiesInput: HTMLInputElement;
  private printBtn: HTMLButtonElement;
  private noPrintersMsg: HTMLElement;

  private _i18nText  = new Map<HTMLElement, keyof Translations>();
  private _i18nTitle = new Map<HTMLElement, keyof Translations>();
  private confirmHandlers: PrintHandler[] = [];

  private _pageCount = 1;
  private _currentPage = 1;

  constructor() {
    this.backdrop      = document.getElementById("print-modal")!;
    this.printerSelect = document.getElementById("print-printer-select") as HTMLSelectElement;
    this.rangeCustomInput = document.getElementById("print-range-input") as HTMLInputElement;
    this.copiesInput   = document.getElementById("print-copies") as HTMLInputElement;
    this.printBtn      = document.getElementById("print-confirm-btn") as HTMLButtonElement;
    this.noPrintersMsg = document.getElementById("print-no-printers")!;
    this.bind();
  }

  applyTranslations(t: Translations): void {
    this._i18nText.forEach((key, el)  => { el.textContent = t[key] ?? null; });
    this._i18nTitle.forEach((key, el) => { el.title = t[key] ?? ""; });
  }

  onConfirm(cb: PrintHandler): void {
    this.confirmHandlers.push(cb);
  }

  async open(pageCount: number, currentPage: number, dims: { w: number; h: number }): Promise<void> {
    this._pageCount = pageCount;
    this._currentPage = currentPage;

    // Reset state
    this.copiesInput.value = "1";
    (this.backdrop.querySelector("input[name='print-range'][value='all']") as HTMLInputElement).checked = true;
    this.rangeCustomInput.value = "";
    this.rangeCustomInput.disabled = true;
    (this.backdrop.querySelector("input[name='print-scale'][value='fit']") as HTMLInputElement).checked = true;

    // Auto-detect orientation from page dimensions
    const isLandscape = dims.w > dims.h;
    (this.backdrop.querySelector(`input[name='print-orient'][value='${isLandscape ? "landscape" : "portrait"}']`) as HTMLInputElement).checked = true;

    // Populate printers
    try {
      const { printers, defaultPrinter } = await listPrinters();
      this.printerSelect.innerHTML = "";
      if (printers.length === 0) {
        this.noPrintersMsg.classList.remove("hidden");
        this.printerSelect.style.display = "none";
        this.printBtn.disabled = true;
      } else {
        this.noPrintersMsg.classList.add("hidden");
        this.printerSelect.style.display = "";
        this.printBtn.disabled = false;
        for (const name of printers) {
          const opt = document.createElement("option");
          opt.value = name;
          opt.textContent = name;
          if (name === defaultPrinter) opt.selected = true;
          this.printerSelect.appendChild(opt);
        }
      }
    } catch {
      // Not running in Tauri (browser preview)
      this.noPrintersMsg.classList.remove("hidden");
      this.printerSelect.style.display = "none";
      this.printBtn.disabled = true;
    }

    this.backdrop.classList.remove("hidden");
  }

  close(): void {
    this.backdrop.classList.add("hidden");
  }

  private parsePageRange(): number[] {
    const rangeType = (this.backdrop.querySelector("input[name='print-range']:checked") as HTMLInputElement).value;
    if (rangeType === "all") {
      return Array.from({ length: this._pageCount }, (_, i) => i + 1);
    }
    if (rangeType === "current") {
      return [this._currentPage];
    }
    // Custom range
    const input = this.rangeCustomInput.value.trim();
    if (!input) return Array.from({ length: this._pageCount }, (_, i) => i + 1);

    const pages = new Set<number>();
    for (const token of input.split(",")) {
      const trimmed = token.trim();
      const rangeMatch = trimmed.match(/^(\d+)\s*-\s*(\d+)$/);
      if (rangeMatch) {
        const start = Math.max(1, parseInt(rangeMatch[1], 10));
        const end = Math.min(this._pageCount, parseInt(rangeMatch[2], 10));
        for (let i = start; i <= end; i++) pages.add(i);
      } else if (/^\d+$/.test(trimmed)) {
        const n = parseInt(trimmed, 10);
        if (n >= 1 && n <= this._pageCount) pages.add(n);
      }
    }
    return pages.size > 0
      ? Array.from(pages).sort((a, b) => a - b)
      : Array.from({ length: this._pageCount }, (_, i) => i + 1);
  }

  private bind(): void {
    // Close button
    document.getElementById("print-close-btn")!
      .addEventListener("click", () => this.close());

    // Cancel button
    document.getElementById("print-cancel-btn")!
      .addEventListener("click", () => this.close());

    // Close on backdrop click
    this.backdrop.addEventListener("click", (e) => {
      if (e.target === this.backdrop) this.close();
    });

    // Enable/disable custom range input based on radio selection
    const rangeRadios = this.backdrop.querySelectorAll<HTMLInputElement>("input[name='print-range']");
    rangeRadios.forEach(radio => {
      radio.addEventListener("change", () => {
        this.rangeCustomInput.disabled = radio.value !== "custom" || !radio.checked;
        if (radio.value === "custom" && radio.checked) {
          this.rangeCustomInput.focus();
        }
      });
    });

    // Print button
    this.printBtn.addEventListener("click", () => {
      const orientation = (this.backdrop.querySelector("input[name='print-orient']:checked") as HTMLInputElement).value as "portrait" | "landscape";
      const fitMode = (this.backdrop.querySelector("input[name='print-scale']:checked") as HTMLInputElement).value as "fit" | "actual";
      const settings: PrintSettings = {
        printerName: this.printerSelect.value,
        pageRange: this.parsePageRange(),
        copies: Math.max(1, parseInt(this.copiesInput.value, 10) || 1),
        orientation,
        fitMode,
      };
      this.close();
      this.confirmHandlers.forEach(h => h(settings));
    });

    // Register i18n targets
    const reg = (id: string, text?: keyof Translations, title?: keyof Translations) => {
      const el = document.getElementById(id);
      if (!el) return;
      if (text)  this._i18nText.set(el, text);
      if (title) this._i18nTitle.set(el, title);
    };
    reg("print-title-span",    "printTitle");
    reg("print-close-btn",     undefined, "printCancel");
    reg("print-printer-label", "printPrinter");
    reg("print-no-printers",   "printNoPrinters");
    reg("print-copies-label",  "printCopies");
    reg("print-orient-label",  "printOrientation");
    reg("print-scale-label",   "printScale");
    reg("print-cancel-btn",    "printCancelBtn");
    reg("print-confirm-btn",   "printConfirmBtn");
  }
}
