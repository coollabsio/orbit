# Marketing

| Path | Content |
| --- | --- |
| `release-video/` | Scripts for the release update video and release screenshots, and `GUIDE.md`, the instructions for an AI agent |
| `releases/<version>/` | `video.json` (the video spec), `contact-sheet.jpg` (all slides in one image), `notes.md` (release notes highlights) |
| `brand/` | Logos |

Videos (`*.mp4`) and `build/` directories are not in git. To make the video of an older release again, run the
scripts with its `video.json` (see `release-video/GUIDE.md`).

To make the video of a new release, give an AI agent this prompt:

> Read `marketing/release-video/GUIDE.md` and make the release update video for v0.4.0.
