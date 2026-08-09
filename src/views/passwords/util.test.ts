import { describe, expect, it } from "vitest";
import { faviconUrl, normalizeUrl } from "./util";

describe("normalizeUrl", () => {
  it("assumes https for a bare host", () => {
    expect(normalizeUrl("example.com")).toBe("https://example.com");
    expect(normalizeUrl("example.com/login?a=b")).toBe("https://example.com/login?a=b");
  });

  it("keeps an explicit web scheme", () => {
    expect(normalizeUrl("https://example.com")).toBe("https://example.com");
    expect(normalizeUrl("http://example.com")).toBe("http://example.com");
    expect(normalizeUrl("HTTPS://EXAMPLE.COM")).toBe("HTTPS://EXAMPLE.COM");
  });

  it("refuses schemes that are not websites", () => {
    // Entries arrive from CSV and JSON exported by other apps, so this field
    // is untrusted input. The capability scope blocks these too; the point is
    // that the rule does not live only in configuration.
    expect(normalizeUrl("file:///etc/passwd")).toBeNull();
    expect(normalizeUrl("file://C:/Windows/System32/calc.exe")).toBeNull();
    expect(normalizeUrl("smb://server/share")).toBeNull();
    expect(normalizeUrl("javascript:alert(1)")).toBeNull();
    expect(normalizeUrl("data:text/html,<script>")).toBeNull();
  });

  it("treats an empty field as nothing to open", () => {
    expect(normalizeUrl("")).toBeNull();
    expect(normalizeUrl("   ")).toBeNull();
  });
});

describe("faviconUrl", () => {
  it("uses the host of a web address", () => {
    expect(faviconUrl("example.com")).toBe("https://example.com/favicon.ico");
  });

  it("fetches nothing for a scheme that is not a website", () => {
    expect(faviconUrl("file:///etc/passwd")).toBeNull();
  });

  it("leaves private and loopback hosts alone", () => {
    // Otherwise the favicon fetch becomes a way to probe the user's own LAN.
    expect(faviconUrl("http://localhost:9200")).toBeNull();
    expect(faviconUrl("http://192.168.1.1")).toBeNull();
    expect(faviconUrl("http://127.0.0.1")).toBeNull();
  });

  it("sees through the forms a dotted quad can be written in", () => {
    // The URL parser normalises all of these to 127.0.0.1 before the check
    // runs, which is worth pinning: it is the reason the check can be a
    // simple dotted-quad match.
    expect(faviconUrl("http://2130706433")).toBeNull();
    expect(faviconUrl("http://0x7f000001")).toBeNull();
    expect(faviconUrl("http://127.1")).toBeNull();
  });

  it("leaves private IPv6 alone, brackets and all", () => {
    // A URL's hostname keeps the brackets, so a check written against the
    // bare address matches nothing and every one of these reached the network.
    expect(faviconUrl("http://[::1]:9200")).toBeNull();
    expect(faviconUrl("http://[::]")).toBeNull();
    expect(faviconUrl("http://[fd00::1]")).toBeNull();
    expect(faviconUrl("http://[fe80::1]")).toBeNull();
    expect(faviconUrl("http://[::ffff:127.0.0.1]")).toBeNull();
  });

  it("still fetches for names that merely start like a private range", () => {
    // "fc"/"fd" are the first hextet of unique-local IPv6, not a prefix any
    // hostname should be judged by.
    expect(faviconUrl("fcbarcelona.com")).toBe("https://fcbarcelona.com/favicon.ico");
    expect(faviconUrl("fdn.fr")).toBe("https://fdn.fr/favicon.ico");
    expect(faviconUrl("[2606:4700::1111]")).toBe("https://[2606:4700::1111]/favicon.ico");
  });
});
