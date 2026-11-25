// The getting-started checklist: which steps a new person has done, read from what they
// have, and whether they have put it away.

/** Where a person is with setting up. */
export interface Progress {
  hasAccount: boolean;
  hasTransaction: boolean;
  hasBudget: boolean;
}

/** The steps, in order, each done or not. */
export function steps(progress: Progress): { key: keyof Progress; done: boolean }[] {
  return [
    { key: "hasAccount", done: progress.hasAccount },
    { key: "hasTransaction", done: progress.hasTransaction },
    { key: "hasBudget", done: progress.hasBudget },
  ];
}

/** Whether every step is done. */
export function isComplete(progress: Progress): boolean {
  return steps(progress).every((step) => step.done);
}

/** Remembered per browser: a person who has put the checklist away does not want it back. */
export const HIDDEN_KEY = "mt-getting-started-hidden";

export function readHidden(storage: Pick<Storage, "getItem"> | undefined = globalThis.localStorage): boolean {
  try {
    return storage?.getItem(HIDDEN_KEY) === "1";
  } catch (error) {
    console.warn("getting started: could not read whether the checklist was hidden", error);
    return false;
  }
}

export function saveHidden(storage: Pick<Storage, "setItem"> | undefined = globalThis.localStorage): void {
  try {
    storage?.setItem(HIDDEN_KEY, "1");
  } catch (error) {
    console.warn("getting started: could not remember hiding the checklist", error);
  }
}
