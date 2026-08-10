import { describe, expect, it } from "vitest";
import {
  CsvImportError,
  csvToEntries,
  detectFormat,
  entriesToCsv,
  parseCsv,
} from "./passwordCsv";
import type { PasswordEntry } from "./types";

function entry(overrides: Partial<PasswordEntry> = {}): PasswordEntry {
  return {
    id: "id-1",
    service: "Example",
    username: "user@example.com",
    password: "hunter2",
    url: "https://example.com",
    notes: "",
    category: "General",
    created_at: 0,
    updated_at: 0,
    ...overrides,
  };
}

describe("parseCsv", () => {
  it("parses a plain row", () => {
    expect(parseCsv("a,b,c\n1,2,3")).toEqual([
      ["a", "b", "c"],
      ["1", "2", "3"],
    ]);
  });

  it("keeps commas inside quoted fields", () => {
    expect(parseCsv('name,note\nSite,"one, two"')).toEqual([
      ["name", "note"],
      ["Site", "one, two"],
    ]);
  });

  it("unescapes doubled quotes", () => {
    expect(parseCsv('a\n"say ""hi"""')).toEqual([["a"], ['say "hi"']]);
  });

  it("keeps newlines inside quoted fields", () => {
    // Multi-line notes are common in real exports and are the single most
    // likely thing to break a naive line-splitting parser.
    expect(parseCsv('note\n"line one\nline two"')).toEqual([["note"], ["line one\nline two"]]);
  });

  it("handles CRLF line endings", () => {
    expect(parseCsv("a,b\r\n1,2\r\n")).toEqual([
      ["a", "b"],
      ["1", "2"],
    ]);
  });

  it("strips a UTF-8 BOM from the first header", () => {
    // Excel writes one, and it would otherwise corrupt the first column
    // name and so defeat format detection.
    expect(parseCsv("﻿name,password\nA,B")[0][0]).toBe("name");
  });

  it("preserves significant whitespace only inside quotes", () => {
    expect(parseCsv('a,b\n  x  ,"  y  "')).toEqual([
      ["a", "b"],
      ["x", "  y  "],
    ]);
  });

  it("keeps empty trailing fields", () => {
    expect(parseCsv("a,b,c\n1,,")).toEqual([
      ["a", "b", "c"],
      ["1", "", ""],
    ]);
  });

  it("skips blank lines", () => {
    expect(parseCsv("a\n\n1\n")).toEqual([["a"], ["1"]]);
  });

  it("returns nothing for empty input", () => {
    expect(parseCsv("")).toEqual([]);
  });
});

describe("detectFormat", () => {
  it("recognises Bitwarden", () => {
    const headers = "folder,favorite,type,name,notes,fields,reprompt,login_uri,login_username,login_password,login_totp".split(",");
    expect(detectFormat(headers)).toBe("bitwarden");
  });

  it("recognises LastPass", () => {
    expect(detectFormat(["url", "username", "password", "totp", "extra", "name", "grouping", "fav"])).toBe("lastpass");
  });

  it("recognises 1Password", () => {
    expect(
      detectFormat(["Title", "Url", "Username", "Password", "OTPAuth", "Favorite", "Archived", "Tags", "Notes"]),
    ).toBe("onepassword");
  });

  it("recognises Apple Passwords by otpauth without 1Password's extras", () => {
    expect(detectFormat(["Title", "URL", "Username", "Password", "Notes", "OTPAuth"])).toBe("apple");
  });

  it("recognises Proton Pass", () => {
    expect(
      detectFormat(["type", "name", "url", "email", "username", "password", "note", "totp", "vault"]),
    ).toBe("protonpass");
  });

  it("recognises Dashlane", () => {
    expect(
      detectFormat(["username", "username2", "username3", "title", "password", "note", "url", "category", "otpSecret"]),
    ).toBe("dashlane");
  });

  it("recognises NordPass", () => {
    expect(
      detectFormat(["name", "url", "username", "password", "note", "cardholdername", "cardnumber", "cvc", "folder"]),
    ).toBe("nordpass");
  });

  it("recognises KeePass", () => {
    expect(
      detectFormat(["Group", "Title", "Username", "Password", "URL", "Notes", "TOTP", "Icon", "Last Modified", "Created"]),
    ).toBe("keepass");
  });

  it("recognises RoboForm", () => {
    expect(detectFormat(["Name", "Url", "MatchUrl", "Login", "Pwd", "Note", "Folder", "RfFieldsV2"])).toBe("roboform");
  });

  it("recognises Firefox", () => {
    expect(
      detectFormat(["url", "username", "password", "httpRealm", "formActionOrigin", "guid", "timeCreated"]),
    ).toBe("firefox");
  });

  it("recognises Chrome", () => {
    expect(detectFormat(["name", "url", "username", "password", "note"])).toBe("chrome");
  });

  it("falls back to generic for an unknown exporter", () => {
    expect(detectFormat(["title", "login", "password", "comments"])).toBe("generic");
  });
});

describe("csvToEntries", () => {
  const clock = () => 1_700_000_000_000;

  it("imports a Bitwarden export", () => {
    const csv = [
      "folder,favorite,type,name,notes,fields,reprompt,login_uri,login_username,login_password,login_totp",
      "Social,0,login,Twitter,my note,,0,https://twitter.com,alice,s3cret,",
    ].join("\n");

    const result = csvToEntries(csv, clock);
    expect(result.format).toBe("bitwarden");
    expect(result.entries).toHaveLength(1);
    expect(result.entries[0]).toMatchObject({
      service: "Twitter",
      username: "alice",
      password: "s3cret",
      url: "https://twitter.com",
      notes: "my note",
      category: "Social",
    });
  });

  it("imports a LastPass export", () => {
    const csv = [
      "url,username,password,totp,extra,name,grouping,fav",
      "https://bank.com,bob,pw123,,some notes,Bank,Finance,0",
    ].join("\n");

    const result = csvToEntries(csv, clock);
    expect(result.format).toBe("lastpass");
    expect(result.entries[0]).toMatchObject({
      service: "Bank",
      username: "bob",
      password: "pw123",
      notes: "some notes",
      category: "Finance",
    });
  });

  it("imports a Chrome export", () => {
    const csv = ["name,url,username,password,note", "GitHub,https://github.com,dev,gh-pass,"].join("\n");
    const result = csvToEntries(csv, clock);
    expect(result.format).toBe("chrome");
    expect(result.entries[0]).toMatchObject({ service: "GitHub", password: "gh-pass" });
  });

  it("reads a TOTP secret from an otpauth URI", () => {
    const csv = [
      "Title,Url,Username,Password,OTPAuth,Notes",
      "Vault,https://v.com,carol,pw,otpauth://totp/Vault:carol?secret=JBSWY3DPEHPK3PXP&digits=8&period=60&algorithm=SHA256,",
    ].join("\n");

    const [imported] = csvToEntries(csv, clock).entries;
    expect(imported.totp_secret).toBe("JBSWY3DPEHPK3PXP");
    expect(imported.totp_digits).toBe(8);
    expect(imported.totp_period).toBe(60);
    expect(imported.totp_algorithm).toBe("SHA-256");
  });

  it("reads a bare base32 TOTP secret", () => {
    const csv = ["name,url,username,password,totp", "X,https://x.com,u,p,JBSWY3DPEHPK3PXP"].join("\n");
    expect(csvToEntries(csv, clock).entries[0].totp_secret).toBe("JBSWY3DPEHPK3PXP");
  });

  it("skips rows with no password and no TOTP", () => {
    // Bitwarden interleaves secure notes with logins; they have no password
    // and must not land in the vault as blank entries.
    const csv = [
      "folder,type,name,notes,login_uri,login_username,login_password,login_totp",
      "Personal,note,A Secure Note,just text,,,,",
      "Personal,login,Real Login,,https://a.com,alice,pw,",
    ].join("\n");

    const result = csvToEntries(csv, clock);
    expect(result.entries).toHaveLength(1);
    expect(result.entries[0].service).toBe("Real Login");
    expect(result.skipped).toBe(1);
  });

  it("keeps a row that has only a TOTP secret", () => {
    const csv = ["name,url,username,password,totp", "Authy,https://a.com,u,,JBSWY3DPEHPK3PXP"].join("\n");
    expect(csvToEntries(csv, clock).entries).toHaveLength(1);
  });

  it("falls back to url then username when the name is blank", () => {
    const csv = ["name,url,username,password", ",https://nameless.com,u,p"].join("\n");
    expect(csvToEntries(csv, clock).entries[0].service).toBe("https://nameless.com");
  });

  it("defaults a missing category to General", () => {
    const csv = ["name,url,username,password", "A,https://a.com,u,p"].join("\n");
    expect(csvToEntries(csv, clock).entries[0].category).toBe("General");
  });

  it("imports a Proton Pass export with the vault as the category", () => {
    // The `type` column is the item kind, not a category: filing every row
    // under "login" is the mistake this asserts against.
    const csv = [
      "type,name,url,email,username,password,note,totp,vault",
      "login,Gitlab,https://gitlab.com,a@b.co,alex,pw1,,otpauth://totp/x?secret=JBSWY3DPEHPK3PXP,Work",
      "note,Ideas,,,,,just text,,Work",
    ].join("\n");

    const result = csvToEntries(csv, clock);
    expect(result.format).toBe("protonpass");
    expect(result.entries).toHaveLength(1);
    expect(result.entries[0].category).toBe("Work");
    expect(result.entries[0].totp_secret).toBe("JBSWY3DPEHPK3PXP");
    expect(result.skipped).toBe(1);
  });

  it("imports a Dashlane export, otpSecret included", () => {
    const csv = [
      "username,username2,username3,title,password,note,url,category,otpSecret",
      "alex,,,Bank,pw2,a note,https://bank.ro,Finance,JBSWY3DPEHPK3PXP",
    ].join("\n");

    const result = csvToEntries(csv, clock);
    expect(result.format).toBe("dashlane");
    expect(result.entries[0].service).toBe("Bank");
    expect(result.entries[0].category).toBe("Finance");
    expect(result.entries[0].totp_secret).toBe("JBSWY3DPEHPK3PXP");
  });

  it("imports a NordPass export, skipping the cards", () => {
    const csv = [
      "name,url,username,password,note,cardholdername,cardnumber,cvc,expirydate,zipcode,folder",
      "Shop,https://shop.com,alex,pw3,,,,,,,Shopping",
      "My Visa,,,,,ALEX S,4111111111111111,123,12/28,,Wallet",
    ].join("\n");

    const result = csvToEntries(csv, clock);
    expect(result.format).toBe("nordpass");
    expect(result.entries).toHaveLength(1);
    expect(result.entries[0].category).toBe("Shopping");
    expect(result.skipped).toBe(1);
  });

  it("imports a KeePass export with the group as the category", () => {
    const csv = [
      "Group,Title,Username,Password,URL,Notes,TOTP",
      "Root/Email,Gmail,alex,pw4,https://mail.google.com,imap note,JBSWY3DPEHPK3PXP",
    ].join("\n");

    const result = csvToEntries(csv, clock);
    expect(result.format).toBe("keepass");
    expect(result.entries[0].category).toBe("Root/Email");
    expect(result.entries[0].totp_secret).toBe("JBSWY3DPEHPK3PXP");
  });

  it("imports a RoboForm export through the Login and Pwd columns", () => {
    const csv = [
      "Name,Url,MatchUrl,Login,Pwd,Note,Folder,RfFieldsV2",
      "Forum,https://forum.net,https://forum.net,alex,pw5,,Personal,",
    ].join("\n");

    const result = csvToEntries(csv, clock);
    expect(result.format).toBe("roboform");
    expect(result.entries[0].username).toBe("alex");
    expect(result.entries[0].password).toBe("pw5");
    expect(result.entries[0].category).toBe("Personal");
  });

  it("imports a Firefox export, naming entries by their url", () => {
    const csv = [
      "url,username,password,httpRealm,formActionOrigin,guid,timeCreated,timeLastUsed,timePasswordChanged",
      "https://site.org,alex,pw6,,https://site.org,{1},1,2,3",
    ].join("\n");

    const result = csvToEntries(csv, clock);
    expect(result.format).toBe("firefox");
    expect(result.entries[0].service).toBe("https://site.org");
    expect(result.entries[0].password).toBe("pw6");
  });

  it("imports an Apple Passwords export", () => {
    const csv = [
      "Title,URL,Username,Password,Notes,OTPAuth",
      "iCloud,https://icloud.com,alex@me.com,pw7,personal,otpauth://totp/x?secret=JBSWY3DPEHPK3PXP",
    ].join("\n");

    const result = csvToEntries(csv, clock);
    expect(result.format).toBe("apple");
    expect(result.entries[0].totp_secret).toBe("JBSWY3DPEHPK3PXP");
  });

  it("gives every imported entry a distinct id", () => {
    const csv = ["name,url,username,password", "A,https://a.com,u,p", "B,https://b.com,u,p"].join("\n");
    const { entries } = csvToEntries(csv, clock);
    expect(new Set(entries.map((e) => e.id)).size).toBe(2);
  });

  it("rejects a file with no password column", () => {
    expect(() => csvToEntries("name,url\nA,https://a.com", clock)).toThrow(CsvImportError);
  });

  it("rejects an empty file", () => {
    expect(() => csvToEntries("", clock)).toThrow(CsvImportError);
  });

  it("rejects a file whose rows are all unimportable", () => {
    const csv = ["name,url,username,password", "Note,,,"].join("\n");
    expect(() => csvToEntries(csv, clock)).toThrow(CsvImportError);
  });
});

describe("entriesToCsv", () => {
  it("writes a header even with no entries", () => {
    expect(entriesToCsv([])).toBe("name,url,username,password,totp,category,note\n");
  });

  it("round-trips through the importer", () => {
    const original = entry({
      service: "Round Trip",
      notes: 'has "quotes", a comma and\na newline',
      totp_secret: "JBSWY3DPEHPK3PXP",
    });

    const [reimported] = csvToEntries(entriesToCsv([original])).entries;
    expect(reimported).toMatchObject({
      service: original.service,
      username: original.username,
      password: original.password,
      url: original.url,
      notes: original.notes,
      category: original.category,
      totp_secret: original.totp_secret,
    });
  });

  it("quotes fields containing separators", () => {
    const csv = entriesToCsv([entry({ service: "a,b", notes: 'say "hi"' })]);
    expect(csv).toContain('"a,b"');
    expect(csv).toContain('"say ""hi"""');
  });

  it("neutralises spreadsheet formula injection", () => {
    // A password beginning with = would be evaluated as a formula when the
    // exported file is opened in Excel or Sheets.
    const csv = entriesToCsv([entry({ password: "=1+1" })]);
    expect(csv).toContain("'=1+1");
  });

  it("ends with a newline", () => {
    expect(entriesToCsv([entry()]).endsWith("\n")).toBe(true);
  });
});
