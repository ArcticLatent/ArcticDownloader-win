# Arctic ComfyUI Helper 0.2.2

## What's New

- Added a manual `Refresh Catalog` action so the running app can reload the remote model catalog without a full restart.

## Improvements

- Models with no variants but with `always` artifacts, such as FlashVSR-style entries, now appear in the Models tab and can be downloaded through an `Always Artifacts Only` flow.

## Notes

- The catalog refresh action pulls the current remote catalog into the running app immediately, so newly published model families can appear without restarting.
- The always-only model path preserves the existing variant-based workflow for normal models while allowing catalog entries that only define shared artifacts to be selected and downloaded.
