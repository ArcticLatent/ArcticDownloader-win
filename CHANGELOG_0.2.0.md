# Arctic ComfyUI Helper 0.2.0

## What's New

- Reworked the Models tab so you can select and download multiple models at once.
- Added search, a selected-model queue, and clearer model destination messaging so the Models tab scales better as the catalog grows.
- Added a new field for extra ComfyUI startup options, with save and clear controls.
- Added a live in-app ComfyUI runtime console with filtering and improved readability.

## Improvements

- The app now shows the effective model download destination more clearly when a shared models folder is being used as the default.
- Runtime logs are easier to scan with cleaner wording, softer filtering labels, and improved visual highlighting.
- Dropdown menus now better match the rest of the app's controls.
- The model selection UI was refined to avoid cramped rows and horizontal scrolling.
- Fixed a Windows managed-install update issue where ComfyUI tag updates could fail on local git divergence instead of recovering cleanly to the latest release tag.

## Notes

- Custom ComfyUI startup options are appended to the normal launch command, so they work alongside the existing launch checkboxes.
- The runtime console only shows live output when ComfyUI is launched from Arctic Helper itself.
