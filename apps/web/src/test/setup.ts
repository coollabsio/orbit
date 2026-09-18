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

afterEach(() => cleanup())
