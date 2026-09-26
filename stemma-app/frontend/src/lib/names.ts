// How people and groups are named in a small space.

/** Up to two initials: the first letters of the first two words, which a
 * group's short name separates with hyphens. */
export function initials(name: string): string {
  const words = name.replace(/^[a-z]+:/, "").split(/[\s_-]+/).filter(Boolean);
  const letters = words.length > 1 ? words[0][0] + words[1][0] : (words[0] ?? "S").slice(0, 1);
  return letters.toUpperCase();
}
