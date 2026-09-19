// The line's descent, drawn as a stemma codicum.
//
// Orthogonal descent in hairlines and sigla: definitions in small caps,
// statements the line rests on ruled, black boxes as hatched bands, and
// bridges spanning two definitions on their own level. Nothing here is
// decorative — every mark is a thing the source declares.
//
// With this script switched off the page keeps the list of what would
// have been drawn, which is why the list is taken away only once the
// drawing is actually on screen.

(() => {
    const LEVEL = 78;
    const CELL = 168;
    const NODE = 132;
    const BAND = 22;
    const EDGE = 16;

    /// The ink this theme is drawn in, read from the stylesheet so the
    /// drawing is never a second opinion about the palette.
    const palette = (figure) => {
        const styles = getComputedStyle(figure);
        const ink = (name, fallback) =>
            styles.getPropertyValue(name).trim() || fallback;

        return {
            ink: ink("--ink", "#16191a"),
            quiet: ink("--ink-3", "#636967"),
            rule: ink("--rule", "#c5c8c0"),
            soft: ink("--rule-soft", "#d7dad2"),
            ditto: ink("--ditto", "#463a85"),
            verified: ink("--verified", "#1d5347"),
            paper: ink("--paper", "#edeee8"),
            mono: ink("--mono", "monospace"),
            grotesk: ink("--grotesk", "sans-serif"),
        };
    };

    /// Where one placed thing sits on the canvas.
    const box = (placed, width) => {
        const columns = Math.max(1, width);
        const step = Math.max(CELL, NODE + 24);
        const x = 24 + placed.across * step;
        const y = 24 + placed.level * LEVEL;

        return { x, y, width: NODE, height: placed.what.shown === "box" ? BAND : 30, columns };
    };

    const draw = (figure, descent) => {
        const canvas = figure.querySelector(".descent__canvas");
        if (!canvas || !descent || !descent.placed.length) return;

        const colours = palette(figure);
        const boxes = descent.placed.map((placed) => box(placed, descent.width));
        const right = Math.max(...boxes.map((at) => at.x + at.width)) + 24;
        const bottom = Math.max(...boxes.map((at) => at.y + at.height)) + 24;
        const ratio = window.devicePixelRatio || 1;

        canvas.width = right * ratio;
        canvas.height = bottom * ratio;
        canvas.style.width = `${right}px`;
        canvas.style.height = `${bottom}px`;

        const pen = canvas.getContext("2d");
        if (!pen) return;

        pen.scale(ratio, ratio);
        pen.clearRect(0, 0, right, bottom);
        pen.lineWidth = 1;
        pen.textBaseline = "alphabetic";

        // Every descent line first, so no hairline crosses a siglum.
        descent.edges.forEach((edge) => {
            const from = boxes[edge.from];
            const to = boxes[edge.to];
            if (!from || !to) return;

            const startY = from.y + from.height;
            const endY = to.y;
            const channel = endY - EDGE / 2;
            const startX = Math.round(from.x + from.width / 2) + 0.5;
            const endX = Math.round(to.x + to.width / 2) + 0.5;

            pen.strokeStyle = colours.rule;
            pen.beginPath();
            pen.moveTo(startX, startY);
            pen.lineTo(startX, channel);
            pen.lineTo(endX, channel);
            pen.lineTo(endX, endY);
            pen.stroke();
        });

        // A bridge spans; it never hangs. Drawn between the two
        // definitions on their own level, with its registration frame.
        descent.spans.forEach((span) => {
            const left = boxes[span.left];
            const right = boxes[span.right];
            if (!left || !right) return;

            const [near, far] = left.x <= right.x ? [left, right] : [right, left];

            pen.font = `13px ${colours.grotesk}`;
            pen.fontVariantCaps = "small-caps";
            const written = pen.measureText(
                (descent.placed[left.x <= right.x ? span.left : span.right].what.siglum || "")
                    .toLowerCase(),
            ).width;
            pen.fontVariantCaps = "normal";

            const y = Math.round(near.y + 14) + 0.5;
            const from = Math.round(near.x + written + 10);
            const to = Math.round(far.x - 10);
            if (to - from < 12) return;

            pen.strokeStyle = colours.ink;
            pen.lineWidth = 1.6;
            pen.beginPath();
            pen.moveTo(from, y);
            pen.lineTo(to, y);
            pen.stroke();
            pen.lineWidth = 1;

            corners(pen, colours, from, y - 7, to - from, 14);
        });

        desced(pen, colours, descent, boxes);
    };

    /// The things themselves.
    const desced = (pen, colours, descent, boxes) => {
        descent.placed.forEach((placed, at) => {
            const where = boxes[at];

            if (placed.what.shown === "box") {
                hatch(pen, colours, where, placed.what.black_box.state);
                return;
            }

            const text = placed.what.siglum;

            if (placed.what.shown === "central") {
                // Weight is human commitment, and a central statement is
                // the one thing in a line somebody committed to.
                pen.strokeStyle = colours.ditto;
                pen.lineWidth = 1.6;
                pen.beginPath();
                pen.moveTo(where.x, Math.round(where.y) + 0.5);
                pen.lineTo(where.x + where.width, Math.round(where.y) + 0.5);
                pen.stroke();
                pen.lineWidth = 1;

                pen.fillStyle = colours.ink;
                pen.font = `13px ${colours.mono}`;
                pen.fillText(text, where.x, where.y + 18);

                if (placed.what.verified) {
                    pen.fillStyle = colours.verified;
                    pen.font = `10px ${colours.grotesk}`;
                    pen.fillText("Lean verified", where.x, where.y + 30);
                }

                return;
            }

            pen.fillStyle = colours.ink;
            pen.font = `13px ${colours.grotesk}`;
            pen.fontVariantCaps = "small-caps";
            pen.fillText(text.toLowerCase(), where.x, where.y + 18);
            pen.fontVariantCaps = "normal";
        });
    };

    /// A black box: a hatched band between its two fixed statements.
    const hatch = (pen, colours, where, state) => {
        const angle = state === "filled_by_agent" ? -1 : 1;

        // The band sits on the paper rather than over the descent, so
        // no hairline shows through the hatch.
        pen.fillStyle = colours.paper;
        pen.fillRect(where.x, where.y, where.width, where.height);

        pen.save();
        pen.beginPath();
        pen.rect(where.x, where.y, where.width, where.height);
        pen.clip();

        pen.strokeStyle = colours.soft;
        for (let at = -where.height; at < where.width + where.height; at += 6) {
            pen.beginPath();
            pen.moveTo(where.x + at, where.y + (angle > 0 ? where.height : 0));
            pen.lineTo(where.x + at + where.height, where.y + (angle > 0 ? 0 : where.height));
            pen.stroke();
        }
        pen.restore();

        pen.strokeStyle = state === "verified" ? colours.verified : colours.rule;
        pen.strokeRect(
            Math.round(where.x) + 0.5,
            Math.round(where.y) + 0.5,
            where.width - 1,
            where.height - 1,
        );
    };

    /// A registration frame: four corner marks and no box.
    const corners = (pen, colours, x, y, width, height) => {
        const arm = 5;
        pen.strokeStyle = colours.quiet;

        [
            [x, y, 1, 1],
            [x + width, y, -1, 1],
            [x, y + height, 1, -1],
            [x + width, y + height, -1, -1],
        ].forEach(([cx, cy, dx, dy]) => {
            pen.beginPath();
            pen.moveTo(Math.round(cx) + 0.5, Math.round(cy + dy * arm) + 0.5);
            pen.lineTo(Math.round(cx) + 0.5, Math.round(cy) + 0.5);
            pen.lineTo(Math.round(cx + dx * arm) + 0.5, Math.round(cy) + 0.5);
            pen.stroke();
        });
    };

    const start = () => {
        document.querySelectorAll(".descent").forEach((figure) => {
            const block = figure.querySelector(".descent__placement");
            if (!block) return;

            let descent;
            try {
                descent = JSON.parse(block.textContent || "null");
            } catch {
                return;
            }
            if (!descent) return;

            draw(figure, descent);
            figure.classList.add("descent--drawn");

            let again;
            window.addEventListener("resize", () => {
                window.clearTimeout(again);
                again = window.setTimeout(() => draw(figure, descent), 120);
            });
            window
                .matchMedia("(prefers-color-scheme: dark)")
                .addEventListener("change", () => draw(figure, descent));
        });
    };

    if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", start);
    } else {
        start();
    }
})();
