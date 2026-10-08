import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Keyboard, Search } from "lucide-react";
import type { Os, PasswordEntry } from "../lib/types";
import { builtInOrKey, platformStrings } from "../lib/platformStrings";
import { describeFidoPrompt, type FidoPrompt } from "../lib/fidoPrompt";
import { formatAppError } from "../lib/errors";
import { rankForAutoType } from "../lib/autoType";
import { useEventSubscription } from "../hooks/useEventSubscription";
import { useModal } from "../hooks/useModal";
import { t, useLocale } from "../i18n";

/** What Rust sends with `autotype-request`. */
type AutoTypePrompt = {
  request_id: string;
  title: string;
  program: string;
  elevated: boolean;
};

/// As in the browser fill: a click already on its way lands on nothing.
const ARM_DELAY_MS = 700;

/**
 * Auto-type: the person pressed Ctrl+Alt+A in another program, and picks
 * here the login to type into it (docs/ARCHITECTURE.md, "Auto-type").
 * Mounted while a silo is open; a request made while it was locked waits
 * and shows once it opens. The password never passes through here: Rust
 * reads it after the key check.
 */
export function AutoTypeDialog({ os, entries }: { os: Os; entries: PasswordEntry[] }) {
  const [prompt, setPrompt] = useState<AutoTypePrompt | null>(null);

  useEffect(() => {
    let cancelled = false;
    invoke<AutoTypePrompt | null>("autotype_pending")
      .then((pending) => {
        if (!cancelled && pending) setPrompt(pending);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, []);

  useEventSubscription(
    () => listen<AutoTypePrompt>("autotype-request", (event) => setPrompt(event.payload)),
    [],
  );

  if (!prompt) return null;
  return (
    <AutoTypeCard
      key={prompt.request_id}
      prompt={prompt}
      os={os}
      entries={entries}
      onClose={() => setPrompt(null)}
    />
  );
}

function AutoTypeCard({
  prompt,
  os,
  entries,
  onClose,
}: {
  prompt: AutoTypePrompt;
  os: Os;
  entries: PasswordEntry[];
  onClose: () => void;
}) {
  useLocale();
  const platform = platformStrings(os);
  const [query, setQuery] = useState("");
  const [busy, setBusy] = useState(false);
  const [armed, setArmed] = useState(false);
  const [progress, setProgress] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const ranked = useMemo(
    () => rankForAutoType(entries, { title: prompt.title, program: prompt.program }),
    [entries, prompt.title, prompt.program],
  );
  const shown = useMemo(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) return ranked;
    return ranked.filter(
      ({ entry }) =>
        entry.service.toLowerCase().includes(needle) ||
        entry.username.toLowerCase().includes(needle) ||
        entry.url.toLowerCase().includes(needle),
    );
  }, [ranked, query]);
  // Nothing is chosen for the person: a match is a suggestion.
  const [chosen, setChosen] = useState<string | null>(null);

  useEffect(() => {
    const timer = window.setTimeout(() => setArmed(true), ARM_DELAY_MS);
    return () => window.clearTimeout(timer);
  }, []);

  const cancel = () => {
    void invoke("autotype_cancel", { requestId: prompt.request_id }).catch(() => {});
    onClose();
  };
  const cardRef = useModal(busy ? undefined : cancel);

  useEventSubscription(
    () =>
      listen<FidoPrompt>("fido-progress", (event) => {
        if (busy) setProgress(describeFidoPrompt(event.payload, platformStrings(os).builtIn));
      }),
    [busy, os],
  );

  const type = async () => {
    const entry = ranked.find((r) => r.entry.id === chosen)?.entry;
    if (!entry) return;
    setBusy(true);
    setError(null);
    setProgress(null);
    try {
      await invoke("autotype_confirm", {
        requestId: prompt.request_id,
        entryId: entry.id,
        label: entry.service,
      });
      onClose();
    } catch (e) {
      setError(formatAppError(e));
    } finally {
      setBusy(false);
      setProgress(null);
    }
  };

  return (
    <div className="modal-overlay" onClick={busy ? undefined : cancel}>
      <div
        ref={cardRef}
        className="modal-card autotype"
        role="alertdialog"
        aria-modal="true"
        aria-label={t("dlg.autotype_title")}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="modal-title-row">
          <span className="modal-title-icon" aria-hidden>
            <Keyboard size={18} />
          </span>
          <h3 className="modal-title">{t("dlg.autotype_title")}</h3>
        </div>
        <div className="modal-body">
          <p>{t("dlg.autotype_body", { program: prompt.program || "?" })}</p>
          <dl className="browser-fill-facts">
            <dt>{t("dlg.autotype_program")}</dt>
            <dd>{prompt.program || "?"}</dd>
            <dt>{t("dlg.autotype_window")}</dt>
            <dd className="autotype-title">{prompt.title || "?"}</dd>
          </dl>
          {prompt.elevated ? (
            <p className="browser-fill-mismatch" role="alert">
              {t("dlg.autotype_elevated")}
            </p>
          ) : ranked.length === 0 ? (
            <p className="hint">{t("dlg.autotype_none")}</p>
          ) : (
            <>
              <label className="autotype-search">
                <Search size={14} aria-hidden />
                <input
                  type="search"
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                  placeholder={t("dlg.autotype_search")}
                  aria-label={t("dlg.autotype_search")}
                  disabled={busy}
                />
              </label>
              <ul className="autotype-list" role="listbox" aria-label={t("dlg.autotype_title")}>
                {shown.map(({ entry, score }) => (
                  <li key={entry.id}>
                    <button
                      type="button"
                      role="option"
                      aria-selected={chosen === entry.id}
                      className={`autotype-row${chosen === entry.id ? " is-chosen" : ""}`}
                      disabled={busy}
                      onClick={() => setChosen(entry.id)}
                    >
                      <span className="autotype-row-name">{entry.service}</span>
                      <span className="autotype-row-user">{entry.username}</span>
                      {score > 0 && (
                        <span className="badge badge-accent">{t("dlg.autotype_suggested")}</span>
                      )}
                    </button>
                  </li>
                ))}
                {shown.length === 0 && <li className="hint">{t("dlg.autotype_no_match")}</li>}
              </ul>
              <p className="hint">
                {t("dlg.autotype_confirm_next", { method: builtInOrKey(platform) })}
              </p>
            </>
          )}
          {busy && progress && (
            <p className="hint" role="status">
              {progress}
            </p>
          )}
          {error && <p className="hint is-error">{error}</p>}
        </div>
        <div className="modal-actions">
          <button type="button" className="btn-secondary" disabled={busy} onClick={cancel}>
            {t("common.cancel")}
          </button>
          {!prompt.elevated && (
            <button
              type="button"
              className="btn-primary"
              disabled={busy || !armed || chosen === null}
              onClick={() => void type()}
            >
              {t("dlg.autotype_button")}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
