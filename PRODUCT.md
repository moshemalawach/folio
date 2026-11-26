# Folio

## What it is

Folio is a local-first notebook: native, free of any account, and built around free-form pages. It has notebooks, section groups, sections, pages and subpages, and its pages let you click anywhere and type into a note container. A note holds outlines, tags, tables, pictures, equations and files, and the page around it takes ink, shapes and PDF printouts. Everything stays on the computer, and the interface is in English or French.

The first audience is a school pupil working with an occupational therapist. The pupil types class notes into outlines and to-do lists, writes on a worksheet printed into the page, follows a timeline of the term, and has a page read back in the Immersive Reader. The product is for one person at a desk: writing quickly, coming back to the same pages often, and searching everything without thinking about where it lives.

## Principles

1. **Local by default.** Notes are a SQLite file and a folder of attachments on the device. There is no account, sync service, telemetry or network connection to trust.
2. **Type first, organise later.** A new page puts the caret in its title, and a click anywhere on the page starts a note. Outline levels, tags, sections and subpages can follow.
3. **Familiar first.** Commands sit where someone coming from a desktop notebook expects them, under familiar names and with familiar shortcuts, doing what they do there. Folio's own additions, such as the frise, backup and web export, keep out of the way.
4. **Keyboard and pointer are peers.** Familiar shortcuts work as they do elsewhere. Search, creation, formatting, outlines and navigation all have keyboard paths, and Help › Keyboard shortcuts lists them.

## Scope today

- Notebooks, section groups, sections, pages and two levels of subpages, in a navigation pane and page list or in Horizontal Tabs, with the 16 section colours, Move or Copy, and a Recycle Bin.
- One free-form page type: the title, date and time on the page; note containers with outline levels, styles, bullets, numbering and tags in the margin; rule lines and page colours.
- The Home, Insert, Draw, History, Review, View and Help tabs, in a Classic or Simplified ribbon, with contextual Table, Picture, Equation and Audio & Video tabs.
- Tables, pictures, attached files, PDF file printouts, links, audio recordings, equations, symbols, date and time stamps, page templates and Folio's frise (a timeline).
- Ink: pens, highlighters, erasers, the lasso, shapes and Ink to Shape.
- Page versions, Recent Edits, spelling in English and French, search with scopes, Find Tags and find on page.
- Importing notebooks (`.onepkg`, notebook folders, `.one` sections), with a report of what was left out.
- Print through the web browser, web page export and a local backup.
- The Immersive Reader with line focus and read-aloud; light, dark or system theme; Full Page View.
- English and French interfaces, with sample notebooks written in each.
- Linux, Windows and macOS from one Rust codebase.

## Not in scope

- **Sync and collaboration:** no account, cloud, sharing, co-authoring, authors or unread changes.
- **Ink recognition:** no handwriting or maths turned into text, and no text recognition (OCR) in pictures or printouts.
- **Dictation:** no dictation, transcription or other speech to text. Read-aloud uses the system's own voice.
- **Online services:** no AI assistant, web research, translation, online pictures or video, stickers or live components.
- **External integrations:** no task sync, e-mailed pages, meeting details, embedded spreadsheets or diagrams, or linked notes.
- **Third-party branding:** no third-party logos or product names in the interface. Folio keeps its own name and mark.

These are deliberate, and adding anything must not make the local notebook slower or noisier. Other gaps, such as section passwords, more built-in tags, more windows and signed installers, are simply not built yet.

## Reference

The reference is a modern desktop notebook: its look and behaviour are the baseline, and [DESIGN.md](DESIGN.md) holds the visual values.

The engineering bar stays as before:

- native Rust and egui, with no Electron, Tauri or webview;
- one binary per platform, with its fonts, icons, dictionaries and translations built in;
- notes in a SQLite database the user can open and back up;
- real desktop conventions on each platform.
