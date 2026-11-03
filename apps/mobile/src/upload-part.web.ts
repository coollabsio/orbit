/** The web preview: the picker's `uri` is a blob/data URL the browser can read back. */
export async function uploadPart(uri: string, _name: string): Promise<Blob> {
  return (await fetch(uri)).blob()
}
