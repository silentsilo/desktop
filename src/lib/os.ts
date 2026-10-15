/** Whether this window runs on Windows, by the webview's own agent string.
 * For telling apart OS error numbers, which mean other things there. */
export function onWindows(): boolean {
  return typeof navigator !== "undefined" && /windows/i.test(navigator.userAgent);
}

/** Whether this window runs on a Mac, for the key names a Mac uses. */
export function onMac(): boolean {
  return typeof navigator !== "undefined" && /mac os x|macintosh/i.test(navigator.userAgent);
}
