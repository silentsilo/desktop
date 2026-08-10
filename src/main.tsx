import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { ThemeProvider } from "./components/ThemeProvider";
import "@fontsource-variable/plus-jakarta-sans/index.css";
import "./styles.css";

// Opening the app in a browser with ?mock renders the pre-unlock screens
// against a stubbed backend. `import.meta.env.DEV` is a compile-time
// constant, so the import and everything it pulls in are dropped from the
// production build rather than merely unreachable in it.
if (import.meta.env.DEV && new URLSearchParams(location.search).has("mock")) {
  const { installMockBackend } = await import("./dev/mockBackend");
  installMockBackend();
  const { installContrastAudit } = await import("./dev/contrastAudit");
  installContrastAudit();
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ThemeProvider>
      <App />
    </ThemeProvider>
  </React.StrictMode>,
);
