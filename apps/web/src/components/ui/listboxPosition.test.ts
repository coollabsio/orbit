import { expect, test } from 'bun:test'
import { listboxPosition } from './listboxPosition'

test('opens above a low mobile trigger and stays within the visible area', () => {
  const position = listboxPosition({top:510,bottom:542,left:20,width:350}, 90, {top:0,bottom:560,left:0,width:390})
  expect(position.top).toBe(416)
  expect(position.left).toBe(20)
  expect(position.maxHeight).toBe(256)
})
test('caps long menus and clamps horizontal overflow', () => {
  const position = listboxPosition({top:150,bottom:182,left:280,width:90}, 500, {top:0,bottom:400,left:0,width:320})
  expect(position.left).toBe(104)
  expect(position.top).toBe(186)
  expect(position.maxHeight).toBe(206)
})
