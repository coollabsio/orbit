export function isMockBackedPath(pathname: string) {
  return pathname === '/'
    || ['/mail', '/activity']
      .some((prefix) => pathname === prefix || pathname.startsWith(`${prefix}/`))
}
