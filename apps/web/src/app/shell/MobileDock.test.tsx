import { expect, test } from 'bun:test'
import { fireEvent, render, within } from '@testing-library/react'
import { MemoryRouter, useLocation } from 'react-router'
import { MobileDock } from './MobileDock'

function Location() {
  return <output data-testid="location">{useLocation().pathname}</output>
}

test('upcoming mobile sections are visible but cannot navigate', () => {
  const view = render(<MemoryRouter initialEntries={['/tasks']}><MobileDock /><Location /></MemoryRouter>)
  const dock = within(view.getByRole('navigation'))
  for (const label of ['Home', 'Docs', 'Mail', 'Chat', 'DMs']) {
    const button = dock.getByRole('button', { name: label }) as HTMLButtonElement
    expect(button.disabled).toBe(true)
    expect(button.textContent).not.toContain('Soon')
    fireEvent.click(button)
    expect(view.getByTestId('location').textContent).toBe('/tasks')
  }
  expect(dock.getAllByRole('link')).toHaveLength(2)
  fireEvent.click(dock.getByRole('link', { name: 'Settings' }))
  expect(view.getByTestId('location').textContent).toBe('/settings')
  fireEvent.click(dock.getByRole('link', { name: 'Tasks' }))
  expect(view.getByTestId('location').textContent).toBe('/tasks')
})
