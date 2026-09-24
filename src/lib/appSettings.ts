import { createContext, useContext } from "react";

/**
 * Opens the app's own settings from the screens before a silo is unlocked.
 * Null where no host offers them, so a shell outside it shows no button.
 */
export const AppSettingsContext = createContext<(() => void) | null>(null);

export function useOpenAppSettings(): (() => void) | null {
  return useContext(AppSettingsContext);
}
