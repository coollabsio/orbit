export function isMockBackedPath(pathname: string) {
  return pathname === '/'
    || ['/mail', '/chat', '/inbox']
      .some((prefix) => pathname === prefix || pathname.startsWith(`${prefix}/`))
}
