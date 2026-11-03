import { type TextProps } from 'react-native'
import { NativeText } from './scaled'

/** React Native's `Text` on the body text (`text-body` in global.css). `className` utilities override any of it. */
export function Text({ className, ...props }: TextProps) {
  return <NativeText {...props} className={className ? `text-body ${className}` : 'text-body'} />
}
