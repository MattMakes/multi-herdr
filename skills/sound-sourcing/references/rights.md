# Usage rights of downloaded sounds

A launch video is commercial use. It modifies each recording (trims, layers, syncs it to picture). It is published on
platforms that scan audio. Each recording therefore needs terms that allow all 3. Read the terms on the item's own
page, write them into the manifest, confirm them in the audit, and credit them. When in doubt, drop the recording: a
substitute always exists.

hark cannot check rights: audio does not carry them. This file and `rights_audit.py` do.

## Allowed and forbidden

| terms | status | why | credit |
|---|---|---|---|
| CC0 1.0, Public Domain Mark | allowed | no conditions | courtesy |
| CC BY 4.0, 3.0, 2.5, 2.0 | allowed | attribution is the only condition; commercial use and edits are fine | required: title, author, source link, licence link, and a note that it was modified |
| Mixkit Sound Effects / Stock Music Free License | allowed | free commercial video use without attribution; check the current terms page | courtesy; never redistribute raw files; never register the film with Content ID |
| Pixabay Content License | allowed | free commercial use; scripts are blocked, so the user downloads by hand | courtesy |
| CC BY-NC (any version) | forbidden | the film is commercial | |
| CC BY-ND | forbidden | trimming and syncing make a derivative | |
| CC BY-SA | forbidden | the whole film would need CC BY-SA | |
| Sampling+, Sampling | forbidden | retired and restrictive | |
| all rights reserved, personal use, editorial, custom terms, "free with credit on social media" | forbidden | terms the audit cannot trust | |
| 2 sources disagree (disputed) | forbidden | the strictest reading applies | |
| "royalty free" with no terms, platform rips, anything behind a login or paywall, AI-generated or code-made audio | not used | unknown terms, or not a recording | |

`rights.py` implements this table. `classify(text)` returns a key (`cc0`, `pd`, `by4`, `by3`, `by25`, `by2`, `by1`,
`mixkit-sfx`, `mixkit-music`, `pixabay`, `forbidden`, `restricted`, `unknown`, `none`) and a status (`allowed`,
`forbidden`, `unknown`). Forbidden words win over everything. CC BY 1.x is `unknown`: confirm it on the page. A text
that names a free licence and "all rights reserved" is `unknown`. Quirks: the bare word "zero" reads as CC0, and
"CC BY 3.5" gives a key `by35` whose URL does not exist; read such entries by hand.

## Verify per site

Freesound

- The item page is `https://freesound.org/people/<user>/sounds/<id>/`. The licence is in its sidebar and applies per
  sound, not per user.
- Original files need a login. Use the public HQ preview:
  `https://cdn.freesound.org/previews/<id // 1000>/<id>_<user_id>-hq.ogg` (Ogg Vorbis, about 192 kbps; `-hq.mp3`
  exists too). The user id is the uploader's number on the same page. Never use the `-lq` preview.
- Read the description. An author's credit form goes into `attribution`. A ban on posting raw files goes into
  `agent_notes`.

Mixkit

- Effects have no page of their own. Record the category page that lists the item and the title shown there.
- The WAV is at `https://assets.mixkit.co/active_storage/sfx/<id>/<id>.wav`.
- Read the licence terms page once per session and quote what it allows. Never publish the raw folder. Never register
  the film with a rights-management service.

Music

- Composer libraries that release under CC BY state the licence on the track page, often with a credit line to use
  unchanged. Confirm on the artist's own page: an aggregator never counts. Example of a trap: an aggregator showed
  CC BY 3.0 for a track whose artist page asked only for a social-media credit. The terms were disputed and the track
  was rejected.
- On Free Music Archive, the Internet Archive and SoundCloud the terms vary per track and are often NC or ND.
- Platform libraries bind their terms to 1 platform. Ask the user first.

Pixabay

- Give the user the page link and ask them to save the file into the assets folder. Audit it like any other file.

## The audit (`rights_audit.py`)

```
{{PY}} -I {{SKILL}}/scripts/rights_audit.py --root . [--report out/qa/mix_report.json] [--fetch] [--decode scope|all|none]
```

- With `--report`, only the files in its `files_used` are in scope. Without it, every entry is.
- The auditor writes only `rights_seen`, `rights_confirmed` and `agent_notes`. The script computes the rest on every
  run and keeps the auditor fields, matched by id and file path.
- Blockers (exit 1): no `file` or `rights`; forbidden or unknown terms; no page reading (`rights_seen` empty);
  `rights_seen` without `rights_confirmed`; a CC BY entry without `attribution`, `author` or `page_url`; a missing,
  unprobeable, broken or silent file; a used file in no manifest; 1 file listed twice under a forbidden licence.
- Warnings: missing id, title, author or page_url; `rights_seen` reads as another licence; an attribution without the
  author or a licence name; Freesound and Mixkit id mismatches and `-lq` previews; a video stream; a very quiet file; a
  duration that changed since analysis (file replaced?); a sample rate under 44.1 kHz; mp3 under 192 kbps; local bytes
  that differ from `download_url` (with `--fetch`).
- `--fetch` downloads each in-scope `download_url` into memory, 1.5 s apart, at most 200 MB, never to disk and never
  run, and compares SHA-1 with the local file. Pixabay is skipped (compare by hand).
- Before it overwrites `rights.json` it keeps `rights.prev.json` (and `rights.handwritten.json` once, for a
  hand-written audit). It also writes the table `rights.md`.

## Attribution and Content ID

A full CC BY credit gives the title, author, source link, licence name and licence link, and says the work was
modified. `credits.py` adds the missing links around an author's requested form and writes `CREDITS.md` plus
`out/CREDITS_description.txt` for the video description. Credits never need to be on screen.

Some composers register CC BY tracks with Content ID through a distributor, so a lawful use can still get an automatic
claim. The use stays permitted: keep the credit in the description and dispute the claim with the licence link. Many
composers release claims when asked. Prefer composers who document this, and warn the user before publishing. Mixkit
and Pixabay forbid the opposite: never register a film that uses their items.
