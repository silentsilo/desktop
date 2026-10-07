/** The languages the app speaks, in the picker's order: alphabetical by
 * each language's own name. */
export const LOCALES = [
  { id: "de", name: "Deutsch", date: "de-DE", reviewed: false },
  { id: "en", name: "English", date: "en-GB", reviewed: true },
  { id: "es", name: "Español", date: "es-ES", reviewed: false },
  { id: "fr", name: "Français", date: "fr-FR", reviewed: false },
  { id: "it", name: "Italiano", date: "it-IT", reviewed: false },
  { id: "pl", name: "Polski", date: "pl-PL", reviewed: false },
  { id: "pt-BR", name: "Português (Brasil)", date: "pt-BR", reviewed: false },
  { id: "ro", name: "Română", date: "ro-RO", reviewed: false },
] as const;

export type Locale = (typeof LOCALES)[number]["id"];

/**
 * The language to use for a preference: an explicit choice, or the first of
 * the system's languages the app has, else English. "pt" alone means
 * Brazilian Portuguese, the only Portuguese here.
 */
export function resolveLocale(preference: string | null, system: readonly string[]): Locale {
  const known = (id: string): Locale | null => {
    const lower = id.toLowerCase();
    const exact = LOCALES.find((l) => l.id.toLowerCase() === lower);
    if (exact) return exact.id;
    const base = lower.split("-")[0];
    if (base === "pt") return "pt-BR";
    return LOCALES.find((l) => l.id === base)?.id ?? null;
  };
  if (preference && preference !== "system") {
    const chosen = known(preference);
    if (chosen) return chosen;
  }
  for (const id of system) {
    const found = known(id);
    if (found) return found;
  }
  return "en";
}
