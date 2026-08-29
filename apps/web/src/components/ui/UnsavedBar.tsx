/** Floating "you have unsaved changes" bar with Reset / Save (rendered by a `position: relative` parent). */
export function UnsavedBar({ onReset, onSave, saving }: { onReset: () => void; onSave: () => void; saving?: boolean }) {
  return (
    <div className="unsaved-bar" role="status">
      <span className="unsaved-bar-text">Careful — you have unsaved changes!</span>
      <button type="button" className="unsaved-bar-reset" onClick={onReset}>
        Reset
      </button>
      <button type="button" className="button button-primary" onClick={onSave} disabled={saving}>
        Save Changes
      </button>
    </div>
  )
}
