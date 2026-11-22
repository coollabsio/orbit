# App Store screenshots

Screenshots of the iOS app with the "Acme" demo workspace, for App Store Connect
(**iOS App → version → Previews and Screenshots**).

| Folder | Size | App Store slot |
| --- | --- | --- |
| `screenshots/iphone-6.9/` | 1320×2868 | iPhone 6.9" |
| `screenshots/iphone-6.3/` | 1206×2622 | "iPhone with Dynamic Island (medium display)", 6.1"/6.3" (scaled from 6.9"; the slot App Store Connect asks for) |
| `screenshots/iphone-6.5/` | 1284×2778 | iPhone 6.5" (scaled from 6.9") |
| `screenshots/ipad-13/` | 2064×2752 | iPad 13" (not used: the app is iPhone only, `supportsTablet: false`) |

The images come from the mobile app's Expo web build in Chromium, not from a real device.

## Make them again

The demo data runs on its own server with an empty SQLite database in `/root/orbit-appstore-demo/`. Your dev
database does not change. Build the server first (`cargo build -p orbit-server`).

1. Start a fresh demo server on `127.0.0.1:19180`. The script prints the setup token:
   `mkdir -p /root/orbit-appstore-demo && cp marketing/app-store/demo-server.sh /root/orbit-appstore-demo/ && S=$(/root/orbit-appstore-demo/demo-server.sh)`
2. Add the demo data. Run it in `/root/orbit-appstore-demo/`, because it writes `maya.token` there. Keep the printed IDs:
   `cd /root/orbit-appstore-demo && node /root/devel/orbit/marketing/app-store/seed-demo.mjs http://127.0.0.1:19180 $S > ids.json`
3. Start Vite in front of the server. It adds the CORS header for the Expo web origin:
   `cd apps/web && VITE_API_PROXY=http://127.0.0.1:19180 bun run dev -- --host 127.0.0.1 --port 19181 --strictPort`
4. Start Expo web: `cd apps/mobile && bunx expo start --web --port 8082`
5. Take the screenshots, for each device:
   `node marketing/app-store/shoot.mjs marketing/app-store/screenshots/iphone-6.9 http://127.0.0.1:19181 /root/orbit-appstore-demo/maya.token "$(cat /root/orbit-appstore-demo/ids.json)" iphone`
   Make the 6.3" set: `ffmpeg -i <6.9 file> -vf "scale=1206:2622:flags=lanczos,format=rgb24" <6.3 file>`. Make the 6.5" set: `ffmpeg -i <6.9 file> -vf "scale=1284:2790:flags=lanczos,crop=1284:2778:0:6,format=rgb24" <6.5 file>`
   (for iPad: folder `screenshots/ipad-13`, last argument `ipad`; add `light` after it for the light theme).

All demo accounts use the password `demo-password-1` (`maya@acme.example`, `leo@…`, `sara@…`, `jon@…`, `ana@…`).
