import { describe, expect, it } from "vitest";
import { en, type Key, type Plural } from "./en";
import { resolveLocale, LOCALES } from "./locales";
import { TRANSLATIONS } from "./translations";
import { renderToStaticMarkup } from "react-dom/server";
import { setLanguage, translate, tx } from "./index";

const placeholders = (text: string) => [...text.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();

describe("translations", () => {
  for (const [lang, table] of Object.entries(TRANSLATIONS)) {
    it(`${lang}: only keys English has, with the same placeholders`, () => {
      for (const [key, value] of Object.entries(table)) {
        expect(key in en, `${lang}: ${key} is not in en.ts`).toBe(true);
        const source = en[key as Key].text as string | Plural;
        const sourceText = typeof source === "string" ? source : source.other;
        const forms = typeof value === "string" ? [value] : Object.values(value ?? {});
        for (const form of forms) {
          expect(placeholders(form ?? ""), `${lang}: ${key}`).toEqual(placeholders(sourceText));
        }
      }
    });

    it(`${lang}: plural texts have every form the language needs`, () => {
      const needed = new Intl.PluralRules(lang).resolvedOptions().pluralCategories;
      for (const [key, value] of Object.entries(table)) {
        const source = en[key as Key].text as string | Plural;
        if (typeof source === "string") continue;
        expect(typeof value, `${lang}: ${key} needs plural forms`).toBe("object");
        for (const category of needed) {
          expect(value, `${lang}: ${key} lacks "${category}"`).toHaveProperty(category);
        }
      }
    });
  }

  it("every language the picker lists has a table, English aside", () => {
    for (const l of LOCALES) {
      if (l.id !== "en") expect(TRANSLATIONS).toHaveProperty(l.id);
    }
  });
});

describe("resolveLocale", () => {
  it("takes an explicit choice", () => {
    expect(resolveLocale("de", ["ro-RO"])).toBe("de");
  });
  it("follows the system when asked to, by base language", () => {
    expect(resolveLocale("system", ["fr-CA", "en-US"])).toBe("fr");
    expect(resolveLocale(null, ["pt-PT"])).toBe("pt-BR");
  });
  it("falls back to English for a language it does not have", () => {
    expect(resolveLocale(null, ["ja-JP"])).toBe("en");
  });
});

describe("translate", () => {
  it("fills placeholders and falls back to English for a missing key", () => {
    expect(translate("ro", "update.available", { version: "1.5.0" })).toBe(
      "SilentSilo 1.5.0 e disponibil",
    );
    expect(translate("en", "nav.files")).toBe("Files");
  });
});

describe("tx", () => {
  it("puts markup where the language puts the value", () => {
    setLanguage("ro");
    const html = renderToStaticMarkup(
      <p>{tx("recovery_new.body_named", { name: <strong>Personal</strong> })}</p>,
    );
    setLanguage("en");
    expect(html.startsWith("<p><strong>Personal</strong> are un cod de recuperare nou.")).toBe(true);
  });
});
