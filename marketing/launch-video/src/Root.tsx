import { Composition } from 'remotion'
import { LaunchVideo, TOTAL_FRAMES } from './LaunchVideo'
import { Promo, PROMO_FRAMES } from './Promo'
import { FPS, HEIGHT, WIDTH } from './theme'

export function Root() {
  return (
    <>
      <Composition id="LaunchVideo" component={LaunchVideo} durationInFrames={TOTAL_FRAMES} fps={FPS} width={WIDTH} height={HEIGHT} />
      <Composition id="Promo" component={Promo} durationInFrames={PROMO_FRAMES} fps={FPS} width={WIDTH} height={HEIGHT} />
    </>
  )
}
