import { DEFAULT_HISTORY_POLICY, HISTORY_POLICIES, type HistoryPolicy } from "./entryHistory";

/** Per computer for now: a setting every device of a silo shares would need
 * a row of its own in the silo, which earlier versions would list as an
 * entry. An organisation's silo gets it from the organisation later. */
const HISTORY_KEY = "silentsilo.passwordHistory";

export function loadHistoryPolicy(): HistoryPolicy {
  try {
    const saved = localStorage.getItem(HISTORY_KEY);
    if (saved === "fit") return "fit";
    const count = Number.parseInt(saved ?? "", 10);
    if (HISTORY_POLICIES.includes(count)) return count;
  } catch {
    // Storage refused: the default.
  }
  return DEFAULT_HISTORY_POLICY;
}

export function saveHistoryPolicy(policy: HistoryPolicy): void {
  try {
    localStorage.setItem(HISTORY_KEY, String(policy));
  } catch {
    // Kept for this session only.
  }
}
