# Third-party source

The initial nested Wayland plumbing in `src/bin/dome-compositor/` is adapted
from Smithay's MIT-licensed Smallvil example, tag v0.7.0, commit
`a166cf4c94b5aedc332a65aa1dd753e8148829c3`.
Copyright (c) 2017 Victor Berger and Victoria Brekenfeld.
The original MIT license is retained in LICENSE.

The native DRM, GBM and libseat/libinput integration follows Smithay's
MIT-licensed Anvil example from the same v0.7.0 release.

Dome's scene model, shell, control service, preview and tests are new code.
No WiVRn-Pro source is imported into this repository.

The Vulkan/OpenXR initialization in `src/bin/dome-xr.rs` is adapted from the
MIT/Apache-2.0 openxrs `openxr` 0.21.1 Vulkan example (Ralith contributors).
Qt6 is dynamically linked and supplies its own LGPL/GPL/commercial licensing;
Qt source is not included. See dependency distributions for license terms.
