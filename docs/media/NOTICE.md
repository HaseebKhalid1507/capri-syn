# docs/media: notice

**Not covered by capri-syn's MIT License. No commercial use.**

Every image and clip in this folder (`banner.png`, `card-*.png`, `hero.webp`, `tour-*.webp`) is a
screenshot or recording of capri-syn running, and every one shows third-party character art. capri-syn
grants no licence of any kind for them. The art in them belongs to its owners, as set out below.
Rights holders: open an issue and it comes down.

They were shot with a pack built by `tools/make-pack` from the two sources below, and nothing else.
The art files themselves are not in this repository: `make-pack` assembles them on your machine.

## Capri's sprites: CapriCore

Her sprite art (the figure on the red starfield) comes from
[CapriCore](https://github.com/CapriChooms/CapriCore). No licence is granted for it. CapriCore's licence
covers its code, not its character art: its `docs/character-assets.md` says the GPL "does not grant
ownership of … supplied artwork", and that anyone redistributing its character assets "must establish
the necessary permissions separately".

## Capri's desktop art: lachydotmcg/capri-desktop

Her perch, sleep, walk, dance, dangle and transition animations come from Capri,
<https://github.com/lachydotmcg/capri-desktop> (commit `156ed18`), under the PolyForm Noncommercial
License 1.0.0: <https://polyformproject.org/licenses/noncommercial/1.0.0>. Those terms apply to the parts
of these files that show it: noncommercial use only. The banner uses its perch drawing; the cards are
composed from the same captures and one frame of its teleport animation.

Its Required Notices, verbatim from its `LICENSE.md`:

Required Notice: Copyright 2026 Lachy (https://github.com/lachydotmcg)

Required Notice: Anyone who uses Capri's code, art, animations or any of its other assets must credit the Capri project (https://github.com/lachydotmcg/capri-desktop). Capri's art and animations are by her artist, who prefers to stay private. Parts made by others keep their own licences, listed in its README.

Credits, from its README: window perch and music perch animations: her artist, who prefers to stay
private. Click effect: mixmix312352. The transformation animation she forms from when she teleports, and
her two secret emotes: Qewiw (mix_qewiw1412_86144). The teleport frame on the cards is Qewiw's; that
README credits it without naming a separate licence.

## The character

capri-syn is an unofficial, non-commercial fan project. It is not affiliated with or endorsed by
CD PROJEKT RED, R. Talsorian Games, Studio Trigger or Netflix. Cyberpunk is a trademark of
CD PROJEKT RED. All trademarks belong to their owners.

## Not media: a note on `packs/capri/moods.json`

JSON can't hold a comment, so this file carries its note. `moods.json` is capri-syn's own behaviour file,
under the MIT License. Its key names follow CapriCore's schema, and a few of its numbers are facts taken
from CapriCore, so her pack lines up with CapriCore's art:

- the intro frame timings in `arrive` (208, 292, 417, 750 and 583 ms) are those of CapriCore's
  `assets/capri/intro-v1/runtime-intro.json`;
- the note emitters for the two music poses (`music.emitters`: `music-window`, `music-floor`) and those
  poses' anchors in `extraFrames` (seat 628,771; ground 641,1164) are CapriCore's `MUSIC_POSES`
  (its `music-animation.mjs`).
