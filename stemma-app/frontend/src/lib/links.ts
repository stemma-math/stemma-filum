// Addresses in plain words, so a message can make links of them.

/** A run of words, and the address it is when it is one. */
export type Piece = { text: string; href?: string };

/** HTTP(S) addresses, up to the first space or quote. */
const ADDRESS = /https?:\/\/[^\s<>"']+/g;

/** Punctuation that ends a sentence rather than the address before it. */
const TRAILING = /[.,;:!?)\]]+$/;

/** Splits words into plain runs and addresses, in order. */
export function pieces(words: string): Piece[] {
  const found: Piece[] = [];
  let from = 0;
  for (const match of words.matchAll(ADDRESS)) {
    const href = match[0].replace(TRAILING, "");
    const at = match.index;
    if (at > from) found.push({ text: words.slice(from, at) });
    found.push({ text: href, href });
    from = at + href.length;
  }
  if (from < words.length) found.push({ text: words.slice(from) });
  return found;
}
