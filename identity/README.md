# Stemma identity

The single source of Stemma's look. It is both a Rust crate
(`stemma-identity`), which the server embeds, and an npm package
(`@stemma-math/identity`), which the desktop frontend imports.

## Layout

| Path | What it holds |
|---|---|
| `tokens/` | `tokens.json`, the generator `build.mjs`, and the `tokens.css` and `tokens.typ` it writes |
| `css/` | `fonts.css` (the `@font-face` rules) and `components.css` (the shared components) |
| `fonts/` | The faces, subset to Latin, each beside its licence |
| `mark/` | The mark for dark and light surfaces, drawn live (`animated.js`) and still (`symbol.webp`) |
| `images/` | The line glyphs (`glyphs.svg`) and the surface texture (`noise.svg`) |
| `js/` | The descent drawing (`descent.js`) and its types |
| `icons/` | The application icons (`app/`) and the site's favicons and home-screen icons (`web/`) |
| `src/lib.rs` | The Rust crate: every file above that the server serves, as a constant |

The npm package exports its files under flat names
(`@stemma-math/identity/tokens.css`, `…/mark-on-dark.svg`, and so on; see
`exports` in `package.json`), so a consumer does not depend on this layout.

## Tokens

`tokens/tokens.json` holds every colour, face, text role, type step,
spacing, radius, layout width, shadow, duration and control density in the
Design Tokens Community Group format. Style Dictionary generates
`tokens/tokens.css` (custom properties) and `tokens/tokens.typ` (colours
and faces for Typst) from it:

```sh
npm ci           # once
npm run build    # after editing tokens.json
npm run check    # fails while the generated files are out of date
```

Neither stylesheet in `css/` defines tokens of its own. The `dark` group
redefines the semantic colours for the dark appearance, which follows the
system unless the root says `data-theme="light"` or `data-theme="dark"`.

## Components

`css/components.css` holds the shared components as classes over native
HTML, in cascade layers so each surface's own rules win: `.button` (with
`--primary`, `--quiet`, `--danger`, `--link` and `--icon`), `.field`,
`.input`, `.choice`, `.fieldset`, `.chip`, `.count`, `.avatar`, `.badge`,
`.card`, `.table`, `.menu`, `.dialog`, `.message`, `.toast`,
`.tooltip`, `.segmented`, `.skeleton`, `.spinner`, `.label` and the
wordmark. Controls are comfortable by default and compact inside
`[data-density="compact"]`.

## Faces

`css/fonts.css` and `fonts/` carry Instrument Sans (the interface), Inter
(the wordmark), JetBrains Mono (identifiers) and Libertinus Serif
(mathematics), each beside its SIL Open Font License. The stylesheet refers
to the faces as `../fonts/<file>`, so whoever serves it serves it from a
directory beside the faces.

## Descent drawing

`js/descent.js` draws a line's descent graph from the placement
`stemma_core::descent` computes. Neither surface shows the graph at present.
