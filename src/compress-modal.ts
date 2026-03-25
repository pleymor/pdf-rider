import type { Translations } from "./i18n";

export type CompressLevel = "screen" | "ebook" | "print";

type ConfirmHandler = (level: CompressLevel) => void;

export class CompressModal {
  private backdrop: HTMLElement;
  private confirmHandlers: ConfirmHandler[] = [];
  private selectedLevel: CompressLevel = "ebook";

  private _i18nText  = new Map<HTMLElement, keyof Translations>();
  private _i18nTitle = new Map<HTMLElement, keyof Translations>();

  constructor() {
    this.backdrop = document.getElementById("compress-modal")!;
    this.bind();
  }

  applyTranslations(t: Translations): void {
    this._i18nText.forEach((key, el)  => { el.textContent = t[key] ?? null; });
    this._i18nTitle.forEach((key, el) => { el.title = t[key] ?? ""; });
  }

  onConfirm(cb: ConfirmHandler): void {
    this.confirmHandlers.push(cb);
  }

  open(): void  { this.backdrop.classList.remove("hidden"); }
  close(): void { this.backdrop.classList.add("hidden"); }

  private bind(): void {
    // Close button
    document.getElementById("compress-close-btn")!
      .addEventListener("click", () => this.close());

    // Close on backdrop click
    this.backdrop.addEventListener("click", (e) => {
      if (e.target === this.backdrop) this.close();
    });

    // Level radio buttons
    const radios = this.backdrop.querySelectorAll<HTMLInputElement>(
      "input[name='compress-level']"
    );
    radios.forEach(radio => {
      radio.addEventListener("change", () => {
        if (radio.checked) this.selectedLevel = radio.value as CompressLevel;
      });
    });

    // Apply button
    document.getElementById("compress-apply-btn")!
      .addEventListener("click", () => {
        const level = this.selectedLevel;
        this.close();
        this.confirmHandlers.forEach(h => h(level));
      });

    // Register i18n targets
    const reg = (id: string, text?: keyof Translations, title?: keyof Translations) => {
      const el = document.getElementById(id);
      if (!el) return;
      if (text)  this._i18nText.set(el, text);
      if (title) this._i18nTitle.set(el, title);
    };
    reg("compress-title-span",  "compressTitle");
    reg("compress-close-btn",   undefined, "compressClose");
    reg("compress-screen-label", "compressScreen");
    reg("compress-ebook-label",  "compressEbook");
    reg("compress-print-label",  "compressPrint");
    reg("compress-apply-btn",   "compressApply");
  }
}
