import { ProjectMark } from '@/components/common/ProjectMark'
import type { SearchPickerOption } from '@/components/common/SearchPicker'
import { UserAvatar } from '@/components/common/UserAvatar'
import type { Project } from '@/features/tasks/api/models'
import type { User } from '@/features/workspaces/models'

/** A person in a search picker: found by name, email or handle. */
export const personOption = (user: User): SearchPickerOption => ({
  value: user.id,
  label: user.name,
  keywords: [user.email, user.handle],
  icon: <UserAvatar user={user} size={16} />,
})

/** A project in a search picker: found by name or key. */
export const projectOption = (project: Project): SearchPickerOption => ({
  value: project.id,
  label: project.name,
  keywords: [project.key],
  icon: <ProjectMark project={project} className="size-2" />,
  hint: project.key,
})
