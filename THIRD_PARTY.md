# Third-party assets

Folio is MIT OR Apache-2.0. The following assets are included under their own licenses.

## Fonts

- **Selawik** — Copyright 2015 Microsoft Corporation, with Reserved Font Name "Selawik". SIL Open Font License 1.1.
  Bundled unmodified. See `licenses/fonts/Selawik-OFL.txt`.
- **Carlito** — Copyright 2013 The Carlito Project Authors, with Reserved Font Name "Carlito". SIL Open Font License 1.1.
  Bundled unmodified. See `licenses/fonts/Carlito-OFL.txt`.
- **Source Sans 3** (Regular, interface fallback) — Copyright 2020 Adobe. SIL Open Font License 1.1.
  See `licenses/fonts/SourceSans3-OFL.txt`.
- **Source Serif 4** (note font choice and equations) — Copyright 2020 Adobe. SIL Open Font License 1.1.
  See `licenses/fonts/SourceSerif4-OFL.txt`.
- **JetBrains Mono** — Copyright 2020 The JetBrains Mono Project Authors. SIL Open Font License 1.1.
  See `licenses/fonts/JetBrainsMono-OFL.txt`.
- **Atkinson Hyperlegible** — Copyright 2020 Braille Institute of America, Inc. SIL Open Font License 1.1.
  See `licenses/fonts/AtkinsonHyperlegible-OFL.txt`.
- **Patrick Hand** — Copyright 2010-2012 Patrick Wagesreiter. SIL Open Font License 1.1.
  See `licenses/fonts/PatrickHand-OFL.txt`.

The fonts are used by the interface and are not sold separately.

## Icons

- **Fluent UI System Icons** — Copyright 2020 Microsoft Corporation. MIT License.
  See `licenses/fonts/FluentSystemIcons-MIT.txt`.

The icons are drawn from `assets/fonts/fluent/FluentIcons-{Regular,Filled}.ttf`, subsets of the
fonts in release 1.1.344 made by `tools/fluent_icons.py` from the names in `assets/fonts/fluent/icons.txt`.

## Spelling dictionaries

- **English (US)** — from SCOWL by Kevin Atkinson and contributors, via Debian `hunspell-en-us`.
  Permissive license; see `licenses/dictionaries/en_US-SCOWL.txt`.
- **French** — "Dictionnaires orthographiques français" v7.7 by Dicollecte (Olivier R. and contributors),
  https://grammalecte.net. Mozilla Public License 2.0; see `licenses/dictionaries/fr-MPL-2.0.txt` and
  `licenses/dictionaries/fr-README.txt`. The unmodified dictionary files are in `assets/dictionaries/`.

The dictionaries are read by the `spellbook` crate (MPL-2.0).

## PDF rendering

File printouts are rendered by [hayro](https://github.com/LaurenzV/hayro) (Apache-2.0 OR MIT). It embeds:

- the PDF standard-14 fonts from PDFium (Foxit fonts), Copyright 2014 PDFium Authors, BSD 3-Clause License, see `licenses/pdf/PDFium-Foxit-BSD-3-Clause.txt`;
- Adobe's predefined CMaps, BSD 3-Clause License, see `licenses/pdf/Adobe-CMaps-BSD-3-Clause.txt`;
- the CGATS001Compat ICC profile, CC0 1.0.

## Libraries

Rust dependencies and their licenses are listed by `cargo metadata` / `cargo deny`.

Audio recording and playback use [cpal](https://github.com/RustAudio/cpal) (Apache-2.0).

Importing notebook files uses [onenote_parser](https://github.com/msiemens/onenote.rs)
(Mozilla Public License 2.0, used unmodified; the license text is in `licenses/MPL-2.0.txt`
and its source is at that address), [cab](https://github.com/mdsteele/rust-cab) (MIT) and
[encoding_rs](https://github.com/hsivonen/encoding_rs) ((Apache-2.0 OR MIT) AND BSD-3-Clause).

## Test files

`tests/fixtures/notebooks/` holds sample notebook files for the import tests only; they are not
part of the app. `mit/` comes from Microsoft's
[Interop-TestSuites](https://github.com/OfficeDev/Interop-TestSuites) (MIT, `mit/LICENSE`);
`mpl/` comes from [onenote.rs](https://github.com/msiemens/onenote.rs) (Mozilla Public
License 2.0, `mpl/LICENSE`). See `tests/fixtures/notebooks/README.md`.
