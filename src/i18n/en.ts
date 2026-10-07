/**
 * The source catalog, gathered from `screens/`: every text the app shows,
 * in English, with a note on where it appears and what it means.
 * Translations are made with the note and the screen in front, never word
 * for word; see README.md for the glossary.
 *
 * `{name}` marks a value filled in; an entry with plural forms is chosen by
 * its `count`. Keys are named by where the text appears.
 */
import type { Plural, Screen } from "./types";
import { SCREENS } from "./screens";

export type { Plural } from "./types";
export type Entry = { text: string | Plural; note: string };

type AllScreens = (typeof SCREENS)[number];
type UnionToIntersection<U> = (U extends unknown ? (x: U) => void : never) extends (
  x: infer I,
) => void
  ? I
  : never;
type Catalog = UnionToIntersection<AllScreens>;

export type Key = keyof Catalog & string;

export const en = Object.fromEntries(
  (SCREENS as readonly Screen[]).flatMap((screen) =>
    Object.entries(screen).map(([key, entry]) => [key, { text: entry.en, note: entry.note }]),
  ),
) as Record<Key, Entry>;
