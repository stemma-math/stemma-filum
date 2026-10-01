// The typeset line with a proposal laid over it: one page, the proposal
// applied, every block it changes marked where it stands, what it removes
// left in place and struck, and what an edited block said before a click
// away. It is built here and shown in a frame where nothing runs.

import { changeLabel, type ReviewedBlock } from "./review";

/** What the marks look like, in the identity's colours, so they follow
 * the page's appearance. */
const STYLE = `
.review-change {
  --mark: var(--accent);
  --mark-bg: var(--accent-soft-bg);
  margin: 1.4em -16px;
  padding: 10px 16px 12px 14px;
  border-left: 3px solid var(--mark);
  border-radius: 0 10px 10px 0;
  background: var(--mark-bg);
}
.review-change--added { --mark: var(--success-icon); --mark-bg: var(--success-bg); }
.review-change--removed { --mark: var(--danger-fg); --mark-bg: var(--danger-bg); }
.review-change--outdated { --mark: var(--warning-fg); --mark-bg: var(--warning-bg); }
.review-change > .stemma-line__block { margin: 0; }
.review-change--removed > .stemma-line__block { opacity: 0.55; text-decoration: line-through; }
.review-tag {
  margin: 0 0 6px;
  font: 600 11px/1.2 var(--font-ui);
  letter-spacing: 0.06em;
  text-transform: uppercase;
  color: var(--mark);
}
.review-was { margin-top: 10px; }
.review-was > summary {
  font: 500 13px/1.4 var(--font-ui);
  color: var(--ink-3);
  cursor: pointer;
}
.review-was > .stemma-line__block {
  margin: 8px 0 0;
  padding-top: 8px;
  border-top: 1px dashed var(--border-strong);
  opacity: 0.7;
}
`;

/** The proposed page, marked against the current one. */
export function reviewPage(before: string, after: string, rows: ReviewedBlock[], theme: string | undefined): string {
  const parser = new DOMParser();
  const was = parser.parseFromString(before, "text/html");
  const now = parser.parseFromString(after, "text/html");
  const main = now.querySelector("main.stemma-line");
  if (!main) return after;

  let placed: Element | null = null;
  for (const row of rows) {
    const old = was.getElementById(row.id);
    const restored = !now.getElementById(row.id) && row.change === "removed" && old
      ? now.importNode(old, true) as Element : null;
    if (restored) {
      if (placed) placed.after(restored);
      else {
        const first = main.querySelector(".stemma-line__block");
        if (first) first.before(restored);
        else main.append(restored);
      }
    }
    const section: Element | null = restored ?? now.getElementById(row.id);
    if (!section) continue;
    placed = section;
    if (row.change === "kept" && !row.outdated) continue;

    const mark = now.createElement("div");
    const kind = row.outdated ? "outdated" : row.change;
    mark.className = `review-change review-change--${kind}`;
    const tag = now.createElement("p");
    tag.className = "review-tag";
    tag.textContent = changeLabel(row);
    section.before(mark);
    mark.append(tag, section);
    if (row.change === "edited" && old) {
      const earlier = now.createElement("details");
      earlier.className = "review-was";
      const summary = now.createElement("summary");
      summary.textContent = "Previous version";
      const copy = now.importNode(old, true) as Element;
      copy.removeAttribute("id");
      earlier.append(summary, copy);
      mark.append(earlier);
    }
    placed = mark;
  }

  const style = now.createElement("style");
  style.textContent = STYLE;
  now.head.append(style);
  if (theme) now.documentElement.dataset.theme = theme;
  return `<!doctype html>\n${now.documentElement.outerHTML}`;
}
