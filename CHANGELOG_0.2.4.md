# Arctic ComfyUI Helper 0.2.4

## Improvements

- Linux packages now depend on the GTK XDG Desktop Portal backend so folder selection uses the desktop-native GTK picker instead of falling back to the simpler Zenity dialog.
- Folder picker dialogs now pass contextual titles for ComfyUI root, install base, workflow root, and shared models folder selection.
- The Models tab now detects RAM and GPU VRAM automatically instead of asking users to choose hardware-size dropdowns.
- Model variant selection is now manual: the Models tab shows every variant in the selected family instead of hiding variants that do not match the selected GPU VRAM.
- Detected GPU VRAM now sorts matching variants first and only warns when a variant may need more VRAM than detected.
- NVIDIA VRAM detection now uses nominal MiB bands so 32 GB cards that report slightly under 32 GiB are classified as 32+ GB VRAM.
- Model search now includes variant IDs, sizes, quantization labels, and notes so specific options such as `3B Q8` are easier to find.
- Model search now searches the full catalog when no model family is selected, and searches inside the selected family when one is selected.
- Model family dropdowns and model-row metadata now show friendly family names instead of raw catalog family IDs.
- Model variant dropdowns now default to the recommended variant for the detected GPU VRAM unless the user manually chooses a different variant.
- The Selected Queue now shows checkboxes for support files. RAM-bucketed text encoders are all visible with the detected best bucket checked by default, and optional LoRAs/upscalers can be unchecked before downloading.
- Clip and text encoder groups now appear first in the Selected Queue's Always Artifacts list.
- Model artifacts can now use `ram_bucket` for exact RAM-bucket alternatives, such as one text encoder per RAM tier.
- RAM artifact requirements now behave as true minimums, so higher RAM tiers satisfy lower RAM-tier artifact requirements.
- Workflow entries can now open external links such as Patreon pages from the workflow button instead of requiring a downloadable JSON file.
- Workflow dropdowns now prefer full workflow name fields from the catalog before falling back to short display names.

## Notes

- On fresh Arch installs, `xdg-desktop-portal-gtk` is required alongside `xdg-desktop-portal` for the richer GTK-style folder picker.
- The fixed-size selectors still map to the existing catalog tier system internally, so current model catalog entries continue to work without schema changes.
- Workflow catalog entries remain compatible with the existing `workflow_json_url` download flow, while link-style entries can use fields such as `patreon_url`, `workflow_url`, or `workflow_link_url`.
