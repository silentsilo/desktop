import { Fragment, createElement, useSyncExternalStore, type ReactNode } from "react";
import { en, type Key, type Plural } from "./en";
import { LOCALES, resolveLocale, type Locale } from "./locales";
import { TRANSLATIONS } from "./translations";

export type { Key } from "./en";
export { LOCALES, type Locale } from "./locales";

const STORAGE_KEY = "silentsilo.language";

function readPreference(): string | null {
  try {
    return localStorage.getItem(STORAGE_KEY);
  } catch {
    return null;
  }
}

function systemLanguages(): readonly string[] {
  return typeof navigator === "undefined" ? [] : (navigator.languages ?? [navigator.language]);
}

let current: Locale = resolveLocale(readPreference(), systemLanguages());
const listeners = new Set<() => void>();
// Screen readers pick their voice from it.
if (typeof document !== "undefined") document.documentElement.lang = current;

/** The language in use now. */
export function locale(): Locale {
  return current;
}

/** What the user chose: a language, or "system". */
export function languagePreference(): string {
  return readPreference() ?? "system";
}

/** The language the system's own settings give, for the picker's label. */
export function systemLocale(): Locale {
  return resolveLocale(null, systemLanguages());
}

/** Chooses a language, or "system" to follow the operating system. */
export function setLanguage(preference: string) {
  try {
    if (preference === "system") localStorage.removeItem(STORAGE_KEY);
    else localStorage.setItem(STORAGE_KEY, preference);
  } catch {
    // Kept for this session only.
  }
  current = resolveLocale(preference, systemLanguages());
  if (typeof document !== "undefined") document.documentElement.lang = current;
  for (const listener of listeners) listener();
}

/** Re-renders a component when the language changes. */
export function useLocale(): Locale {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => current,
    () => current,
  );
}

/** The locale dates and numbers are written in for the language in use. */
export function dateLocale(): string {
  return LOCALES.find((l) => l.id === current)?.date ?? "en-GB";
}

type Params = Record<string, string | number>;

/**
 * The text for `key` in the language in use, with `{name}` filled from
 * `params`. A text with plural forms is chosen by `params.count`. A key not
 * translated yet falls back to English.
 */
export function t(key: Key, params: Params = {}): string {
  return translate(current, key, params);
}

export function translate(lang: Locale, key: Key, params: Params = {}): string {
  const source = en[key].text as string | Plural;
  const local = lang === "en" ? undefined : TRANSLATIONS[lang][key];
  let text: string;
  if (typeof source === "string") {
    text = typeof local === "string" ? local : source;
  } else {
    const count = Number(params.count ?? 0);
    const category = new Intl.PluralRules(lang).select(count);
    const forms = (local && typeof local === "object" ? local : undefined) ?? undefined;
    text =
      forms?.[category] ??
      forms?.other ??
      source[new Intl.PluralRules("en").select(count) as keyof Plural] ??
      source.other;
  }
  return text.replace(/\{(\w+)\}/g, (whole, name: string) =>
    name in params ? String(params[name]) : whole,
  );
}

/**
 * [`t`] for a text with markup inside: each `{name}` whose value is a React
 * node is placed where the language puts it, so a bold silo name does not
 * force English word order on every translation.
 */
export function tx(key: Key, params: Record<string, ReactNode>): ReactNode {
  const plain: Params = {};
  for (const [name, value] of Object.entries(params)) {
    if (typeof value === "string" || typeof value === "number") plain[name] = value;
  }
  const text = translate(current, key, plain);
  const parts = text.split(/\{(\w+)\}/);
  return createElement(
    Fragment,
    null,
    ...parts.map((part, i) =>
      i % 2 === 1 && part in params
        ? createElement(Fragment, { key: i }, params[part])
        : part,
    ),
  );
}
