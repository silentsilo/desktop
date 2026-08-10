/**
 * One confirmation on screen at a time, and the question already there wins.
 *
 * Split out from the app so the rule can be tested without a renderer: what
 * matters is not how the dialog looks but what happens to the promise behind
 * the one it would have replaced.
 *
 * The dialog is a single slot. A second request used to overwrite the first
 * in place, at the same position and with the same buttons, so the person
 * reading "delete these permanently?" saw it turn into a question about disk
 * space and pressed Confirm on that. Worse, the first promise never settled,
 * so the deletion they believed they had answered simply never ran and never
 * reported.
 */

export type ConfirmSlotDecision =
  /** Nothing is open: show this one. */
  | { action: "show" }
  /** Something is already open: decline this one and leave the screen alone. */
  | { action: "decline" };

export function decideConfirmSlot(somethingIsOpen: boolean): ConfirmSlotDecision {
  // Declined rather than queued. Every caller reads a decline as "do not do
  // the thing", which is the safe answer to a question nobody got to read,
  // and a queue would show the second question after the first is answered,
  // by which time its context has gone.
  return somethingIsOpen ? { action: "decline" } : { action: "show" };
}
