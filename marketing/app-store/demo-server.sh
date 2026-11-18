#!/usr/bin/env bash
# Fresh demo server on 127.0.0.1:19180 with an empty SQLite database.
cd /root/orbit-appstore-demo
[ -f server.pid ] && kill "$(cat server.pid)" 2>/dev/null; sleep 1
rm -rf orbit.db* files server.log
env -i PATH="$PATH" HOME="$HOME" ORBIT_ENV=development ORBIT_APP_KEY="$(openssl rand -base64 32)" nohup /root/devel/orbit/target/debug/orbit serve --database /root/orbit-appstore-demo/orbit.db --attachments /root/orbit-appstore-demo/files --listen 127.0.0.1:19180 --origin http://127.0.0.1:19180 > server.log 2>&1 & echo $! > server.pid
for i in $(seq 1 30); do grep -q 'token=' server.log 2>/dev/null && break; sleep 1; done
grep -o 'token=[A-Za-z0-9_-]*' server.log | head -1 | cut -d= -f2
