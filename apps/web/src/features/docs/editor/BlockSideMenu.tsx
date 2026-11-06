import { getNodeById } from '@blocknote/core'
import { SideMenuExtension } from '@blocknote/core/extensions'
import {
  BlockColorsItem,
  blockTypeSelectItems,
  DragHandleMenu,
  RemoveBlockItem,
  SideMenu,
  SideMenuController,
  TableColumnHeaderItem,
  TableRowHeaderItem,
  useBlockNoteEditor,
  useComponentsContext,
  useDictionary,
  useExtensionState,
  usePortalElement,
  type BlockTypeSelectItem,
  type SideMenuProps,
} from '@blocknote/react'
import { offset } from '@floating-ui/react'
import { useMemo } from 'react'
import { LampOn } from 'reicon-react'
import { applyTurnInto, canTurnBlock, choiceMatches, turnIntoChoices, type TurnIntoChoice } from './turnInto'

function CalloutIcon({ size }: { size?: number }) {
  return <LampOn className="size-4" width={size} height={size} />
}

function sameProps(actual: BlockTypeSelectItem['props'], wanted: TurnIntoChoice['props']): boolean {
  const left = actual ?? {}
  const right = wanted ?? {}
  const keys = new Set([...Object.keys(left), ...Object.keys(right)])
  for (const key of keys) if (left[key] !== right[key]) return false
  return true
}

function iconFor(choice: TurnIntoChoice, defaults: readonly BlockTypeSelectItem[]) {
  if (choice.type === 'callout') return CalloutIcon
  const exact = defaults.find((item) => item.type === choice.type && sameProps(item.props, choice.props))
  if (exact) return exact.icon
  if (choice.type === 'heading') {
    const plain = defaults.find((item) => item.type === 'heading' && item.props?.level === choice.props?.level && item.props?.isToggleable === false)
    if (plain) return plain.icon
  }
  return defaults.find((item) => item.type === choice.type)?.icon
}

/** "Turn into" on the block handle: the block types a text block can become. Hidden for an image, a file, a table, or a page. */
function TurnIntoItem({ children }: { children: string }) {
  const Components = useComponentsContext()!
  const portalElement = usePortalElement()
  const editor = useBlockNoteEditor()
  const dict = useDictionary()
  const block = useExtensionState(SideMenuExtension, {
    editor,
    selector: (state) => state?.block,
  })
  if (!block || !canTurnBlock(editor, block)) return null

  const choices = turnIntoChoices(editor)
  const icons = blockTypeSelectItems(dict)

  return (
    <Components.Generic.Menu.Root position="right" sub portalElement={portalElement}>
      <Components.Generic.Menu.Trigger sub>
        <Components.Generic.Menu.Item className="bn-menu-item" subTrigger>
          {children}
        </Components.Generic.Menu.Item>
      </Components.Generic.Menu.Trigger>
      <Components.Generic.Menu.Dropdown sub className="bn-menu-dropdown w-max min-w-56">
        {choices.map((choice) => {
          const Icon = iconFor(choice, icons)
          return (
            <Components.Generic.Menu.Item
              key={choice.name}
              className="bn-menu-item whitespace-nowrap"
              checked={choiceMatches(block, choice)}
              icon={Icon ? <Icon size={16} /> : undefined}
              onClick={() => {
                editor.focus()
                applyTurnInto(editor, block, choice)
              }}
            >
              {choice.name}
            </Components.Generic.Menu.Item>
          )
        })}
      </Components.Generic.Menu.Dropdown>
    </Components.Generic.Menu.Root>
  )
}

function BlockDragMenu() {
  const Components = useComponentsContext()!
  const dict = useDictionary()
  return (
    <DragHandleMenu>
      <TurnIntoItem>Turn into</TurnIntoItem>
      <BlockColorsItem>{dict.drag_handle.colors_menuitem}</BlockColorsItem>
      <TableRowHeaderItem>{dict.drag_handle.header_row_menuitem}</TableRowHeaderItem>
      <TableColumnHeaderItem>{dict.drag_handle.header_column_menuitem}</TableColumnHeaderItem>
      <Components.Generic.Menu.Divider />
      <RemoveBlockItem>{dict.drag_handle.delete_menuitem}</RemoveBlockItem>
    </DragHandleMenu>
  )
}

/** The default block handle (+ and drag) with "Turn into" in its menu. */
export function BlockSideMenu(props: SideMenuProps) {
  return <SideMenu {...props} dragHandleMenu={BlockDragMenu} />
}

/** BlockNote's side menu is 30px tall (`.bn-side-menu`). */
const HANDLE_HEIGHT = 30

/**
 * BlockNote's built-in shift assumes its own heading sizes. Ours are smaller, so that shift
 * drops the handle below the line. Measure the first line and center the 30px handle on it.
 */
function handleShift(editor: ReturnType<typeof useBlockNoteEditor>, blockId: string): number {
  if (!editor.prosemirrorView) return 0
  let reference: Element | null = null
  editor.transact((tr) => {
    const info = getNodeById(blockId, tr.doc)
    if (!info) return
    const { node } = editor.prosemirrorView.domAtPos(info.posBeforeNode + 1)
    reference = node instanceof Element ? node : node.parentElement
  })
  if (!reference) return 0
  const line =
    reference.querySelector('h1, h2, h3, h4, h5, h6') ??
    reference.querySelector('[data-page-id]') ??
    reference.querySelector('.bn-inline-content') ??
    reference
  const box = line.getClientRects()[0] ?? line.getBoundingClientRect()
  if (box.height === 0) return 0
  const refTop = reference.getBoundingClientRect().top
  return Math.round(box.top + box.height / 2 - refTop - HANDLE_HEIGHT / 2)
}

/** Block handle, centered on the block's first line. */
export function BlockSideMenuController() {
  const editor = useBlockNoteEditor()
  const blockId = useExtensionState(SideMenuExtension, {
    editor,
    selector: (state) => state?.block.id,
  })
  const crossAxis = blockId ? handleShift(editor, blockId) : 0
  const floatingUIOptions = useMemo(
    () => ({
      useFloatingOptions: {
        middleware: [offset({ crossAxis })],
      },
    }),
    [crossAxis],
  )
  return <SideMenuController sideMenu={BlockSideMenu} floatingUIOptions={floatingUIOptions} />
}
