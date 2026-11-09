import { getNodeById, type BlockNoteEditor } from '@blocknote/core'
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
import { usePageEditorContext } from './pageEditorContext'
import { applyTurnInto, canTurnBlock, choiceMatches, databaseTurn, turnIntoChoices, type TurnIntoChoice } from './turnInto'

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

/** "Turn into" for a page block that links to a database, and for a database block: they become each other. */
function TurnDatabaseItem() {
  const Components = useComponentsContext()!
  const editor = useBlockNoteEditor()
  const { resolvePage } = usePageEditorContext()
  const block = useExtensionState(SideMenuExtension, {
    editor,
    selector: (state) => state?.block,
  })
  const turn = block ? databaseTurn(block, (pageId) => !!resolvePage(pageId)?.database) : null
  if (!block || !turn) return null
  return (
    <Components.Generic.Menu.Item
      className="bn-menu-item"
      onClick={() => {
        editor.focus()
        editor.updateBlock(block.id, { type: turn.type, props: turn.props } as never)
      }}
    >
      Turn into {turn.label.toLowerCase()}
    </Components.Generic.Menu.Item>
  )
}

function BlockDragMenu() {
  const Components = useComponentsContext()!
  const dict = useDictionary()
  return (
    <DragHandleMenu>
      <TurnIntoItem>Turn into</TurnIntoItem>
      <TurnDatabaseItem />
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

/** Taller than this is a card, an image, or a table, not one line of text. */
const ONE_LINE = 48

/** The top row: a heading, a file name, a picture, a page chip, or the first text line. */
function firstRow(reference: Element): Element {
  return (
    reference.querySelector('h1, h2, h3, h4, h5, h6') ??
    reference.querySelector('.bn-file-name-with-icon, .bn-add-file-button') ??
    reference.querySelector('.bn-visual-media') ??
    reference.querySelector('[data-page-id]') ??
    reference.querySelector('.bn-inline-content') ??
    reference
  )
}

/**
 * BlockNote's built-in shift assumes its own heading sizes. Measure the top row instead.
 * One line: center the 30px handle on it. A taller block: keep the handle on that top row.
 */
function handleShift(editor: BlockNoteEditor<any, any, any>, blockId: string): number {
  const view = editor.prosemirrorView
  if (!view) return 0
  const reference = editor.transact((tr) => {
    const info = getNodeById(blockId, tr.doc)
    if (!info) return null
    const { node } = view.domAtPos(info.posBeforeNode + 1)
    return node instanceof Element ? node : node.parentElement
  })
  if (!reference) return 0
  const line = firstRow(reference)
  const box = line.getClientRects()[0] ?? line.getBoundingClientRect()
  if (box.height === 0) return 0
  const refTop = reference.getBoundingClientRect().top
  const top = box.height > ONE_LINE ? box.top : box.top + box.height / 2 - HANDLE_HEIGHT / 2
  return Math.round(top - refTop)
}

/** Block handle, on the block's first line. */
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
