// The line's descent, drawn as a stemma codicum.
//
// Orthogonal descent in hairlines and sigla: definitions in small caps,
// statements the line rests on ruled, black boxes as hatched bands, and
// bridges spanning two definitions on their own level. Nothing here is
// decorative — every mark is a thing the source declares.
//
// One file, two surfaces. The instance's pages hand it a placement in
// the page and it draws them on load; the application imports `draw`
// and calls it while somebody types. Two drawings of the same graph
// would start to differ the first time either was touched.
//
// Without JavaScript the page keeps its list of what would be drawn;
// `start` marks a figure `descent--drawn` only once it is on screen.

const LEVEL = 78;
const CELL = 168;
const NODE = 132;
const BAND = 22;
const EDGE = 16;

/**
 * How long the one animation in the product lasts.
 *
 * Promoting a lemma out of a box splits that box in two, and the
 * reader has to follow which half is which. It is the only motion here
 * that explains something rather than decorating an arrival.
 */
const SPLIT = 300;

/**
 * Colours and faces read from the identity's tokens, with the same
 * values as fallbacks.
 */
const palette = (figure) => {
    const styles = getComputedStyle(figure);
    const token = (name, fallback) => styles.getPropertyValue(name).trim() || fallback;

    return {
        ink: token("--ink", "#101d2f"),
        quiet: token("--ink-3", "#758298"),
        rule: token("--rule", "#d5dfea"),
        soft: token("--rule-soft", "#e6edf4"),
        ditto: token("--ditto", "#2d6de7"),
        verified: token("--verified", "#0c7a66"),
        paper: token("--paper", "#f1f5f9"),
        mono: token("--mono", "monospace"),
        grotesk: token("--grotesk", "sans-serif"),
    };
};

/** Where one placed thing sits on the canvas. */
const box = (placed) => {
    const step = Math.max(CELL, NODE + 24);

    return {
        x: 24 + placed.across * step,
        y: 24 + placed.level * LEVEL,
        width: NODE,
        height: placed.what.shown === "box" ? BAND : 30,
    };
};

/** What a descent rests on, by siglum. */
const central = (descent) =>
    descent.placed.filter((placed) => placed.what.shown === "central").map((placed) => placed.what.siglum);

/** Everything inside a box, which the graph never shows. */
const interior = (descent) =>
    new Set(
        descent.placed.flatMap((placed) =>
            placed.what.shown === "box" ? placed.what.black_box.interior : [],
        ),
    );

/**
 * The statement that was just promoted out of a box, if one was.
 *
 * One statement, central now and inside a box before: that is a
 * promotion and nothing else is.
 */
const promoted = (before, after) => {
    const was = new Set(central(before));
    const gained = central(after).filter((siglum) => !was.has(siglum));

    if (gained.length !== 1 || !interior(before).has(gained[0])) {
        return null;
    }

    return after.placed.findIndex(
        (placed) => placed.what.shown === "central" && placed.what.siglum === gained[0],
    );
};

/** Whether this reader asked for no motion. */
const still = () =>
    typeof window.matchMedia === "function" &&
    window.matchMedia("(prefers-reduced-motion: reduce)").matches;

/**
 * Draws a descent, and parts the band when a lemma has just been
 * promoted out of it.
 */
const animations = new WeakMap();

export function draw(figure, descent, previous) {
    const pending = animations.get(figure);
    if (pending !== undefined) window.cancelAnimationFrame(pending);
    animations.delete(figure);
    if (!descent || !descent.placed || !descent.placed.length) {
        const canvas = figure.querySelector(".descent__canvas");

        if (canvas) {
            canvas.width = 0;
            canvas.height = 0;
        }

        return;
    }

    const split = previous ? promoted(previous, descent) : null;

    if (split === null || still()) {
        paint(figure, descent, 1, null);

        return;
    }

    const began = performance.now();
    const step = (now) => {
        animations.delete(figure);
        if (!figure.isConnected) return;
        const through = Math.min(1, (now - began) / SPLIT);

        paint(figure, descent, through, split);

        if (through < 1) {
            animations.set(figure, window.requestAnimationFrame(step));
        }
    };

    animations.set(figure, window.requestAnimationFrame(step));
}

/**
 * One frame.
 *
 * `through` runs from nothing to one while a box is parting; `parting`
 * is the statement arriving between its halves. At rest they are one
 * and nothing.
 */
function paint(figure, descent, through, parting) {
    const canvas = figure.querySelector(".descent__canvas");
    if (!canvas) return;

    const pen = canvas.getContext("2d");
    if (!pen) return;

    const colours = palette(figure);
    const boxes = descent.placed.map(box);
    const right = Math.max(...boxes.map((at) => at.x + at.width)) + 24;
    const bottom = Math.max(...boxes.map((at) => at.y + at.height)) + 24;
    const ratio = window.devicePixelRatio || 1;

    if (canvas.width !== Math.round(right * ratio) || canvas.height !== Math.round(bottom * ratio)) {
        canvas.width = right * ratio;
        canvas.height = bottom * ratio;
        canvas.style.width = `${right}px`;
        canvas.style.height = `${bottom}px`;
    }

    // While the band parts, everything below the arriving statement
    // slides down into place from where it stood.
    const arriving = parting === null ? null : descent.placed[parting];

    if (arriving) {
        const closed = (1 - through) * LEVEL;

        descent.placed.forEach((placed, at) => {
            if (placed.level >= arriving.level) {
                boxes[at] = { ...boxes[at], y: boxes[at].y - closed };
            }
        });
    }

    pen.setTransform(1, 0, 0, 1, 0, 0);
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

    // A bridge spans; it never hangs. Drawn between the two definitions
    // on their own level, with its registration frame.
    descent.spans.forEach((span) => {
        const left = boxes[span.left];
        const right = boxes[span.right];
        if (!left || !right) return;

        const nearer = left.x <= right.x ? span.left : span.right;
        const [near, far] = left.x <= right.x ? [left, right] : [right, left];

        pen.font = `13px ${colours.grotesk}`;
        pen.fontVariantCaps = "small-caps";
        const written = pen.measureText((descent.placed[nearer].what.siglum || "").toLowerCase()).width;
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

    descent.placed.forEach((placed, at) => {
        const where = boxes[at];
        const arriving = at === parting;

        if (placed.what.shown === "box") {
            hatch(pen, colours, where, placed.what.black_box.state);
            return;
        }

        const text = placed.what.siglum;

        if (placed.what.shown === "central") {
            // Weight is human commitment, and a central statement is
            // the one thing in a line somebody committed to. A rule
            // that is arriving draws itself in from the middle.
            const half = (where.width / 2) * (arriving ? through : 1);
            const middle = where.x + where.width / 2;

            pen.strokeStyle = colours.ditto;
            pen.lineWidth = 1.6;
            pen.beginPath();
            pen.moveTo(middle - half, Math.round(where.y) + 0.5);
            pen.lineTo(middle + half, Math.round(where.y) + 0.5);
            pen.stroke();
            pen.lineWidth = 1;

            pen.globalAlpha = arriving ? through : 1;
            pen.fillStyle = colours.ink;
            pen.font = `13px ${colours.mono}`;
            pen.fillText(text, where.x, where.y + 18);

            if (placed.what.verified) {
                pen.fillStyle = colours.verified;
                pen.font = `10px ${colours.grotesk}`;
                pen.fillText("Lean verified", where.x, where.y + 30);
            }

            pen.globalAlpha = 1;

            return;
        }

        pen.fillStyle = colours.ink;
        pen.font = `13px ${colours.grotesk}`;
        pen.fontVariantCaps = "small-caps";
        pen.fillText(text.toLowerCase(), where.x, where.y + 18);
        pen.fontVariantCaps = "normal";
    });
}

/** A black box: a hatched band between its two fixed statements. */
function hatch(pen, colours, where, state) {
    const angle = state === "filled_by_agent" ? -1 : 1;

    // The band sits on the paper rather than over the descent, so no
    // hairline shows through the hatch.
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
}

/** A registration frame: four corner marks and no box. */
function corners(pen, colours, x, y, width, height) {
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
}

/** Draws whatever placements a page carries. */
export function start() {
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
}

// A page that carries a placement is a page that wants it drawn. The
// application imports `draw` instead and there is nothing here to find.
if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", start);
} else {
    start();
}
