// Tab picks the highlighted entry of an open suggestion menu ("@", "/", ":"), as in Discord. BlockNote's menus only
// take Enter, and Tab would indent the block or leave the editor.
import { useEffect } from 'react'
import { useEditorDOMElement } from '@blocknote/react'
import { completeOnTab } from './tabCompletion'

/** Mount inside a BlockNote view: Tab picks the highlighted entry of its open suggestion menu. */
export function TabCompletesSuggestion() {
  const element = useEditorDOMElement()
  useEffect(() => {
    if (!element) return
    const onKeyDown = completeOnTab(element)
    // capture: before ProseMirror's Tab (indent) and the browser's focus move
    element.addEventListener('keydown', onKeyDown, true)
    return () => element.removeEventListener('keydown', onKeyDown, true)
  }, [element])
  return null
}
