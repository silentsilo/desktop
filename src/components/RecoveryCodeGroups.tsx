import { toGroups } from "../lib/recoveryCode";
import { t, useLocale } from "../i18n";

/**
 * A recovery code in the eight groups it is printed in. As one string it
 * wrapped wherever the card ended, splitting a group across two lines; here
 * a group never breaks, and a row on screen is a row on the paper.
 */
export function RecoveryCodeGroups({ code }: { code: string }) {
  useLocale();
  return (
    <div className="recovery-groups" role="group" aria-label={t("first.code_title")}>
      {toGroups(code).map((group, i) => (
        <span key={i} className="recovery-group">
          {group}
        </span>
      ))}
    </div>
  );
}
