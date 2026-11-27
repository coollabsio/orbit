---
type: added
---
Update notifications for the admins of self-hosted servers

- When a newer Orbit release is out, the sidebar shows `Update to vX.Y.Z`, which opens its GitHub release
- The server reads the latest version from `https://cdn.coollabs.io/orbit/version.json` at most once a day
- `ORBIT_UPDATE_CHECK=false` turns the check off, Orbit Cloud never checks
