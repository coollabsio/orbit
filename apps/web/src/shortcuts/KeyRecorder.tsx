import { detectPlatform, useHotkeySequenceRecorder } from '@tanstack/react-hotkeys'
import { useEffect, useRef } from 'react'
import { fromRecorded } from './bindings'
import { ShortcutKeys } from './Shortcut'
import { useSuspendShortcuts } from './useCommand'

/** How long the recorder waits for a second key before it takes a single key as the shortcut. */
const SECOND_KEY_WAIT_MS = 1000

/** Listens for the next keys and reports them in registry notation: one combination, or a sequence of two keys.
 *  Esc cancels; Backspace asks for "no shortcut". Every other shortcut is off while it listens. */
export function KeyRecorder({ onRecord, onCancel, onClear }: { onRecord: (keys: string) => void; onCancel: () => void; onClear: () => void }) {
  useSuspendShortcuts()
  const mac = detectPlatform() === 'mac'
  const recorder = useHotkeySequenceRecorder({
    recordBy: 'code',
    idleTimeoutMs: SECOND_KEY_WAIT_MS,
    commitKeys: 'none',
    ignoreInputs: false,
    onRecord: (sequence) => onRecord(fromRecorded(sequence, mac)),
    onCancel,
    onClear,
  })
  const { steps } = recorder
  // the recorder's functions are new on each render: the effects below must not run again for that
  const latest = useRef(recorder)
  useEffect(() => {
    latest.current = recorder
  })
  useEffect(() => {
    latest.current.startRecording()
    return () => latest.current.stopRecording()
  }, [])
  // two keys are a whole sequence: no need to wait for the pause
  useEffect(() => {
    if (steps.length >= 2) latest.current.commitRecording()
  }, [steps.length])

  return (
    <button type="button" autoFocus data-slot="key-recorder" aria-label="Press the new keys" onBlur={onCancel} className="flex h-7 min-w-28 items-center justify-center rounded-md px-2 text-xs text-muted-foreground ring-2 ring-primary outline-none">
      {steps.length > 0 ? <ShortcutKeys keys={fromRecorded(steps, mac)} /> : 'Press keys…'}
    </button>
  )
}
