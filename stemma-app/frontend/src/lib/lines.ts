// Telling lines apart, and knowing when what the rail shows is out of date.

import type { LineState, Mirrored } from "./types";

/** A line named by its group, repository and name. */
export type Named = { group: string; side: string; name: string };

/** A line as the engine names it in its reports: `group/side/name`. */
export function lineLabel(line: Named): string {
  return `${line.group}/${line.side}/${line.name}`;
}

/** Whether two descriptions name the same line. */
export function sameLine(one: Named, other: Named): boolean {
  return one.group === other.group && one.side === other.side && one.name === other.name;
}

/** Whether a round of the engine changed where any line the rail shows
 * stands. A line the engine does not list is sent. */
export function statesChanged(shown: (Named & { state: LineState })[], mirrored: Mirrored): boolean {
  const states = new Map(mirrored.lines.map((line) => [lineLabel(line), line.state]));
  return shown.some((line) => (states.get(lineLabel(line)) ?? "sent") !== line.state);
}

/** The lines a round of the engine changed on this machine, by label. */
export function touchedLines(mirrored: Mirrored): Set<string> {
  const report = mirrored.report;
  return new Set([...report.arrived, ...report.merged, ...report.gone]);
}
