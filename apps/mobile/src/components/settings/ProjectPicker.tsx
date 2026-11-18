import { Pressable, View } from 'react-native'

import { useTheme } from '../../theme'
import { Icon, Text } from '../../ui'

/**
 * The web's `ProjectPicker`: a project checklist with "Select all" above two or more projects. With `onAllChange`,
 * `all` covers every project, including future ones, and hides the list.
 */
export function ProjectPicker({ projects, selected, onChange, all = false, onAllChange, empty }: { projects: { id: string; name: string }[]; selected: string[]; onChange: (ids: string[]) => void; all?: boolean; onAllChange?: (all: boolean) => void; empty?: string }) {
  const some = selected.length > 0 && selected.length < projects.length
  return (
    <View accessibilityLabel="Projects" className="gap-3">
      {onAllChange ? <CheckRow label="All projects, including future ones" checked={all} onPress={() => onAllChange(!all)} /> : null}
      {all ? null : (
        <>
          {projects.length > 1 ? (
            <CheckRow label="Select all" muted checked={selected.length === projects.length} indeterminate={some} onPress={() => onChange(selected.length === projects.length ? [] : projects.map((project) => project.id))} />
          ) : null}
          {projects.map((project) => {
            const checked = selected.includes(project.id)
            return <CheckRow key={project.id} label={project.name} checked={checked} onPress={() => onChange(checked ? selected.filter((other) => other !== project.id) : [...selected, project.id])} />
          })}
          {empty ? <Text className="text-[14px] leading-[20px] text-muted-foreground">{empty}</Text> : null}
        </>
      )}
    </View>
  )
}

export function CheckRow({ label, hint, checked, indeterminate = false, muted = false, onPress }: { label: string; hint?: string; checked: boolean; indeterminate?: boolean; muted?: boolean; onPress: () => void }) {
  return (
    <Pressable accessibilityRole="checkbox" accessibilityLabel={label} accessibilityState={{ checked: indeterminate ? 'mixed' : checked }} onPress={onPress} hitSlop={4} className={`flex-row gap-2 ${hint ? 'items-start' : 'items-center'}`}>
      <Checkbox checked={checked} indeterminate={indeterminate} />
      <View className="flex-1 gap-0.5">
        <Text className={`text-[14px] leading-[18px] ${muted ? 'text-muted-foreground' : 'font-noto-medium text-foreground'}`}>{label}</Text>
        {hint ? <Text className="text-[12px] leading-[16px] text-muted-foreground">{hint}</Text> : null}
      </View>
    </Pressable>
  )
}

/** The web `Checkbox`: a 16px box, primary with a check (or a dash) when on. */
export function Checkbox({ checked, indeterminate = false }: { checked: boolean; indeterminate?: boolean }) {
  const { colors } = useTheme()
  const on = checked || indeterminate
  return (
    <View className={`size-4 items-center justify-center rounded-[4px] border ${on ? 'border-primary bg-primary' : 'border-input dark:bg-input/30'}`}>
      {on ? <Icon name={indeterminate ? 'minus' : 'check'} size={14} color={colors.primaryForeground} /> : null}
    </View>
  )
}
