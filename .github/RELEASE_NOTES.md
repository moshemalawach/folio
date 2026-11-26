# Folio 0.2.0

Folio is an early-alpha, local-first notebook for Linux, Windows and macOS. It is a native Rust application: no Electron, no webview, and no account. Your notes live in a SQLite file and an attachments folder that you can copy.

## Downloads

- **Linux x86_64** — `folio-linux-x86_64.tar.gz`
- **Windows x86_64** — `folio-windows-x86_64.tar.gz`
- **macOS arm64** — `folio-macos-aarch64.tar.gz`

The Linux build is built on Ubuntu 22.04, with the glibc 2.35 baseline, rather than the rolling 2.43 glibc of the development machine. It requires **glibc 2.35 or newer**. Verify the archive with `sha256sum -c SHA256SUMS`.

## Run

On Linux:

```sh
tar -xzf folio-linux-x86_64.tar.gz
./folio
```

On Windows, extract the archive and run `folio.exe`. On macOS, extract the archive and run `./folio`; the binary is unsigned, so macOS will ask for confirmation.

## What is in it

- Free-form pages, notebooks, sections and subpages
- Note containers, outlines, tags, tables, images and PDF printouts
- Pens, highlighters, shapes and Ink to Shape
- English and French interfaces
- Local backup, autosave and web export
- `.onepkg` packages, notebook folders and `.one` sections

The Linux package includes the desktop entry, icon and the open-source licence. The Windows and macOS archives are portable folders; no installer is provided.

## If something goes wrong

The user-facing documentation, with the keyboard shortcuts and the data location, is in the archive (`README.md`) and on the [website](https://moshemalawach.github.io/folio/).

The project is MIT OR Apache-2.0. The third-party notices and the open font and icon licences are included in the archive.
