export function createExclusiveProgressPause(stopProgress, startProgress) {
  let tail = Promise.resolve()

  return async function withPausedProgress(operation) {
    const previous = tail
    let release
    tail = new Promise((resolve) => { release = resolve })
    await previous
    try {
      await stopProgress()
      return await operation()
    } finally {
      try {
        await startProgress()
      } finally {
        release()
      }
    }
  }
}
