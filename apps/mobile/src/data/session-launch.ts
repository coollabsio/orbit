import { ApiProblem } from '@orbit/api/problem'

/**
 * Whether a failed check of the saved session at launch ends it. Only a 401 does: offline, a timeout or a 5xx keep the
 * session (and its token) so the app can retry.
 */
export const launchErrorEndsSession = (error: unknown) => error instanceof ApiProblem && error.status === 401
