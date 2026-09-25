export function isMockBackedPath(pathname: string) {
  return pathname === '/'
    || ['/mail', '/chat', '/dm', '/inbox']
      .some((prefix) => pathname === prefix || pathname.startsWith(`${prefix}/`))
}
