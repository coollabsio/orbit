import { useEffect, useState } from 'react'
import { AccessibilityInfo } from 'react-native'

/** The system's Reduce motion setting. */
export function useReduceMotion(): boolean {
  const [reduce, setReduce] = useState(false)
  useEffect(() => {
    void AccessibilityInfo.isReduceMotionEnabled().then(setReduce)
    const subscription = AccessibilityInfo.addEventListener('reduceMotionChanged', setReduce)
    return () => subscription.remove()
  }, [])
  return reduce
}
