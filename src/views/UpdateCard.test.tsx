import { describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));

import { UpdateCard } from "./UpdateCard";

const card = () => renderToStaticMarkup(<UpdateCard version="9.1.0" onPostpone={() => {}} />);

describe("update card", () => {
  it("names the new version and the one running", () => {
    const html = card();
    expect(html).toContain("SilentSilo 9.1.0 is available");
    expect(html).toContain(`You have ${__APP_VERSION__}.`);
  });

  it("offers only the download page and a week's postponement, and no install", () => {
    const html = card();
    expect(html).toContain("Get latest version");
    expect(html).toContain("Postpone 1 week");
    expect(html.match(/<button/g)).toHaveLength(2);
    expect(html).not.toContain("Install");
  });
});
