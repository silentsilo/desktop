/**
 * When the credential store may be re-read from disk.
 *
 * The panel keeps the whole store in memory and updates its list before the
 * write reaches the vault, so a reload landing between the two would show the
 * row as it was and read as the edit having been lost. A reload asked for
 * during a write is therefore deferred rather than dropped: a sync pass that
 * arrives mid-edit still has to reach the screen, just afterwards.
 *
 * Split out from the app so the ordering can be tested without a renderer.
 * Getting it wrong in either direction is invisible until it costs somebody
 * a password: refuse too much and the panel goes stale again, refuse too
 * little and a local edit flickers back to its old value.
 */
export class PasswordRefreshGate {
  private writes = 0;
  private owed = false;
  /** Counts writes started, so a read can tell one began while it ran. */
  private generation = 0;

  /** A write is starting. */
  beginWrite(): void {
    this.writes += 1;
    this.generation += 1;
  }

  /** A read is starting: the token its result is checked against. */
  startRead(): number {
    return this.generation;
  }

  /**
   * A read started at `token` has its answer. "apply" when no write began
   * since. Otherwise the answer may predate that write and would put the
   * old value back over the optimistic one: "later" when the write is still
   * running (the refresh is owed to its end), "again" when it has finished
   * and a fresh read should run now.
   */
  finishRead(token: number): "apply" | "later" | "again" {
    if (token === this.generation) return "apply";
    if (this.writes > 0) {
      this.owed = true;
      return "later";
    }
    return "again";
  }

  /**
   * A write has finished. Answers true when a refresh was asked for while
   * writes were in flight and the last of them has now finished.
   */
  endWrite(): boolean {
    this.writes = Math.max(0, this.writes - 1);
    if (this.writes > 0 || !this.owed) return false;
    this.owed = false;
    return true;
  }

  /**
   * Something asked for a reload. Answers true when it may happen now, and
   * remembers it for later when it may not.
   */
  requestRefresh(): boolean {
    if (this.writes > 0) {
      this.owed = true;
      return false;
    }
    return true;
  }
}
