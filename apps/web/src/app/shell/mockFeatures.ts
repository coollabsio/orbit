export function isMockBackedPath(pathname: string) {
  return pathname === '/'
    || ['/mail']
      .some((prefix) => pathname === prefix || pathname.startsWith(`${prefix}/`))
}
