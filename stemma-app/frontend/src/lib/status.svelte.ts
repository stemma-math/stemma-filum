/** The message bar at the bottom of the window. */
export const status = $state({ message: "" });

/** Replaces what the message bar says; an empty message hides it. */
export function tell(message: string) {
  status.message = message;
}
