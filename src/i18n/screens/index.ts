import { settingsGeneral } from "./settings-general";
import { sidebar } from "./sidebar";
import { unlock } from "./unlock";
import { updateCard } from "./update-card";

/** Every screen's texts. A new screen file is added here. */
export const SCREENS = [sidebar, settingsGeneral, updateCard, unlock] as const;
