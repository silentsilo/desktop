import { useState } from "react";
import { ChevronDown, ChevronUp } from "lucide-react";
import { t, useLocale } from "../i18n";
import type { BackgroundTask, TaskPlace } from "../lib/backgroundTasks";

type Props = {
  tasks: BackgroundTask[];
  collapsed: boolean;
  onOpen: (place: TaskPlace) => void;
};

/**
 * What runs in the background, under the storage figure. Absent when
 * nothing does. One task is shown as itself; several fold into a count that
 * opens the list. Each row leads to the page where the work can be
 * followed or stopped.
 */
export function SidebarTasks({ tasks, collapsed, onOpen }: Props) {
  useLocale();
  const [open, setOpen] = useState(false);
  if (tasks.length === 0) return null;

  if (collapsed) {
    return (
      <button
        type="button"
        className="sidebar-tasks-collapsed"
        onClick={() => onOpen(tasks[0]!.place)}
        data-tooltip={tasks.map(line).join("\n")}
        aria-label={tasks.map(line).join(", ")}
      >
        <span className="spinner" aria-hidden />
        {tasks.length > 1 && <span className="sidebar-tasks-count">{tasks.length}</span>}
      </button>
    );
  }

  if (tasks.length === 1) {
    return (
      <div className="sidebar-tasks">
        <Row task={tasks[0]!} onOpen={onOpen} spinner />
      </div>
    );
  }

  return (
    <div className="sidebar-tasks">
      <button
        type="button"
        className="sidebar-task"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
      >
        <span className="spinner" aria-hidden />
        <span className="sidebar-task-label">
          {t("nav.tasks_running", { count: tasks.length })}
        </span>
        {open ? <ChevronUp size={14} /> : <ChevronDown size={14} />}
      </button>
      {open && (
        <ul className="sidebar-tasks-list" aria-label={t("nav.tasks_list")}>
          {tasks.map((task) => (
            <li key={task.id}>
              <Row task={task} onOpen={onOpen} />
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function line(task: BackgroundTask): string {
  return task.detail ? `${task.label}: ${task.detail}` : task.label;
}

function Row({
  task,
  onOpen,
  spinner = false,
}: {
  task: BackgroundTask;
  onOpen: (place: TaskPlace) => void;
  spinner?: boolean;
}) {
  return (
    <button
      type="button"
      className="sidebar-task"
      onClick={() => onOpen(task.place)}
      data-tooltip={line(task)}
    >
      {spinner && <span className="spinner" aria-hidden />}
      <span className="sidebar-task-text">
        <span className="sidebar-task-label">{task.label}</span>
        {task.detail && <span className="sidebar-task-detail">{task.detail}</span>}
      </span>
    </button>
  );
}
