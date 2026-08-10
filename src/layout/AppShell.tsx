import { useState, type ReactNode } from "react";
import {
  ChevronsUpDown,
  FolderClosed,
  HardDrive,
  HeartPulse,
  KeyRound,
  Lock,
  Moon,
  PanelLeftClose,
  PanelLeftOpen,
  Settings2,
  Star,
  Sun,
  Trash2,
} from "lucide-react";
import { BrandLogo } from "../components/BrandLogo";
import { useMediaQuery } from "../hooks/useMediaQuery";
import { formatBytes } from "../lib/format";
import type { View } from "../lib/types";

export type SyncIndicator = {
  configured: boolean;
  pending: number;
  state: "idle" | "syncing" | "ok" | "error";
  /** Unix ms of the last pass that reached the bucket. */
  lastSyncAt: number | null;
  lastError: string | null;
};

/// Deliberately vague past an hour: "3 minutes ago" is actionable, "47
/// minutes ago" is not, and precision the user cannot act on reads as noise.
function describeAge(at: number): string {
  const seconds = Math.round((Date.now() - at) / 1000);
  if (seconds < 45) return "just now";
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return hours === 1 ? "an hour ago" : `${hours} hours ago`;
  // "49 hours ago" made the reader do arithmetic to learn it was the day
  // before yesterday.
  const days = Math.round(hours / 24);
  return days === 1 ? "yesterday" : `${days} days ago`;
}

const SIDEBAR_COLLAPSED_KEY = "silentsilo.sidebar.collapsed";

/// Below this the expanded sidebar takes a quarter of the window, and the
/// views that split into two panes (a list beside a detail) have nothing
/// left to split. The sidebar collapses to icons on its own and the stored
/// preference is left alone, so widening the window brings it back the way
/// the user had it.
const NARROW_WINDOW = "(max-width: 900px)";

export type StorageUsage = {
  localBytes: number;
  unsyncedBytes: number;
};

type Props = {
  view: View;
  onView: (v: View) => void;
  onLock: () => void;
  storage: StorageUsage | null;
  trashCount?: number;
  /** Findings worth acting on, badged on the Health tab. Excludes the
   * informational ones, which would make the number permanent. */
  healthCount?: number;
  children: ReactNode;
  title?: string;
  theme: "dark" | "light";
  onToggleTheme: () => void;
  sync: SyncIndicator;
  onSyncNow: () => void;
  /** Backup lives inside Settings now, so everything that used to point at
   * its own view points here instead. */
  onOpenBackup: () => void;
  /** Right-hand summary of what the current view is showing. */
  statusSummary?: string;
  siloName: string;
  onSwitchSilo: () => void;
};

/** Views that lay out their own panes rather than scrolling as a page. */
const PANE_VIEWS = new Set<View>([
  "files",
  "passwords",
  "favorites",
  "health",
  "trash",
  "settings",
]);

const NAV: { id: View; label: string; title: string; icon: typeof FolderClosed }[] = [
  // Favourites leads, the way Explorer and Finder put quick access above the
  // tree: it is the shortest path to what someone opens repeatedly, and the
  // two views below it are where everything else lives.
  { id: "favorites", label: "Favourites", title: "Favourites", icon: Star },
  { id: "files", label: "Files", title: "Files", icon: FolderClosed },
  { id: "passwords", label: "Credentials", title: "Credentials", icon: KeyRound },
  { id: "health", label: "Health", title: "Health", icon: HeartPulse },
  { id: "trash", label: "Trash", title: "Trash", icon: Trash2 },
  { id: "settings", label: "Settings", title: "Settings", icon: Settings2 },
];

export function AppShell({
  view,
  onView,
  onLock,
  storage,
  trashCount = 0,
  healthCount = 0,
  children,
  title,
  theme,
  onToggleTheme,
  sync,
  onSyncNow,
  onOpenBackup,
  statusSummary,
  siloName,
  onSwitchSilo,
}: Props) {
  const [preferCollapsed, setPreferCollapsed] = useState(
    () => localStorage.getItem(SIDEBAR_COLLAPSED_KEY) === "true"
  );
  const narrow = useMediaQuery(NARROW_WINDOW);
  const collapsed = preferCollapsed || narrow;

  const toggleCollapsed = () => {
    setPreferCollapsed((prev) => {
      const next = !prev;
      localStorage.setItem(SIDEBAR_COLLAPSED_KEY, String(next));
      return next;
    });
  };

  return (
    <div className="shell">
      <aside className={`sidebar${collapsed ? " sidebar-collapsed" : ""}`}>
        <div className="sidebar-brand">
          <BrandLogo size={collapsed ? 28 : 34} showWordmark={!collapsed} />
          {!collapsed && <span className="sidebar-version">v{__APP_VERSION__}</span>}
        </div>

        {/* Which silo this is, and the way out of it. Named at all times
            because every action below applies to one silo, and acting on the
            wrong one is the mistake this feature makes possible. */}
        <button
          type="button"
          className="sidebar-silo"
          onClick={onSwitchSilo}
          title={`${siloName}. Click to switch silo.`}
        >
          <HardDrive size={15} />
          {!collapsed && (
            <>
              <strong>{siloName}</strong>
              <ChevronsUpDown size={14} style={{ marginLeft: "auto", flexShrink: 0 }} />
            </>
          )}
        </button>

        <nav className="sidebar-nav" aria-label="Main Navigation">
          {NAV.map((item) => {
            const Icon = item.icon;
            const active = view === item.id;
            const badgeCount =
              item.id === "trash" ? trashCount : item.id === "health" ? healthCount : 0;
            const badgeLabel = badgeCount > 99 ? "99+" : String(badgeCount);
            const title =
              badgeCount === 0
                ? item.title
                : item.id === "health"
                  ? // "to look at", the Health page's own wording: the count
                    // mixes must-fix findings with worth-doing ones.
                    `${item.title} (${badgeCount} to look at)`
                  : `${item.title} (${badgeCount})`;
            return (
              <button
                key={item.id}
                type="button"
                className={`tab-item${active ? " active" : ""}`}
                onClick={() => onView(item.id)}
                title={title}
                aria-label={title}
                aria-current={active ? "page" : undefined}
              >
                <span className="tab-icon-wrap">
                  <Icon size={18} />
                  {collapsed && badgeCount > 0 && (
                    <span className="tab-badge tab-badge-dot" aria-hidden />
                  )}
                </span>
                {!collapsed && <span className="tab-label">{item.label}</span>}
                {!collapsed && badgeCount > 0 && <span className="tab-badge">{badgeLabel}</span>}
              </button>
            );
          })}
        </nav>

        <div className="sidebar-spacer" />

        {/* What this silo occupies here. Content arrives only when a file is
            opened, so this grows with use rather than with the silo, and the
            second line is the part that can need acting on. */}
        {storage &&
          (collapsed ? (
            /* Turned on its side rather than dropped: a collapsed sidebar is
               40px of width and the figure still fits down it, so the one
               number that says how much of this machine the silo is using
               stays on screen instead of disappearing with the labels. */
            <button
              type="button"
              className="sidebar-storage sidebar-storage-vertical"
              onClick={onOpenBackup}
              title={`${formatBytes(storage.localBytes)} on this disk${
                storage.unsyncedBytes > 0
                  ? `, ${formatBytes(storage.unsyncedBytes)} waiting to back up`
                  : ""
              }. Click to open Backup.`}
            >
              {storage.unsyncedBytes > 0 && (
                <span className="sidebar-storage-dot" aria-hidden />
              )}
              <span>{formatBytes(storage.localBytes)}</span>
            </button>
          ) : (
            <button
              type="button"
              className="sidebar-storage"
              onClick={onOpenBackup}
              title="What this silo occupies on this disk. Click to open Backup."
            >
              <span className="sidebar-storage-line">
                <span>{formatBytes(storage.localBytes)}</span>
                <span className="sidebar-storage-limit">on this disk</span>
              </span>
              {storage.unsyncedBytes > 0 && (
                <span className="sidebar-storage-note">
                  {formatBytes(storage.unsyncedBytes)} waiting to back up
                </span>
              )}
            </button>
          ))}

        <div className="sidebar-footer">
          <div className="sidebar-actions">
            <button
              type="button"
              className="btn-theme"
              onClick={onToggleTheme}
              title={theme === "light" ? "Switch to Dark Mode" : "Switch to Light Mode"}
              aria-label="Toggle theme"
            >
              {theme === "light" ? <Moon size={16} /> : <Sun size={16} />}
            </button>

            {/* Gone rather than disabled while the window is narrow: the
                sidebar cannot expand there, and a button that answers a
                click with nothing is worse than one that isn't offered. */}
            {!narrow && (
              <button
                type="button"
                className="btn-collapse"
                onClick={toggleCollapsed}
                title={collapsed ? "Expand sidebar" : "Collapse sidebar"}
                aria-label="Toggle sidebar"
              >
                {collapsed ? <PanelLeftOpen size={16} /> : <PanelLeftClose size={16} />}
              </button>
            )}

            <button
              type="button"
              className="btn-lock"
              onClick={onLock}
              title="Lock silo"
              aria-label="Lock silo"
            >
              <Lock size={16} />
              {!collapsed && <span className="lock-label">Lock</span>}
            </button>
          </div>
        </div>
      </aside>

      {/* The view drives one content width, shared by the page title and the
          panel below it, so the heading sits over what it names. */}
      <div className="main-area" data-view={view}>
        {title && view !== "files" && (
          <header className="main-header">
            <h2>{title}</h2>
          </header>
        )}
        {/* Every view but Backup lays out its own panes and scrolls inside
            them: a page-level scrollbar would carry their rails and
            toolbars off the top instead of their contents. */}
        <div className={`main-content${PANE_VIEWS.has(view) ? " main-content-panes" : ""}`}>
          {children}
        </div>
        <footer className="status-bar">
          <div className="status-bar-inner">
          {sync.configured ? (
            <button
              type="button"
              className="status-sync"
              onClick={onSyncNow}
              disabled={sync.state === "syncing"}
              title={sync.lastError ?? "Sync now"}
            >
              <span
                className={`dot ${
                  sync.state === "error" ? "warn" : sync.state === "syncing" ? "busy" : "ok"
                }`}
              />
              {sync.state === "syncing"
                ? "Syncing…"
                : sync.state === "error"
                  ? "Backup failed. Click to retry"
                  : sync.pending > 0
                    ? `${sync.pending} change${sync.pending === 1 ? "" : "s"} to send`
                    : sync.lastSyncAt
                      ? `Backed up ${describeAge(sync.lastSyncAt)}`
                      : "Backup connected"}
            </button>
          ) : (
            <button
              type="button"
              className="status-sync"
              onClick={onOpenBackup}
              title="Open Backup to connect storage"
            >
              {/* Neutral, not green: a silo with no backup is not a state
                  worth a reassuring colour, it is the one Health flags. */}
              <span className="dot neutral" />
              Local silo only
            </button>
          )}
            {statusSummary && <span className="status-summary">{statusSummary}</span>}
          </div>
        </footer>
      </div>
    </div>
  );
}

