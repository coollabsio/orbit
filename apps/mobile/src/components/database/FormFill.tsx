/**
 * A form view at phone size (spec P3 18.2): the form to fill in and its thank-you state. Building the form is on the
 * web. Text answers are typed in place; the other answers open the row value editor in a sheet, as on a row page.
 */
import { answerState, answersBody, blockingAnswers, formConfig, formQuestions, type FormAnswers, type FormQuestion } from '@orbit/database/form'
import type { CellValue, Database, DatabaseView, Property, Row, ViewConfig, ViewContext } from '@orbit/database/types'
import { apiValue, numberFromEditText, parseValue } from '@orbit/database/values'
import { type ReactElement, useState } from 'react'
import { Pressable, ScrollView, View } from 'react-native'

import { PROPERTY_ICONS, VIEW_ICONS } from '../../data/database'
import { formErrorMessage } from '../../data/form-api'
import { useSubmitForm } from '../../hooks/form'
import { useTheme } from '../../theme'
import { Button, Icon, Input, Text } from '../../ui'
import { Sheet } from '../tasks/Sheet'
import { CellValue as ValueView } from './cells'
import { ValueEditor } from './ValueEditor'

/** Types typed in place; the rest edit in a sheet. */
const TYPED = new Set<string>(['title', 'text', 'number', 'url', 'email', 'phone'])

export function FormFill({ database, viewId, config, ctx, header, canSubmit, editOnWeb = false }: {
  database: Database
  viewId: string
  config: ViewConfig
  ctx: ViewContext
  /** Above the form (the page header and the view picker on a database page). */
  header?: ReactElement
  /** A read-only workspace refuses new rows. */
  canSubmit: boolean
  /** The viewer can change this view: the screen says where the builder is. */
  editOnWeb?: boolean
}) {
  const { colors } = useTheme()
  const submit = useSubmitForm(database.database.id)
  const [answers, setAnswers] = useState<FormAnswers>({})
  const [typed, setTyped] = useState<Readonly<Record<string, string>>>({})
  const [tried, setTried] = useState(false)
  const [sent, setSent] = useState(false)
  const [editing, setEditing] = useState<string | null>(null)
  const [sheetVisible, setSheetVisible] = useState(false)
  const form = formConfig(config, database.properties)
  const questions = formQuestions(form, database.properties)
  const blocking = new Set(blockingAnswers(questions, answers))
  const closed = form.access === 'closed'
  const title = form.title || database.database.title || 'Untitled form'
  const draft = draftRow(questions, answers)
  const answer = (propertyId: string, value: unknown) => setAnswers((current) => ({ ...current, [propertyId]: value }))
  const edited = questions.find((question) => question.property.id === editing)

  const send = () => {
    setTried(true)
    if (blocking.size > 0) return
    submit.mutate({ viewId, answers: answersBody(questions, answers) }, { onSuccess: () => setSent(true) })
  }
  const restart = () => {
    setAnswers({})
    setTyped({})
    setTried(false)
    setSent(false)
  }

  return (
    <ScrollView contentContainerClassName="pb-10" keyboardShouldPersistTaps="handled">
      {header}
      {sent ? (
        <View className="mx-4 mt-4 items-center gap-3 rounded-lg border border-border px-5 py-8">
          <Icon name="checkCircle" size={32} color={colors.primary} />
          <Text className="font-noto-semibold text-[17px]">Thanks for your answer</Text>
          <Text className="text-center text-[14px] text-muted-foreground">{form.confirmation || 'Your answer was added to the database.'}</Text>
          <Button label="Submit another answer" variant="outline" onPress={restart} />
        </View>
      ) : (
        <View className="gap-5 px-4 pt-3">
          <View className="gap-1.5">
            <Text className="font-noto-semibold text-[22px] leading-[28px]">{title}</Text>
            {form.description ? <Text className="text-[14px] text-muted-foreground">{form.description}</Text> : null}
            {editOnWeb ? <Text className="text-[12px] text-muted-foreground/70">Edit the questions on the web.</Text> : null}
          </View>
          {closed ? (
            <Text accessibilityRole="alert" className="rounded-md border border-border px-3 py-2 text-[14px] text-muted-foreground">
              This form does not take answers right now.
            </Text>
          ) : null}
          {questions.map((question) => {
            const { property } = question
            const raw = answers[property.id]
            const state = answerState(property, raw)
            const error = tried && blocking.has(property.id) ? (state === 'invalid' ? 'Enter a valid answer.' : 'Answer this question.') : undefined
            if (TYPED.has(property.type)) {
              return (
                <Input
                  key={property.id}
                  label={question.label}
                  required={question.required}
                  description={question.description || undefined}
                  error={error}
                  editable={!closed}
                  multiline={property.type === 'text'}
                  keyboardType={keyboardOf(property)}
                  autoCapitalize={property.type === 'url' || property.type === ('email' as Property['type']) ? 'none' : undefined}
                  value={property.type === 'number' ? (typed[property.id] ?? '') : typeof raw === 'string' ? raw : ''}
                  onChangeText={(text) => {
                    if (property.type !== 'number') return answer(property.id, text)
                    setTyped((current) => ({ ...current, [property.id]: text }))
                    answer(property.id, text.trim() === '' ? null : (numberFromEditText(property, text) ?? text))
                  }}
                />
              )
            }
            const value = draft.values[property.id] ?? null
            return (
              <View key={property.id} className="gap-1.5">
                <Text className="text-[14px] font-noto-medium">
                  {question.label}
                  {question.required ? <Text className="text-primary"> *</Text> : null}
                </Text>
                {question.description ? <Text className="text-[12px] text-muted-foreground">{question.description}</Text> : null}
                <Pressable
                  accessibilityRole="button"
                  accessibilityLabel={`Answer ${question.label}`}
                  disabled={closed}
                  onPress={() => {
                    if (property.type === 'checkbox') return answer(property.id, raw !== true)
                    setEditing(property.id)
                    setSheetVisible(true)
                  }}
                  className={`min-h-9 flex-row items-center gap-2 rounded-lg border px-2.5 py-1.5 active:bg-muted ${error ? 'border-destructive' : 'border-input'}`}
                >
                  <Icon name={PROPERTY_ICONS[property.type]} size={15} color={colors.mutedForeground} />
                  {property.type === 'checkbox' ? (
                    <Text className="text-[14px]">{raw === true ? 'Yes' : 'No'}</Text>
                  ) : value ? (
                    <View className="flex-1 min-w-0">
                      <ValueView property={property} row={draft} ctx={ctx} />
                    </View>
                  ) : (
                    <Text className="text-[14px] text-muted-foreground/70">Choose…</Text>
                  )}
                </Pressable>
                {error ? <Text className="text-[12px] text-destructive">{error}</Text> : null}
              </View>
            )
          })}
          {submit.isError ? (
            <Text accessibilityRole="alert" className="text-[14px] text-destructive">
              {formErrorMessage(submit.error)}
            </Text>
          ) : null}
          <Button label={form.submit_label || 'Submit'} block loading={submit.isPending} disabled={closed || !canSubmit} onPress={send} />
        </View>
      )}
      <Sheet visible={sheetVisible} title={edited?.label ?? ''} onClose={() => setSheetVisible(false)}>
        {edited ? (
          <ValueEditor
            key={edited.property.id}
            property={edited.property}
            row={draft}
            onCommit={(next: CellValue | null) => answer(edited.property.id, apiValue(next))}
            onDone={() => setSheetVisible(false)}
          />
        ) : null}
      </Sheet>
    </ScrollView>
  )
}

/** The view picker of a form view (a form has no filter, sort or search). */
export function FormViewPicker({ view, onPress }: { view: DatabaseView; onPress: () => void }) {
  const { colors } = useTheme()
  return (
    <View className="px-3 pb-1">
      <View className="flex-row items-center border-b border-border pb-1.5">
        <Pressable accessibilityRole="button" accessibilityLabel={`View: ${view.name}`} onPress={onPress} className="h-8 flex-row items-center gap-1.5 px-2 rounded-md active:bg-muted shrink">
          <Icon name={VIEW_ICONS[view.type]} size={15} color={colors.foreground} />
          <Text numberOfLines={1} className="font-noto-medium text-[14px] shrink">
            {view.name}
          </Text>
          <Icon name="chevronDown" size={12} color={colors.mutedForeground} />
        </Pressable>
      </View>
    </View>
  )
}

function keyboardOf(property: Property) {
  switch (property.type as string) {
    case 'number':
      return 'decimal-pad' as const
    case 'url':
      return 'url' as const
    case 'email':
      return 'email-address' as const
    case 'phone':
      return 'phone-pad' as const
    default:
      return 'default' as const
  }
}

/** The answers as a row, so the value editors and value views of a row page show and edit them. */
function draftRow(questions: readonly FormQuestion[], answers: FormAnswers): Row {
  const values: Record<string, CellValue> = {}
  for (const { property } of questions) {
    const value = property.type === 'title' ? null : parseValue(property, answers[property.id])
    if (value) values[property.id] = value
  }
  const title = answers[questions.find((question) => question.property.type === 'title')?.property.id ?? '']
  return {
    id: 'form-draft',
    title: typeof title === 'string' ? title : '',
    icon: null,
    position: 0,
    version: 0,
    created_at: new Date(0).toISOString(),
    created_by: '',
    updated_at: new Date(0).toISOString(),
    updated_by: '',
    has_content: false,
    values,
  }
}
