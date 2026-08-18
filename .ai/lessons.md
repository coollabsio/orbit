# Lessons

## Design fidelity
- **Coolify v4 is flat-first, not card-first.** The app canvas is one flat surface; columns and second sidebars are separated by 1px borders directly on the canvas. Rounded bordered cards are ONLY for form sections and resource tiles inside content — never wrap whole panes/lists/readers in cards. (Corrected by user on 2026-08-21 after I carded every pane.)
- Coolify v4 dark mode uses the graphite palette with a **purple** accent (not the old yellow). Selection/active nav states are neutral gray fills (#1f1f1f), not accent-tinted.
- Before restyling to match a reference, restudy the actual screenshot section by section (shell, second nav, content, controls) instead of extrapolating a single pattern to everything.

## Reference discipline
- **Always study the assigned reference repo BEFORE building a feature, not after.** Chat was built generically without reading `the chat reference`; the user caught it. The brief maps each feature to a reference (Tasks/Mail→the tasks reference, Docs→the docs reference, Chat→the chat reference) — dispatch an Explore agent on the reference first and implement from its spec.
- Layer/nested two-tone card surfaces are a reference pattern, not Coolify. Coolify cards = one surface + one border.

## Tooling
- The user-run Vite dev server (port 8888) can serve a broken module graph after files are renamed (`.tsx`→`.ts`) mid-session — pages render blank with no console errors. Verify against `aubr build` + `aubx vite preview` before debugging app code, and report the dev server needs a restart instead of touching it.
- Probing dev-mode React modules via `import()` from a bare page fails with "@vitejs/plugin-react can't detect preamble" — that's a probe artifact, not an app error.
- Headless Firefox `--screenshot` can paint stale CSS custom-property values (showed light-theme values inside dark theme). Trust a beacon probe of `getComputedStyle` over screenshot colors before "fixing" color bugs.
- Browser-default `text-align: center` on `<button>` inherits into child spans — the base reset must set `text-align: left` on buttons.
