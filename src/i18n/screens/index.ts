import { emergencyKit } from "./emergency-kit";
import { firstRun } from "./first-run";
import { recoveryCodeDialog } from "./recovery-code-dialog";
import { settingsGeneral } from "./settings-general";
import { sidebar } from "./sidebar";
import { unlock } from "./unlock";
import { welcome } from "./welcome";
import { updateCard } from "./update-card";

/** Every screen's texts. A new screen file is added here. */
export const SCREENS = [
  sidebar,
  settingsGeneral,
  updateCard,
  unlock,
  recoveryCodeDialog,
  firstRun,
  welcome,
  emergencyKit,
] as const;
