# Media browser

The launcher sits immediately left of Color Tone Studio. It opens a full workspace overlay below the application menu. Opening either overlay closes the other; the native canvas is hidden while browsing. Escape or Close returns to the document. Browse by folder picker (macOS), typed path, places, parent, back, or child folders. The last folder is stored in the application configuration directory.

## Display and scheduling

- Rust owns read-only catalogues. A folder scan reads names and file metadata, not image pixels. Superseded scans stop between directory entries. Each scan is limited to 50,000 media files and 2,000 child folders, with a visible limit notice. Up to 16 recent catalogues are retained; an expired view can be refreshed.
- Search, media filtering and sorting happen in Rust. The last matching index is reused across 256-item pages. The webview renders only visible grid rows plus one overscan row. The feature code and styles load on demand.
- Two workers decode thumbnails. Selected previews go to the front of the queue, which has a 64-job limit. Queued offscreen jobs are discarded; up to two in-flight decodes may finish and populate the cache. Switching folders discards queued work for the old catalogue.
- Thumbnail long edge: 320px. Image preview long edge: at most 1600px. Small supported images are not enlarged. Alpha is shown over a neutral checker. The source files are never rewritten.
- The private `media` protocol accepts catalogue/item identifiers and fixed variants, not arbitrary filesystem paths. Metadata crosses Tauri IPC; compressed preview bytes use the resource protocol.

## Cache

`app_cache_dir()/media-proxies-v1` holds JPEG proxies. Keys include the source path, size, modification time and requested resolution. Writes use temporary files with atomic persistence. An in-memory LRU inventory enforces a 512MiB disk budget and avoids rescanning the cache on every thumbnail. Cache hits read existing JPEG bytes without decoding the source. The inventory is loaded lazily on first use. The webview uses no-store resources so revisiting an item checks its source identity again.

## Decoders and movies

On macOS, the system `sips` decoder handles supported images, orientation and sRGB conversion; Quick Look produces movie poster frames. Exact HEIF, RAW, PSD, EXR and movie support depends on the installed OS decoders. The portable Rust fallback currently handles PNG, JPEG and WebP. Unsupported or damaged files stay visible with an unavailable-preview message. System decoder jobs time out after 20 seconds.

Movie playback uses the original local file with byte-range reads capped at 4MiB per response. It does not copy an entire movie into JavaScript or transcode the original. Playback codec support depends on the WebView. Low-resolution movie transcoding and non-macOS poster generation remain future work. No FFmpeg installation is required by the app.

## Verification

Rust tests cover filter-before-pagination, bounded/suffix/invalid ranges, actual video byte slices, source/size cache invalidation, LRU replacement/eviction, and real image proxy generation/reuse/refresh. An opt-in macOS test generates a poster from a local movie fixture (`LUMAPAINT_MEDIA_VIDEO_FIXTURE`, `native_movie_poster`).

Browser QA uses a mocked metadata bridge with 1,000 entries and a real H.264 MP4 test clip. It covers desktop and narrow windows, bounded grid DOM, pagination, search, type filtering, previews, movie playback, back/folder navigation, three locales and Escape. These checks do not claim a production-folder latency benchmark or a full native WebView movie-playback test.
