import { router } from 'expo-router'
import { useState } from 'react'
import { Alert } from 'react-native'

import type { Teamspace } from '../../data/orbit'
import { useTeamspaceWrites } from '../../hooks/docs'
import { usePages, useTeamspaces } from '../../queries'
import { useTheme } from '../../theme'
import { Icon } from '../../ui'
import { ActionSheet, type SheetAction } from '../settings/Dialog'
import { Note, RowText, SettingsCard, SettingsRow } from '../settings/Shell'

/** The teamspace that holds the project's docs (#64), as on the web's project settings: pick one, make one, or open it. */
export function ProjectDocsCard({ project }: { project: { id: string; name: string } }) {
  const { colors } = useTheme()
  const teamspaces = useTeamspaces()
  const pages = usePages()
  const writes = useTeamspaceWrites()
  const [picking, setPicking] = useState(false)
  const all = teamspaces.data ?? []
  const linked = all.find((teamspace) => teamspace.project_id === project.id)
  // A teamspace of another project is not offered: a project has one, and a teamspace holds one project.
  const choices = all.filter((teamspace) => !teamspace.project_id || teamspace.project_id === project.id)
  const firstPage = linked ? pages.data?.find((page) => page.teamspace_id === linked.id && !page.parent_id) : undefined
  const busy = writes.save.isPending || writes.create.isPending

  const link = (teamspace: Teamspace, projectId: string | null) => writes.save.mutateAsync({ teamspace, project_id: projectId })
  const choose = async (next: Teamspace | null) => {
    if ((next?.id ?? null) === (linked?.id ?? null)) return
    try {
      if (linked) await link(linked, null)
      if (next) await link(next, project.id)
    } catch {
      Alert.alert('Could not change the docs teamspace', 'Try again.')
    }
  }
  const create = async () => {
    try {
      await link(await writes.create.mutateAsync(project.name.slice(0, 100)), project.id)
    } catch {
      Alert.alert('Could not create the docs teamspace', 'Try again.')
    }
  }

  const actions: SheetAction[] = [
    { label: 'No docs teamspace', icon: linked ? undefined : 'check', onPress: () => void choose(null) },
    ...choices.map((teamspace): SheetAction => ({ label: teamspace.name, icon: teamspace.id === linked?.id ? 'check' : 'people', onPress: () => void choose(teamspace) })),
    ...(linked ? [] : [{ label: 'Create docs teamspace', icon: 'plus', onPress: () => void create() } satisfies SheetAction]),
  ]

  return (
    <SettingsCard
      title="Docs"
      description="The teamspace that holds this project's docs. Teamspaces without a project hold global docs. An AI client can list the global docs and this project's docs, without the docs of other projects."
      flush={teamspaces.isSuccess}
    >
      {teamspaces.isPending ? <Note loading>Loading teamspaces…</Note> : null}
      {teamspaces.isError ? <Note error>Could not load the teamspaces. Open this screen again to retry.</Note> : null}
      {teamspaces.isSuccess ? (
        <>
          <SettingsRow first label={`Docs teamspace, ${linked?.name ?? 'none'}`} onPress={busy ? undefined : () => setPicking(true)}>
            <Icon name="people" size={16} color={colors.mutedForeground} />
            <RowText title={linked?.name ?? 'No docs teamspace'} subtitle={linked ? (firstPage ? undefined : 'No pages yet. Add one in Docs.') : 'Tap to pick or create one'} />
          </SettingsRow>
          {firstPage ? (
            <SettingsRow label="Open docs" onPress={() => router.push(`/docs/${firstPage.id}`)}>
              <Icon name="docs" size={16} color={colors.mutedForeground} />
              <RowText title="Open docs" />
            </SettingsRow>
          ) : null}
        </>
      ) : null}
      <ActionSheet visible={picking} title="Docs teamspace" actions={actions} onClose={() => setPicking(false)} />
    </SettingsCard>
  )
}
