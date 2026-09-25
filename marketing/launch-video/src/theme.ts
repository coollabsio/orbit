import { loadFont as loadGeist } from '@remotion/google-fonts/Geist'
import { loadFont as loadGeistMono } from '@remotion/google-fonts/GeistMono'
import { loadFont as loadNoto } from '@remotion/google-fonts/NotoSans'
import { Easing, staticFile } from 'remotion'
import manifest from '../public/shots/manifest.json'

export const FPS = 30
export const WIDTH = 1920
export const HEIGHT = 1080

export const display = loadGeist('normal', { weights: ['400', '500', '600', '700'], subsets: ['latin'] }).fontFamily
export const mono = loadGeistMono('normal', { weights: ['400', '500'], subsets: ['latin'] }).fontFamily
/** The app's own UI font, for text drawn on top of screenshots. */
export const appFont = loadNoto('normal', { weights: ['400', '500'], subsets: ['latin'] }).fontFamily

export const color = {
  bg: '#060608',
  text: '#f6f6f7',
  muted: '#9a9aa3',
  faint: 'rgba(255,255,255,0.08)',
  pink: '#f2458f',
  pinkDeep: '#b3125a',
  violet: '#8b5cf6',
  green: '#4cb782',
  // Sampled from the dark-theme screenshots.
  appCanvas: '#0a0a0a',
  appColumn: '#151515',
  appCard: '#171717',
  appInput: '#212121',
}

export const ease = Easing.bezier(0.65, 0, 0.35, 1)
export const easeOut = Easing.bezier(0.16, 1, 0.3, 1)

/** Screenshots are 1600x1000 CSS px captured at 2x. */
export const SHOT_W = 1600
export const SHOT_H = 1000

export type Box = { x: number; y: number; w: number; h: number }
type ShotEntry = { file: string; regions?: Record<string, Box>; targets?: Record<string, { x: number; y: number }> }

const byFile = new Map((manifest as unknown as ShotEntry[]).map((entry) => [entry.file.replace(/\.png$/, ''), entry]))

export const shot = (name: string) => staticFile(`shots/${name}.png`)
export const region = (name: string, key: string): Box => {
  const box = byFile.get(name)?.regions?.[key]
  if (!box) throw new Error(`missing region ${name}.${key}`)
  return box
}
export const center = (box: Box) => ({ x: box.x + box.w / 2, y: box.y + box.h / 2 })
