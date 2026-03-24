// ── Types ─────────────────────────────────────────────────────────────────────

export interface AppSettings {
  language: string; // BCP-47 code, e.g. "fr"
}

// ── Persistence ───────────────────────────────────────────────────────────────

const STORAGE_KEY = "pdf-reader-settings";
const DEFAULTS: AppSettings = { language: "fr" };

export function loadSettings(): AppSettings {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) return { ...DEFAULTS, ...(JSON.parse(raw) as Partial<AppSettings>) };
  } catch {
    // ignore parse errors
  }
  return { ...DEFAULTS };
}

function saveSettings(s: AppSettings): void {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(s));
}

// ── Language list (20 most spoken) ────────────────────────────────────────────

export const LANGUAGES: { code: string; native: string; label: string }[] = [
  { code: "en", native: "English",          label: "Anglais"           },
  { code: "zh", native: "中文",              label: "Chinois mandarin"  },
  { code: "hi", native: "हिन्दी",             label: "Hindi"             },
  { code: "es", native: "Español",          label: "Espagnol"          },
  { code: "fr", native: "Français",         label: "Français"          },
  { code: "ar", native: "العربية",           label: "Arabe"             },
  { code: "bn", native: "বাংলা",              label: "Bengali"           },
  { code: "ru", native: "Русский",           label: "Russe"             },
  { code: "pt", native: "Português",        label: "Portugais"         },
  { code: "ur", native: "اردو",              label: "Ourdou"            },
  { code: "id", native: "Bahasa Indonesia", label: "Indonésien"        },
  { code: "de", native: "Deutsch",          label: "Allemand"          },
  { code: "ja", native: "日本語",             label: "Japonais"          },
  { code: "sw", native: "Kiswahili",        label: "Swahili"           },
  { code: "mr", native: "मराठी",              label: "Marathi"           },
  { code: "te", native: "తెలుగు",             label: "Télougou"          },
  { code: "tr", native: "Türkçe",           label: "Turc"              },
  { code: "ta", native: "தமிழ்",              label: "Tamoul"            },
  { code: "vi", native: "Tiếng Việt",       label: "Vietnamien"        },
  { code: "ko", native: "한국어",             label: "Coréen"            },
];

// ── Modal ─────────────────────────────────────────────────────────────────────

export class SettingsModal {
  private backdrop: HTMLElement;
  private settings: AppSettings;
  private changeHandlers: Array<(s: AppSettings) => void> = [];

  constructor() {
    this.settings = loadSettings();
    this.backdrop = document.getElementById("settings-modal")!;
    this.populateLangSelect();
    this.bind();
  }

  getSettings(): AppSettings {
    return { ...this.settings };
  }

  onChange(cb: (s: AppSettings) => void): void {
    this.changeHandlers.push(cb);
  }

  open(): void  { this.backdrop.classList.remove("hidden"); }
  close(): void { this.backdrop.classList.add("hidden"); }

  private set<K extends keyof AppSettings>(key: K, value: AppSettings[K]): void {
    this.settings[key] = value;
    saveSettings(this.settings);
    this.changeHandlers.forEach(h => h({ ...this.settings }));
  }

  private populateLangSelect(): void {
    const sel = document.getElementById("settings-lang") as HTMLSelectElement;
    for (const lang of LANGUAGES) {
      const opt = document.createElement("option");
      opt.value = lang.code;
      opt.textContent = `${lang.native} — ${lang.label}`;
      sel.appendChild(opt);
    }
  }

  private bind(): void {
    document.getElementById("settings-close-btn")!
      .addEventListener("click", () => this.close());

    // Close on backdrop click
    this.backdrop.addEventListener("click", (e) => {
      if (e.target === this.backdrop) this.close();
    });

    // Language selector
    const langSelect = document.getElementById("settings-lang") as HTMLSelectElement;
    langSelect.value = this.settings.language;
    langSelect.addEventListener("change", () => {
      this.set("language", langSelect.value);
    });
  }
}
