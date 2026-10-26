Smithay 0.7.0, copied from the crates.io source distribution.

Local change: backend/egl/display.rs preserves explicit LINEAR (zero) modifiers
when EGL_EXT_image_dma_buf_import_modifiers is available. The upstream
Dmabuf::has_modifier helper excludes LINEAR; using it to build EGL attributes
incorrectly converted an explicit linear layout into an implicit layout.
The existing error for unsupported non-linear modifiers is retained.
Upstream source and MIT license remain alongside this note.
