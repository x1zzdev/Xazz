import { useCallback, useEffect, useRef, useState } from 'react'
import { ApiError } from './api'

/**
 * One fetch lifecycle for every server-backed panel: `loading` → `ready`, or
 * `error` (the server answered non-2xx) vs `offline` (it was not reached). Keeping
 * those two apart is the point — "the server refused" and "there is no server" call
 * for different next actions. `revision` re-fetches after a run or an access change;
 * `reload` re-fetches on demand. Stale data is dropped while loading so a previous
 * tenant's values can never sit under a new tenant's heading.
 */
export function useServerData(load, revision) {
  const [state, setState] = useState({ status: 'loading' })
  const [nonce, setNonce] = useState(0)
  const currentRevision = useRef(revision)
  currentRevision.current = revision

  useEffect(() => {
    let live = true
    setState({ status: 'loading' })
    load().then(
      (data) => live && setState({ status: 'ready', data }),
      (error) =>
        live && setState({ status: error instanceof ApiError ? 'error' : 'offline', error }),
    )
    return () => {
      live = false
    }
    // `load` is a module-level api function; identity changes must not refetch.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [revision, nonce])

  const reload = useCallback(() => setNonce((value) => value + 1), [])
  const replace = useCallback((data) => {
    if (currentRevision.current === revision) setState({ status: 'ready', data })
  }, [revision])
  return [state, reload, replace]
}
