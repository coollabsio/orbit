/**
 * Registers the DOM globals. This is its own preload, listed before `setup.ts`, because react-dom looks for `window`
 * and `document` once, when it is first loaded: without them it decides the `input` event is unsupported and ignores
 * `change` events on text inputs. In one file Bun loads `@testing-library/react` (and with it react-dom) before
 * the registration runs, whatever the order of the imports.
 */
import { GlobalRegistrator } from '@happy-dom/global-registrator'

GlobalRegistrator.register({ url: 'http://localhost/' })
