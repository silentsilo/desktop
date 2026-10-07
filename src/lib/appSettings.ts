import { createContext, useContext, type ReactNode } from "react";

/**
 * Opens the app's own settings from the screens before a silo is unlocked.
 * Null where no host offers them, so a shell outside it shows no button.
 */
export const AppSettingsContext = createContext<(() => void) | null>(null);

export function useOpenAppSettings(): (() => void) | null {
  return useContext(AppSettingsContext);
}

/** The update card those screens show under the mark, or nothing. */
export const UpdateCardContext = createContext<ReactNode>(null);

export function useUpdateCard(): ReactNode {
  return useContext(UpdateCardContext);
}
