import { useLayoutEffect, useState } from 'react'

class LatestValue<T> {
  #value: T
  constructor(value: T) {
    this.#value = value
  }
  set(value: T) {
    this.#value = value
  }
  read = () => this.#value
}

/**
 * A stable getter for the value of the latest commit. For handlers that are
 * created once — an editor's extensions, a debounced saver — but must call the
 * newest callback or read the newest props when they eventually fire.
 */
export function useLatest<T>(value: T): () => T {
  const [box] = useState(() => new LatestValue(value))
  useLayoutEffect(() => box.set(value))
  return box.read
}
