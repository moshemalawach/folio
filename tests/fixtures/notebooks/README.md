# Notebook test files

Sample notebook files for `src/import_notebook.rs` tests. They are read, never changed, and are not
part of the app.

| Path | What it is | Source | Licence |
|---|---|---|---|
| `mit/AlternativePackaging.one` | A section in the FSSHTTP format: one page | Microsoft Interop-TestSuites @ fe87ed3253de, `FileSyncandWOPI/Source/MS-ONESTORE/TestSuite/Resources/` | MIT (`mit/LICENSE`) |
| `mit/Encryption.one` | A password-protected section (desktop format) | same | MIT |
| `mit/OneWithFileData.one` | A section with an attached `testing.docx` (desktop format) | same | MIT |
| `mit/OneWithoutFileData.one` | A section with one text page (desktop format) | same | MIT |
| `mit/interop-desktop-stored.onepkg` | A notebook package (stored CAB) of the MS-ONESTORE `Open Notebook.onetoc2` and the two `OneWith*` sections, made with the `cab` crate | built from the files above | MIT |
| `mpl/handwriting_recognition.one` | A rich desktop section: headings, lists to three levels, to-do and Important tags, a link, two tables, a picture, an equation, 62 ink strokes | msiemens/onenote.rs @ de3a64e8159c, `crates/parser/tests/samples/` | MPL-2.0 (`mpl/LICENSE`) |
| `mpl/notebook/` | An FSSHTTP-format notebook: `Open Notebook.onetoc2`, `New Section 1.one` and the section group `New Section Group/` (two sections, one with a PDF printout page and an MP3 file) | same | MPL-2.0 |

The MPL-2.0 files are unmodified; their source is the onenote.rs repository above.
