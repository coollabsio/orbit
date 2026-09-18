import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, render } from '@testing-library/react'
import { RichTextEditor } from '../components/editor/RichTextEditor'

/** Renders the lazily loaded editor once, inside act(), so its React.lazy boundary is already resolved for every test. */
export async function warmRichTextEditor() {
  const client = new QueryClient()
  let unmount = () => {}
  await act(async () => {
    unmount = render(
      <QueryClientProvider client={client}>
        <RichTextEditor value={null} placeholder="" ariaLabel="warm-up" workspaceId="warm-up" members={[]} />
      </QueryClientProvider>,
    ).unmount
    await import('../components/editor/RichTextEditorSurface')
    await new Promise((resolve) => setTimeout(resolve, 0))
  })
  unmount()
  client.clear()
}
