import { describe, expect, it } from "vitest";
import { isPlainHttp } from "./plainHttp";

describe("isPlainHttp", () => {
  it("flags an unencrypted address", () => {
    expect(isPlainHttp("http://minio.local:9000")).toBe(true);
    expect(isPlainHttp("HTTP://192.168.1.10")).toBe(true);
    expect(isPlainHttp("  http://nas.home  ")).toBe(true);
  });

  it("says nothing about an encrypted one", () => {
    expect(isPlainHttp("https://s3.example.com")).toBe(false);
    expect(isPlainHttp("")).toBe(false);
  });

  it("does not fire on a host that merely starts with the letters", () => {
    expect(isPlainHttp("https://http.example.com")).toBe(false);
    expect(isPlainHttp("httpbin.org")).toBe(false);
  });
});
