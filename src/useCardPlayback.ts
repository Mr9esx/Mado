import { useCallback, useEffect, useRef, useState } from 'react'
import type { Reading } from './layout'

export type CardTrial = { from: Reading; to: Reading; holdMs: number }
export type CardPlaybackFrame = { playId: number; value: Reading }

export function useCardPlayback(scope: string | undefined) {
  const [active, setActive] = useState<(CardPlaybackFrame & { scope: string })>()
  const timer = useRef<number | undefined>(undefined)
  const nextId = useRef(0)
  const cancelTimer = useCallback(() => window.clearTimeout(timer.current), [])

  useEffect(() => {
    setActive(undefined)
    return cancelTimer
  }, [scope, cancelTimer])

  function stop() {
    cancelTimer()
    setActive(undefined)
  }

  function play(trial: CardTrial) {
    if (scope === undefined) return
    cancelTimer()
    const playId = ++nextId.current
    setActive({ scope, playId, value: trial.from })
    timer.current = window.setTimeout(() => {
      setActive({ scope, playId, value: trial.to })
      timer.current = window.setTimeout(() => setActive(undefined), trial.holdMs)
    }, 150)
  }

  return { frame: active?.scope === scope ? active : undefined, play, stop }
}
