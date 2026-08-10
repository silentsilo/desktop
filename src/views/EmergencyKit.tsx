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
  const printedOn = new Date().toLocaleDateString(undefined, {
    year: "numeric",
    month: "long",
    day: "numeric",
  });

  return (
    <div className={`kit-sheet${variant ? ` ${variant}` : ""}`} aria-hidden>
      <header className="kit-head">
        <div>
          <h1>SilentSilo emergency kit</h1>
          <p className="kit-sub">
            Everything needed to open <strong>{siloName}</strong> again, on any computer.
          </p>
        </div>
        <p className="kit-date">Printed {printedOn}</p>
      </header>

      <section className="kit-section">
        <h2>1. Your recovery code</h2>
        <p>
          Thirty-two characters. Capitals and digits only, and the dashes are just for reading:
          type it however it is easiest.
        </p>
        <CodeBoxes code={code} />
        {!code && (
          <p className="kit-note">
            Copy it from the screen into the boxes above, in ink, and check it twice. Nothing was
            sent to the printer, which is the safest way to do this.
          </p>
        )}
      </section>

      <section className="kit-section">
        <h2>2. Getting your files back</h2>
        <p>
          On any computer, including one that has never seen this silo. You need this sheet and
          the way in to wherever the silo is kept.
        </p>
        {/* The sheet is read on the worst day, possibly in front of a
            borrowed machine, so the step that says "download the app" has
            to say which machines that works on. The app is Windows only;
            the extraction tool is what covers the other two, and it is
            precisely the case this sheet exists for. */}
        <ol className="kit-steps">
          <li>
            On a <strong>Windows</strong> computer, go to <strong>silentsilo.com</strong> and
            download SilentSilo, then follow the steps below. The app is Windows only. On a
            Mac or on Linux, download <strong>silentsilo-extract</strong> from the same
            place: it is a small command-line program that writes your files back out with
            nothing but this code. Copy the backup to a folder first, then run{" "}
            <code>silentsilo-extract extract --from &lt;folder&gt; --code &lt;code&gt; --to
            &lt;folder&gt;</code>. Run it with no arguments and it explains itself.
          </li>
          <li>Install it and open it.</li>
          <li>
            On the first screen, look under <strong>Already have one?</strong>.
          </li>
          <li>
            Say where the silo is. Either of these works, whichever you have:
            <ul className="kit-substeps">
              <li>
                <strong>Copy one from backup storage</strong>, then sign in to wherever this
                silo backs up: an S3-compatible bucket, a folder or network share, WebDAV, or
                SFTP. Not written on this sheet, because you know it and a stranger holding
                this should not.
              </li>
              <li>
                <strong>Add a folder from this computer</strong>, if you still have the silo
                folder itself on an external drive or another computer.
              </li>
            </ul>
          </li>
          <li>
            When it asks how to unlock, choose <strong>recovery code</strong> and type the code
            from part 1. A security key works here too if you still have one.
          </li>
          <li>
            Your files are rebuilt on that computer. Enrol a security key straight away, and
            print a fresh copy of this sheet.
          </li>
        </ol>
      </section>

      <section className="kit-section kit-warning">
        <h2>Keep this like a key</h2>
        <p>
          Anyone holding this sheet and able to reach your storage can read everything in the
          silo. It is the way in rather than the contents, so losing it costs you nothing on its
          own. A safe, a deposit box, or a sealed envelope away from your house. Not a drawer.
        </p>
        <p>
          If you think someone has seen it: open SilentSilo, generate a new recovery code, and
          print this again. The old sheet stops working the moment you do.
        </p>
      </section>

      <footer className="kit-foot">
        Silo: {siloName} · SilentSilo emergency kit · Keep with your important documents
      </footer>
    </div>
  );
}
