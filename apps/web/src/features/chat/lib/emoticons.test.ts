import { expect, test } from 'bun:test'
import { completeEmoticon, replaceEmoticons } from './emoticons'

test('an emoticon converts only as a whole word', () => {
  expect(replaceEmoticons(':D')).toBe('😄')
  expect(replaceEmoticons('ok :) then\n<3 xD')).toBe('ok 🙂 then\n❤️ 😆')
  expect(replaceEmoticons('http://x at 10:30 (:D) a:) :)b :Dx')).toBe('http://x at 10:30 (:D) a:) :)b :Dx')
  expect(replaceEmoticons('constructor toString')).toBe('constructor toString')
})

test('a backslash keeps the emoticon as typed', () => {
  expect(replaceEmoticons('\\:D and \\<3')).toBe('\\:D and \\<3')
})

test('code keeps its emoticons', () => {
  expect(replaceEmoticons('`:D` :D')).toBe('`:D` 😄')
  expect(replaceEmoticons(':) ```\n:( \n``` :P')).toBe('🙂 ```\n:( \n``` 😛')
  expect(replaceEmoticons('```\n:( still open')).toBe('```\n:( still open')
})

test('white space after an emoticon converts it and the caret stays after the white space', () => {
  expect(completeEmoticon(':D ', 3)).toEqual({ text: '😄 ', cursor: 3 })
  expect(completeEmoticon('hi <3\nrest', 6)).toEqual({ text: 'hi ❤️\nrest', cursor: 6 })
  expect(completeEmoticon('a :) b', 5)).toEqual({ text: 'a 🙂 b', cursor: 5 })
})

test('typing converts nothing else', () => {
  expect(completeEmoticon(':D', 2)).toBeNull()
  expect(completeEmoticon(':D x', 4)).toBeNull()
  expect(completeEmoticon('\\:D ', 4)).toBeNull()
  expect(completeEmoticon('10:3 ', 5)).toBeNull()
  expect(completeEmoticon('http:/ ', 7)).toBeNull()
  expect(completeEmoticon('`a :D ', 6)).toBeNull()
  expect(completeEmoticon('```\n:D ', 7)).toBeNull()
  expect(completeEmoticon('`a` :D ', 7)).toEqual({ text: '`a` 😄 ', cursor: 7 })
  expect(completeEmoticon(' ', 1)).toBeNull()
})
