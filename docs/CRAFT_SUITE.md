# Native Craft suite and Calligra retirement

WordCraft, GridCraft and DeckCraft are DomeD's replacement office targets.
PhotoCraft is a separate image editor. VectorCraft is excluded; SoundCraft,
DesignCraft and LightCraft remain possible future shared-base targets. The
ArtCraft AI application is not part of these ports.

The ports use pinned Storytold engines with the shared
[DomeD native base](https://github.com/jebot-git/dome-craft-base). Office text
uses installed BDF fonts for native UI, layout, rendering and PDF export.
PhotoCraft uses bitmap UI text and its upstream image/type compositor.

Each application has an independent downstream branch:

- [WordCraft](https://github.com/jebot-git/wordcraft-xr/tree/domed)
- [GridCraft](https://github.com/jebot-git/gridcraft-xr/tree/domed)
- [DeckCraft](https://github.com/jebot-git/deckcraft-xr/tree/domed)
- [PhotoCraft](https://github.com/jebot-git/photocraft-xr/tree/domed)

The native windows use DomeD's GPU/FLTK path, universal menus, optional tray
integration and existing desktop/XR routing. All three office apps follow the
macOS menu/ribbon arrangement. PhotoCraft has a macOS Photoshop-style main
workspace with tool options, image canvas and docked panels. These remain
development ports: advanced bespoke controls and physical XR qualification are
incomplete, and office bitmap PDF text currently lacks search/selection.

See [office implementation and validation](https://github.com/jebot-git/dome-craft-base/blob/main/OFFICE-PORT.md)
and [PhotoCraft implementation and validation](https://github.com/jebot-git/dome-craft-base/blob/main/PHOTO-PORT.md).
`packaging/craft-suite.json` records the replacement policy. Staging prepares
associations for validated office formats without applying live defaults or
replacing the image viewer. No application is installed by this policy file.

The retirement evidence and original feasibility proposal below are retained
as historical context. The implemented upstream-engine approach supersedes
the original new-engine and VectorCraft-first recommendations.

## Retirement scope and evidence

The published fork is [jebot-git/calligra-xr](https://github.com/jebot-git/calligra-xr),
default branch `domed-fltk`. Preserve its source, commits, licenses, port status
and validation records as a read-only reference. Stop extending or distributing
the fork as DomeD's future office suite. Retirement does not retire upstream KDE
Calligra or declare the Craft proposal a working replacement.

The fork's [FLTK build guide](https://github.com/jebot-git/calligra-xr/blob/domed-fltk/fltk/README.md)
records QtCore/QtXml and headless KDE framework dependencies, despite having no
QtGui/QtWidgets frontend. Sheets retains the Calligra calculation engine. Words
has paragraph editing without the original pagination engine; Stage has
text/image frames without its full shape and animation stack. Office imports
are content subsets and Office export is unavailable. Renaming these frontends
would preserve the dependency and document-engine problems.

Local inspection found no Calligra checkout under this workspace, no matching
installation under `/opt` or `/usr/local`, no matching desktop entries in the
system/local/user application directories, no matching executable on PATH and
no Calligra package in the available dpkg inventory. No active Calligra build
or launcher registration was found in the inspected DomeD/FLTK integration.
These are scoped observations, not a disk-wide uninstall certification. The
older [toolkit retirement notes](GTK_QT_RETIREMENT.md) describe a separate target
installation and must not be treated as this machine's package inventory.

Do not delete documents, change MIME defaults to nonexistent Craft executables,
or remove unrelated office software. If a later target inventory finds a fork
installation, retain recovery data, remove only its owned payload and launcher
entries, and repair only associations naming removed handlers.

## Recommended direction

Build five native FLTK applications sharing a small Craft foundation, with
independent document engines. Use C++20 for the initial FLTK integration and
headless model libraries, matching the existing native frontends. Keep models
independent of FLTK and GPU objects so import, calculation, layout, recovery and
export can be tested without a compositor. A Rust component remains possible
where it provides a measured benefit; an additional language boundary is not
required for the first milestone.

Use the installed DomeD-patched FLTK 1.4.5 and its BDF runtime. No Qt/KDE,
GTK, browser engine or external office executable is required at runtime.
Inspect transitive linkage as well as direct dependencies. Reusing isolated
Calligra algorithms requires a dependency and license review; retaining its
Qt-based engines would contradict this direction. Preserve upstream notices
for any reused code and choose the new project's license before importing it.

| Application | Useful initial scope | Main feasibility risk |
| --- | --- | --- |
| WordCraft | Styled paragraphs, lists, simple tables, inline images, page view, undo, ODT subset and PDF export | Pagination, Unicode editing, text metrics, table layout and document fidelity; highest initial text-engine risk |
| GridCraft | Sparse sheets, typed cells, basic formulas, cross-sheet references, relative/absolute copy, TSV/CSV and ODS subset | A new calculation engine must replace the fork's strongest reused component; coercion, dates, cycles and compatibility are substantial work |
| DeckCraft | Ordered slides, positioned text/images/vector shapes, simple themes, notes, presenter and audience views, ODP subset | Text layout and scene editing are tractable after the shared canvas; master inheritance, animation and PPTX fidelity are later work |
| PhotoCraft | Crop/rotate, raster layers, selections, brush, masks, basic adjustments, PNG/JPEG export | Tiled editing, bounded undo, color management and GPU/CPU result agreement; RAW, PSD and advanced retouching are later work |
| VectorCraft | Paths/nodes, fills/strokes, transforms, grouping, snapping, text and an editable SVG subset | Tessellation, stroke geometry, hit testing, robust boolean operations and SVG fidelity; filters and full SVG support are later work |

All five are plausible as focused native tools. A full Microsoft Office,
Photoshop or Illustrator replacement is a multi-stage product program and
cannot be inferred from FLTK integration alone. Scope and format coverage must
remain visible in each release.

## Shared native foundation

Start in the separate `artcraft` source project rather than linking document
models into the compositor. Suggested boundaries, not directories created by
this decision:

```text
craft-platform   appearance, menus, dialogs, activation, tray integration
craft-document   stable object IDs, transactions, undo, recovery, assets
craft-text       BDF metrics, Unicode editing, layout and glyph atlases
craft-canvas     transforms, selection, paths, images, damage and GPU caches
craft-formats    bounded ZIP/XML, ODF adapters, SVG subset and raster codecs
apps/           wordcraft, gridcraft, deckcraft, photocraft, vectorcraft
```

Share stable primitives rather than one universal document object model.
WordCraft owns flowing blocks and pages; GridCraft owns cells and dependencies;
DeckCraft and VectorCraft can share scene primitives but retain different
document semantics; PhotoCraft owns tiles and adjustment operations. Changes
are transactions with stable identifiers. Undo acts on model changes, never
captured screen pixels. Embedded graphics use versioned asset snapshots first;
live linked objects and cross-application editing can follow later.

Reuse and consolidate the existing integration patterns before inventing new
protocols:

- `native/fltk_appearance.h`, `native/bdf_font.hpp` and `native/gpu-window.h`
  provide live appearance, BDF metrics and the GL widget window pattern.
- The sibling `fsv-xr/fltk/desktop-menu.*` and `qimgvr/fltk/desktop-menu.*`
  implement real menu export. Their application commands are examples, not an
  office document API.
- `qimgvr/fltk/image.*` and `canvas.*` provide raster editing/rendering reference
  code. They do not establish layered editing, ICC accuracy or RAW parity.
- [Working-mode routing](WORKING_MODE_AUDIT.md), the link-handler registry and
  [tray manager](TRAY_FRONTENDS.md) define session integration.

Pin shared integration sources or package a versioned native support library;
avoid five independently drifting copies of these headers.

## Bitmap fonts and document layout

Use native Unicode BDF strikes for controls, menus, dialogs, formula fields,
canvas text and document text. Apply palette/accent/font changes live. Keep
the user's UI font independent from a document's explicit family, strike and
physical-size metadata: changing the desktop appearance must not repaginate a
saved document. Missing document fonts require a visible substitution report.
Never silently load an outline font to make an import look correct.

Maintain document coordinates in physical units or stable logical units, with
an explicit mapping from bitmap strike pixels to document size. Zoom changes
the view transform, not saved line breaks or page geometry. Offer pixel-accurate
editing at integer scales and clearly defined filtered previews at fractional
zoom and in XR. Arbitrary imported outline typography cannot retain identical
appearance under a bitmap-only policy; record the limitation during import.

The text layer must provide grapheme-aware caret/deletion, selection, IME
preedit/commit, Unicode line breaking, bidi and shaping where required by the
supported languages. The existing BDF metrics path alone does not prove these
features. Validate the patched HarfBuzz/Pango integration against actual BDF
coverage before selecting it as the document layout backend. Missing glyphs
need deterministic handling and explicit reporting. Rich document editing
requires a model and layout engine beyond `Fl_Text_Editor`.

PDF export should use the same positioned glyph runs and page geometry as the
editor. Prototype bitmap glyph embedding and searchable text mapping; a page
screenshot is insufficient as the only export. Printing must preserve those
metrics and bitmap-font license requirements. Accessibility needs semantic
document/cell/object information, not only GPU pixels.

## Universal menu and tray behavior

Keep one command registry per application. Keyboard shortcuts, context menus,
tool controls and exported DBusMenu actions invoke those same commands. Export
enabled, checked and selection-dependent states and refresh the surface/menu
association after hide/show. Use KDE Wayland AppMenu association as the existing
frontends do. Standard Ctrl shortcuts belong to the app; Super shortcuts remain
with DomeD. Native FLTK dialogs inherit appearance and bitmap fonts.

Craft editors remain document windows with normal close/save behavior. Use the
existing StatusNotifier/DBusMenu route for an optional background export or
recovery service, so the tray manager can expose its actual commands. Do not
add five permanent tray icons or embed document editors as tray settings pages.
Restore actions must present the correct document in the requested realm;
closing the last document should not leave a daemon unless work or an explicit
background preference requires it.

## GPU ownership

The intended interactive path is model change → bounded layout/geometry or
tile update → retained GL resources → FLTK Wayland/EGL surface → DomeD desktop
composition and leased DMA-BUF transport → native XR presentation. Follow the
existing `GpuWindow` context-loss and text-cache invalidation patterns.

Use retained glyph atlases, raster tiles, meshes and image textures, with damage
tracking and memory budgets. Layout, archive parsing, decoding and initial BDF
rasterization can remain CPU work; upload changed resources rather than a full
CPU-rendered application image on every frame. PhotoCraft needs an explicit
linear/premultiplied color pipeline and ICC conversion policy, with headless
reference results for accelerated operations. VectorCraft needs real editable
path geometry, not just an SVG image widget.

Normal rendering must avoid synchronous framebuffer readback. Readback for
explicit export, recovery of GPU-produced edits and QA is allowed and must be
measured. Keep authoritative document data recoverable after context loss;
never make the only copy of an edit a transient texture. CPU fallbacks for
unsupported GPU paths must be reported rather than counted as native GPU
validation. Test buffer leases, synchronization and import on the actual target
GPU; GL widgets alone are not proof of end-to-end zero-copy behavior.

## Desktop and XR contract

Follow the existing dual desktop/XR presentation architecture. DomeD currently
exposes two Wayland sockets owned by its compositor and a separate native XR
renderer; this is not two independent application document sessions. Desktop
origin windows normally appear in both projections; XR-origin windows follow
the XR-only lifecycle. See the README's XR routing section for current behavior;
some older paragraphs in `DESIGN.md` still describe prototype rendering.

Launch ordinary documents through the verified realm-aware `dome-link` pattern.
Headset availability must not override desktop working mode. A single-instance
app must explicitly present a reused window in the requested realm before
forwarding its document argument. Preserve arguments as arguments, including
spaces and Unicode. Do not guess a different session's Wayland socket.

DomeD owns placement, curvature, focus, ray-to-surface input and OpenXR. Craft
editors submit ordinary Wayland surfaces, without starting their own OpenXR
session or copying document state into an XR-only app. Floating inspectors,
dialogs and menus follow their owner. Keep one authoritative document model
and explicit view state for any additional view. XR renderer loss must leave
unsaved work recoverable on the desktop through DomeD's promotion behavior.

DeckCraft can later open separate audience and presenter surfaces backed by
one deck. Notes must never be rendered into the audience surface. Default
presentation follows the caller's realm; selecting a different audience realm
is an explicit action and requires supported activation/routing. An XR session
does not implicitly move a desktop slide show.

## Formats and data preservation

Use explicitly documented ODT/ODS/ODP subsets for the office apps and editable
SVG subsets for VectorCraft. The [OASIS ODF specifications](https://docs.oasis-open.org/office/)
and [W3C SVG specification](https://www.w3.org/TR/SVG/) define interchange
semantics; they do not supply editing engines or guarantee round-trip fidelity.
Pin the implemented versions and feature matrix when the format adapters land.
GridCraft's formula parser and evaluator need a declared OpenFormula subset,
typed values, error propagation, cycle handling, reference adjustment and a
deterministic date/locale policy.

PhotoCraft needs a versioned layered project format; PNG/JPEG exports cannot
preserve its editable model. Evaluate a bounded ZIP manifest/assets format
before assigning a new extension. Do not claim PSD, RAW or OpenRaster support
until the corresponding model mapping and independent fixtures exist.

For every importer distinguish editable, retained-but-not-editable, converted
and unsupported content. Retaining unknown XML is not sufficient when edits
invalidate its references. Offer read-only access or Save As when fidelity is
uncertain. Preserve original imports. Initial OOXML support should be deferred
until native formats work; later import must clearly report losses and export
must be assessed separately. Do not reuse Calligra's import claims as Craft
coverage.

Bound archive entries, expanded bytes, XML depth, image dimensions and model
object counts. Disable external entities and unsolicited external resources;
macros and embedded code are outside the initial scope. Use atomic saves with
durable replacement, versioned recovery journals, conflict detection and
explicit handling of signatures/encryption. These protect user documents,
including unsupported ones.

## Delivery sequence and acceptance gates

1. **Native foundation spike.** One real FLTK canvas with live BDF appearance,
   exported selection-sensitive commands, native file dialog, model undo,
   atomic save/recovery and realm-aware activation. Exercise desktop and XR,
   tray restore/context recreation and renderer-loss recovery. This tests the
   architectural assumptions before building five shells.
2. **VectorCraft vertical slice.** Editable paths, grouping, selection, undo,
   bounded SVG subset and deterministic rendering. This establishes reusable
   scene/canvas work for DeckCraft. In the same stage, run a focused BDF text
   layout spike; its result controls WordCraft scope.
3. **DeckCraft and PhotoCraft slices.** DeckCraft adds slides and audience views
   on the scene foundation. PhotoCraft proves tiled raster edits and recovery
   with a small operation set. qimgvr remains an independent viewer while its
   reusable code is assessed.
4. **WordCraft and GridCraft engine milestones.** WordCraft must demonstrate
   stable multi-page layout with tables and Unicode editing. GridCraft must
   demonstrate formula dependencies, copy adjustment, dates and recalculation
   against independent expected results. Prototype these risks early, but do
   not advertise either app on the strength of a text box or cell grid.
5. **Interchange and release.** Expand supported format subsets only with
   independent round-trip fixtures, loss reports, recovery testing and native
   Wayland desktop/XR evidence. Add MIME handlers only for formats each shipped
   app can actually open. Stage release artifacts before any installation.

Each milestone needs a reviewable feature matrix, headless model checks,
real menu/shortcut actions, save/reopen verification and transitive dependency
inspection. Check IME, clipboard, drag/drop, accessibility and fractional-scale
behavior explicitly. Measure cold open, edit latency, static redraws, peak
CPU/GPU memory and large-document behavior on a declared reference machine;
set numerical release budgets from that baseline rather than inventing them.
Simulated XR can verify routing and pixels; physical-headset readability,
input and latency require separate evidence.

The first implementation target is the native foundation plus VectorCraft
vertical slice. Proceed to the broader suite only after its text, rendering,
document-preservation and realm-routing gates pass. No implementation schedule
or full compatibility promise follows from this feasibility assessment.
