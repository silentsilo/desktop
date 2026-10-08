import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { Segmented } from "./Segmented";
import { OptionTiles } from "./OptionTiles";

const noop = () => {};

describe("segmented control", () => {
  it("is a radio group with one tab stop on the picked answer", () => {
    const html = renderToStaticMarkup(
      <Segmented
        label="Sign in with"
        value="key"
        onChange={noop}
        options={[
          { value: "password", label: "Password" },
          { value: "key", label: "Private key" },
        ]}
      />,
    );
    expect(html).toContain('role="radiogroup"');
    expect(html).toContain('aria-label="Sign in with"');
    expect(html.match(/role="radio"/g)).toHaveLength(2);
    expect(html).toMatch(/aria-checked="true" tabindex="0"[^>]*>.*Private key/);
    expect(html.match(/tabindex="0"/g)).toHaveLength(1);
  });

  it("speaks as tabs when it switches what is shown", () => {
    const html = renderToStaticMarkup(
      <Segmented
        kind="tab"
        label="App settings"
        value="general"
        onChange={noop}
        options={[
          { value: "general", label: "General" },
          { value: "updates", label: "Updates" },
        ]}
      />,
    );
    expect(html).toContain('role="tablist"');
    expect(html).toContain('aria-selected="true"');
    expect(html).not.toContain("aria-checked");
  });
});

describe("option tiles", () => {
  it("marks the picked tile by role, state and a check", () => {
    const html = renderToStaticMarkup(
      <OptionTiles
        label="Where"
        value="s3"
        onChange={noop}
        options={[
          { value: "folder", title: "Folder", description: "A drive or a NAS" },
          { value: "s3", title: "S3 bucket" },
        ]}
      />,
    );
    expect(html).toContain('role="radiogroup"');
    expect(html.match(/role="radio"/g)).toHaveLength(2);
    expect(html).toContain('aria-checked="true"');
    expect(html).toContain("option-tile-check");
    expect(html).toContain("A drive or a NAS");
  });

  it("leaves the first tile reachable when nothing is picked", () => {
    const html = renderToStaticMarkup(
      <OptionTiles
        label="Accounts"
        value={null}
        onChange={noop}
        options={[
          { value: "a", title: "A" },
          { value: "b", title: "B" },
        ]}
      />,
    );
    expect(html.match(/tabindex="0"/g)).toHaveLength(1);
    expect(html).not.toContain('aria-checked="true"');
  });
});
