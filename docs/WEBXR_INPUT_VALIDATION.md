# Browser input integration validation

The FLTK toolkit patches expose optional window-visibility, touch and pen
callbacks. Native compositor input routing preserves contact ownership and
cancels contacts when their window becomes unavailable. OpenXR controller and
reference-space extensions retain compatibility with the existing protocol-v1
head/stereo bridge.

The 27 October 2026 Servo browser candidate passed 185 selected unit tests,
548 runtime checks and seven upstream Pointer Events cases. This includes an
87-check native pen suite and regression coverage for touch, mouse, controllers,
visibility and reference-space resets. See the browser repository's
`ports/dome-fltk/docs/validation-2026-10-27.md` for scope and commands.

The clean publication snapshot also passed 219 optimized host unit tests. Two
runtime/asset-dependent tests were ignored.

Private native input tests require the `input-test-fixture` feature and nested
backend. Normal compositor builds exclude this fixture. Tests use isolated
profiles, session buses and compositor instances; they do not restart desktop or
XR sessions. Physical touchscreen/tablet and WiVRn-Pro/controller validation
remain open. These results do not establish full WebXR conformance.
