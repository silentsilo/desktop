import { useEffect, useState } from "react";
import {
  AlertTriangle,
  ArrowLeft,
  Cloud,
  FolderOpen,
  FolderPlus,
  HardDrive,
  Info,
  Plus,
  X,
} from "lucide-react";
import { open as openDialog } from "../lib/dialog";
import { invoke } from "@tauri-apps/api/core";
import { AuthShell } from "../layout/AuthShell";
import { SiloReportDialog } from "./SiloReportDialog";
import type { SiloReport } from "../lib/siloReport";
import { formatAppError } from "../lib/errors";
import { formatDay } from "../lib/format";
import type { Silo } from "../lib/types";
import { t, useLocale } from "../i18n";

type Props = {
  silos: Silo[];
  busy: boolean;
  onOpen: (id: string) => void;
  onCreate: (name: string, location: string | null) => void;
  onJoin: () => void;
  onAdded: () => void;
  onForget: (silo: Silo) => void;
};

type Mode = "list" | "create";

function describeLastOpened(at: number): string {
  if (!at) return t("start.never_opened");
  const days = Math.floor((Date.now() / 1000 - at) / 86400);
  if (days < 1) return t("start.opened_today");
  if (days === 1) return t("start.opened_yesterday");
  if (days < 30) return t("start.opened_days_ago", { count: days });
  return t("start.opened_on", { date: formatDay(at) });
}

/**
 * Which silo to work in.
 *
 * Shown whenever none is open, including at first run, because "where do my
 * files live" is a question the user should answer before putting anything
 * in — not one the app should answer for them and reveal later.
 */
export function SiloPickerView({
  silos,
  busy,
  onOpen,
  onCreate,
  onJoin,
  onAdded,
  onForget,
}: Props) {
  useLocale();
  const [mode, setMode] = useState<Mode>(silos.length === 0 ? "create" : "list");
  const [name, setName] = useState(() => t("start.default_name"));
  const [location, setLocation] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [syncProvider, setSyncProvider] = useState<string | null>(null);
  const [report, setReport] = useState<SiloReport | null>(null);

  // Reachable before anything is unlocked, because the case it answers is a
  // silo that will not open: after unlocking is exactly when it is no use.
  const showReport = async (id: string) => {
    try {
      setReport(await invoke<SiloReport>("silo_report", { id }));
    } catch (e) {
      setError(String(e));
    }
  };

  // The suggested folder follows the name until the user overrides it, so
  // the common case needs no decision at all and the uncommon one is one
  // click away.
  const [locationEdited, setLocationEdited] = useState(false);
  useEffect(() => {
    if (mode !== "create" || locationEdited) return;
    let cancelled = false;
    void invoke<string>("silo_default_location", { name: name.trim() || "Silo" })
      .then((suggested) => {
        if (!cancelled) setLocation(suggested);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [name, mode, locationEdited]);

  // Checked as the path changes rather than on submit: the point is to
  // inform the choice, not to reject it afterwards.
  useEffect(() => {
    if (!location.trim()) {
      setSyncProvider(null);
      return;
    }
    let cancelled = false;
    void invoke<string | null>("silo_sync_provider_at", { path: location })
      .then((p) => {
        if (!cancelled) setSyncProvider(p);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [location]);

  const chooseLocation = async () => {
    const picked = await openDialog({ directory: true, multiple: false });
    if (typeof picked === "string") {
      setLocation(picked);
      setLocationEdited(true);
    }
  };

  const addExisting = async () => {
    const picked = await openDialog({ directory: true, multiple: false });
    if (typeof picked !== "string") return;
    setError(null);
    try {
      await invoke("silo_add_existing", { path: picked, name: null });
      onAdded();
    } catch (e) {
      setError(formatAppError(e));
    }
  };

  if (mode === "create") {
    return (
      <AuthShell
        title="SilentSilo"
        subtitle={
          silos.length === 0 ? t("start.create_first_subtitle") : t("start.create_another_subtitle")
        }
      >
        <form
          className="card auth-card is-wide"
          onSubmit={(e) => {
            e.preventDefault();
            if (!busy && name.trim()) onCreate(name.trim(), location.trim() || null);
          }}
        >
          <h2>{t("start.new_silo")}</h2>
          <p className="hint">{t("start.new_silo_intro")}</p>

          <label className="field">
            <span>{t("start.name_label")}</span>
            <input
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder={t("start.default_name")}
              autoFocus
            />
          </label>

          <label className="field">
            <span>{t("start.location_label")}</span>
            <div className="path-picker">
              <input
                value={location}
                onChange={(e) => {
                  setLocation(e.target.value);
                  setLocationEdited(true);
                }}
                spellCheck={false}
              />
              <button type="button" className="secondary" disabled={busy} onClick={() => void chooseLocation()}>
                <FolderOpen size={15} />
                {t("start.browse")}
              </button>
            </div>
            <p className="hint">{t("start.location_hint")}</p>
            {syncProvider && (
              <p className="hint is-warning">
                <AlertTriangle size={14} />
                {t("start.sync_folder_warning", { provider: syncProvider })}
              </p>
            )}
          </label>

          {error && (
            <p className="hint is-error" role="status">
              <AlertTriangle size={14} />
              {error}
            </p>
          )}

          <div className="actions">
            <button type="submit" disabled={busy || name.trim().length === 0}>
              <Plus size={15} />
              {busy ? t("start.creating") : t("start.create")}
            </button>
            {silos.length > 0 && (
              <button
                type="button"
                className="secondary"
                disabled={busy}
                onClick={() => setMode("list")}
              >
                <ArrowLeft size={15} />
                {t("start.back")}
              </button>
            )}
          </div>

          {/* First run only, which is the case this was always for: there is
              no list yet, and someone who remembers they have a folder or a
              bucket should not have to create a silo they do not want in
              order to reach it. With a list behind this form they came from
              a screen offering both of these, one click ago, and repeating
              them here reads as a different pair of choices. */}
          {silos.length === 0 && (
            <div className="auth-alternatives">
              <p className="hint">{t("welcome.already_have")}</p>
              <div className="actions">
                <button
                  type="button"
                  className="secondary"
                  disabled={busy}
                  onClick={() => void addExisting()}
                >
                  <FolderPlus size={15} />
                  {t("welcome.add_folder")}
                </button>
                <button type="button" className="secondary" disabled={busy} onClick={onJoin}>
                  <Cloud size={15} />
                  {t("welcome.join")}
                </button>
              </div>
            </div>
          )}
        </form>
      </AuthShell>
    );
  }

  return (
    <AuthShell title="SilentSilo" subtitle={t("start.picker_subtitle")}>
      <section className="card auth-card">
        <h2>{t("start.your_silos")}</h2>
        <ul className="silo-list">
          {silos.map((silo) => (
            <li key={silo.id} className="silo-row">
              <button
                type="button"
                className={`silo-item${silo.present ? "" : " is-missing"}`}
                disabled={busy || !silo.present}
                onClick={() => onOpen(silo.id)}
                title={silo.present ? silo.path : t("start.path_not_reachable", { path: silo.path })}
              >
                <HardDrive size={18} />
                <span className="silo-item-text">
                  <strong>
                    {silo.name}
                    {/* Says what happens next, not what the app is doing:
                        an unlocked silo opens on click, a locked one asks
                        for a key first, and that is the difference worth
                        knowing before clicking. */}
                    {silo.unlocked && <span className="silo-open-badge">{t("start.unlocked_badge")}</span>}
                  </strong>
                  <span className="hint">
                    {silo.present ? describeLastOpened(silo.last_opened) : t("start.not_reachable")} ·{" "}
                    {silo.path}
                  </span>
                </span>
              </button>
              <button
                type="button"
                className="silo-info"
                disabled={busy}
                onClick={() => void showReport(silo.id)}
                title={t("start.report_tooltip", { name: silo.name })}
                aria-label={t("start.about_silo", { name: silo.name })}
              >
                <Info size={15} />
              </button>
              <button
                type="button"
                className="silo-remove"
                disabled={busy}
                onClick={() => onForget(silo)}
                title={t("start.remove_from_list", { name: silo.name })}
                aria-label={t("start.remove_from_list", { name: silo.name })}
              >
                <X size={15} />
              </button>
            </li>
          ))}
        </ul>

        {silos.some((s) => !s.present) && (
          <p className="hint">
            <AlertTriangle size={14} />
            {t("start.unreachable_hint")}
          </p>
        )}

        {error && (
          <p className="hint is-error" role="status">
            <AlertTriangle size={14} />
            {error}
          </p>
        )}

        {/* Named exactly as the first-run form names them. They are the same
            two actions, and a person meets one screen before the other: two
            sets of words for one thing reads as four choices. The pair says
            where the silo already is, which is the whole of the difference.

            Stacked rather than a wrapping row: three buttons of unequal
            label length wrapped 2 + 1 and centred each line on its own, so
            no two edges lined up. One per line, full width, icons in a
            column, and the three ways in read as a list of choices. */}
        <div className="silo-actions">
          <button type="button" disabled={busy} onClick={() => setMode("create")}>
            <Plus size={16} />
            <span>{t("start.new_silo")}</span>
          </button>
          <button
            type="button"
            className="secondary"
            disabled={busy}
            onClick={() => void addExisting()}
          >
            <FolderPlus size={16} />
            <span>{t("welcome.add_folder")}</span>
          </button>
          <button type="button" className="secondary" disabled={busy} onClick={onJoin}>
            <Cloud size={16} />
            <span>{t("welcome.join")}</span>
          </button>
        </div>
      </section>
      {report && <SiloReportDialog report={report} onClose={() => setReport(null)} />}
    </AuthShell>
  );
}
