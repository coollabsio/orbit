import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './index.css'
import App from './App.tsx'
import { Providers } from './app/Providers.tsx'
import { bindVisibleViewport } from './lib/visibleViewport'

bindVisibleViewport()

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <Providers>
      <App />
    </Providers>
  </StrictMode>,
)
