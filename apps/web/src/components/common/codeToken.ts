import { cva } from 'class-variance-authority'

/** The colour of a syntax token in a code block (GitHub's light and dark palettes). */
export const codeTokenVariants = cva('', {
  variants: {
    type: {
      keyword: 'text-[#cf222e] dark:text-[#ff7b72]',
      string: 'text-[#0a3069] dark:text-[#a5d6ff]',
      comment: 'text-[#6e7781] dark:text-[#8b949e]',
      // type names and numbers
      class: 'text-[#953800] dark:text-[#ffa657]',
      property: 'text-[#0550ae] dark:text-[#79c0ff]',
      // function and tag names
      entity: 'text-[#8250df] dark:text-[#d2a8ff]',
      jsxliterals: 'text-[#116329] dark:text-[#7ee787]',
      identifier: '',
      sign: '',
      break: '',
      space: '',
    },
  },
})
