// Small pieces of the page the editor's components share.

/** The card a block is drawn in, when it is on the page. */
export function blockCard(id: string): HTMLElement | null {
  return document.querySelector<HTMLElement>(`[data-block="${id}"]`);
}

/** The text editor inside a block's card, to give it the keyboard. */
export function blockText(id: string): HTMLElement | null {
  return blockCard(id)?.querySelector<HTMLElement>(".cm-content") ?? null;
}

/** Leaves a one-line field on Enter, which commits what was typed in it. */
export function blurOnEnter(event: KeyboardEvent & { currentTarget: HTMLElement }) {
  if (event.key === "Enter") event.currentTarget.blur();
}
