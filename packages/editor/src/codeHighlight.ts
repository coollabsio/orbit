import { createExtension, defaultBlockSpecs } from '@blocknote/core'
import { Decoration } from '@tiptap/pm/view'
import { createHighlightPlugin } from 'prosemirror-highlight'
import { codeLines, type TokenType } from './codeLanguages'

const defaultCodeBlock = defaultBlockSpecs.codeBlock

type CodeBlockImplementation = typeof defaultCodeBlock.implementation
/** The node view of a code block: `dom`, `contentDOM` and the ProseMirror node view hooks. */
export type CodeBlockView = ReturnType<CodeBlockImplementation['render']>

export interface CodeBlockOptions {
  /** The class of a syntax token (`undefined`: no class). */
  tokenClass: (type: TokenType) => string | undefined
  /** Adds the host's controls to an editor's code block (not to HTML export or the clipboard). */
  decorate?: (view: CodeBlockView, ...args: Parameters<CodeBlockImplementation['render']>) => CodeBlockView
}

/**
 * Syntax colors for the code of every code block. They are decorations, so the document holds plain text only and
 * the schema stays BlockNote's code block.
 */
function highlightExtension(tokenClass: CodeBlockOptions['tokenClass']) {
  return createExtension({
    key: 'codeHighlight',
    prosemirrorPlugins: [
      createHighlightPlugin({
        nodeTypes: ['codeBlock'],
        languageExtractor: (node) => node.attrs.language,
        parser: ({ content, language, pos }) => {
          const decorations: Decoration[] = []
          // the text of a node starts one position after the node
          let from = pos + 1
          for (const line of codeLines(content, language ?? '')) {
            for (const token of line) {
              const className = tokenClass(token.type)
              if (className) decorations.push(Decoration.inline(from, from + token.value.length, { class: className }))
              from += token.value.length
            }
            // the line break
            from += 1
          }
          return decorations
        },
      }),
    ],
  })
}

/** BlockNote's code block with syntax colors and the host's controls. */
export function codeBlockSpec({ tokenClass, decorate }: CodeBlockOptions): typeof defaultCodeBlock {
  return {
    ...defaultCodeBlock,
    extensions: [...(defaultCodeBlock.extensions ?? []), highlightExtension(tokenClass)],
    implementation: {
      ...defaultCodeBlock.implementation,
      render(block, editor) {
        const view = defaultCodeBlock.implementation.render.call(this, block, editor)
        // `dom` render (HTML export, clipboard) must stay plain code.
        return decorate && this.renderType === 'nodeView' ? decorate(view, block, editor) : view
      },
    },
  }
}
