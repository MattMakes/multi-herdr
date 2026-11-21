# Openverse: 1 search over many libraries

Openverse is a search index of openly licensed media. Its audio index covers several libraries at once, among them Freesound, Jamendo, Wikimedia Commons and ccMixter. Use it in step 2 to find candidates fast. It does not host the files and it does not own the rights: it points to them.

Everything in [rights.md](rights.md) still applies. Openverse makes the search shorter. It does not make the audit shorter.

## Before you search

- It is a network service. The host must be allowed to reach `api.openverse.org`, and each provider's own host for the download. When a request is refused by the network policy, report it and name the host; do not look for another route.
- Anonymous use is allowed and rate-limited, with small pages. That is enough for a film. A registered client gets higher limits through a token. A token is a credential: ask the operator, never put one in a file, a command line that is logged, or a manifest.
- Send no personal detail. Use a generic client name in the request header.
- Check the live API description at `https://api.openverse.org/v1/` when a parameter is refused. Names change.

## Search

```sh
curl -sS --get "https://api.openverse.org/v1/audio/" \
  --data-urlencode "q=door latch click close" \
  --data-urlencode "license=cc0,by,pdm" \
  --data-urlencode "category=sound_effect" \
  --data-urlencode "page_size=20" \
  -H "Accept: application/json" -o out/audio-review/openverse-door.json
```

| Parameter | Use |
|---|---|
| `q` | 2 to 5 plain words: the object, the action, the material. Try 3 wordings per need. |
| `license` | `cc0,by,pdm` only. Never leave it out: the index also holds NC, ND and SA items, which the film cannot use. |
| `category` | `sound_effect` or `music` |
| `length` | `shortest`, `short`, `medium` or `long`, to drop hour-long field tapes or 1 s blips |
| `source` | limit to 1 library when it has the best takes for a category |
| `page`, `page_size` | walk the result pages; stop at 15 to 40 candidates per category |

Save every response under `out/audio-review/`. It is the record of what the search showed on that day.

## Read a result

Each item in `results` carries, among others: `id`, `title`, `creator`, `creator_url`, `license`, `license_version`, `license_url`, `foreign_landing_url` (the item's own page at the library), `url` (the audio file), `duration` in milliseconds, `filetype`, `sample_rate`, `bit_rate`, `source`, `provider`, and a ready `attribution` line.

1. Drop on sight: any licence value other than `cc0`, `by` or `pdm`; a `duration` far from the need; a `bit_rate` that means a low-quality file.
2. **Open `foreign_landing_url` and read the licence there.** The index is built by crawlers and can be out of date or wrong: an author may have changed the terms, or the item may be gone. The item's own page is the evidence that the audit accepts. The index entry is not.
3. When the 2 disagree, the item is disputed, and a disputed item is forbidden.
4. Read the page for more than the licence: the recording notes, a requested credit form, and whether the upload looks like the author's own recording. A film soundtrack, a game rip or a famous sample under a free licence is a rip. Drop it.
5. For a `by` item, check the version on the page. Version 1.x needs a hand check; the audit marks it unknown.

## Download and record

- Download from the library, 1 file at a time, with a pause between. Follow the per-site notes in [rights.md](rights.md): some libraries give a preview without a login and keep the original behind one. Nothing behind a login is used.
- Prefer a lossless file. Take a compressed preview only at high quality, and note it in the manifest.
- Name the file `audio/assets/<category>/<source>-<id>_<slug>.<ext>`, where `<source>` is the library (for example `freesound`), not `openverse`.
- Fill every field of the manifest entry from the item's own page. Put the Openverse `id` and the search file in `notes`, so that the rights auditor can follow the trail.
- Audio files only. Never unpack or run anything downloaded.

## Then measure

A search result is a claim. Run step 2's measurements on every file as for any other source: `analyze.py`, the realness reading and the hark auditions. Generated and synthesized audio is uploaded to open libraries too, and its page seldom says so.

## Credits

`credits.py` writes the credits from the manifests, as always. The `attribution` text of the index is a convenience for checking; the credit comes from the fields that you confirmed on the item's page. A CC BY credit holds the title, the author, the source link, the licence link and a note that the recording was modified.

## What it is not for

- Voice-over, and sounds built in code. Those belong to the `video-audio` and `hark` skills.
- Music that carries a film. The index finds candidates, and the music role still needs a track with a clear structure, stems or clean edit points, and terms confirmed on the composer's own page.
