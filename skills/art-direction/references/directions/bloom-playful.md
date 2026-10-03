# Bloom playful

Warm, friendly and confident. White and blush surfaces, a rose primary, rounded shapes and generous space. Playful without becoming childish: the playfulness is in shape and motion, not in clutter.

## Fits / avoid

- Fits: consumer social, education, family and lifestyle products, community apps, creative hobbies.
- Avoid: enterprise and regulated products, technical tools, brands that need gravity.

## Signature moves

1. A rose primary used for one large shape per view (a blob behind the hero subject, a full band), with blush tints for surfaces.
2. Rounded geometry with one shape language (circles and soft rectangles), repeated as a motif.
3. Illustration or photography of people with warmth and direct eye contact.

## Tokens

```css
:root {
  --color-bg: #FFFFFF;
  --color-surface: #FFF1F2;     /* blush */
  --color-text: #18181B;
  --color-text-muted: #6B6B73;  /* 5.3:1 on white, 4.8:1 on blush */
  --color-border: #FFE4E6;
  --color-accent: #E11D48;      /* rose; white text on it is 4.7:1 */
  --color-accent-soft: #FB7185; /* decoration only, never text */
  --color-deep: #881337;        /* bands and footer */
  --font-display: "Plus Jakarta Sans", "Bricolage Grotesque", "Nunito", sans-serif;
  --font-body: "Plus Jakarta Sans", system-ui, sans-serif;
  --type-ratio: 1.333;
  --radius: 20px;               /* buttons full pill */
  --border: 1px solid var(--color-border);
  --shadow: 0 12px 32px -12px rgb(136 19 55 / 0.18);
  --dur-ui: 220ms;
  --dur-scene: 600ms;
  --ease-enter: cubic-bezier(0.34, 1.4, 0.64, 1); /* gentle spring */
}
```

## Type

- Display: rounded or friendly grotesk, weight 700 to 800, `letter-spacing: -0.02em`.
- Body: 16 to 18 px, `line-height: 1.6`.
- One accent word per headline may take the rose color.

## Layout

- Z-split alternating sections, organic blob shapes behind subjects, cards with large radius and soft tinted shadow.
- Generous spacing: 24 / 32 / 48 / 80 px steps.
- Composition families: split, layered, broken grid (light scatter).

## Imagery

Warm, bright photography of real people; or flat illustrations in the palette with rounded forms. Crop people inside circles or soft rectangles.

## Motion character

Bouncy but brief. Entrances: pop (scale 0.9 to 1 with a gentle spring), stagger cut for cards, whip pan for one band. Hover: lift by 2 to 4 px with shadow growth. One heavy interaction at most, such as a draggable or swipeable showcase.

## Storyboard defaults

- Hero posture: subject photo inside a large rose shape, headline and one pill action beside it.
- Arc: promise, encounter, tutorial, community voice, montage, invitation, farewell.
- Density cadence: medium, quiet, medium, spectacle, quiet.

## Do not

- No rose text on blush or on white for body copy (use rose only for large text and fills).
- No more than 2 decorative blobs per view.
- No emoji as icons or decoration.
- No cartoon mascot unless the brand owns one.

## Checks

- Every rose text use is 24 px or larger, or bold 18.66 px or larger.
- `rg -n -P '[\x{1F300}-\x{1FAFF}]' src/` returns nothing.
- Shape motif: the same radius family appears on buttons, cards and image crops.
