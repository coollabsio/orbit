/**
 * Drives every task call of the detail page and the New task dialog against a real Orbit server, through the same
 * functions the screens use, then removes what it created (tasks to the trash; labels, relations, comments and
 * files deleted).
 * Run: bun apps/mobile/scripts/api-check-tasks.ts [server] [email] [password]
 * Defaults: http://127.0.0.1:18080, test@example.com, password.
 */
import * as api from '../src/data/orbit'
import * as taskApi from '../src/data/task-api'
import {
  attachmentPath,
  bodyWithFiles,
  dueOn,
  dueWeek,
  groupRelations,
  identifier,
  isVersionConflict,
  labelChoices,
  nextLabelColor,
  parentErrorMessage,
  projectStatuses,
  refIdentifier,
  relatedTaskIds,
  sourceValue,
  splitInlineFiles,
  subIssueBody,
  taskCandidates,
} from '../src/data/tasks'

const [server = 'http://127.0.0.1:18080', email = 'test@example.com', password = 'password'] = process.argv.slice(2)

function check(ok: boolean, message: string) {
  if (!ok) throw new Error(`FAILED: ${message}`)
  console.log(`ok   ${message}`)
}

const failure = (promise: Promise<unknown>) => promise.then(
  () => null,
  (error: unknown) => error,
)

let token: string | null = null
const client = api.createClient(server, () => token)
const signIn = await api.signIn(client, email, password)
check(signIn.kind === 'signed_in', `signed in as ${email} without an Origin header`)
if (signIn.kind !== 'signed_in') process.exit(1)
token = signIn.token
const userId = signIn.user.id
const cleanup: (() => Promise<unknown>)[] = []
const step = (label: string, run: () => Promise<unknown>) =>
  cleanup.push(async () => {
    await run()
    console.log(`ok   cleanup: ${label}`)
  })

try {
  const ws = (await api.workspaces(client))[0]!.id
  const projects = await api.projects(client, ws)
  const statuses = await api.statuses(client, ws, projects.map((project) => project.id))
  const members = await api.members(client, ws)
  const project = projects[0]!
  const own = projectStatuses(statuses, project.id)

  // a label made from the picker's Create row
  const labelsBefore = await api.labels(client, ws)
  const name = `mobile-check-${Date.now().toString(36)}`
  const choices = labelChoices(labelsBefore, project.id, name, [])
  check(choices.create === name && choices.rows.length === 0, `the label picker offers "Create ${name}"`)
  const label = await taskApi.newLabel(client, ws, project.id, { name, color: nextLabelColor(labelsBefore.filter((item) => item.project_id === project.id)) })
  step(`deleted label ${label.name}`, () => taskApi.removeLabel(client, ws, label))
  check(label.project_id === project.id && label.name === name, `created label ${label.name} ${label.color}`)

  // parent, then a child created with every property of the New task dialog
  const parent = await taskApi.createTaskWith(client, ws, { title: 'Mobile task check parent (safe to delete)', project_id: project.id, status_id: own[0]!.id })
  const trashParent = { ...parent }
  step(`trashed ${identifier(parent, projects)}`, () => api.trashTask(client, ws, trashParent))
  const due = dueOn(new Date(2030, 0, 15), '09:00')
  const child = await taskApi.createTaskWith(
    client,
    ws,
    taskApi.newTaskBody({
      title: '  Mobile task check child (safe to delete) ',
      description: '',
      projectId: project.id,
      statusId: own[0]!.id,
      priority: 'high',
      assigneeIds: [userId],
      labelIds: [label.id],
      dueAt: due.end,
      dueStartAt: due.start,
      parentId: parent.id,
    }),
  )
  check(
    child.title === 'Mobile task check child (safe to delete)' &&
      child.priority === 'high' &&
      child.label_ids.join() === label.id &&
      child.assignee_ids.join() === userId &&
      new Date(child.due_at!).getTime() === new Date(due.end).getTime() &&
      child.parent_task_id === parent.id,
    `created ${identifier(child, projects)} with priority, assignee, label, due date and parent`,
  )
  let current = child
  const childRef = () => current
  step(`trashed ${identifier(child, projects)}`, () => api.trashTask(client, ws, childRef()))

  // property writes, each with expected_version
  const patch = async (body: Omit<Parameters<typeof api.changeTask>[3], 'expected_version'>, what: string, ok: (task: api.TaskRecord) => boolean) => {
    const next = await api.changeTask(client, ws, current.id, { ...body, expected_version: current.version })
    check(next.version > current.version && ok(next), what)
    current = next
  }
  await patch({ priority: 'urgent' }, 'priority -> urgent', (task) => task.priority === 'urgent')
  const other = members.find((member) => member.user_id !== userId)
  const assignees = other ? [userId, other.user_id] : []
  await patch({ assignee_ids: assignees }, `assignees -> ${assignees.length}`, (task) => task.assignee_ids.length === assignees.length)
  await patch({ label_ids: [] }, 'labels -> none', (task) => task.label_ids.length === 0)
  const week = dueWeek(1, '17:30', new Date())
  await patch({ due_at: week.end, due_start_at: week.start }, 'due -> next week (a range)', (task) => new Date(task.due_start_at!).getTime() === new Date(week.start!).getTime())
  await patch({ due_at: null, due_start_at: null }, 'due -> no due date', (task) => !task.due_at && !task.due_start_at)
  await patch({ source_url: sourceValue(' https://github.com/coollabsio/orbit/issues/1 ') }, 'source -> set', (task) => task.source_url === 'https://github.com/coollabsio/orbit/issues/1')
  await patch({ source_url: sourceValue('   ') }, 'source -> cleared', (task) => !task.source_url)
  const stale = await failure(api.changeTask(client, ws, current.id, { priority: 'low', expected_version: child.version }))
  check(isVersionConflict(stale), 'a stale expected_version is a version conflict')

  // parent: clear, set, and the cycle error
  await patch({ parent_task_id: null }, 'parent -> removed', (task) => task.parent === null)
  await patch({ parent_task_id: parent.id }, 'parent -> set', (task) => task.parent?.id === parent.id)
  const freshParent = await api.task(client, ws, parent.id)
  const cycle = await failure(api.changeTask(client, ws, parent.id, { parent_task_id: child.id, expected_version: freshParent.version }))
  check(parentErrorMessage(cycle) === 'A task can’t be a sub-issue of itself or of its own sub-issues.', `a cycle is refused: ${parentErrorMessage(cycle)}`)

  // sub-issues: listed, and one added the web's way
  const siblings = await taskApi.subIssues(client, ws, parent.id)
  check(siblings.map((task) => task.id).join() === child.id, 'the parent lists its sub-issue')
  const subBody = subIssueBody(freshParent, 'Mobile task check sub-issue (safe to delete)', { statuses, userId, siblings })!
  const sub = await taskApi.createTaskWith(client, ws, subBody)
  step(`trashed ${identifier(sub, projects)}`, async () => api.trashTask(client, ws, await api.task(client, ws, sub.id)))
  check(sub.parent_task_id === parent.id && (await taskApi.subIssues(client, ws, parent.id)).length === 2, `added sub-issue ${identifier(sub, projects)}`)

  // task search for the pickers
  const found = await taskApi.searchTasks(client, ws, 'Mobile task check')
  const candidates = taskCandidates(found, projects, { query: 'mobile task check', excludeIds: [child.id] })
  check(candidates.some((task) => task.id === parent.id) && !candidates.some((task) => task.id === child.id), `search found ${candidates.length} candidates without the task itself`)

  // relations
  const blocks = await taskApi.addRelation(client, ws, child.id, { type: 'blocks', task_id: sub.id })
  const blockedBy = await taskApi.addRelation(client, ws, child.id, { type: 'blocked_by', task_id: parent.id })
  const related = await taskApi.addRelation(client, ws, sub.id, { type: 'related', task_id: parent.id })
  const relations = await taskApi.relations(client, ws, child.id)
  check(
    groupRelations(relations).map((group) => `${group.label}:${group.relations.map((relation) => relation.task.id === parent.id ? 'parent' : 'sub').join()}`).join(' ') ===
      'Blocked by:parent Blocks:sub',
    `relations of the child: ${groupRelations(relations).map((group) => group.label).join(', ')}`,
  )
  check(relatedTaskIds(relations).length === 2, 'the picker leaves both related tasks out')
  check((await api.task(client, ws, child.id)).blocked, 'the child is blocked while the parent is open')
  for (const [taskId, relation] of [
    [child.id, blocks],
    [child.id, blockedBy],
    [sub.id, related],
  ] as const) {
    await taskApi.removeRelation(client, ws, taskId, relation.id)
  }
  check((await taskApi.relations(client, ws, child.id)).length === 0 && (await taskApi.relations(client, ws, sub.id)).length === 0, 'removed the three relations')

  // duplicate of
  current = await api.task(client, ws, child.id)
  await patch({ duplicate_of_id: sub.id }, 'marked as duplicate', (task) => task.duplicate_of?.id === sub.id)
  console.log(`info duplicate of ${refIdentifier(current.duplicate_of!)}`)
  await patch({ duplicate_of_id: null }, 'unmarked as duplicate', (task) => task.duplicate_of === null)

  // comment with an image and a file, uploaded inline as the web composer does
  const png = Uint8Array.from(atob('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=='), (char) => char.charCodeAt(0))
  const image = await taskApi.uploadInline(client, ws, child.id, { name: 'pixel.png', mimeType: 'image/png', uri: '', blob: new Blob([png], { type: 'image/png' }) })
  const text = await taskApi.uploadInline(client, ws, child.id, { name: 'notes [v1].txt', mimeType: 'text/plain', uri: '', blob: new Blob(['hello from mobile'], { type: 'text/plain' }) })
  check(image.media_type === 'image/png' && text.display_name === 'notes [v1].txt', `uploaded ${image.display_name} (${image.byte_size} B) and ${text.display_name}`)
  const files = [
    { name: image.display_name, path: attachmentPath(image), image: true },
    { name: text.display_name, path: attachmentPath(text), image: false },
  ]
  const comment = await api.addComment(client, ws, child.id, bodyWithFiles('Two files from the mobile check', files))
  const stored = (await api.comments(client, ws, child.id)).find((item) => item.id === comment.id)!
  const split = splitInlineFiles(stored.body)
  check(
    split.text === 'Two files from the mobile check' && split.files.map((file) => `${file.image ? 'image' : 'file'}:${file.name}`).join() === 'image:pixel.png,file:notes [v1].txt',
    `the stored comment links both files: ${JSON.stringify(split.files.map((file) => file.name))}`,
  )
  console.log(`info stored body_json block types: ${(stored.body_json ?? []).map((block) => String(block.type)).join(', ') || 'none (markdown only)'}`)
  const download = await fetch(`${server}${split.files[0]!.path}`, { headers: { Authorization: `Bearer ${token}` } })
  const bytes = new Uint8Array(await download.arrayBuffer())
  check(download.ok && bytes.length === png.length && download.headers.get('content-type') === 'image/png', `downloaded the image with the bearer token (${bytes.length} B)`)
  check((await taskApi.taskAttachments(client, ws, child.id)).length === 0, 'inline files stay out of the attachment list')
  check((await taskApi.commentAttachments(client, ws, child.id, comment.id)).length === 0, 'and out of the comment attachment list')
  await api.removeComment(client, ws, stored)
  for (const attachment of [image, text]) await taskApi.removeAttachment(client, ws, child.id, attachment.id)
  check((await api.comments(client, ws, child.id)).length === 0, 'deleted the comment and both files')
  current = await api.task(client, ws, child.id)
} finally {
  for (const run of cleanup.reverse()) await run().catch((error: unknown) => console.log(`warn cleanup: ${error instanceof Error ? error.message : String(error)}`))
  await api.signOut(client)
  token = null
  console.log('ok   signed out')
}
