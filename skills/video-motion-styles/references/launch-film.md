# The launch-film form

A launch film is 15 to 45 s of type and product that says 1 thing. This page gives its structure. The plan gates of `video-storyboard` and the `Format` rules (`short` or `long`) still hold; where a number here differs from a plan gate, the plan gate wins.

## The spine

1. Write the film as 1 sentence. If it needs "and", it is 2 films.
2. Play the sentence as acts: 4 acts for 15 to 35 s, 5 acts from about 45 s. The first act takes 20 to 32 % of the body. The last takes 12 to 31 %.
3. Use 1 case through the whole film: the same request, the same names, the same numbers. A viewer who sees 3 different examples learns none.
4. Choose 1 element that changes form from act to act and carries the eye across the scene changes: a word, a pill, a chip, a button, a card, a page, a window, a list, a notice, a thread, a photo, a count, a mark. At least 2 of 3 scene changes carry it.
5. Pick the music before any timing. The picture is cut to the music. The music is not stretched to the picture.

## Beats

| Beat | What is on screen | Length |
|---|---|---|
| Type | 1 line alone, in the middle band of the frame (35 to 65 % of the height), readable in 1.2 s or less | 1.0 to 1.5 s |
| Product | The product doing 1 thing | 1.5 to 3 s |
| Type with product | A line over a product beat | at most 40 % of beats |
| End | The mark, built from the carried element | 2 to 3 s |

- Type-only beats are at least 15 % of the shots, and at least 2.
- The first line is on screen by 1 s. The film says what the product is by 3 s.
- A line has 1 to 5 words. The whole film has at most about 50 words on screen.
- 6 to 8 story beats per 30 s for a calm film; 8 to 10 for a crisp one.

A 20 s example: type 0 to 1.2 s; product to 4.0; type to 5.2; product to 7.8; type to 9.0; product to 11.4; product to 14.0; type to 15.2; product to 17.5; end to 20.

Acts for 30 to 45 s on a bar grid: hook, what it is, the problem (bars 1 to 3); the turn on the downbeat of bar 4; proof 1 (bars 6 to 9); proof 2 (bars 10 to 12); the promise, after a stop of 0.2 to 0.5 s (bars 13 to 15); the mark, with a tail of sound that fades to silence.

## Grid and pace

- Calm: about 100 beats per minute (a bar is 2.4 s). Crisp: about 120.
- Every cut and every hit is on the grid of `audio/beats.json`.
- 2 energy peaks. The larger sits at 75 to 90 % of the film.
- A line holds, after it has fully arrived, for 0.6 s plus about 0.12 s per word (calm), or 0.47 s plus 0.10 s per word (crisp).
- No more than 12 words arrive in any 5 s.
- Text is on screen at most 85 % of the time. Every 10 s has a run of 1.5 s with no text.
- No stretch longer than 1.2 s without a visible event. A designed freeze is at most 0.25 s, and only on a silence.
- The slowest move in the film is at least 3 times as long as the fastest. That contrast is the rhythm.
- At most 2 fast moves per film, and at most 1 whip.

## Handovers: how the carried element changes form

| Handover | What happens | Lead and landing |
|---|---|---|
| Grow | The element grows into the next surface | 0.2 s and 0.3 s |
| Shrink | A surface shrinks into the element | 0.2 s and 0.13 s |
| Press and open | It squeezes to 88 %, then opens | 0.37 s and 0.6 s |
| Zoom into | The camera enters it | 0.37 s and 0.47 s |
| Pull out | The camera leaves it; it is a part of a larger thing | 0.5 s and 0.8 s |
| Fly | It travels to a new place | 0.23 s and 0.37 s |
| Stay | It holds; the world changes around it | 0.3 s landing |
| Word | 1 word stays while the line around it changes | 0.6 s landing |
| Part or converge | It splits into several, or several join into it | 0.3 to 0.43 s |

- A size change runs on a logarithmic scale, so that the zoom looks even.
- The content inside the element does not scale with it. The old content fades over the first 35 % of the handover and the new content arrives over the last 40 %.
- Mix colours in a perceptual space (OKLCH) so that the middle of a colour change is not grey.

## A cut needs a device

A hard cut reads as designed when 1 of these holds across it:

- the ground stays (the border colour barely changes);
- both sides dip into blur at the cut;
- an anchor object stays within about 10 px;
- motion carries: both sides move the same way, fast out and slower in.

At most 1 cut in 10 is bare. No single kind of scene change makes up more than about 40 % of the changes. The ground flips between light and dark at most 2 times.

## Motion numbers of the form

- Arrivals decelerate, exits accelerate, travel eases at both ends. About half of all moves are arrivals; under 1 in 10 is linear; springs are rare.
- Duration by distance: about 0.5 s plus 1.6 ms per px for a calm film; 0.35 s plus 1.35 ms per px for a crisp one. Camera moves take 0.9 to 3 s.
- Nothing scales up from zero. A thing arrives at 1.3 to 1.9 times its size and settles.
- Type is solid within 4 to 8 frames at 60 fps. Type never overshoots.
- 1 kind of visible spring per film. A critically damped settle of about 0.45 s is the default landing.
- Staggers at 60 fps: 4 frames per letter, 10 per word, 9 per row, made slightly uneven.

## The end

- The end card is built from the last form of the carried element. It is centred, holds 2 to 3 s for a launch film (the `short` plan gate may ask for less), and creeps by at most 6 %.
- An address on screen is readable for at least 1.7 s.
- The mark is at least 1/4 of the frame width and readable for at least 1.5 s.
- Credits for music, sounds and footage go in a credits file, not in the picture.
- The sound decays to silence. The last 50 ms are below -40 dBFS.

## Review before the critic

Watch the draft once with the words covered. The picture alone must tell the spine. Then check:

1. Could this film be for another product? Then the case is missing.
2. Does every scene change carry the element, or have a device?
3. Is the biggest peak late, and is there quiet before it?
4. Is there a frame with 2 things to read?
5. Is any shot there because the technique was available?

Fix the first failed question before the later ones.
