import { Language, LanguageSupport } from "@codemirror/language";
import { Tag, styleTags, tagHighlighter, tags } from "@lezer/highlight";
import { typst_lezer, typstTags, type TypstLezerParser } from "codemirror-lang-typst/lezer";

/** Everything between the dollars of an equation. */
const math = Tag.define();

/** The Typst language of a line, highlighted and nothing more.
 *
 * `typst_lezer()` also installs a syntax linter and completions. Its
 * diagnostics would draw in the text, which is a separate decision from
 * highlighting, so only the language itself is taken. The package leaves
 * the names of arguments and the inside of equations untagged; in a line
 * those are the sigla, the `uses` and the mathematics, so they are tagged
 * here. */
const base = typst_lezer().language;
const parser = (base.parser as TypstLezerParser).configure([
  styleTags({ "Named/Ident": tags.attributeName, "Math/...": math }),
]);
export const typst = new LanguageSupport(new Language(base.data, parser, [], "typst"));

/** Class names the stylesheet colours, so the palette follows the theme.
 * The tags and their grouping are those of the package's own style, which
 * is the Typst web app's; the stylesheet gives each group that app's
 * colour in light and a matching one in dark. */
export const typstHighlighter = tagHighlighter([
  { tag: tags.comment, class: "tt-comment" },
  { tag: [tags.keyword, tags.null, tags.atom, tags.bool], class: "tt-keyword" },
  { tag: tags.number, class: "tt-number" },
  { tag: tags.string, class: "tt-string" },
  { tag: tags.function(tags.variableName), class: "tt-call" },
  { tag: typstTags.interpolated, class: "tt-hash" },
  { tag: [tags.attributeName, tags.propertyName], class: "tt-argument" },
  { tag: [tags.punctuation, tags.operator, tags.paren], class: "tt-punctuation" },
  { tag: math, class: "tt-math" },
  { tag: typstTags.mathDelimiter, class: "tt-dollar" },
  { tag: [tags.escape, tags.labelName], class: "tt-escape" },
  { tag: tags.heading, class: "tt-heading" },
  { tag: tags.strong, class: "tt-strong" },
  { tag: tags.emphasis, class: "tt-emphasis" },
  { tag: tags.link, class: "tt-link" },
  { tag: tags.monospace, class: "tt-raw" },
  { tag: typstTags.listMarker, class: "tt-marker" },
  { tag: tags.invalid, class: "tt-invalid" },
]);
