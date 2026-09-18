import { GlobalRegistrator } from '@happy-dom/global-registrator'
import { afterEach } from 'bun:test'

// Register the DOM before anything loads react-dom. Bun evaluates CommonJS
// dependencies of static imports ahead of this module's body, so a static
// `@testing-library/react` import here would evaluate react-dom while `window`
// is still undefined. react-dom then caches "no DOM" feature checks — notably
// `isInputEventSupported` — and every text input's onChange silently stops
// firing under fireEvent.
GlobalRegistrator.register()
;(window as unknown as { happyDOM: { setURL: (url: string) => void } }).happyDOM.setURL('http://localhost/')

const { cleanup } = await import('@testing-library/react')
const { warmRichTextEditor } = await import('./warmRichTextEditor')

// The rich text editor is a React.lazy boundary, which suspends on its first
// render in the process. Resolve it once here, inside act(), so no test sees
// that suspension settle outside act().
await warmRichTextEditor()

afterEach(() => cleanup())
