import { linearTiming, TransitionSeries } from '@remotion/transitions'
import { fade } from '@remotion/transitions/fade'
import { slide } from '@remotion/transitions/slide'
import type { FC } from 'react'
import { AbsoluteFill } from 'remotion'
import { Grid, Outro, SelfHost } from './scenes/Closing'
import { Board, Bulk, Detail, Palette, Roadmap, Themes } from './scenes/Features'
import { Chat, Mail, UpcomingChapter } from './scenes/Upcoming'
import { DocsRich, DocsSlash, DocsSpaces, DocsWrite, NotionImport } from './scenes/Docs'
import { DocsChapter, Hero, Hook, Intro, Merge, TasksChapter } from './scenes/Opening'
import { color, ease } from './theme'

const TRANSITION = 16

type Scene = { id: string; component: FC; frames: number; enter?: 'fade' | 'slide' }

export const SCENES: Scene[] = [
  { id: 'intro', component: Intro, frames: 100 },
  { id: 'hook', component: Hook, frames: 100, enter: 'fade' },
  { id: 'merge', component: Merge, frames: 125, enter: 'fade' },
  { id: 'hero', component: Hero, frames: 150, enter: 'fade' },
  { id: 'tasks', component: TasksChapter, frames: 70, enter: 'fade' },
  { id: 'board', component: Board, frames: 165, enter: 'slide' },
  { id: 'roadmap', component: Roadmap, frames: 150, enter: 'slide' },
  { id: 'detail', component: Detail, frames: 140, enter: 'slide' },
  { id: 'bulk', component: Bulk, frames: 120, enter: 'slide' },
  { id: 'docs', component: DocsChapter, frames: 70, enter: 'fade' },
  { id: 'docs-write', component: DocsWrite, frames: 150, enter: 'slide' },
  { id: 'docs-slash', component: DocsSlash, frames: 125, enter: 'slide' },
  { id: 'docs-rich', component: DocsRich, frames: 150, enter: 'slide' },
  { id: 'docs-spaces', component: DocsSpaces, frames: 125, enter: 'slide' },
  { id: 'notion-import', component: NotionImport, frames: 240, enter: 'slide' },
  { id: 'upcoming', component: UpcomingChapter, frames: 70, enter: 'fade' },
  { id: 'mail', component: Mail, frames: 150, enter: 'slide' },
  { id: 'chat', component: Chat, frames: 150, enter: 'slide' },
  { id: 'palette', component: Palette, frames: 125, enter: 'fade' },
  { id: 'themes', component: Themes, frames: 110, enter: 'fade' },
  { id: 'grid', component: Grid, frames: 130, enter: 'fade' },
  { id: 'selfhost', component: SelfHost, frames: 150, enter: 'slide' },
  { id: 'outro', component: Outro, frames: 150, enter: 'fade' },
]

export const TOTAL_FRAMES = SCENES.reduce((sum, scene) => sum + scene.frames, 0) - (SCENES.length - 1) * TRANSITION

export function LaunchVideo() {
  const timing = linearTiming({ durationInFrames: TRANSITION, easing: ease })
  return (
    <AbsoluteFill style={{ backgroundColor: color.bg }}>
      <TransitionSeries>
        {SCENES.flatMap((scene, index) => [
          index > 0 ? (
            <TransitionSeries.Transition
              key={`${scene.id}-in`}
              timing={timing}
              presentation={scene.enter === 'slide' ? slide({ direction: 'from-right' }) : fade()}
            />
          ) : null,
          <TransitionSeries.Sequence key={scene.id} durationInFrames={scene.frames}>
            <scene.component />
          </TransitionSeries.Sequence>,
        ])}
      </TransitionSeries>
    </AbsoluteFill>
  )
}
