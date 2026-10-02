export function isMockBackedPath(pathname: string) {
  return pathname === '/'
    || ['/mail', '/inbox']
      .some((prefix) => pathname === prefix || pathname.startsWith(`${prefix}/`))
}
