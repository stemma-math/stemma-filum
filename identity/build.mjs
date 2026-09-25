// Builds the token outputs from `tokens.json`.
//
//     node build.mjs          write tokens.css and tokens.typ
//     node build.mjs --check  fail if either differs from what would be written

import { readFile, writeFile } from "node:fs/promises";
import StyleDictionary from "style-dictionary";

const HEADER = "Generated from tokens.json by build.mjs. Do not edit.";

// The first level of a path is the token's category, not part of its name.
const nameOf = (path) => path.slice(1).join("-");

// A value that is one reference, as `{size.step-3}`, becomes the custom
// property it refers to so the output keeps the relation.
const reference = /^\{([^}]+)\}$/;
const cssValue = (token) => {
    const original = token.original.$value;
    const match = typeof original === "string" && original.match(reference);
    return match ? `var(--${nameOf(match[1].split("."))})` : token.$value;
};

const declarations = (tokens, name) =>
    tokens.flatMap((token) => [
        ...(token.$description ? [`    /* ${token.$description} */`] : []),
        `    --${name(token)}: ${cssValue(token)};`,
    ]);

const css = ({ dictionary }) => {
    const plain = dictionary.allTokens.filter((token) => token.path[0] !== "density");
    const density = (which) =>
        dictionary.allTokens.filter((token) => token.path[0] === "density" && token.path[1] === which);
    const densityName = (token) => token.path.slice(2).join("-");
    return [
        `/* ${HEADER} */`,
        "",
        ":root {",
        ...declarations(plain, (token) => token.name),
        "}",
        "",
        ":root,",
        '[data-density="comfortable"] {',
        ...declarations(density("comfortable"), densityName),
        "}",
        "",
        '[data-density="compact"] {',
        ...declarations(density("compact"), densityName),
        "}",
        "",
    ].join("\n");
};

// Typst has no pixels and no generic families, so only colours and the named
// faces cross over.
const generic = new Set(["serif", "sans-serif", "monospace", "ui-monospace"]);
const typst = ({ dictionary }) => {
    const lines = [`// ${HEADER}`, ""];
    for (const token of dictionary.allTokens) {
        if (token.path[0] === "color") {
            lines.push(`#let ${token.name} = rgb("${token.$value}")`);
        } else if (token.path[0] === "font") {
            const faces = token.original.$value.filter((face) => !generic.has(face));
            lines.push(`#let ${token.name} = (${faces.map((face) => `"${face}"`).join(", ")})`);
        }
    }
    return lines.join("\n") + "\n";
};

const dictionary = new StyleDictionary({
    source: ["tokens.json"],
    usesDtcg: true,
    log: { verbosity: "silent" },
    hooks: {
        transforms: {
            "name/stemma": { type: "name", transform: (token) => nameOf(token.path) },
        },
        formats: { "stemma/css": css, "stemma/typst": typst },
    },
    platforms: {
        css: {
            transforms: ["name/stemma", "fontFamily/css", "shadow/css/shorthand"],
            files: [{ destination: "tokens.css", format: "stemma/css" }],
        },
        typst: {
            transforms: ["name/stemma"],
            files: [{ destination: "tokens.typ", format: "stemma/typst" }],
        },
    },
});

const outputs = [
    ...(await dictionary.formatPlatform("css")),
    ...(await dictionary.formatPlatform("typst")),
];

if (process.argv.includes("--check")) {
    const stale = [];
    for (const { destination, output } of outputs) {
        const current = await readFile(destination, "utf8").catch(() => null);
        if (current !== output) stale.push(destination);
    }
    if (stale.length) {
        console.error(`Out of date: ${stale.join(", ")}. Run \`npm run build\` in identity/.`);
        process.exit(1);
    }
} else {
    for (const { destination, output } of outputs) await writeFile(destination, output);
}
