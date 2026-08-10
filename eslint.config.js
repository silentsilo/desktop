import js from "@eslint/js";
import tseslint from "typescript-eslint";
import reactHooks from "eslint-plugin-react-hooks";

// Deliberately narrow: correctness rules only, no stylistic ones. A rule that
// only rearranges code would make the diff on every contribution larger
// without making it better.
//
// The react-hooks plugin ships its React Compiler rules in the same
// recommended set as the two classic ones. Only the classic pair is enabled
// here: they catch real bugs (a hook behind a condition, a stale closure over
// props), and they are what the suppressions in this codebase refer to.
export default tseslint.config(
  { ignores: ["dist", "target", "src-tauri", "node_modules"] },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    // Build scripts run under Node, not in the webview.
    files: ["scripts/**/*.mjs"],
    languageOptions: { globals: { console: "readonly", process: "readonly" } },
  },
  {
    files: ["**/*.{ts,tsx}"],
    plugins: { "react-hooks": reactHooks },
    languageOptions: {
      parserOptions: { ecmaVersion: "latest", sourceType: "module" },
      globals: { __APP_VERSION__: "readonly" },
    },
    rules: {
      "react-hooks/rules-of-hooks": "error",
      "react-hooks/exhaustive-deps": "warn",
      // An unused argument is often part of a signature the caller requires.
      "@typescript-eslint/no-unused-vars": [
        "error",
        { argsIgnorePattern: "^_", varsIgnorePattern: "^_" },
      ],
    },
  },
);
