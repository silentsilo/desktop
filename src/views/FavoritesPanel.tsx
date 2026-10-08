import { Star } from "lucide-react";
import { ViewHeader } from "../components/ViewHeader";
import type { PasswordEntry, SearchHit } from "../lib/types";
import { formatBytes } from "../lib/format";
import { fileIconFor, fileKindOf } from "../lib/fileKinds";
import { categoryLabel, subtitleFor, typeOf } from "./passwords/util";
import { TYPE_ICONS } from "./passwords/CategoryRail";
import { IconFolder } from "../ui/Icons";
import { t, useLocale } from "../i18n";

type Props = {
  /** Starred files and folders, each with the folder it lives in. */
  hits: SearchHit[];
  /** Starred credentials, which live in their own store rather than the tree. */
  credentials: PasswordEntry[];
  busy: boolean;
  onOpenHit: (hit: SearchHit) => void;
  onOpenCredential: (id: string) => void;
  onUnstarHit: (hit: SearchHit) => void;
  onUnstarCredential: (entry: PasswordEntry) => void;
};

/**
 * One list for everything starred, files and credentials together.
 *
 * Split into two sections rather than one mixed list: they are opened by
 * different means and the eye sorts them by kind anyway. Unstarring is here
 * as well as in the views the items come from, because this is where a user
 * notices the list has grown into a second copy of the silo.
 */
export function FavoritesPanel({
  hits,
  credentials,
  busy,
  onOpenHit,
  onOpenCredential,
  onUnstarHit,
  onUnstarCredential,
}: Props) {
  useLocale();
  const total = hits.length + credentials.length;

  const parts: string[] = [];
  if (hits.length > 0) {
    parts.push(t("files.fav_items_from_files", { count: hits.length }));
  }
  if (credentials.length > 0) {
    parts.push(t("files.fav_entries_from_passwords", { count: credentials.length }));
  }
  const subtitle = total === 0 ? t("files.fav_nothing") : parts.join(" · ");

  return (
    <div className="favorites-view">
      <ViewHeader icon={Star} title={t("nav.favorites")} subtitle={subtitle} />

      <div className="favorites-pane">
        {total === 0 ? (
          <div className="favorites-empty-state">
            <Star size={28} />
            <p className="hint">
              {t("files.fav_empty_hint", { action: t("files.add_favorite") })}
            </p>
          </div>
        ) : (
          <>
            {hits.length > 0 && (
              <section className="favorites-section">
                <h3>{t("nav.files")}</h3>
                <ul className="favorites-grid">
                  {hits.map((hit) => {
                    const isFolder = hit.kind === "folder";
                    const Icon = isFolder ? IconFolder : fileIconFor(hit.name);
                    return (
                      <li key={hit.id} className="favorites-card">
                        <button
                          type="button"
                          className="favorites-card-open"
                          onClick={() => onOpenHit(hit)}
                          data-tooltip={t("files.fav_open", { name: hit.name || "/" })}
                        >
                          <span
                            className={`favorites-card-icon ${
                              isFolder ? "row-folder" : `row-file kind-${fileKindOf(hit.name)}`
                            }`}
                          >
                            <Icon size={30} />
                          </span>
                          <span className="favorites-card-name">{hit.name || "/"}</span>
                          <span className="favorites-card-sub">{hit.folder_path}</span>
                          <span className="favorites-card-meta">
                            {hit.kind === "file" ? formatBytes(hit.size_bytes) : t("files.folder")}
                          </span>
                        </button>
                        <button
                          type="button"
                          className="favorites-unstar"
                          onClick={() => onUnstarHit(hit)}
                          disabled={busy}
                          data-tooltip={t("files.remove_favorite")}
                          aria-label={
                            hit.name
                              ? t("files.fav_remove_named", { name: hit.name })
                              : t("files.fav_remove_this_folder")
                          }
                        >
                          <Star size={14} fill="currentColor" />
                        </button>
                      </li>
                    );
                  })}
                </ul>
              </section>
            )}

            {credentials.length > 0 && (
              <section className="favorites-section">
                <h3>{t("nav.passwords")}</h3>
                <ul className="favorites-grid">
                  {credentials.map((entry) => {
                    const Icon = TYPE_ICONS[typeOf(entry)];
                    const sub = subtitleFor(entry);
                    return (
                      <li key={entry.id} className="favorites-card">
                        <button
                          type="button"
                          className="favorites-card-open"
                          onClick={() => onOpenCredential(entry.id)}
                          data-tooltip={t("files.fav_open_in_passwords", { name: entry.service })}
                        >
                          <span className="favorites-card-icon">
                            <Icon size={26} />
                          </span>
                          <span className="favorites-card-name">{entry.service || t("files.untitled")}</span>
                          <span className="favorites-card-sub">{sub}</span>
                          <span className="favorites-card-meta">{categoryLabel(entry.category)}</span>
                        </button>
                        <button
                          type="button"
                          className="favorites-unstar"
                          onClick={() => onUnstarCredential(entry)}
                          disabled={busy}
                          data-tooltip={t("files.remove_favorite")}
                          aria-label={t("files.fav_remove_named", { name: entry.service })}
                        >
                          <Star size={14} fill="currentColor" />
                        </button>
                      </li>
                    );
                  })}
                </ul>
              </section>
            )}
          </>
        )}
      </div>
    </div>
  );
}
