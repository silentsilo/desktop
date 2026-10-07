import type { Key } from "./en";
import type { Locale } from "./locales";
import type { Screen, Translated } from "./types";
import { SCREENS } from "./screens";

export type { Translated } from "./types";
export type Translations = Partial<Record<Key, Translated>>;

const LANGS = ["ro", "de", "fr", "es", "it", "pt-BR", "pl"] as const;

/** Every language's texts, gathered from `screens/`. A key missing for a
 * language falls back to English, so a half-translated screen still says
 * everything. */
export const TRANSLATIONS = Object.fromEntries(
  LANGS.map((lang) => [
    lang,
    Object.fromEntries(
      (SCREENS as readonly Screen[]).flatMap((screen) =>
        Object.entries(screen).flatMap(([key, entry]) =>
          entry[lang] === undefined ? [] : [[key, entry[lang]]],
        ),
      ),
    ),
  ]),
) as Record<Exclude<Locale, "en">, Translations>;
