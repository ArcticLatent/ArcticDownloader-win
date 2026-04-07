# Arctic ComfyUI Helper 0.2.1

## What's New

- Added a full-screen loading transition when switching into `Manage Existing` and when changing between managed ComfyUI installs.
- Added Linux Flatpak packaging to the multi-package build and GitHub release flow.

## Improvements

- The ComfyUI tab now hides manage-only controls when you are in `Install New`, so the workflow is less confusing when an existing install has already been detected.
- Entering `Manage Existing` now gives immediate visual feedback instead of leaving the previous UI visible while the app loads the selected install.
- The Models tab now shows `Always Artifacts` under each selected model so users can see the extra files that will be downloaded automatically.
- The Models tab now shows the actual selected variant file names, including multi-file variants, in the `Selected Queue`.
- The Models family, GPU VRAM, and RAM dropdowns now start with clearer placeholder-style defaults.
- The Models RAM dropdown now reflects custom catalog RAM thresholds when the current model context supports them.
- The Models tab no longer offers `All Model Families`, which avoids mixed-family RAM-label ambiguity.
- Startup catalog refresh now always fetches the latest remote catalog body so newly published catalog changes show up more reliably.
- The Fedora/RPM packaging flow now uses the correct AppIndicator pkg-config dependency name for Fedora builds.
- The Linux packaging flow now includes `.flatpak` artifacts in release outputs, checksums, manifest generation, and GitHub uploads.

## Notes

- The Flatpak package targets `org.gnome.Platform//50`.
- The Flatpak release flow produces a single-file `.flatpak` bundle alongside the existing Arch, Debian, and RPM artifacts.
- Fixed Fedora RPM packaging to depend on the Ayatana AppIndicator development/runtime packages required by the Tauri tray build, and tightened the Fedora distrobox bootstrap so the needed devel package is always installed before `rpmbuild`.
- Fixed Flatpak bundling to export and bundle the app on the same `stable` branch, resolving the final `Refspec ... stable not found` failure after a successful Flatpak build.
