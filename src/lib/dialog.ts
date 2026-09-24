import { open as openNative, save as saveNative } from "@tauri-apps/plugin-dialog";

/// The file dialogs, with one way in for the end-to-end tests of the real
/// app (`e2e-app/`): a runner cannot press a native dialog, so it queues
/// answers on `window.__silentsiloDialogAnswers`, and each call takes the
/// next one. Nothing in the app sets it, and script that could set it could
/// already call any command itself.
function nextAnswer(): { answer: unknown } | undefined {
  const queue = (window as unknown as { __silentsiloDialogAnswers?: unknown[] })
    .__silentsiloDialogAnswers;
  return queue && queue.length > 0 ? { answer: queue.shift() } : undefined;
}

export const open = (async (options?: Parameters<typeof openNative>[0]) => {
  const queued = nextAnswer();
  return queued ? queued.answer : openNative(options);
}) as typeof openNative;

export const save = (async (options?: Parameters<typeof saveNative>[0]) => {
  const queued = nextAnswer();
  return queued ? queued.answer : saveNative(options);
}) as typeof saveNative;
