# Arctic ComfyUI Helper 0.2.5

## Improvements

- On high-RAM systems, text encoder support-file defaults now select the largest full non-quantized Tier A encoder when catalog file sizes are available, leaving other Tier A encoder alternatives unchecked unless selected manually.
- Text encoder support-file lists now place full/fp encoders at the top and quantized encoders at the bottom, with higher-precision quantized files listed before lower-precision ones.
- Model artifact rows now show catalog file sizes beside artifact names when `size_bytes` is available.
- Model artifact rows now show catalog-provided runtime RAM estimates when `memory_estimate.runtime_ram_bytes` is available.
- Model artifact rows now describe RAM-bucketed alternatives with tradeoff text that explains fidelity versus memory use.
- Model artifact display names now hide file extensions, and support-file group headings are derived from artifact target categories.
- The Selected Queue now includes a short note explaining that required files are selected automatically and optional support files can be adjusted before downloading.
- The Selected Queue now labels support-file sections as Additional Model Files instead of Always Artifacts.
- The Selected Queue now labels chosen variant files as Selected Model.
- Models that only provide required support files now say All required files in the dropdown and queue.
- Catalog refresh now reads from the Supabase `catalog_documents.main` row instead of the old GitHub-hosted JSON file.
- Models, LoRAs, and Workflows now show cloud catalog loading and unavailable states during startup and refresh.
- Flux.1 CLIP-L companion files now stay selected automatically when the recommended T5 text encoder is selected.
- Linux release packaging now falls back to temporary Podman containers when Distrobox setup fails and excludes local `.env` secrets from RPM source archives.
