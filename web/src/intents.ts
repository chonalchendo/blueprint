// The five Moving groups and the intents each holds (32, design change 7). The first two owe the human a decision.
import type { Intent } from "./types/Intent";

export const GROUPS: { group: string; intents: Intent[] }[] = [
  { group: "needs landing", intents: ["needs-landing"] },
  { group: "needs acceptance", intents: ["needs-acceptance", "needs-milestone-approval", "re-asked"] },
  { group: "blocked", intents: ["blocked", "drifted"] },
  { group: "ready", intents: ["ready"] },
  {
    group: "working",
    intents: ["working", "waiting", "needs-review", "needs-response", "needs-approval", "needs-derivation", "needs-shaping"],
  },
];

/** The groups that owe a decision. */
export const DECISION_GROUPS = GROUPS.slice(0, 2);

/** The other three. */
export const OTHER_GROUPS = GROUPS.slice(2);

export type Counts = { [key in Intent]?: number };

/** The nodes in `groups`, from a page's count per intent. */
export function count(intents: Counts, groups = GROUPS): number {
  return groups.flatMap((g) => g.intents).reduce((n, i) => n + (intents[i] ?? 0), 0);
}

/** An intent as words: `needs-landing` reads "needs landing"; `re-asked` keeps its hyphen. */
export function words(intent: string): string {
  return intent === "re-asked" ? intent : intent.replace(/-/g, " ");
}

/** A count as words (19, principle 7): a zero keeps its noun ("no designs"); a bare zero is "nothing". */
export function many(n: number, noun?: string): string {
  if (!noun) return n ? String(n) : "nothing";
  const nouns = n === 1 ? noun : `${noun}s`;
  return n ? `${n} ${nouns}` : `no ${nouns}`;
}
