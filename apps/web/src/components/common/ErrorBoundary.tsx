import { Component, type ReactNode } from 'react'
import { Danger } from 'reicon-react'
import { Button } from '@/components/ui/button'
import { EmptyState } from './EmptyState'

interface ErrorBoundaryProps {
  children: ReactNode
  /** The error screen goes away when this changes (the route: the sidebar can leave a page that crashed). */
  resetKey?: unknown
}

/** A render error shows "Something went wrong" and Reload in place of the part of the app that crashed. */
export class ErrorBoundary extends Component<ErrorBoundaryProps, { error: unknown; resetKey: unknown }> {
  state = { error: null as unknown, resetKey: this.props.resetKey }

  static getDerivedStateFromError(error: unknown) {
    return { error: error ?? new Error('Unknown error') }
  }

  static getDerivedStateFromProps(props: ErrorBoundaryProps, state: { resetKey: unknown }) {
    return props.resetKey === state.resetKey ? null : { error: null, resetKey: props.resetKey }
  }

  render() {
    if (!this.state.error) return this.props.children
    return (
      <div role="alert" data-slot="error-boundary" className="grid min-h-full flex-1 place-items-center p-6">
        <EmptyState
          icon={Danger}
          title="Something went wrong"
          description="This part of Orbit stopped working. Reload the page to try again."
          action={<Button onClick={() => window.location.reload()}>Reload</Button>}
        />
      </div>
    )
  }
}
