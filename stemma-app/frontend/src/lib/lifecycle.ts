/** Runs work in submission order; a failure does not block later work. */
export function serially() {
  let pending: Promise<unknown> = Promise.resolve();
  return <T>(work: () => Promise<T>): Promise<T> => {
    const result = pending.then(work);
    pending = result.catch(() => {});
    return result;
  };
}

/** Each call starts a request and returns a check that stays true only
 * until the next call, so a stale answer can be dropped. */
export function latestRequest() {
  let generation = 0;
  return () => {
    const started = ++generation;
    return () => started === generation;
  };
}
