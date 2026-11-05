# Page contract

A finished page has every property below. A reviewer checks each one. Items marked **required** fail the page when missing.

## Corners and centre

1. A title in the top-left corner states the point of the page in one sentence.
2. A date sits in the bottom-right corner, with an optional status word (Draft, Proposal, Final).
3. The centre holds one picture of the designed thing, drawn or commissioned.

## Margins

4. Callouts sit in the margins. Each one links to the picture by a leader line or a matching number.
5. When any symbol appears more than once, a legend is present.
6. Repeated items are cards, and every card has the same layout.
7. **Required:** a callout or card for a design that has story, rules and parameters shows the story in italic, the rules in upright text and the parameters in one small italic line. A callout or card in one text style only fails. Example: a spell card whose name, flavour, rules and numbers are all upright fails; set the flavour italic, the rules upright and the numbers as a small italic line.

## Picture share rule (required)

8. The region that holds the picture covers at least half of the sheet.
9. No other part of the page except the title is wider than that region.
10. A drawn panel as wide as the picture that sits against it counts as part of the picture. Any other panel counts as a card set.

Example: a 30 x 20 in page whose map covers 35 percent of the sheet while rule panels take the rest fails. Grow the map to at least half the sheet and shrink or cut the panels.

## Text sizes (required)

11. Only the title and the labels on the picture use the largest text level.
12. All margin text, including callout and card headings, is 11 pt or less.
13. No text anywhere is smaller than 6 pt.

## Counts (required)

14. At most 8 callouts. Example: a map with twelve callouts fails; combine callouts or move content into a card set.
15. Every leader line stops on the picture itself, never on another label or panel.
16. The rest of the required content is grouped into at most 3 card sets, placed along one edge or two edges.

## Time and difficulty

17. A design that involves time or difficulty draws them: rulers, phase dividers, shaded zones. It does not explain them in sentences.

## One surface

18. All content is visible on the one sheet. No links, tabs, collapsed sections, hidden content, or content that shows only on hover.
19. The width and height in inches are set one time, on the document root. The screen sheet and the printed page size both read those two values.
20. The page loads no external fonts, scripts or stylesheets.

## Too large

21. When the checker reports a side longer than 44 in, treat the design as too complex for one page. Remove content or split it into more pages. Never make fine print smaller than 6 pt to gain space. Example: a design that needs 48 x 36 in to keep 6 pt fine print gets cut down or split; its text does not shrink.

## What the checker covers

`scripts/check_page.py` checks items 13, 14, 18, 19 and 20, the 44 in limit, and the drawing layer scale. It does not render the page, so it cannot see the picture share rule, overlap, clipping, text styles or leader line ends. Check those by eye on the preview image.
