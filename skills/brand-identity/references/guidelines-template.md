# Brand guidelines template

Copy the template, fill every field, mark unknowns `TODO(owner)`, and save it where the brief says (default `docs/brand-guidelines.md`). Keep values exact; a rule without a value or an example is not a rule.

## Template

```markdown
# <Brand> Brand Guidelines

> Version <x.y> - <YYYY-MM-DD> - Status: <draft | approved> - Owner: <role>

## Quick reference
| Element | Value |
|---|---|
| Brand idea | <one sentence> |
| Positioning | <one sentence> |
| Voice | <trait>, <trait>, <trait> |
| Primary color | <Name> `#xxxxxx` |
| Accent color | <Name> `#xxxxxx` |
| Heading font | <Family weight> |
| Body font | <Family weight> |
| Logo minimum | <px> digital, <mm> print |

## 1. Strategy
- Mission: <We ... for ... by ... so they can ...>
- Vision: <A world where ...>
- Value proposition: <For ... who ..., <brand> is a ... that .... Unlike ..., we ....>
- Positioning: <...>
- Audience: <segments, with their main need>
- Brand idea and metaphor: <one sentence and the reasoning>

### Messages
| Message | Audience need | Proof point |
|---|---|---|

### Elevator pitches
- 10 s: <...>
- 30 s: <...>

## 2. Voice and tone
### Traits
| Trait | We are | We are not |
|---|---|---|

Spectrums (1-5): formality <n>, complexity <n>, character <n>, emotion <n>

### Tone by context
| Context | Tone | Example |
|---|---|---|
| Marketing | | |
| Onboarding | | |
| Error | | |
| Success | | |
| Support | | |

### Vocabulary
| Use | Avoid | Reason |
|---|---|---|

### Mechanics
Case: <sentence case for headings and buttons> - Numbers: <digits> - Dates: <format> -
Contractions: <yes/no> - Exclamation marks: <rule> - Emoji: <rule>

### Rewrites
| Before | After |
|---|---|

## 3. Logo
- Type and concept: <type>, <method>: <why the mark looks like this>
- Variants: primary (horizontal), stacked, symbol, wordmark - <file paths>
- Color versions: full color, reversed, one-color dark, one-color light
- Clear space: <rule, for example "height of the symbol on all sides">
- Minimum size: <lockup px / symbol px / print mm>
- Approved backgrounds: <list, with the version to use on each>
- Misuse: no stretching, rotating, recoloring, effects, outlines, cropping, rearranging, busy backgrounds
- Co-branding: <rule>

## 4. Color
### Brand
| Name | Hex | RGB | Role |
|---|---|---|---|

### Neutrals
| Name | Hex | RGB | Role |
|---|---|---|---|

### Status
| State | Hex | Use (always with an icon or text) |
|---|---|---|

### Contrast (measured)
| Foreground | Background | Ratio | Use allowed |
|---|---|---|---|

Proportion: neutrals <60-70%>, primary <20-30%>, accent <5-10%>.

## 5. Typography
| Role | Family | Weight | Size desktop / mobile | Line height | Tracking |
|---|---|---|---|---|---|
| H1 | | 700 | 48 / 32 px | 1.2 | -0.02em |
| H2 | | 600 | 36 / 28 px | 1.25 | |
| H3 | | 600 | 28 / 24 px | 1.3 | |
| Body | | 400 | 16 / 16 px | 1.5-1.6 | 0 |
| Small | | 400 | 14 / 14 px | 1.5 | 0 |
| Caption | | 400-500 | 12 / 12 px | 1.4 | |

Licenses: <family: license, source>. Loading: <self-hosted or service, display=swap>.

## 6. Imagery
- Photography: <lighting, subjects, composition, treatment>
- Illustration: <style, stroke, palette>
- Graphic devices: <patterns, textures, grid>
- Do: <3 lines> - Don't: <3 lines>

## 7. Icons
Family <name> - style <outline/filled> - stroke <px> - grid <24 px> - radius <px>

## 8. Applications
- Digital UI: <how brand maps to the design system; link DESIGN.md or tokens>
- Social: <avatar = symbol; cover image rules>
- Email: <header, signature, max width 600 px>
- Documents and slides: <logo placement, title styles>
- Print: <stationery rules, if any>

## 9. Token mapping
| Brand name | Token | Value |
|---|---|---|
| <Primary name> | `--color-brand-600` / `--primary` | `#...` |

## Changelog
| Version | Date | Change |
|---|---|---|
```

## Asset naming and storage

- Pattern: `<type>_<campaign-or-evergreen>_<description>_<YYYYMMDD>_<variant>.<ext>`, kebab-case parts. Example: `banner_spring-launch_hero_20261003_16x9.png`.
- Logos live in one folder (`assets/logos/`) with subfolders per variant; never copy logo files into feature folders.
- Keep source files (SVG, design files) beside exports; record the license of every stock image and font.

## Consistency checklist (audits)

### Visual
- [ ] Current logo version, correct variant for the space, clear space and minimum size met.
- [ ] Logo color version matches the background; no effects or modifications.
- [ ] Only palette colors; proportions roughly 60/30/10; status colors used only for status.
- [ ] Text contrast at least 4.5:1 (large text 3:1); color never the only signal.
- [ ] Brand fonts, correct weights, defined hierarchy, readable sizes.
- [ ] Imagery matches the stated direction; licensed; consistent treatment.
- [ ] One icon family and style.

### Verbal
- [ ] Copy matches the voice traits; no "we are not" side appears.
- [ ] Tone fits the context (error, success, marketing, legal).
- [ ] Vocabulary list respected; no banned terms; consistent capitalization.
- [ ] Claims have proof; no invented numbers or customers.
- [ ] Calls to action are specific and consistent across channels.

### Channels
- [ ] Website: header, footer, key pages.
- [ ] Product UI: empty states, errors, emails sent by the product.
- [ ] Social profiles: avatar, cover, bio.
- [ ] Email templates and signatures.
- [ ] Documents and slides.

### Findings format

| Severity | Surface | Rule | Issue | Fix |
|---|---|---|---|---|
| High | Pricing page | Contrast 4.5:1 | Accent text on cream is 2.9:1 | Use the 700 step (5.1:1) |

Severity: High (legal, accessibility, wrong or distorted logo), Medium (off-palette color, off-voice copy on a key surface), Low (minor inconsistency).

## Approval readiness (before an asset ships)

- [ ] Serves the stated purpose and audience.
- [ ] Visual and verbal checklists pass.
- [ ] Accessible: contrast, alt text, readable sizes, logical reading order.
- [ ] Technical: correct format, dimensions, file size, naming.
- [ ] Legal: images, fonts, music licensed; required disclosures present; no misleading claims; no third-party trademark misuse.

Report readiness to the orchestrator with the findings table. Sign-off by people happens outside the worker session.
