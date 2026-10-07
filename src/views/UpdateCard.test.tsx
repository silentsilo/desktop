import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import type { Update } from "@tauri-apps/plugin-updater";
import { UpdateCard } from "./UpdateCard";

const card = () =>
  renderToStaticMarkup(
    <UpdateCard
      version="9.1.0"
      update={{} as Update}
      onLater={() => {}}
      onFailedAfterLock={() => {}}
    />,
  );

describe("update card", () => {
  it("names the new version and the one running", () => {
    const html = card();
    expect(html).toContain("SilentSilo 9.1.0 is available");
    expect(html).toContain(`You have ${__APP_VERSION__}.`);
  });

  it("offers the install, Later and the notes", () => {
    const html = card();
    expect(html).toContain("Install and restart");
    expect(html).toContain("Later");
    expect(html).toContain("What&#x27;s new");
  });
});
