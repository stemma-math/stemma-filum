// The smallest single edit between two texts.

/** The one change that turns `before` into `after`: only the part between
 * their common beginning and end, so the cursor outside it stays put. */
export function difference(before: string, after: string) {
  let start = 0;
  while (start < before.length && start < after.length && before[start] === after[start]) start += 1;
  let end = 0;
  while (
    end < before.length - start && end < after.length - start
    && before[before.length - 1 - end] === after[after.length - 1 - end]
  ) end += 1;
  return { from: start, to: before.length - end, insert: after.slice(start, after.length - end) };
}
