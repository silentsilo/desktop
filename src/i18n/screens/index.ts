import { common } from "./common";
import { files } from "./files";
import { passwords } from "./passwords";
import { backup } from "./backup";
import { settingsPanels } from "./settings-panels";
import { start } from "./start";
import { dialogs } from "./dialogs";
import { appMessages } from "./app-messages";
import { emergencyKit } from "./emergency-kit";
import { firstRun } from "./first-run";
import { recovery } from "./recovery";
import { recoveryCodeDialog } from "./recovery-code-dialog";
import { settingsGeneral } from "./settings-general";
import { settingsRail } from "./settings-rail";
import { rotateKey } from "./rotate-key";
import { sidebar } from "./sidebar";
import { trash } from "./trash";
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
  common,
  trash,
  settingsRail,
  recovery,
  rotateKey,
  files,
  passwords,
  backup,
  settingsPanels,
  start,
  dialogs,
  appMessages,
] as const;
