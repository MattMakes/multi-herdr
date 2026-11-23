# Internationalization and accessibility

Target engine: Godot 4.7. Translation, locales, right-to-left text and fonts, plus screen reader support, scaling, text to speech and other access options.

## Use this shelf when

- You add a second language, or text overflows and breaks layouts after translation.
- You need plurals, context, or the same string translated two ways.
- You ship Arabic, Hebrew, CJK or Thai and text renders wrong or wraps badly.
- A menu or HUD must work with a screen reader or without a mouse.
- You add options for UI scale, reduced motion, contrast or spoken text.

## How the engine works here

- `TranslationServer` holds `Translation` resources per locale. `tr(message, context)` looks the message up for the current locale, then falls back to `internationalization/locale/fallback`, then returns the message unchanged.
- Locale codes are `language_Script_COUNTRY_VARIANT`. Only the language is required. `TranslationServer.compare_locales` scores how close two locales are (0 to 10).
- Controls translate their own text when it matches a key. `Node.auto_translate_mode` (inherit, always, disabled) controls this per subtree and also decides which strings go into a generated template. `Control.auto_translate` is deprecated.
- `Node.atr()` and `atr_n()` are `tr()` and `tr_n()` that respect `auto_translate_mode`.
- A locale change sends `NOTIFICATION_TRANSLATION_CHANGED` to every node. Rebuild text made in code (formatted strings, custom drawing) there.
- A `TranslationDomain` (since 4.4) is a separate set of translations. `Object.set_translation_domain` picks the domain for `tr()`; nodes inherit it from the parent. Project Settings fills the main domain.
- `TextServerAdvanced` (ICU, HarfBuzz) shapes BiDi text and complex scripts. `TextServerFallback` does neither.
- For a right-to-left locale the engine mirrors anchors, margins, text alignment and container child order, but not coordinates or non-UI nodes.
- Screen reader support (since 4.5) uses AccessKit on Windows, macOS and Linux; mobile and web are not covered yet, and the release page calls the feature experimental. Each node can own an accessibility element; built-in controls fill theirs. Updates run only while an assistive app is active (`accessibility/general/accessibility_support`: Auto). The screen reader speaks, not the game.

## Advanced techniques

### Keys or source text: choose once

Keys (`MENU_START`) let you revise English without breaking translations and fit CSV. Source strings fit gettext, but identical English strings collide: pass a context, `tr("Close", "Actions")` versus `tr("Close", "Distance")`.

### Plurals and placeholders

```gdscript
var count: int = 3
var line: String = tr_n("There is %d apple", "There are %d apples", count) % count
var pickup: String = tr("{who} picked up the {item}").format({"who": "Ogre", "item": "Sword"})
```

`tr_n` picks the form by the locale's plural rule (more than two forms in many languages). Non-negative integers only. Named placeholders let the translator reorder words; `%s` lists do not.

### gettext workflow

**Project Settings > Localization > Template Generation** extracts strings from scenes and scripts into a `.pot`: `tr`, `tr_n`, `atr`, `atr_n` calls with constant strings, assignments to `text`, `placeholder_text`, `tooltip_text`, and calls such as `add_item`. A `# TRANSLATORS:` comment on or above the line reaches the translator. `# NO_TRANSLATE` skips a string. Update with `msgmerge --update fr.po messages.pot`. Fuzzy entries are not loaded. `.mo` files are smaller and faster to read; worth it from several thousand strings up. Custom formats: `EditorTranslationParserPlugin`. Since 4.6 C# `Tr` and `TrN` calls are extracted too, and the same dialog can write a CSV template in place of a `.pot`.

### CSV with plurals and context (since 4.6)

Add a `?plural` column for the plural source, a `?context` column for context, and a `?pluralrule` row to override a rule. Extra plural forms go on following rows with an empty key. Rows that use either are not auto-translated by controls; call `tr` or `tr_n`. Save CSV as UTF-8 without a byte order mark. Columns whose header starts with `_` are comments. Importing does not register the translation: add the generated files under **Localization > Translations**. Without context or plurals the importer makes an `OptimizedTranslation`, which cannot list its messages.

### Pseudolocalization before any translation exists

Enable `internationalization/pseudolocalization/use_pseudolocalization`. Accents reveal hard-coded strings and missing glyphs. `expansion_ratio` of 0.3 lengthens text by 30 percent, enough for most cases. `fake_bidi` tests mirrored layout. `override` turns localized strings into asterisks. Runtime toggle: `TranslationServer.pseudolocalization_enabled`; after a setting change call `reload_pseudolocalization()`.

Real locales: `godot --language fr`, the `internationalization/locale/test` setting (do not commit it), or **View > Preview Translation** in the editor (since 4.5). The 4.5 release page notes that layout direction did not follow the previewed locale, so check right-to-left layouts in a running game as well.

### Fonts per script

Translation remaps do not work for fonts. Add fallbacks to the font (**Fallbacks** in its Advanced Import Settings, or `Font.fallbacks`): one for Cyrillic, CJK, emoji and so on. Fallbacks work for any text, also chat in a language other than the UI. A `SystemFont` can be a fallback, but coverage varies per machine. The default project font covers only part of Latin-1.

### Right-to-left and structured text

Mirroring is automatic. Override with `Control.layout_direction` and, on text controls, `text_direction` and `language`. Paths, URLs and email addresses break under the BiDi algorithm: set `structured_text_bidi_override` (for example `TextServer.STRUCTURED_TEXT_FILE`). Flip arrow icons that mean direction. Test without a translation: `internationalization/rendering/force_right_to_left_layout_direction`.

### Word breaking for CJK and Thai

Languages without spaces need dictionary data to wrap lines. It is not exported by default: enable `internationalization/locale/include_text_server_data` (about 4 MB). `internationalization/locale/line_breaking_strictness` or a locale suffix such as `ja@lb=strict` tunes the rules.

### Name every control for the screen reader

```gdscript
extends Label

func _ready() -> void:
	accessibility_live = AccessibilityServer.LIVE_POLITE

func announce(message: String) -> void:
	text = message
```

Set `accessibility_name` on icon-only buttons and `accessibility_description` for extra help. Link a field to its caption with `accessibility_labeled_by_nodes`. A live region is read when it changes while focus is elsewhere: polite waits for the current sentence, assertive interrupts. Use it for toasts and combat logs. `Control.FOCUS_ACCESSIBILITY` makes a control focusable only while a screen reader runs, so static text is reachable without changing keyboard flow for others. Keep `focus_next` order logical. `focus_behavior_recursive` (since 4.5) disables focus for a whole subtree, for example a menu hidden behind a dialog.

Since 4.7, set `accessibility_region` on a `Container` and give it an `accessibility_name`: screen readers treat it as a landmark, announce it when focus enters, and let the user jump between regions (inventory, map, chat). A field named "Name" means little without the region it sits in.

### Custom-drawn nodes

```gdscript
extends Control

var hit_points: int = 10

func _notification(what: int) -> void:
	if what == NOTIFICATION_ACCESSIBILITY_UPDATE:
		var element: RID = get_accessibility_element()
		AccessibilityServer.update_set_role(element, AccessibilityServer.ROLE_STATIC_TEXT)
		AccessibilityServer.update_set_name(element, "Health: %d" % hit_points)
```

`AccessibilityServer` is new in 4.7; on 4.5 and 4.6 the same calls are `DisplayServer.accessibility_update_*`. Call `queue_accessibility_update()` when the value changes. Only call `get_accessibility_element()` inside this notification. `update_set_flag` with `FLAG_HIDDEN` hides decoration. `_get_accessibility_configuration_warnings()` in a tool script shows warnings in the Scene dock.

### Follow system preferences

```gdscript
var reduce_motion: bool = DisplayServer.accessibility_should_reduce_animation() == 1
var high_contrast: bool = DisplayServer.accessibility_should_increase_contrast() == 1
var reader_on: bool = DisplayServer.accessibility_screen_reader_active() == 1
```

All three exist since 4.5. Each returns 1, 0, or -1 for unknown; coverage differs (reduce animation: macOS and Windows only). Use them as defaults for your own options.

### UI scale as an option

Set `get_tree().root.content_scale_factor` at runtime with stretch mode `canvas_items`. It scales all 2D content, themed or not. `ThemeDB.fallback_base_scale` only affects controls without a theme override. The OS scale factor is read on macOS, Android and Linux Wayland only, so offer a manual slider. MSDF fonts stay sharp at any scale.

### Text to speech

Enable `audio/general/text_to_speech` first.

```gdscript
var voices: PackedStringArray = DisplayServer.tts_get_voices_for_language("en")
if not voices.is_empty():
	DisplayServer.tts_speak("Game saved", voices[0], 50, 1.0, 1.0, 0, true)
```

The call is asynchronous; delay varies by platform. Linux needs speech-dispatcher. A screen reader serves blind players better because it keeps their voice and speed. With TTS, let players pick voice and rate and save both.

## Niche but useful

- **Localization > Remaps**: another resource per locale (voice lines, textures with baked text). Loading the original path returns the remapped one.
- `Control.translation_context` (since 4.7) gives auto-translated text a context.
- `Control.localize_numeral_system` converts digits in `SpinBox` and `ProgressBar`; use `TranslationServer.format_number` (since 4.6) for digits inside other text.
- `TranslationDomain.set_locale_override` (since 4.5): one subtree in another language.
- `Translation.plural_rules_override` (since 4.6) fixes a wrong plural rule per file.
- Patch PCKs use delta encoding since 4.6: a patch that changes or adds a language ships only the changed parts of large translation files.
- `application/config/name_localized`: app name per locale.
- `OS.get_locale_language()` for the first run; store the player's choice afterwards.
- `--accessibility always` forces updates on, for inspection tools.
- `input/ui_accessibility_drag_and_drop`: keyboard drag and drop action.

## Practice not provided by the engine

Build these from ordinary features; the engine has no switch for them.

- Remapping: `InputMap.action_erase_events` and `action_add_event`, saved to a file. See `godot-input-handling`.
- Color vision: never encode state by hue alone; add shape or text. Keep palettes in a `Theme` or shader uniforms so a second palette is a data swap.
- Reduced motion: one flag that gates screen shake, flashes and parallax.
- Subtitles: a `RichTextLabel` on a background panel, speaker name, size option, timing data next to the audio, text through `tr()`.

## Traps

- Text set from code with `%` formatting is not auto-translated again on locale change.
- A player name that matches a key gets translated: disable `auto_translate_mode` there.
- Excel does not save CSV as UTF-8 by default.
- `TextServer.format_number` is deprecated; use `TranslationServer.format_number`.
- `DisplayServer.accessibility_update_*` methods are deprecated; use `AccessibilityServer`.
- `set_locale_override` does not refresh the tree; send the notification yourself.

## Version notes

- 4.4: `TranslationDomain` (absent from the 4.3 class reference); `EditorTranslationParserPlugin._parse_file` changed (breaking).
- 4.5: screen reader support through AccessKit, the `accessibility_*` properties, `FOCUS_ACCESSIBILITY`, the system preference queries; editor translation preview; editor language switch without restart. In the editor only the Project Manager, standard UI nodes and the inspector are covered (4.5 release page).
- 4.6: CSV `?plural`, `?context`, `?pluralrule` and CSV templates; C# string extraction; `TranslationServer.format_number`; CSV import `compress` is now a mode.
- 4.7: `AccessibilityServer` singleton holds the accessibility API; `Control.accessibility_live` uses its enum; `Container.accessibility_region`; `Control.translation_context`; `OptimizedTranslation.generate` returns `bool`.

## Related skills

- `godot-localization`: step-by-step setup of CSV and PO translation and locale menus.
- `godot-responsive-ui`: layouts that survive longer text and larger scale.
- `godot-input-handling`: input remapping.
- Sibling shelves: `ui-advanced`, `input-advanced`, `audio-advanced`, `editor-tools-plugins`.

## Sources

- https://docs.godotengine.org/en/stable/tutorials/i18n/internationalizing_games.html
- https://docs.godotengine.org/en/stable/tutorials/i18n/localization_using_gettext.html
- https://docs.godotengine.org/en/stable/tutorials/i18n/localization_using_spreadsheets.html
- https://docs.godotengine.org/en/stable/tutorials/i18n/pseudolocalization.html
- https://docs.godotengine.org/en/stable/tutorials/i18n/locales.html
- https://docs.godotengine.org/en/stable/tutorials/ui/creating_applications.html
- https://docs.godotengine.org/en/stable/tutorials/audio/text_to_speech.html
- https://docs.godotengine.org/en/stable/tutorials/ui/gui_using_fonts.html (and `tutorials/rendering/multiple_resolutions`)
- https://godotengine.org/releases/4.5/ , https://godotengine.org/releases/4.6/ , https://godotengine.org/releases/4.7/
- https://godotengine.org/article/dev-snapshot-godot-4-5-dev-3/ (screen reader support)
- https://godotengine.org/article/dev-snapshot-godot-4-5-beta-1/
- https://godotengine.org/article/dev-snapshot-godot-4-6-beta-1/ (CSV, C# extraction)
- https://godotengine.org/article/godot-4-5-making-dreams-accessible/
