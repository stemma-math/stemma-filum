// Keeping asynchronous work in order: writes one after another, only the
// latest answer shown, and listeners stopped with whoever started them.

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

/** Collects what stops a set of listeners, some of which are only known
 * once a promise settles. `stop` stops everything collected so far, and
 * anything that settles afterwards is stopped as soon as it does, so a
 * listener never outlives whoever started it. */
export function subscriptions() {
  const stops: (() => void)[] = [];
  let stopped = false;
  return {
    add(stop: (() => void) | Promise<() => void>) {
      void Promise.resolve(stop).then((one) => {
        if (stopped) one();
        else stops.push(one);
      });
    },
    stop() {
      stopped = true;
      for (const one of stops.splice(0)) one();
    },
  };
}
