import { AppWindow, Highlight, ShotLayer } from '../components/AppWindow'
import { Cursor } from '../components/Cursor'
import { FeatureLayout } from '../components/FeatureLayout'
import { center, color, region, shot } from '../theme'
import { Chapter } from './Opening'

const W = 1240

export const UpcomingChapter = () => <Chapter index="03 — COMING SOON" name="Talk it out." line="*Email* and team *chat,* in the same workspace." />

export function Mail() {
  const message = region('mail', 'expandedMessage')
  const selected = region('mail', 'selectedThread')
  const composer = region('mail-reply', 'composer')
  const reply = { x: 887, y: 833 }
  return (
    <FeatureLayout
      eyebrow="Coming soon · Mail"
      title="Your inbox, *inside* Orbit."
      body="A full email client with folders, drag-to-file, stars and threaded replies, right next to the tasks and docs it's about."
      chips={[{ label: 'Folders', dot: color.violet }, { label: 'Threads', dot: color.pink }, { label: 'Drag to file', dot: color.green }, { label: 'Attachments', dot: '#f2c94c' }]}
    >
      <AppWindow
        src={shot('mail')}
        width={W}
        url="orbit.yourcompany.com/mail"
        camera={[
          { f: 0, z: 1, x: 800, y: 500 },
          { f: 34, z: 1.35, x: 760, y: 360 },
          { f: 72, z: 1.35, x: 1100, y: 560 },
          { f: 110, z: 1.5, x: center(composer).x, y: center(composer).y - 60 },
          { f: 150, z: 1.55, x: center(composer).x, y: center(composer).y - 60 },
        ]}
      >
        <Highlight box={selected} from={30} to={62} pad={2} radius={8} dim={0.35} />
        <Highlight box={message} from={62} to={96} pad={6} radius={12} dim={0.35} />
        <ShotLayer name="mail-reply" from={98} />
        <Highlight box={composer} from={108} pad={6} radius={12} dim={0.35} />
        <Cursor from={70} path={[{ f: 70, x: 1300, y: 700 }, { f: 92, x: reply.x, y: reply.y }, { f: 150, x: reply.x + 40, y: reply.y + 90 }]} clicks={[94]} />
      </AppWindow>
    </FeatureLayout>
  )
}

export function Chat() {
  const code = region('chat', 'codeBlock')
  const reactions = region('chat', 'reactionMessage')
  const thread = region('chat-thread', 'threadPanel')
  const preview = { x: 693, y: 507 }
  return (
    <FeatureLayout
      side="right"
      eyebrow="Coming soon · Chat"
      title="Team chat, *without* another app."
      body="Channels, threads, DMs, reactions, code blocks and GitHub previews, living next to the work they're about."
      chips={[{ label: 'Channels', dot: color.pink }, { label: 'Threads', dot: color.violet }, { label: 'Direct messages', dot: color.green }, { label: 'Reactions', dot: '#f2c94c' }]}
    >
      <AppWindow
        src={shot('chat')}
        width={W}
        url="orbit.yourcompany.com/chat/engineering"
        camera={[
          { f: 0, z: 1, x: 800, y: 500 },
          { f: 32, z: 1.45, x: center(code).x + 120, y: center(code).y + 60 },
          { f: 70, z: 1.45, x: center(reactions).x - 80, y: center(reactions).y - 60 },
          { f: 112, z: 1.35, x: center(thread).x - 120, y: 460 },
          { f: 150, z: 1.4, x: center(thread).x - 100, y: 440 },
        ]}
      >
        <Highlight box={code} from={30} to={64} pad={6} radius={10} dim={0.35} />
        <Highlight box={reactions} from={64} to={96} pad={4} radius={10} dim={0.35} />
        <Cursor from={70} path={[{ f: 70, x: 1150, y: 800 }, { f: 92, x: preview.x, y: preview.y }, { f: 150, x: preview.x + 30, y: preview.y + 40 }]} clicks={[96]} />
        <ShotLayer name="chat-thread" from={100} />
        <Highlight box={thread} from={110} pad={0} radius={8} dim={0.35} />
      </AppWindow>
    </FeatureLayout>
  )
}
