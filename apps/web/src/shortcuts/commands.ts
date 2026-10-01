import { chatEnabled } from '@/lib/chatEnabled'

/** Where a command can run. A page mounts the handlers of its contexts; `task-target` needs a target task. */
export type Context = 'global' | 'task-list' | 'task-board' | 'timeline' | 'task-detail' | 'task-target' | 'new-task' | 'docs' | 'views' | 'chat'
export type Group = 'General' | 'Navigation' | 'List' | 'Task' | 'Docs' | 'Chat'

export interface Command {
  id: string
  title: string
  group: Group
  /** Default binding. Steps of a sequence are separated by a space (`G I`); `Mod` is ⌘ on macOS, Ctrl elsewhere. */
  keys: string | null
  context: Context
  /** Shown in the help and settings lists, but cannot be rebound. */
  fixed?: true
  /** Fires again while the key is held. */
  repeat?: true
  /** Runs while the focus is in a text field. Only for keys that have no meaning in text. */
  inInputs?: true
}

const REGISTRY = [
  { id: 'palette.open', title: 'Open command menu', group: 'General', keys: 'Mod+K', context: 'global', inInputs: true },
  { id: 'help.open', title: 'Show keyboard shortcuts', group: 'General', keys: '?', context: 'global' },
  { id: 'search.open', title: 'Search', group: 'General', keys: '/', context: 'global' },
  { id: 'task.create', title: 'Create task', group: 'General', keys: 'C', context: 'global' },
  { id: 'settings.open', title: 'Open settings', group: 'General', keys: 'Mod+,', context: 'global', inInputs: true },
  { id: 'theme.toggle', title: 'Change theme', group: 'General', keys: 'Mod+Shift+L', context: 'global', inInputs: true },

  { id: 'nav.inbox', title: 'Go to inbox', group: 'Navigation', keys: 'G I', context: 'global' },
  { id: 'nav.tasks', title: 'Go to all tasks', group: 'Navigation', keys: 'G T', context: 'global' },
  { id: 'nav.mine', title: 'Go to my tasks', group: 'Navigation', keys: 'G M', context: 'global' },
  { id: 'nav.week', title: 'Go to current week', group: 'Navigation', keys: 'G W', context: 'global' },
  { id: 'nav.overdue', title: 'Go to overdue', group: 'Navigation', keys: 'G O', context: 'global' },
  { id: 'nav.views', title: 'Go to views', group: 'Navigation', keys: 'G V', context: 'global' },
  { id: 'nav.docs', title: 'Go to docs', group: 'Navigation', keys: 'G D', context: 'global' },
  { id: 'nav.chat', title: 'Go to chat', group: 'Navigation', keys: 'G C', context: 'global' },
  { id: 'nav.trash', title: 'Go to trash', group: 'Navigation', keys: 'G X', context: 'global' },
  { id: 'nav.settings', title: 'Go to settings', group: 'Navigation', keys: 'G S', context: 'global' },
  { id: 'nav.profile', title: 'Go to profile', group: 'Navigation', keys: 'G P', context: 'global' },

  { id: 'list.down', title: 'Move focus down', group: 'List', keys: 'J', context: 'task-list', repeat: true },
  { id: 'list.up', title: 'Move focus up', group: 'List', keys: 'K', context: 'task-list', repeat: true },
  { id: 'list.downArrow', title: 'Move focus down', group: 'List', keys: 'ArrowDown', context: 'task-list', fixed: true, repeat: true },
  { id: 'list.upArrow', title: 'Move focus up', group: 'List', keys: 'ArrowUp', context: 'task-list', fixed: true, repeat: true },
  { id: 'list.open', title: 'Open the focused task', group: 'List', keys: 'O', context: 'task-list' },
  { id: 'list.openEnter', title: 'Open the focused task', group: 'List', keys: 'Enter', context: 'task-list', fixed: true },
  { id: 'list.select', title: 'Select the focused task', group: 'List', keys: 'X', context: 'task-list' },
  { id: 'list.extendDown', title: 'Extend the selection down', group: 'List', keys: 'Shift+J', context: 'task-list', repeat: true },
  { id: 'list.extendUp', title: 'Extend the selection up', group: 'List', keys: 'Shift+K', context: 'task-list', repeat: true },
  { id: 'list.selectAll', title: 'Select all tasks', group: 'List', keys: 'Mod+A', context: 'task-list' },
  { id: 'list.clearSelection', title: 'Clear the selection', group: 'List', keys: 'Escape', context: 'task-list', fixed: true },
  { id: 'view.filter', title: 'Filter', group: 'List', keys: 'F', context: 'task-list' },
  { id: 'view.display', title: 'Display options', group: 'List', keys: 'Shift+V', context: 'task-list' },
  { id: 'view.save', title: 'Save view', group: 'List', keys: 'Mod+S', context: 'task-list', inInputs: true },
  { id: 'view.layout', title: 'Change layout', group: 'List', keys: 'Mod+B', context: 'task-list' },
  { id: 'timeline.today', title: 'Scroll to today', group: 'List', keys: 'T', context: 'timeline' },

  { id: 'task.setStatus', title: 'Change status', group: 'Task', keys: 'S', context: 'task-target' },
  { id: 'task.setPriority', title: 'Change priority', group: 'Task', keys: 'P', context: 'task-target' },
  { id: 'task.setAssignee', title: 'Change assignee', group: 'Task', keys: 'A', context: 'task-target' },
  { id: 'task.assignMe', title: 'Assign to me', group: 'Task', keys: 'I', context: 'task-target' },
  { id: 'task.setLabels', title: 'Change labels', group: 'Task', keys: 'L', context: 'task-target' },
  { id: 'task.setDueDate', title: 'Set due date', group: 'Task', keys: 'Shift+D', context: 'task-target' },
  { id: 'task.addSubIssue', title: 'Add sub-issue', group: 'Task', keys: 'Mod+Shift+O', context: 'task-target' },
  { id: 'task.addRelation', title: 'Add relation', group: 'Task', keys: 'Mod+Shift+M', context: 'task-target' },
  { id: 'task.copyId', title: 'Copy task ID', group: 'Task', keys: 'Mod+.', context: 'task-target' },
  { id: 'task.copyLink', title: 'Copy task link', group: 'Task', keys: 'Mod+Shift+,', context: 'task-target' },
  { id: 'task.trash', title: 'Move to trash', group: 'Task', keys: 'Mod+Backspace', context: 'task-target' },

  { id: 'detail.next', title: 'Go to next task', group: 'Task', keys: 'J', context: 'task-detail', repeat: true },
  { id: 'detail.prev', title: 'Go to previous task', group: 'Task', keys: 'K', context: 'task-detail', repeat: true },
  { id: 'detail.close', title: 'Close the task', group: 'Task', keys: 'Escape', context: 'task-detail', fixed: true },
  { id: 'detail.sendComment', title: 'Send comment', group: 'Task', keys: 'Mod+Enter', context: 'task-detail', fixed: true },
  { id: 'newTask.submit', title: 'Create the task', group: 'Task', keys: 'Mod+Enter', context: 'new-task', fixed: true },
  { id: 'newTask.submitMore', title: 'Create and add another', group: 'Task', keys: 'Mod+Shift+Enter', context: 'new-task', inInputs: true },

  { id: 'docs.createPage', title: 'Create page', group: 'Docs', keys: null, context: 'docs' },
  { id: 'docs.history', title: 'Show page history', group: 'Docs', keys: 'Mod+Shift+H', context: 'docs', inInputs: true },
  { id: 'docs.comment', title: 'Add comment', group: 'Docs', keys: 'Mod+Alt+M', context: 'docs', inInputs: true },

  // all of them run while the composer has the focus
  { id: 'chat.previous', title: 'Go to previous conversation', group: 'Chat', keys: 'Alt+ArrowUp', context: 'chat', inInputs: true },
  { id: 'chat.next', title: 'Go to next conversation', group: 'Chat', keys: 'Alt+ArrowDown', context: 'chat', inInputs: true },
  { id: 'chat.previousUnread', title: 'Go to previous unread conversation', group: 'Chat', keys: 'Alt+Shift+ArrowUp', context: 'chat', inInputs: true },
  { id: 'chat.nextUnread', title: 'Go to next unread conversation', group: 'Chat', keys: 'Alt+Shift+ArrowDown', context: 'chat', inInputs: true },
  { id: 'chat.unreads', title: 'Open unreads', group: 'Chat', keys: 'Mod+Shift+A', context: 'chat', inInputs: true },
  { id: 'chat.markAllRead', title: 'Mark all as read', group: 'Chat', keys: 'Shift+Escape', context: 'chat', inInputs: true },
] as const satisfies readonly Command[]

export type CommandId = (typeof REGISTRY)[number]['id']

/** The commands of this build. With `chat` off (production, for now), the Chat commands are left out. */
export function commandsFor(chat: boolean): readonly Command[] {
  return chat ? REGISTRY : REGISTRY.filter((command: Command) => command.group !== 'Chat' && command.context !== 'chat' && command.id !== 'nav.chat')
}

/** The single source of truth for shortcuts: the key handler, command menu, help dialog, labels and settings read it. */
export const COMMANDS = commandsFor(chatEnabled)

const byId = new Map<string, Command>(COMMANDS.map((command) => [command.id, command]))
export const commandById = (id: string): Command | undefined => byId.get(id)
