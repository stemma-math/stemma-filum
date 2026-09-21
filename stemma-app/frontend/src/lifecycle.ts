/** Escape text for both HTML content and quoted attributes. */
export function escapeHtml(text: string): string {
  return text.replace(/[&<>"']/g, (character) => ({
    "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;",
  })[character]!);
}

/** Run writes in submission order, without poisoning the queue after a failure. */
export function serially() {
  let pending: Promise<unknown> = Promise.resolve();
  return <T>(work: () => Promise<T>): Promise<T> => {
    const result = pending.then(work);
    pending = result.catch(() => {});
    return result;
  };
}

/** A newer request invalidates every earlier answer, including pending failures. */
export function latestRequest() {
  let generation = 0;
  return () => {
    const started = ++generation;
    return () => started === generation;
  };
}
