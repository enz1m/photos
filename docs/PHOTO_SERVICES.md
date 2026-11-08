# Photo services

Immich is the first photo-service provider. Photos can connect with an API key,
browse photos and videos in a continuously loading library, search filenames or
image content, open server albums,
import originals/albums, upload selected local originals or local albums, and
synchronize a chosen folder automatically. All transfers are additive: no original is overwritten, replaced or
deleted on either side. Remote edits and deletions are not propagated.

## Connect and transfer photos

1. Choose **Add service** in the sidebar, or **Settings → Services**. Saved
   servers appear under **Services** in the sidebar and open as normal libraries.
2. In **Services → Connections**, choose **Add** beside **Servers**. Enter a name,
   server URL and API key, then choose **Connect**. A URL ending in
   `/api` is accepted too. For an existing server, create a key in Immich's user
   settings with `user.read`, `asset.read`, `asset.view`, `asset.download` and
   `asset.upload`, plus `album.read`, `album.create` and `albumAsset.create` for
   albums. Existing keys need those album permissions added in Immich. See [Immich's key instructions](https://docs.immich.app/features/command-line-interface/#obtain-the-api-key).
3. Click a photo to open its zoomable viewer, or click a video's tile to play it.
   Hover a tile to select it, or Ctrl-click to toggle selection. Use the titlebar
   **Download selected originals** icon to copy selected media; in the viewer the
   same icon imports the open original. Copies go to
   the destination and folder layout configured in Photos' **Settings → Imports**.
   Existing filenames are preserved; collisions create numbered copies. Capture
   dates and the original encoded bytes are retained.
4. To back up local photos, select them and choose **Upload to service…** from
   the selection toolbar, titlebar, or photo's right-click menu. In
   **Settings → Services → Upload**, choose the destination server, then choose
   **Upload photos**. Choosing the server alone does not upload.
   An open local photo also has a **Upload to service…** titlebar action.
   Embedded metadata is retained. Local edit recipes are not applied; export an
   edited or metadata-cleaned local copy first, then back up that copy.
   Immich reuses duplicate content; Photos never deletes the local file.
   Local backup starts at the server timeline, without inheriting a previously
   browsed remote album. The Upload page displays the destination. Refresh the
   service library to see newly uploaded media.

Saved connections appear in the sidebar. Connections, Upload and Download pages
live in **Settings → Services**, also reached through the contextual **Service
settings** titlebar icon. The key icon changes only that
server’s API key; the trash icon removes its saved key and configuration, without
deleting server photos. A locked or unavailable keyring is reported; Photos never
falls back to a plaintext key in the catalog. A connection can work for the
current session when key storage is unavailable. Unlock the keyring to remove a
previously saved key or reconnect after restarting.

## Albums and video

Choose the titlebar **Albums** tab to browse remote albums and open their cover
cards. **Download album** is a contextual titlebar action when an album is
open. It walks every page, creates safe new local copies and a local virtual
album, and
retains completed copies if interrupted. It never replaces a local album with
the same name. Selected uploads while an album is open add their originals to
that album without removing any existing members.

To back up a local album, open it and choose **Upload to service…** in the
titlebar or its sidebar right-click menu. Choose the destination, then **Upload album**. **Settings → Services → Upload → Upload photos once… → Source** also lets you choose a local album. This creates a new
remote album.
Each explicit album export creates a separate album; Immich deduplicates the
original media. Completed uploads/memberships remain if a later item fails.
Unavailable originals stop the export rather than silently omitting members.

Click a video tile to play it. Photos downloads a private temporary copy and
opens its existing video player, including seek/playback and frame export. This
is not an import into the library. Import a video to retain it locally. Videos
stream through disk-backed transfers, with a 4 GiB per-original limit; still
photo imports retain the existing 128 MiB limit. Incomplete or invalid downloads
are not published, and cancellation removes the partial temporary file. Uploads
use an immutable temporary snapshot after checking the original's identity.

## Loading and previews

The gallery lists metadata first, then fills in server-generated thumbnails in
batches of at most four concurrent requests. It does not download originals for
gallery tiles. Pulsing placeholders replace loading text and status pills and
respect the theme's reduced-motion setting. The most recent 96 thumbnails are
kept in an account-scoped memory cache for the session; resizing tiles and
returning to cached items reuse their image handles. Only the viewport and nearby
rows request images. Scrolling near the end appends more metadata automatically,
without page controls or resetting selection and scroll position. A failed batch
pauses with a retry action; failed thumbnails can also be retried. Refresh updates metadata
and retries missing thumbnails without discarding successful ones. This is not
an offline service library or a persistent remote cache.

Opening a photo requests its original for a bounded high-quality view, falling
back to the server's larger preview if necessary. This can take longer than the
gallery. Video playback still downloads a private temporary original first.

## Search and settings

Filename search is the default. The sparkles button inside search enables Immich
Smart Search, using natural-language image content such as “a sailboat at sea”.
The server must have machine learning and Smart Search enabled and its assets
indexed; Photos disables the toggle otherwise. Search runs on the server. Photos
does not download originals or run an AI model for this search. Clearing or
submitting a search starts a new result stream; editing an unsubmitted query does
not change results already being loaded. Album searches keep their album scope.

**General**, **Imports**, **Shortcuts** and **Services** use compact settings
rows. Hover an info icon for scope and safety details. Connections show only a
name, URL and key/delete actions. Upload and Download keep independent automatic rules separate from explicit
one-time copies. Closing Settings discards an unfinished API-key draft.

## Automatic uploads and downloads

Open **Settings → Services → Upload** or **Download**, then choose **Add** beside
**Automatic uploads/downloads**. Each new rule starts **off**. Choose **Add rule**
to save it, then turn on its switch when ready. Rules run every minute while
Photos is open; this is not a background service after Photos exits. The play
button runs one saved rule once, including a paused rule. The stop button cancels
its current transfer. The trash icon removes the rule and keeps existing copies.

For an upload rule, choose a server, **From → Entire local library** or a local
album, and **To → Entire server library** or an existing Immich album. The local
library includes Photos and its added source folders, excluding hidden photos.
New files are discovered automatically; album rules use current membership.
Unavailable album originals stop that upload rather than silently omitting
members or widening its scope. Deleting an album does not turn its rule into an
entire-library upload. Edit recipes stay local; originals are uploaded.

For a download rule, choose a server, **From → Entire server library** or an
Immich album, and a local destination under **To**. Destinations include Photos,
added source folders and the configured import destination. Add another folder
from the library sidebar if needed. The server scope is its visible timeline,
not trash or private/archived media. Completed copies appear in the local library.

Create as many independent rules as needed, including several for one server or
different albums on three different servers. Each rule retains its own progress;
pausing or removing one does not change another. Enabled rules run sequentially
within each polling cycle. Keyring problems pause the affected transfer and
leave the other rules eligible. Result details are behind each rule's info icon.

Automatic transfers only add originals. They do not mirror deletions, overwrite
edited copies, replace remote assets or synchronize edit recipes. Local content
changes upload as separate media. Missing or edited downloaded copies are not
recreated over local choices. A changed server checksum for a received identity
is reported as a conflict. Download names include a stable remote identity, so
equal filenames remain distinct. Immich deduplicates uploads, including identical
copies shared by different rules.

Private atomic receipts survive restarts and interrupted transfers. Downloads
published before receipt saving are compared with a fresh download; mismatches
remain conflicts. Competing processes, damaged history and replaced destination
folders stop transfers. Completed copies remain on cancellation. Closing Photos
cancels active work and waits for its publication/catalog reconciliation.

Search, Select All, Back and Help shortcut bindings also apply to services.
Left/right navigates an open remote item, loading the next metadata batch when
needed. Right-click an open photo for Copy Image, Save Original Copy and Download
to Library. `Ctrl+C` copies decoded original pixels; `Ctrl+Shift+S` opens a file
browser to save the original bytes without overwriting existing files. These
actions fetch the original on a bounded worker; they do not substitute a thumbnail.
An error displayed on the service page does not also produce a notification.
Errors from work whose result is not visible there still produce a notification.
Search opens/focuses the titlebar filename field. On compact windows it temporarily
replaces other titlebar controls and provides a Back button. Back leaves search,
a preview, an album, or the service; it cancels a cancellable active request. Select All selects only the
currently loaded remote results. A selection count appears in the gallery; its close button
clears selection. Photos share the local library's hover controls and tile styling.
Other local-library commands do not act on the hidden library. Field editing and Enter-to-search remain native.

Cancellation keeps completed local copies. An upload may already have committed
on the server before its cancellation is acknowledged; retrying uses Immich's
duplicate detection. Video transfers are bounded to 4 GiB; still photo imports are bounded to 128 MiB.
The browser loads metadata in 24-item batches, virtualizes its rows and retains
bounded, color-aware image previews; it is not a full offline mirror.

Use HTTPS for a real server. TLS uses the system trust configuration; there is no
certificate-verification bypass. Redirects are refused so an API key is not
forwarded to another destination. Keys are redacted in app messages and stored
through the shared credential service, scoped to catalog identity, account and
server URL. Moving a catalog to another location requires reconnecting its keys.

Adding the first connection upgrades the catalog body to version 2. Existing
albums, settings and edit recipes remain. Older Photos versions reject that
catalog instead of saving over the new service records; the existing file-custody
checks also reject a stale writer. Removing all connections does not downgrade it.
