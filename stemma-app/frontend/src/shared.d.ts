/// The descent's drawing, shared with the instance's pages.
declare module "*descent.js" {
  export function draw(figure: Element, descent: unknown, previous?: unknown): void;
  export function start(): void;
}
