---
type: maintenance
---
Removed the `ORBIT_BUILD_REVISION` build argument of the container image

- The web build names its revision with a hash of its files, so an image build reuses its cached layers when only the docs or the mobile app change, and open tabs offer a reload only when the web app changed
