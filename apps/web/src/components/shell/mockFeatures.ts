export function isMockBackedPath(pathname: string) {
  return pathname === '/'
    || ['/docs', '/mail', '/chat', '/dm', '/inbox']
      .some((prefix) => pathname === prefix || pathname.startsWith(`${prefix}/`))
}
