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
        const forms = typeof value === "string" ? { other: value } : (value ?? {});
        for (const [category, form] of Object.entries(forms)) {
          const found = placeholders(form ?? "");
          // "One of your copies": the singular may say the number in words,
          // but never brings a placeholder English lacks.
          if (category === "one") {
            for (const name of found) expect(placeholders(sourceText), `${lang}: ${key}`).toContain(name);
          } else {
            expect(found, `${lang}: ${key}`).toEqual(placeholders(sourceText));
          }
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

describe("plurals", () => {
  it("picks the form by the language's own rules", () => {
    const deleted = (lang: Parameters<typeof translate>[0], count: number) =>
      translate(lang, "trash.deleted", { count });
    expect(deleted("en", 1)).toBe("Deleted 1 item for good.");
    expect(deleted("en", 3)).toBe("Deleted 3 items for good.");
    expect(deleted("ro", 1)).toBe("1 element șters definitiv.");
    expect(deleted("ro", 3)).toBe("3 elemente șterse definitiv.");
    expect(deleted("ro", 25)).toBe("25 de elemente șterse definitiv.");
    expect(deleted("pl", 3)).toBe("Trwale usunięto 3 elementy.");
    expect(deleted("pl", 5)).toBe("Trwale usunięto 5 elementów.");
    expect(deleted("pl", 22)).toBe("Trwale usunięto 22 elementy.");
  });

  it("lets the singular say the number in words", () => {
    expect(translate("ro", "trash.archive_note", { count: 1 })).toMatch(/^Una dintre copiile tale/);
    expect(translate("ro", "trash.archive_note", { count: 2 })).toMatch(/^2 dintre copiile tale/);
  });
});
