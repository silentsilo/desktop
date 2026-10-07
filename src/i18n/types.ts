/** English plural forms; other languages use their own categories. */
export type Plural = { one: string; other: string };

/** One translated text: a string, or plural forms by the language's own
 * categories (Romanian and Polish have "few", for instance). */
export type Translated = string | Partial<Record<Intl.LDMLPluralRule, string>>;

/**
 * One text, in every language, beside each other: the note says where it
 * appears and what it means, `en` is the source, and a language missing
 * here falls back to English.
 */
export type ScreenEntry = {
  note: string;
  en: string | Plural;
  ro?: Translated;
  de?: Translated;
  fr?: Translated;
  es?: Translated;
  it?: Translated;
  "pt-BR"?: Translated;
  pl?: Translated;
};

/** A screen's texts, keyed by where they appear. */
export type Screen = Record<string, ScreenEntry>;
