# Prompt 04: Slint PageModel renderer

Implement tasks 7.2 through 7.5 and the applicable part of 7.11.

Generate or expose Slint-compatible design values from `themes/default/tokens.json`; avoid maintaining unrelated duplicate constants. Render every current `Widget` variant from validated `PageModel` data. Plugins must not provide absolute coordinates, Slint source, HTML, scripts, colors outside approved semantic tones, or arbitrary font files.

The renderer must handle:

- adaptive column count
- vertical overflow
- long text
- maximum-size valid models
- empty sections
- many pages without indicator overflow
- scale and Reduce Motion configuration

Keep model conversion outside generated Slint code where practical. Add unit tests for model-to-view conversion and screenshot tests only if they are deterministic in CI. Do not redesign the visual language; use the existing token seed and Swift screenshots from `legacy/swift` only as a visual reference.

Run full validation and create one focused commit.
