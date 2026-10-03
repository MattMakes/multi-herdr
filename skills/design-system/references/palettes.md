# Palettes

Starting palettes by product type, adapted and curated from a 192-palette dataset. Every row passes 4.5:1 for primary on its "on" color, accent on its "on" color, foreground on background, and muted text on background (measured with the formula in `tokens.md`). Brand colors override these rows; use a row as a base when no brand exists.

## How to use a row

1. Pick the row closest to the product and audience. Read its "Primary on bg" column:
   - 4.5 or more: primary works as text and link color on the background.
   - 3.0 to 4.4: primary works for large text, icons, and control boundaries only. Use a darker step for links.
   - under 3.0: primary is a fill or surface color. Use the accent for calls to action and a darker tint for links.
2. Build an 11-step ramp around primary and accent (`tokens.md`), then map semantic tokens.
3. Destructive: `#DC2626` with white text on light themes; `#EF4444` on dark themes (text on it: near-black `#0F172A`, 4.74:1; for white text use `#DC2626`, 4.83:1).
4. Derive the dark theme from the same hues; do not reuse light-theme values. Measure again.
5. Record the final pairs in the contrast table and in `DESIGN.md`.

### Software and SaaS

| Product | Primary / on | Secondary | Accent / on | Background | Foreground | Muted text | Border | Primary on bg |
|---|---|---|---|---|---|---|---|---|
| SaaS | `#2563EB` / `#FFFFFF` | `#3B82F6` | `#EA580C` / `#000000` | `#F8FAFC` | `#1E293B` | `#475569` | `#E2E8F0` | 4.9 |
| Micro SaaS | `#6366F1` / `#000000` | `#818CF8` | `#059669` / `#000000` | `#F5F3FF` | `#1E1B4B` | `#475569` | `#E0E7FF` | 4.1 |
| Analytics Dashboard | `#1E40AF` / `#FFFFFF` | `#3B82F6` | `#D97706` / `#000000` | `#F8FAFC` | `#1E3A8A` | `#475569` | `#DBEAFE` | 8.3 |
| Productivity Tool | `#0D9488` / `#000000` | `#14B8A6` | `#EA580C` / `#000000` | `#F0FDFA` | `#134E4A` | `#475569` | `#99F6E4` | 3.6 |
| AI/Chatbot Platform | `#7C3AED` / `#FFFFFF` | `#A78BFA` | `#0891B2` / `#000000` | `#FAF5FF` | `#1E1B4B` | `#475569` | `#DDD6FE` | 5.3 |
| Docs / knowledge base | `#475569` / `#FFFFFF` | `#64748B` | `#2563EB` / `#FFFFFF` | `#F8FAFC` | `#1E293B` | `#475569` | `#E2E8F0` | 7.2 |
| Developer Tool / IDE (dark) | `#1E293B` / `#FFFFFF` | `#334155` | `#22C55E` / `#0F172A` | `#0F172A` | `#F8FAFC` | `#94A3B8` | `#475569` | 1.2 |
| Financial Dashboard (dark) | `#0F172A` / `#FFFFFF` | `#1E293B` | `#22C55E` / `#0F172A` | `#020617` | `#F8FAFC` | `#94A3B8` | `#334155` | 1.1 |

### Commerce and services

| Product | Primary / on | Secondary | Accent / on | Background | Foreground | Muted text | Border | Primary on bg |
|---|---|---|---|---|---|---|---|---|
| E-commerce | `#059669` / `#000000` | `#10B981` | `#EA580C` / `#000000` | `#ECFDF5` | `#064E3B` | `#475569` | `#A7F3D0` | 3.6 |
| E-commerce Luxury | `#1C1917` / `#FFFFFF` | `#44403C` | `#A16207` / `#FFFFFF` | `#FAFAF9` | `#0C0A09` | `#475569` | `#D6D3D1` | 16.7 |
| B2B Service | `#0F172A` / `#FFFFFF` | `#334155` | `#0369A1` / `#FFFFFF` | `#F8FAFC` | `#020617` | `#475569` | `#E2E8F0` | 17.1 |
| Food Delivery / On-Demand | `#EA580C` / `#000000` | `#F97316` | `#2563EB` / `#FFFFFF` | `#FFF7ED` | `#0F172A` | `#475569` | `#FCEAE1` | 3.4 |
| Real Estate/Property | `#0F766E` / `#FFFFFF` | `#14B8A6` | `#0369A1` / `#FFFFFF` | `#F0FDFA` | `#134E4A` | `#475569` | `#99F6E4` | 5.2 |
| Travel/Tourism Agency | `#0EA5E9` / `#0F172A` | `#38BDF8` | `#EA580C` / `#000000` | `#F0F9FF` | `#0C4A6E` | `#475569` | `#BAE6FD` | 2.6 |
| Construction/Architecture | `#64748B` / `#FFFFFF` | `#94A3B8` | `#EA580C` / `#000000` | `#F8FAFC` | `#334155` | `#475569` | `#E2E8F0` | 4.5 |
| Government / civic | `#1E40AF` / `#FFFFFF` | `#3B82F6` | `#16A34A` / `#000000` | `#EFF6FF` | `#1E3A8A` | `#475569` | `#BFDBFE` | 8.0 |

### Health, education, wellbeing

| Product | Primary / on | Secondary | Accent / on | Background | Foreground | Muted text | Border | Primary on bg |
|---|---|---|---|---|---|---|---|---|
| Healthcare App | `#0891B2` / `#000000` | `#22D3EE` | `#059669` / `#000000` | `#ECFEFF` | `#164E63` | `#475569` | `#A5F3FC` | 3.5 |
| Mental Health App | `#8B5CF6` / `#000000` | `#C4B5FD` | `#059669` / `#000000` | `#FAF5FF` | `#4C1D95` | `#475569` | `#EDE9FE` | 3.9 |
| Yoga & Stretching Guide | `#6B7280` / `#FFFFFF` | `#78716C` | `#0891B2` / `#000000` | `#F5F5F0` | `#0F172A` | `#475569` | `#EDEEEF` | 4.4 |
| Educational App | `#4F46E5` / `#FFFFFF` | `#818CF8` | `#EA580C` / `#000000` | `#EEF2FF` | `#1E1B4B` | `#475569` | `#C7D2FE` | 5.6 |
| Academic / scholarly | `#1E3A5F` / `#FFFFFF` | `#334155` | `#B45309` / `#FFFFFF` | `#F8FAFC` | `#0F172A` | `#475569` | `#CBD5E1` | 11.0 |

### Finance, legal, trust

| Product | Primary / on | Secondary | Accent / on | Background | Foreground | Muted text | Border | Primary on bg |
|---|---|---|---|---|---|---|---|---|
| Banking/Traditional Finance | `#0F172A` / `#FFFFFF` | `#1E3A8A` | `#A16207` / `#FFFFFF` | `#F8FAFC` | `#020617` | `#475569` | `#E2E8F0` | 17.1 |
| Legal Services | `#1E3A8A` / `#FFFFFF` | `#1E40AF` | `#B45309` / `#FFFFFF` | `#F8FAFC` | `#0F172A` | `#475569` | `#CBD5E1` | 9.9 |
| Fintech/Crypto (dark) | `#F59E0B` / `#0F172A` | `#FBBF24` | `#8B5CF6` / `#000000` | `#0F172A` | `#F8FAFC` | `#94A3B8` | `#334155` | 8.3 |

### Media, creative, entertainment

| Product | Primary / on | Secondary | Accent / on | Background | Foreground | Muted text | Border | Primary on bg |
|---|---|---|---|---|---|---|---|---|
| Creative Agency | `#EC4899` / `#000000` | `#F472B6` | `#0891B2` / `#000000` | `#FDF2F8` | `#831843` | `#475569` | `#FBCFE8` | 3.2 |
| Portfolio/Personal | `#18181B` / `#FFFFFF` | `#3F3F46` | `#2563EB` / `#FFFFFF` | `#FAFAFA` | `#09090B` | `#475569` | `#E4E4E7` | 17.0 |
| Magazine/Blog | `#18181B` / `#FFFFFF` | `#3F3F46` | `#EC4899` / `#000000` | `#FAFAFA` | `#09090B` | `#475569` | `#E4E4E7` | 17.0 |
| News/Media Platform | `#DC2626` / `#FFFFFF` | `#EF4444` | `#1E40AF` / `#FFFFFF` | `#FEF2F2` | `#450A0A` | `#475569` | `#FECACA` | 4.4 |
| Social Media App | `#E11D48` / `#FFFFFF` | `#FB7185` | `#2563EB` / `#FFFFFF` | `#FFF1F2` | `#881337` | `#475569` | `#FECDD3` | 4.3 |
| Gaming (dark) | `#7C3AED` / `#FFFFFF` | `#A78BFA` | `#F43F5E` / `#000000` | `#0F0F23` | `#E2E8F0` | `#94A3B8` | `#4C1D95` | 3.3 |
| Video Streaming/OTT (dark) | `#0F0F23` / `#FFFFFF` | `#1E1B4B` | `#E11D48` / `#FFFFFF` | `#000000` | `#F8FAFC` | `#94A3B8` | `#312E81` | 1.1 |

### Food, craft, nature

| Product | Primary / on | Secondary | Accent / on | Background | Foreground | Muted text | Border | Primary on bg |
|---|---|---|---|---|---|---|---|---|
| Restaurant/Food Service | `#DC2626` / `#FFFFFF` | `#F87171` | `#A16207` / `#FFFFFF` | `#FEF2F2` | `#450A0A` | `#475569` | `#FECACA` | 4.4 |
| Bakery/Cafe | `#92400E` / `#FFFFFF` | `#B45309` | `#92400E` / `#FFFFFF` | `#FEF3C7` | `#78350F` | `#475569` | `#FDE68A` | 6.4 |
| Brewery/Winery | `#7C2D12` / `#FFFFFF` | `#B91C1C` | `#A16207` / `#FFFFFF` | `#FEF2F2` | `#450A0A` | `#475569` | `#FECACA` | 8.6 |
| Agriculture/Farm Tech | `#15803D` / `#FFFFFF` | `#22C55E` | `#A16207` / `#FFFFFF` | `#F0FDF4` | `#14532D` | `#475569` | `#BBF7D0` | 4.8 |
| Notes & Writing App | `#78716C` / `#FFFFFF` | `#A8A29E` | `#D97706` / `#000000` | `#FFFBEB` | `#0F172A` | `#475569` | `#EEEDED` | 4.6 |

### Status and security

| Product | Primary / on | Secondary | Accent / on | Background | Foreground | Muted text | Border | Primary on bg |
|---|---|---|---|---|---|---|---|---|
| Status page / incidents | `#16A34A` / `#000000` | `#22C55E` | `#DC2626` / `#FFFFFF` | `#F0FDF4` | `#14532D` | `#475569` | `#BBF7D0` | 3.1 |
| Cybersecurity Platform (dark) | `#00FF41` / `#0F172A` | `#0D0D0D` | `#FF3333` / `#000000` | `#000000` | `#E0E0E0` | `#94A3B8` | `#1F1F1F` | 15.4 |

## Color roles

| Role | Share of a screen | Rule |
|---|---|---|
| Neutrals (background, surface, text, border) | 60 to 70% | One neutral family. Warm or cool, not both. |
| Primary | 20 to 30% (fills, headings, selection) | The brand hue. One per product. |
| Accent | 5 to 10% | Calls to action, highlights. At most one. |
| Status | as needed | Success, warning, error, info. Never decoration. Always with an icon or text. |

- Off-black (`#0A0A0A` to `#1E293B`) reads better than `#000000` for text on light surfaces.
- Keep accent saturation moderate; a fully saturated accent on a large area tires the eye.
- Do not let a status color double as the accent (a green "Buy" button next to green success toasts confuses meaning).
- Gradients: two adjacent hues, low contrast between stops, never behind body text.

## Hue associations (use as a tiebreaker, not as a rule)

| Hue | Associations | Common in | Watch for |
|---|---|---|---|
| Blue | Trust, calm, competence | Finance, health, SaaS, government | The default; differentiate with a second hue or tone |
| Green | Growth, health, money, nature | Fintech, wellness, climate, food | Conflicts with success state |
| Red | Energy, urgency, appetite | Food, sports, media, sales | Conflicts with error state; avoid as primary in health |
| Orange | Friendly, active, affordable | Consumer apps, delivery, retail | Low contrast with white text at light tints |
| Yellow / gold | Optimism, value, luxury (gold) | Food, kids, premium accents | Needs dark text; fails on white as text |
| Purple / violet | Creativity, imagination, premium | Creative tools, beauty, AI | The "AI purple gradient" is a cliché; justify it |
| Pink | Warmth, play, care | Beauty, social, creator tools | Pair with a deep neutral for weight |
| Teal / cyan | Clarity, freshness, tech | Health, productivity, data | Thin contrast on white at mid tints |
| Black / near-black | Authority, luxury, editorial | Fashion, portfolio, publishing | Needs one accent to avoid heaviness |
| Brown / earth | Craft, warmth, heritage | Coffee, bakery, outdoor | Muddy at low lightness; keep chroma |

## Harmony types

- Monochromatic: one hue, many lightness steps. Safest; add one accent.
- Analogous: neighbors on the wheel (blue, teal, green). Calm.
- Complementary: opposites (blue and orange). Strong call-to-action contrast; keep one dominant.
- Triadic: three evenly spaced hues. Energetic; use one as primary and two sparingly.

## Checks

- Every pair you ship is in the contrast table, in each mode.
- View the UI in grayscale: hierarchy still reads.
- Simulate deuteranopia and protanopia if a tool is available: status and chart colors stay distinct (add shape or text if not).
