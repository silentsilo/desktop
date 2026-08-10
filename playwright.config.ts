import { defineConfig } from "@playwright/test";

/**
 * Flow tests over the browser build of the frontend, against the mock
 * backend (`?mock=…`). No Tauri, no hardware key, no real silo: what these
 * prove is the wiring between screens, which is exactly the layer the unit
 * tests in src/ cannot see.
 *
 * The visual specs are gated behind SILENTSILO_VISUAL=1 because their
 * baselines are rendered by one platform's font stack and compared
 * per-platform. Run them where the committed baselines were made.
 */
export default defineConfig({
  testDir: "e2e",
  timeout: 30_000,
  fullyParallel: true,
  // A flow that only passes on retry is a flaky flow; surface it instead.
  retries: 0,
  reporter: [["list"]],
  use: {
    baseURL: "http://localhost:1420",
    // The app's default window, minus nothing: the size most users see.
    viewport: { width: 1200, height: 800 },
    colorScheme: "dark",
    // Dates are printed through toLocaleString with the runtime's own locale
    // and zone, so an assertion on a formatted timestamp only holds where it
    // was written unless both are pinned. Without these, the modified column
    // reads "Nov 15, 2023, 00:13" in Bucharest and "Nov 14, 2023, 22:13" on
    // a UTC runner, and the suite passes only in one timezone.
    locale: "en-US",
    timezoneId: "UTC",
  },
  expect: {
    toHaveScreenshot: {
      // Kill animations, or every screenshot of a toast is a coin toss.
      animations: "disabled",
      caret: "hide",
    },
  },
  webServer: {
    command: "npm run dev",
    port: 1420,
    reuseExistingServer: true,
  },
});
