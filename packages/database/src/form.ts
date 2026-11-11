import type { DatabaseFormConfig, DatabaseFormQuestion } from '@orbit/api/generated/types.gen'
import type { Property, ViewConfig } from './types'
import { parseValue } from './values'

export type { DatabaseFormAccess, DatabaseFormConfig, DatabaseFormQuestion } from '@orbit/api/generated/types.gen'

/**
 * Types a form cannot ask for (spec P3 18.2): computed, related and generated values, buttons, and files (a files value
 * names files of the row page, which does not exist before the submission). The server drops such questions.
 */
const NOT_ANSWERABLE: ReadonlySet<string> = new Set([
  'created_time',
  'created_by',
  'last_edited_time',
  'last_edited_by',
  'unique_id',
  'files',
  'relation',
  'rollup',
  'formula',
  'button',
])

export function answerable(property: Property): boolean {
  return !NOT_ANSWERABLE.has(property.type)
}

/** A question with its property; `label` falls back to the property name. */
export type FormQuestion = { property: Property; label: string; description: string; required: boolean }

/** The form of a form view's config; a view without one asks for the title. */
export function formConfig(config: ViewConfig, properties: readonly Property[]): DatabaseFormConfig {
  if (config.form) return config.form
  const title = properties.find((property) => property.type === 'title')
  return { questions: title ? [{ property_id: title.id, required: true }] : [], access: 'members' }
}

/** The questions in order, with their properties; questions of deleted or unanswerable properties are left out. */
export function formQuestions(form: DatabaseFormConfig, properties: readonly Property[]): FormQuestion[] {
  const known = new Map(properties.map((property) => [property.id, property]))
  const seen = new Set<string>()
  return (form.questions ?? []).flatMap((question) => {
    const property = known.get(question.property_id)
    if (!property || !answerable(property) || seen.has(property.id)) return []
    seen.add(property.id)
    return [{ property, label: question.label || property.name, description: question.description ?? '', required: question.required ?? false }]
  })
}

/** The properties a builder can still add, in property order. */
export function addableProperties(form: DatabaseFormConfig, properties: readonly Property[]): Property[] {
  const asked = new Set((form.questions ?? []).map((question) => question.property_id))
  return [...properties].sort((a, b) => a.position - b.position).filter((property) => answerable(property) && !asked.has(property.id))
}

/** A form with one question changed (`null` removes it). */
export function withQuestion(form: DatabaseFormConfig, propertyId: string, change: Partial<DatabaseFormQuestion> | null): DatabaseFormConfig {
  const questions = form.questions ?? []
  return {
    ...form,
    questions:
      change === null
        ? questions.filter((question) => question.property_id !== propertyId)
        : questions.map((question) => (question.property_id === propertyId ? { ...question, ...change, property_id: propertyId } : question)),
  }
}

/** A form with the question of `propertyId` moved to `index`. */
export function movedQuestion(form: DatabaseFormConfig, propertyId: string, index: number): DatabaseFormConfig {
  const questions = [...(form.questions ?? [])]
  const from = questions.findIndex((question) => question.property_id === propertyId)
  if (from === -1) return form
  const [moved] = questions.splice(from, 1)
  questions.splice(Math.max(0, Math.min(index, questions.length)), 0, moved!)
  return { ...form, questions }
}

/** Answers by property id in the API's value shape (the title's is its text); absent or `null` is skipped. */
export type FormAnswers = Readonly<Record<string, unknown>>

/** What a question's answer is: none, one that does not fit the property (a typed URL that is not one), or a value. */
export type AnswerState = 'empty' | 'invalid' | 'filled'

export function answerState(property: Property, raw: unknown): AnswerState {
  if (raw === null || raw === undefined || raw === false || (typeof raw === 'string' && raw.trim() === '') || (Array.isArray(raw) && raw.length === 0)) return 'empty'
  if (property.type === 'title') return typeof raw === 'string' ? 'filled' : 'invalid'
  return parseValue(property, raw) ? 'filled' : 'invalid'
}

/** The property ids of questions that keep the form from being sent: a required one left empty, or an invalid answer. */
export function blockingAnswers(questions: readonly FormQuestion[], answers: FormAnswers): string[] {
  return questions
    .filter((question) => {
      const state = answerState(question.property, answers[question.property.id])
      return state === 'invalid' || (state === 'empty' && question.required)
    })
    .map((question) => question.property.id)
}

/** The submit body's `answers`: the filled answers (the title trimmed). */
export function answersBody(questions: readonly FormQuestion[], answers: FormAnswers): Record<string, unknown> {
  const body: Record<string, unknown> = {}
  for (const { property } of questions) {
    const raw = answers[property.id]
    if (answerState(property, raw) !== 'filled') continue
    body[property.id] = typeof raw === 'string' ? raw.trim() : raw
  }
  return body
}

/** The in-app link of a form view (members only); `linkId` names the linked view block whose views hold it. */
export function formPath(databaseId: string, viewId: string, linkId?: string | null): string {
  return `/docs/${databaseId}/form/${viewId}${linkId ? `?link=${encodeURIComponent(linkId)}` : ''}`
}
