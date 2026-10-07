/**
 * The sheet of paper that gets a silo back when everything else is gone.
 *
 * Paper rather than a file, and never a file this app writes. The page is
 * rendered here and handed to the operating system's print dialog, so the
 * code does not touch the disk on the way. Someone who picks "Save as PDF" in
 * that dialog has decided that themselves, which is a different thing from us
 * deciding it for them.
 *
 * It says nothing about where the files are kept, and is not given the
 * settings to say it with. The owner knows where their storage is; a sheet
 * carrying both the code and the address is one that opens the silo for
 * whoever finds it, and the two halves are worth keeping apart.
 */

import { dateLocale, t, tx } from "../i18n";

/** One box per character, so a handwritten copy has somewhere to go. */
function CodeBoxes({ code }: { code: string }) {
  // Groups of four, which is how the code is generated and how anyone
  // reading it aloud will chunk it anyway.
  const groups: string[] = [];
  const bare = code.replace(/-/g, "");
  for (let i = 0; i < 32; i += 4) {
    groups.push(bare.slice(i, i + 4));
  }

  return (
    <div className="kit-code">
      {groups.map((group, gi) => (
        <div key={gi} className="kit-code-group">
          {[0, 1, 2, 3].map((ci) => (
            <span key={ci} className="kit-box">
              {group[ci] ?? ""}
            </span>
          ))}
        </div>
      ))}
    </div>
  );
}

type Props = {
  siloName: string;
  /** Empty prints a blank sheet to be filled in by hand. */
  code: string;
  /** Extra class, so the same sheet can be a preview or the printed copy. */
  variant?: string;
};

export function EmergencyKit({ siloName, code, variant }: Props) {
  const printedOn = new Date().toLocaleDateString(dateLocale(), {
    year: "numeric",
    month: "long",
    day: "numeric",
  });

  return (
    <div className={`kit-sheet${variant ? ` ${variant}` : ""}`} aria-hidden>
      <header className="kit-head">
        <div>
          <h1>{t("kit.sheet_title")}</h1>
          <p className="kit-sub">{tx("kit.sheet_sub", { name: <strong>{siloName}</strong> })}</p>
        </div>
        <p className="kit-date">{t("kit.printed_on", { date: printedOn })}</p>
      </header>

      <section className="kit-section">
        <h2>{t("kit.s1_title")}</h2>
        <p>{t("kit.s1_body")}</p>
        <CodeBoxes code={code} />
        {!code && (
          <p className="kit-note">{t("kit.s1_note")}</p>
        )}
      </section>

      <section className="kit-section">
        <h2>{t("kit.s2_title")}</h2>
        <p>{t("kit.s2_body")}</p>
        {/* The sheet is kept for years and read on the worst day, possibly in
            front of a borrowed machine. Which platforms the app runs on will
            change in that time, so the sheet names none and points at the
            extraction tool for any computer the app does not run on. */}
        <ol className="kit-steps">
          <li>
            {tx("kit.step_download", {
              site: <strong>silentsilo.com</strong>,
              tool: <strong>silentsilo-extract</strong>,
              command: (
                <code>
                  silentsilo-extract extract --from &lt;folder&gt; --code &lt;code&gt; --to
                  &lt;folder&gt;
                </code>
              ),
            })}
          </li>
          <li>{t("kit.step_install")}</li>
          <li>{tx("kit.step_first", { label: <strong>{t("welcome.already_have")}</strong> })}</li>
          <li>
            {t("kit.step_where")}
            <ul className="kit-substeps">
              <li>{tx("kit.step_join", { label: <strong>{t("welcome.join")}</strong> })}</li>
              <li>{tx("kit.step_folder", { label: <strong>{t("welcome.add_folder")}</strong> })}</li>
            </ul>
          </li>
          <li>{tx("kit.step_unlock", { label: <strong>{t("kit.recovery_choice")}</strong> })}</li>
          <li>{t("kit.step_after")}</li>
        </ol>
      </section>

      <section className="kit-section kit-warning">
        <h2>{t("kit.warn_title")}</h2>
        <p>{t("kit.warn_body1")}</p>
        <p>{t("kit.warn_body2")}</p>
      </section>

      <footer className="kit-foot">
        {t("kit.footer", { name: siloName })}
      </footer>
    </div>
  );
}
