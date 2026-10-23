export function isMockBackedPath(pathname: string) {
  return pathname === '/'
    || ['/activity']
      .some((prefix) => pathname === prefix || pathname.startsWith(`${prefix}/`))
}
