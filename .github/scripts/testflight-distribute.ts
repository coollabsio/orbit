// Gives an uploaded build to an external TestFlight group (the one with the public link) and submits it for Beta App
// Review. Testers in the group get the build when Apple approves it.
// Env: ASC_KEY_ID, ASC_ISSUER_ID, ASC_APP_ID, ASC_KEY_PATH, BUILD_NUMBER, APP_VERSION, TESTFLIGHT_GROUP, WHAT_TO_TEST.
const API = 'https://api.appstoreconnect.apple.com/v1'

function env(name: string): string {
  const value = process.env[name]
  if (!value) throw new Error(`${name} is not set`)
  return value
}

const appId = env('ASC_APP_ID')
const buildNumber = env('BUILD_NUMBER')
const groupName = env('TESTFLIGHT_GROUP')
const pem = await Bun.file(env('ASC_KEY_PATH')).text()
const key = await crypto.subtle.importKey(
  'pkcs8',
  Buffer.from(pem.replace(/-----[^-]+-----|\s/g, ''), 'base64'),
  { name: 'ECDSA', namedCurve: 'P-256' },
  false,
  ['sign'],
)

const b64url = (data: string | ArrayBuffer) =>
  Buffer.from(typeof data === 'string' ? new TextEncoder().encode(data) : new Uint8Array(data)).toString('base64url')

// A new token for each request: processing can take longer than the 20 minutes that a token can live.
async function token(): Promise<string> {
  const now = Math.floor(Date.now() / 1000)
  const header = b64url(JSON.stringify({ alg: 'ES256', kid: env('ASC_KEY_ID'), typ: 'JWT' }))
  const payload = b64url(JSON.stringify({ iss: env('ASC_ISSUER_ID'), iat: now, exp: now + 600, aud: 'appstoreconnect-v1' }))
  const signature = await crypto.subtle.sign({ name: 'ECDSA', hash: 'SHA-256' }, key, new TextEncoder().encode(`${header}.${payload}`))
  return `${header}.${payload}.${b64url(signature)}`
}

type Resource = { id: string; attributes?: Record<string, unknown> }

async function request(method: string, path: string, body?: unknown): Promise<{ ok: boolean; status: number; json: any }> {
  const res = await fetch(`${API}${path}`, {
    method,
    headers: { Authorization: `Bearer ${await token()}`, 'Content-Type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  const text = await res.text()
  return { ok: res.ok, status: res.status, json: text ? JSON.parse(text) : {} }
}

async function call(method: string, path: string, body?: unknown): Promise<any> {
  const res = await request(method, path, body)
  if (!res.ok) throw new Error(`${method} ${path} failed (${res.status}): ${JSON.stringify(res.json)}`)
  return res.json
}

const buildQuery = new URLSearchParams({
  'filter[app]': appId,
  'filter[version]': buildNumber,
  'filter[preReleaseVersion.version]': env('APP_VERSION'),
  'fields[builds]': 'processingState',
})
const deadline = Date.now() + 45 * 60_000
let build: Resource | undefined
for (;;) {
  build = (await call('GET', `/builds?${buildQuery}`)).data[0]
  const state = build?.attributes?.processingState
  console.log(`Build ${buildNumber}: ${state ?? 'not visible yet'}`)
  if (state === 'VALID') break
  if (state === 'FAILED' || state === 'INVALID') throw new Error(`Build processing ended with ${state}`)
  if (Date.now() > deadline) throw new Error('Build did not finish processing in 45 minutes')
  await Bun.sleep(30_000)
}
const buildRef = { type: 'builds', id: build!.id }

const groupQuery = new URLSearchParams({ 'filter[app]': appId, 'filter[name]': groupName, 'filter[isInternalGroup]': 'false' })
const group = ((await call('GET', `/betaGroups?${groupQuery}`)).data as Resource[]).find((g) => g.attributes?.name === groupName)
if (!group) {
  throw new Error(`No external TestFlight group named "${groupName}". Make it in App Store Connect, or change TESTFLIGHT_GROUP in the workflow.`)
}

const whatsNew = (process.env.WHAT_TO_TEST?.trim() || `Build ${buildNumber}`).slice(0, 4000)
const localization: Resource | undefined = (await call('GET', `/builds/${buildRef.id}/betaBuildLocalizations`)).data[0]
if (localization) {
  await call('PATCH', `/betaBuildLocalizations/${localization.id}`, {
    data: { type: 'betaBuildLocalizations', id: localization.id, attributes: { whatsNew } },
  })
} else {
  await call('POST', '/betaBuildLocalizations', {
    data: { type: 'betaBuildLocalizations', attributes: { locale: 'en-US', whatsNew }, relationships: { build: { data: buildRef } } },
  })
}

await call('POST', `/betaGroups/${group.id}/relationships/builds`, { data: [buildRef] })
console.log(`Added the build to "${groupName}"`)

const review = await request('POST', '/betaAppReviewSubmissions', {
  data: { type: 'betaAppReviewSubmissions', relationships: { build: { data: buildRef } } },
})
if (review.ok) {
  console.log('Submitted the build for Beta App Review')
} else {
  // The build is in the group already; it can be submitted by hand (for example when another build waits for review).
  console.log(`::warning::Beta App Review submission failed (${review.status}): ${review.json.errors?.[0]?.detail ?? JSON.stringify(review.json)}`)
}
