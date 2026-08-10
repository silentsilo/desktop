import { useMemo } from "react";
import { Star } from "lucide-react";
import { ViewHeader } from "../components/ViewHeader";
import type { PasswordEntry, SearchHit } from "../lib/types";
import { formatBytes } from "../lib/format";
import { fileIconFor, fileKindOf } from "../lib/fileKinds";
import { subtitleFor, typeOf } from "./passwords/util";
import { TYPE_ICONS } from "./passwords/CategoryRail";
import { IconFolder } from "../ui/Icons";

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
  const total = hits.length + credentials.length;

  const subtitle = useMemo(() => {
    if (total === 0) return "Nothing starred yet";
    const parts: string[] = [];
    if (hits.length > 0) {
      parts.push(`${hits.length} ${hits.length === 1 ? "item" : "items"} from Files`);
    }
    if (credentials.length > 0) {
      parts.push(
        `${credentials.length} ${credentials.length === 1 ? "credential" : "credentials"}`
      );
    }
    return parts.join(" · ");
  }, [credentials.length, hits.length, total]);

  return (
    <div className="favorites-view">
      <ViewHeader icon={Star} title="Favourites" subtitle={subtitle} />

      <div className="favorites-pane">
        {total === 0 ? (
          <div className="favorites-empty-state">
            <Star size={28} />
            <p className="hint">
              Right-click a file or folder and choose Add to favourites, or star a credential from
              its page. Favourites travel with the silo, so they are the same on every device.
            </p>
          </div>
        ) : (
          <>
            {hits.length > 0 && (
              <section className="favorites-section">
                <h3>Files</h3>
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
                          title={`Open ${hit.name || "/"}`}
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
                            {hit.kind === "file" ? formatBytes(hit.size_bytes) : "Folder"}
                          </span>
                        </button>
                        <button
                          type="button"
                          className="favorites-unstar"
                          onClick={() => onUnstarHit(hit)}
                          disabled={busy}
                          title="Remove from favourites"
                          aria-label={`Remove ${hit.name || "this folder"} from favourites`}
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
                <h3>Credentials</h3>
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
                          title={`Open ${entry.service} in Credentials`}
                        >
                          <span className="favorites-card-icon">
                            <Icon size={26} />
                          </span>
                          <span className="favorites-card-name">{entry.service || "Untitled"}</span>
                          <span className="favorites-card-sub">{sub}</span>
                          <span className="favorites-card-meta">{entry.category}</span>
                        </button>
                        <button
                          type="button"
                          className="favorites-unstar"
                          onClick={() => onUnstarCredential(entry)}
                          disabled={busy}
                          title="Remove from favourites"
                          aria-label={`Remove ${entry.service} from favourites`}
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
