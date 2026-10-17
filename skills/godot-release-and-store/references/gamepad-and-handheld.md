# Gamepad and handheld support

Read this when the game must play fully on a gamepad or a handheld PC, with remaps and the right button glyphs.

## Gamepad and handheld support

1. Probe the engine input defaults headless before you design.
   Print each built-in UI action and its events. In some versions the UI accept and cancel
   actions have no pad events.
2. Use 1 input hub autoload, plus static helpers for defaults, event encoding, device family
   and glyphs. All screens ask the hub.
3. Add pad events to the UI accept and cancel actions.
   Set the device of each new pad event on purpose. A new joypad event defaults to device 0,
   so only the first pad works. Use device -1 (all devices) for shared actions. Use 1 device
   only for a deliberate single-player read.
4. Save remaps as plain dictionaries (action, event type, code), not as resource objects.
   Plain data survives version changes. Add a remap page to settings.
5. Pick the glyph family (for example Xbox, PlayStation, Nintendo, generic, keyboard) from the
   vendor id in `Input.get_joy_info` first, then from the joypad name.
   Make 1 glyph sheet per family. Swap the sheet when the last used device changes.
6. Put every screen prompt on 1 prompt control that asks the hub for the glyph.
   - Keep the text as the fallback when no glyph exists.
   - Put the icon on the left of the text, so a long label cannot cover it.
   - Change the words for pad families, for example "Press any button" for "Press any key".
7. Floor the dead-zone slider, and clamp the applied value too.
   A value of 0 makes stick drift move the cursor.
8. Check that pad focus reaches every focusable control.
   Test edge cases, for example 2 tab bars on 1 screen edge. Choose the tab of the focused
   group, else the nearest one.
9. In tests, send events with `Input.parse_input_event`. Send both the press and the release.
   `Input.action_press` does not reach `_unhandled_input`, and a button with only a press
   event can act on release. Reset all bindings after each test.
10. For a handheld PC checklist, write a check tool.
    - Run it at the handheld size (for example 1280x800) in a real window.
    - Override the pad info so the game sees the handheld pad.
    - Save 1 capture per screen and per input family, at 1x and 2x UI scale.
    - Look at each capture and mark each checklist item pass or fail.
11. Ask 1 question per failed item that you do not own, for example a project window-mode
    default. Do not edit a file outside your scope to pass a checklist.
