import { Pressable, ScrollView, View } from 'react-native'

import { BLOCK_KINDS, type EditorCommand, type FormatState, type InlineStyle } from '../../data/page-editor'
import { useTheme } from '../../theme'
import { Icon, type IconName, Text } from '../../ui'

const STYLE_BUTTONS: { style: InlineStyle; glyph: string; label: string; className: string }[] = [
  { style: 'bold', glyph: 'B', label: 'Bold', className: 'font-noto-bold' },
  { style: 'italic', glyph: 'I', label: 'Italic', className: 'italic' },
  { style: 'underline', glyph: 'U', label: 'Underline', className: 'underline' },
  { style: 'strike', glyph: 'S', label: 'Strikethrough', className: 'line-through' },
]

/**
 * The page editor's formatting bar, above the keyboard while the editor has focus: the block type, inline styles,
 * lists, indent, an image, undo and redo. Commands go to the editor in its web view.
 */
export function EditorToolbar({ format, onCommand, onBlockMenu, onImage }: { format: FormatState; onCommand: (command: EditorCommand) => void; onBlockMenu: () => void; onImage: () => void }) {
  const { colors } = useTheme()
  const block = format.block ? BLOCK_KINDS[format.block].label : 'Block'
  const listToggle = (kind: 'bullet' | 'numbered' | 'check') => onCommand({ type: 'block', kind: format.block === kind ? 'paragraph' : kind })
  return (
    <View className="border-t border-border bg-background">
      <ScrollView horizontal keyboardShouldPersistTaps="always" showsHorizontalScrollIndicator={false} contentContainerClassName="items-center gap-0.5 px-2 py-1.5">
        <Pressable accessibilityRole="button" accessibilityLabel={`Block type, ${block}`} onPress={onBlockMenu} className="flex-row items-center gap-1 h-9 px-2.5 rounded-md active:bg-muted">
          <Text className="text-[14px] font-noto-medium">{block}</Text>
          <Icon name="chevronDown" size={14} color={colors.mutedForeground} />
        </Pressable>
        <Divider />
        {STYLE_BUTTONS.map((item) => (
          <ToolButton key={item.style} label={item.label} active={format.styles[item.style]} onPress={() => onCommand({ type: 'style', style: item.style })}>
            <Text className={`text-[16px] ${item.className}`}>{item.glyph}</Text>
          </ToolButton>
        ))}
        <ToolButton label="Code" active={format.styles.code} onPress={() => onCommand({ type: 'style', style: 'code' })} icon="code" />
        <Divider />
        <ToolButton label="Bulleted list" active={format.block === 'bullet'} onPress={() => listToggle('bullet')} icon="list" />
        <ToolButton label="Numbered list" active={format.block === 'numbered'} onPress={() => listToggle('numbered')}>
          <Text className="text-[14px] font-noto-medium">1.</Text>
        </ToolButton>
        <ToolButton label="To-do list" active={format.block === 'check'} onPress={() => listToggle('check')} icon="checkCircle" />
        <ToolButton label="Outdent" disabled={!format.canOutdent} onPress={() => onCommand({ type: 'outdent' })} icon="chevronLeft" />
        <ToolButton label="Indent" disabled={!format.canIndent} onPress={() => onCommand({ type: 'indent' })} icon="chevronRight" />
        <Divider />
        <ToolButton label="Add an image" onPress={onImage} icon="sticker" />
        <ToolButton label="Add a divider" onPress={() => onCommand({ type: 'divider' })} icon="minus" />
        <ToolButton label="Undo" onPress={() => onCommand({ type: 'undo' })} icon="rotateLeft" />
        <ToolButton label="Redo" onPress={() => onCommand({ type: 'redo' })} icon="rotateLeft" mirrored />
        <ToolButton label="Hide the keyboard" onPress={() => onCommand({ type: 'blur' })} icon="keyboard" />
      </ScrollView>
    </View>
  )
}

function Divider() {
  return <View className="w-px h-5 mx-1 bg-border" />
}

function ToolButton({ label, icon, mirrored, active, disabled, onPress, children }: { label: string; icon?: IconName; mirrored?: boolean; active?: boolean; disabled?: boolean; onPress: () => void; children?: React.ReactNode }) {
  const { colors } = useTheme()
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={label}
      accessibilityState={{ selected: active, disabled }}
      disabled={disabled}
      onPress={onPress}
      className={`size-9 items-center justify-center rounded-md active:bg-muted ${active ? 'bg-muted' : ''} ${disabled ? 'opacity-40' : ''}`}
    >
      {icon ? (
        <View style={mirrored ? { transform: [{ scaleX: -1 }] } : undefined}>
          <Icon name={icon} size={18} color={active ? colors.primary : colors.foreground} />
        </View>
      ) : (
        children
      )}
    </Pressable>
  )
}
