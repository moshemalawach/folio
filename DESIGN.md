# Folio design

How Folio looks, and where each value lives in the code. Sizes are logical pixels (egui points) at 100 % zoom; 1 pt is 4/3 px at 96 dpi, so 11 pt is 14.67 px. The code is the source of truth: `src/theme.rs` (colours, fonts, widget skin), `src/widgets.rs` (shared controls), `src/ribbon.rs` (the ribbon), `src/app.rs` (the window's regions, menus and dialogs), `src/canvas.rs` and `src/editor.rs` (the page and the notes), `src/model.rs` (page geometry and styles).

## Direction

**Folio uses a modern desktop-notebook look.** It takes the familiar light grey window with white rounded cards, a purple accent, a full ribbon, a navigation pane and page list, a page with the title and date on it, note containers that show a frame only on hover, and desktop proportions. When a choice is open, familiar desktop conventions decide it.

The work is independent. Folio holds no third-party application code or logos, and puts no other product name in its interface. It uses open-licensed fonts and icons: **Selawik** (OFL), **Carlito**, which has Calibri's metrics (OFL), and the **Fluent UI System Icons** (MIT). Folio keeps its own name and its own mark.

## Colour

The colours are the fields of `theme::Tokens`, built by `Tokens::light()` and `Tokens::dark()`. Folio follows the system's appearance, or File › Options › General › Theme (System, Light or Dark). `theme::style(t)` turns the tokens into egui's widget skin.

| Token | Light | Dark | Role |
| --- | --- | --- | --- |
| `bg` | `#F3F3F3` | `#1F1F1F` | The window: header strip, tab row, the margins round the cards |
| `raised` | `#FFFFFF` | `#292929` | Cards (ribbon, page list, page, task pane), menus, dialogs, tooltips, buttons and boxes, a page with no colour |
| `sunken` | `#F5F5F5` | `#333333` | Hovered and open buttons and menu rows, a pressed icon button, a table's header row, code |
| `panel` | `#F3F3F3` | `#1F1F1F` | The navigation pane, which sits on the window rather than on a card |
| `ink` | `#242424` | `#F5F5F5` | Text and glyphs, the caret, the toast's background |
| `ink_strong` | `#000000` | `#FFFFFF` | Text on a pressed control |
| `muted` | `#616161` | `#ADADAD` | Unselected tabs, group labels, dialog explanations, shortcuts, idle glyphs, the Quote and Citation styles |
| `faint` | `#9E9E9E` | `#858585` | The page's date and time, greyed controls, placeholder text |
| `line` | `#E6E6E6` | `#3D3D3D` | Hairlines: the title rule, group separators, dividers in panes and Backstage |
| `line_strong` | `#D1D1D1` | `#575757` | Borders of buttons, boxes, menus, dialogs and swatches; Section Color › None |
| `accent` | `#7719AA` | `#C08AE6` | The selected tab's underline, contextual tabs, "+ New section", Add page, primary buttons, focus rings, links |
| `accent_soft` | `#F1E7F8` | `#3D2A4D` | The fill of a pressed widget |
| `danger` | `#C42B1C` | `#FF99A4` | Destructive buttons, errors, Home › Delete's glyph, an equation that does not parse |
| `warning` | `#F7630C` | `#FAA06B` | Warnings; the frames of the cells a new table takes in Insert › Table's grid (orange) |
| `selection` | `#CCE4F7` | `#264F78` | Selected text, and selected table cells (at alpha 170) |
| `hover` | black at alpha `0x0F` | white at alpha `0x14` | The translucent fill under the pointer on tabs, rows and icon buttons, so one value works on the grey window and on white cards. It is a fill only, never a text or stroke colour. |
| `text2` | `#424242` | `#D6D6D6` | The page title, section names, page rows |
| `nav_selected` | `#E7E7E7` | `#383838` | The selected section or section group in the navigation pane; a checked ribbon toggle or toggle chip |
| `page_selected` | `#EBEBEB` | `#3D3D3D` | The selected page row |
| `grab_bar` | `#DEDEDE` | `#474747` | A note container's grab bar at rest |
| `container_border` | `#C8C8C8` | `#5C5C5C` | A note's frame, its hovered grab bar, table lines, a hovered button's border, the paragraph handle's edge, a checked toggle's frame |
| `card_shadow` | black at alpha 26 | black at alpha 90 | The shadow under the cards and the toast |

**Section and notebook colours** are data, not tokens. `theme::SECTION_COLORS` holds the sixteen, in the Section Color menu's order: Blue `#0166AC`, Yellow `#FABE14`, Green `#00B24A`, Red `#D13438`, Purple `#8764B8`, Cyan `#00B7C3`, Orange `#F2610C`, Magenta `#BE0151`, Blue Mist `#6E9BD1`, Purple Mist `#9A8BC9`, Tan `#C39F74`, Lemon Lime `#B5CC18`, Apple `#6BB700`, Teal `#038387`, Red Chalk `#D47E7E`, Silver `#A0A0A0`. None (`theme::NO_COLOR`) draws in `line_strong`. Horizontal Tabs mixes a section's colour toward the card's colour with `theme::pastel`, by measured amounts: 0.6 for every tab and the band over the page (`TAB_TINT`; the selected tab is not tinted apart), 0.5 for a hovered tab (`TAB_HOVER_TINT`), 0.45 for the page list's header (`LIST_HEADER_TINT`) and 0.86 for the page list itself (`LIST_TINT`).

**Other fixed colours:**

- **Rule lines** (`canvas::rule_colors`): blue `#A9C8E8` with a red margin line `#E8A0A0`; in dark mode `#3B5572` and `#7A3B3B`. A page's Rule Line Color replaces the blue in both themes.
- **Page colours** (`model::PageColor`): sixteen pale fills, each with a deep twin for dark mode. More Colors… takes any colour, and dark mode keeps its hue at a fifth of the brightness.
- **Heading inks:** see Typography.
- **Find on page** (`theme::find_highlight`): `#FFE87A` behind a match and `#FFB347` behind the current one; translucent ambers in dark mode.
- **Tag glyphs** (`LineTag::color`): each tag has its own colour.
- **Backstage's column** (`theme::backstage_fill`): a deep purple `#7719AA`, and a deeper `#5B2A86` in dark mode, both with white text.
- **Text on a fill** (`theme::on_fill`): primary and danger buttons and the To Do box's tick take white or near-black (`#1B1B1B`), whichever contrasts more with the fill by WCAG's measure (`theme::contrast`). That is white on the light theme's purple and red, and near-black on the dark theme's light purple and pink.

Colour is never the only sign: the selected tab is also semibold, the selected page row is also semibold, the To Do box has a tick, and lists have markers.

## Typography

Every font is bundled and loaded by `theme::install_fonts`; nothing is downloaded at run time. The licences are in [THIRD_PARTY.md](THIRD_PARTY.md).

**Interface: Selawik.** The proportional family is Selawik Regular. The other weights are named families, reached through helpers in `theme.rs`:

| Weight | Family, helper | Where |
| --- | --- | --- |
| Regular | `ui` (Proportional), `theme::ui` | Every label: tabs (13 px), ribbon labels (12 px), group labels (11 px), menu rows (12.5 px), shortcuts (11.5 px), pane rows and the header's page name (13 px), the page's date and time (13 px). egui's text styles: Body 13, Button 12.5, Small 11. |
| Light | `ui-light`, `theme::light` | The page title (20 pt, `theme::TITLE_SIZE` = 26.7 px), the empty page's title (26 px), the Immersive Reader's title (30 px) |
| Semilight | `ui-semilight`, `theme::semilight` | Empty-state titles (20 px) |
| Semibold | `ui-semibold`, `theme::semibold` | The selected tab, notebook names, the selected page row, captions over groups of settings (12 px), button labels (12 px), dialog titles (20 px), egui's Heading style (20 px) |
| Bold | `ui-bold` | Bold text in a note set in the Selawik face |

Labels are in sentence case in French and in Title Case in English ribbons and menus ("Insert Space", "Page Width"). No interface label is set in capitals or in a monospaced face.

**Notes: Carlito, 11 pt.** Note text defaults to Carlito (`FontFace::Body`), which has Calibri's metrics: 11 pt (14.67 px, `editor::BODY_SIZE`) on 18 px lines (`editor::BODY_LINE`), with a true bold, italic and bold italic. The font box also offers Selawik, a serif, Atkinson Hyperlegible (for readers who need it), Patrick Hand and a monospaced face. The paragraph styles follow the reference design (`editor::kind_defaults`, `kind_ink`, `kind_bold`, `kind_italic`):

| Style | Size | Weight and slant | Ink (light / dark) |
| --- | --- | --- | --- |
| Page Title | 20 pt | regular | ink |
| Heading 1 | 16 pt | regular | `#1E4E79` / `#8AB4E8` |
| Heading 2 | 14 pt | regular | `#2E75B5` / `#6CB4F0` |
| Heading 3 | 12 pt | bold | `#1F3763` / `#A9C3E8` |
| Heading 4 | 12 pt | bold italic | `#2F5496` / `#9AB3E3` |
| Heading 5 | 11 pt | bold | `#2E75B5` / `#6CB4F0` |
| Heading 6 | 11 pt | bold italic | `#2E75B5` / `#6CB4F0` |
| Normal | 11 pt | regular | ink |
| Citation | 9 pt | regular | muted |
| Quote | 11 pt | italic | muted |
| Code | 11 pt | regular, monospaced | ink |

A style's line height is its size × 1.22, rounded; the 11 pt styles take Normal's 18 px. Paragraphs have no space between them, as on a page.

**The page title** is Selawik Light at 20 pt in `text2`, a light face for the title. The note-font setting does not change it.

**Monospace is for code only.** JetBrains Mono sets the Code style, the box where an equation is typed in linear form or LaTeX, and the hex field of More Colors…. **Source Serif 4** is bundled for the serif choice in the font box and for drawing equations.

**Fallbacks.** Selawik lacks the no-break spaces French needs (U+00A0, U+202F), and º, ª, ◦ and ▪. The proportional family is therefore Selawik, then Source Sans 3 Regular (`ui-fallback`, kept for those glyphs only), then the Fluent icons, then egui's built-in fallbacks, with egui's Hack last (the only face that draws ↵, the macOS Return key). Every named family (`body`, `ui-light`, `serif` and the others) puts its own face first and then the same chain, so an icon or a French space never shows as a box. The monospaced family is the code face, the icons, then egui's monospaced fallbacks. The test `the_interface_and_note_fonts_draw_every_character_messages_use` checks the chain against every character the messages use.

**Icons: Fluent UI System Icons.** Icons are glyphs of a font, drawn as text in the `icons` family (`theme::icon(size)`) from the 20 px Regular style. They take the text colour, except where a colour means something: tags, pens, Delete. The Filled style (`icons-filled`, `theme::icon_filled`) is used where an icon is filled with a colour: the notebook icon in the notebook's colour, and the pen and highlighter bodies where a pen is listed in a menu. Draw's band paints its pens as pictures instead (see The ribbon framework). The fonts in `assets/fonts/fluent/` are subsets of release 1.1.344. To add an icon:

1. Add its Fluent name to `assets/fonts/fluent/icons.txt`: the part between `ic_fluent_` and `_<size>_<style>`, for example `table_add`.
2. Rebuild the subsets and `codepoints.txt` with `python3 -I tools/fluent_icons.py <dir>`, where `<dir>` holds `FluentSystemIcons-{Regular,Filled}.{ttf,json}` from the release's `fonts/` folder. The script needs fontTools.
3. Declare a constant in `src/icons.rs`, inside the `icons!` macro, with its 20 px Regular code point from `codepoints.txt`. A Filled glyph is a plain `pub const` outside the macro and joins `every_filled_icon_is_its_fluent_glyph_in_the_bundled_font`.
4. Run `cargo test --bin folio icons`. The tests check that every constant is the right glyph and is in the subset.

## Layout

The window is a stack of egui panels, built in `FolioApp::draw`. From top to bottom and left to right:

- **Header strip:** 48 px on the window grey.
  - At the left, 12 px in: Folio's mark, its own app icon (`assets/icon.png`) drawn at 20 px (`paint_mark`), never a letter on a coloured tile. Then the open page's title in Selawik 13, cut with "…" before the search box ("Folio" when no page is open).
  - In the centre: the search box, 32 px high and 470 px wide (`HEADER_SEARCH_WIDTH`), or a third of the window when that is less. It is white with a 6 px radius and a `line_strong` border that turns `accent` while typing. It holds a magnifier, "Search", the `Ctrl+E` hint and, while open, the scope chip. In Horizontal Tabs it moves to the end of the tab row.
  - At the right, 8 px in: the 32 px gear, which opens Options.
- **Tab row:** 30 px (`ribbon::TAB_ROW`) on the window grey.
  - The tabs start 12 px in, 4 px apart, with 10 px of padding each side, in Selawik 13: `muted` text, the selected one semibold `ink` over a 3 px `accent` underline as wide as its text.
  - Contextual tabs (Table, Picture, Equation, Audio & Video) follow 12 px after Help, in `accent`.
  - Each tab's slot is measured in semibold, so nothing moves when the selection changes.
- **Ribbon card:** white, 8 px from the window's sides and from whatever is below it, with 4 px of padding inside.
  - **Classic** (`CLASSIC_HEIGHT`): the card is 96 px, holding 66 px of controls over an 18 px row of group labels.
  - **Simplified** (`SIMPLIFIED_HEIGHT`): the card is 44 px, one 28 px row of icons.
  - **Panel height** (`ribbon::panel_height`): 134 px with the Classic band, 82 px with the Simplified band, 30 px with the tabs only.
- **Navigation pane** (Vertical Tabs, the default): 195 px (`NAV_WIDTH`), resizable from 150 to 360 px, on the window grey with no card.
  - At its top, a 36 px row with ≡ (Hide navigation).
  - Each notebook is a 32 px row: a fold chevron, the Filled notebook icon in the notebook's colour (16 px), and its name in semibold 13.
  - Under it come its section rows (28 px), each with a 6 × 16 px pill in the section's colour (3 px radius) and the name in `text2`. Section groups have a folder icon and nest.
  - The selected row is a rounded `nav_selected` fill.
  - "+ New section" is a 28 px row in `accent`, and a 1 px divider separates notebooks.
  - "Quick notes" is pinned at the bottom: a 36 px row under a divider.
- **Page list:** a white card, 180 px (`PAGE_LIST_WIDTH`), resizable from 140 to 400 px.
  - Its 36 px header holds "Add page", outlined in `line_strong` with its glyph and label in `accent`, and a 28 px sort button.
  - Page rows are 30 px: titles only, cut with "…". Unselected rows are Selawik 13 in `text2`; the selected row is semibold `ink` on `page_selected`. Subpages are indented 16 px a level.
- **Page card:** a white card filling the rest, 8 px from the window's edges and flush against the pane beside it.
  - The page fills it edge to edge (see The page).
  - The ↗↙ Full Page View button sits 36 px from its right and 8 px from its top: 28 px, with a 14 px glyph.
- **Task pane:** a white card on the right, 300 px (`TASKPANE_WIDTH`), resizable from 240 to 480 px.
  - It holds Find Tags, Page Versions, Spelling, and Search Results or Recent Edits.
  - When the page card would drop under 420 px (`MIN_PAGE_WIDTH`), the navigation pane folds away, then the page list. A 40 px strip with ≡ at the page card's left brings them back.

Every card (`widgets::card`) has an 8 px radius and a soft shadow (offset 1 px down, 6 px blur, `card_shadow`). There is no status bar. The window opens at 1320 × 860 and is never narrower than 980 × 640.

**Horizontal Tabs** (View › Tabs Layout) replaces the navigation pane with a 40 px row of folder tabs over the cards:

- The tabs are rounded 6 px at the top, at most 200 px wide and 4 px apart; the parent, "+" and "⌄" tabs are 30 px.
- Every tab is the same pastel of its section's colour (see Colour). The selected tab is taller, its name semibold, and joins a 6 px band of that pastel across the page card's top.
- The search box sits at the right end of the tab row, above the page list: 220 × 28 px (`TAB_SEARCH_WIDTH`), with the tabs' room ending 8 px before it. The header strip then shows none.
- The page list moves to the page's right (Options can move it back), tinted with the section's colour under a header of the stronger tint, and the selected page is a white row with a `line_strong` border.
- The notebook picker drops the notebook list and can pin it as a pane.

**Full Page View** (F11, or the ↗↙ button) hides the navigation pane, the page list, the task pane and the ribbon band, leaving the header strip, the tab row and the page card. A click on a tab opens its band over the page until a command runs, `Esc` is pressed or the click lands elsewhere. Show tabs only (`Ctrl+F1`) works the same way, with the panes left in place.

**Backstage** (File) covers everything below the header strip:

- Its left column is 220 px of deep purple (`theme::backstage_fill`), with white text and glyphs: ← Back, Info, New, Open, Print and Export, a white rule (alpha 70), then Options and Close. The page shown is a darker row (black at alpha 56), and the row under the pointer a lighter one (white at alpha 28).
- The page picked there sits on a white card with 32 × 24 px margins, its content at most 640 px wide.

## The ribbon framework

The ribbon is data, rebuilt every frame for the open tab, so its labels follow the interface language and it fits the window as measured in that language.

- **Tabs → groups → controls.** `ribbon::groups(tab, host)` returns the tab's `Group`s, left to right, from one builder per tab (`home_groups`, `insert_groups`, `draw_groups`, `history_groups`, `review_groups`, `view_groups`, `help_groups`, and `table_groups`, `picture_groups`, `equation_groups`, `recording_groups` for the contextual tabs). File has no band: it opens Backstage. A `Group` has an id, a label, a glyph for its folded button, a priority and a layout. `Flow` stacks Small controls three to a column beside the Large ones; `Rows` uses fixed rows, like Basic Text's two.
- **Control kinds** (`ControlKind`):
  - `Button` runs a `Cmd`. One with no command is a read-only readout, like the recording timer.
  - `Toggle` shows a checked state as a familiar desktop toggle: a `nav_selected` fill with a 1 px `container_border` frame. The current rule in View › Rule Lines has a 2 px `muted` frame.
  - `Split` has a main part that runs a command and an arrow that opens a menu.
  - `Menu` only opens a menu.
  - `Combo` is a white box showing a value, like the font and size boxes.
  - `Gallery` holds items in columns of three, at most three columns in the band, with a ▾ column that opens the full list. Its items are labelled rows, styled previews, 20 px colour swatches, 24 × 22 px icons or pen pictures. Once its group has shrunk (level 1), a gallery of labelled rows (Styles, Tags) shows one column, as a scrolling gallery does; pens and shapes keep theirs.
  - **Styles** shows each style as a preview of its name (`editor::style_preview`), in the band and in its ▾ menu: the style's own font, weight, slant and colour, at 11 to 15 px (`PREVIEW_MAX`) so every row stays a row.
  - **Pens** (Draw) are pictures in one row of 34 × 56 px cells (`PEN_CELL`), at most eight (`PENS_SHOWN`, the pen in hand always among them), painted with vectors (`paint_pen_picture`). A pen is a 14 px barrel in its colour over a dark cone and a 5 px nib in its colour; a highlighter is a 20 px barrel, a dark collar and a chisel tip. In dark mode they take a 1 px `line_strong` edge, so the black pen shows. The pen in hand is boxed as a checked toggle with a chevron at its bottom right, and a click on it or on the chevron opens its menu (Pen settings). There is no ▾ column.
- **Sizes** (`Size`):
  - Large: a 24 px glyph over a label of one or two lines (Selawik 12; a label over 64 px breaks at the space nearest its middle), at least 44 px wide and 66 px high. A button with a menu draws a chevron: on a line of its own under a one-line label (Paste, Table), or 12 px after the second line of a two-line one (`LARGE_CHEVRON`: Page Templates). A menu or split button, and a folded group's button, is drawn pressed (the checked look) while its menu is open.
  - Small: 22 px high, with a 16 px glyph and the label beside it.
  - Icon: 24 × 22 px.
  - `Control.glyph_size` is 16 px, except Undo and Redo at 20 px, the reference size.
  - Classic groups have 6 px of padding and are separated by a 52 px line in `line`. Their labels are Selawik 11 in `muted`.
- **Enabled and checked come from the host.** The ribbon holds no state of the document. It asks the app through `RibbonHost` (`enabled`, `checked`, `menu_enabled`, `combo_text`, `contextual`, `menu`, `recording`, `audio_selected`, `pens`), which `ShellHost` in `app.rs` answers each frame from the page, the canvas and the caret. A command that cannot run is greyed (`faint`), never hidden.
- **Measured scaling.** Each group has four levels:
  - level 0: as designed;
  - level 1: Large controls made Small, and labelled galleries one column (only for a group that has Large controls);
  - level 2: everything an icon, except in a group whose gallery holds named items (Styles, Tags): it goes from level 1 straight to level 3, as the reference design folds it;
  - level 3: the whole group one Large button whose menu lists its commands.

  `ribbon::fit_levels` steps the group with the **lowest priority** down first, the rightmost among equals, and only ever to a narrower level, until the band fits. It then hands any room a big step freed back to the groups shrunk earlier, most important first. On Home the priorities are Undo 9, Basic Text 8, Clipboard 7, Styles 6 and Tags 5, so Tags shrinks first and Undo last. A group made of one gallery (Styles) drops its items straight into its folded menu. The Simplified band does not shrink groups: whole groups move into the "⋯" menu at the row's end. Its controls are icons, except Styles, which keeps its label (`keeps_label`), as the reference design's does.
- **Display options.** A 20 px chevron at the card's bottom-right corner opens Classic Ribbon / Simplified Ribbon and Always Show Ribbon / Show Tabs Only. Both choices are kept for the next start (`ribbon_layout`, `ribbon_collapsed`).
- **The caret stays in the note.** A click on the ribbon that runs nothing (a tab, empty band, a greyed control) sets `RibbonState.idle_click`, and the app gives the keyboard back to the note being typed in.
- **Adding a command:**
  1. Add a `Cmd` variant in `src/ribbon.rs`, and a `MenuId` if it opens a menu.
  2. Add its label and tooltip (`ribbon-…`, `ribbon-…-tip`) to `assets/locales/en/ribbon.ftl` and `assets/locales/fr/ribbon.ftl`: Title Case in English, sentence case in French, the familiar French term where there is one.
  3. Place the control in the tab's builder with the private helpers `button`, `toggle`, `split`, `menu`, `combo`, `gallery`, `item` and `tip`, give it a glyph from `src/icons.rs`, and pick its group and priority.
  4. Answer `enabled` and `checked` for it in `ShellHost`, and run it in `FolioApp::run_cmd`, or in `canvas::run_ribbon_cmd` for a page command. A command that edits text goes through `Cmd::Text` and `editor::apply_command`; any other command must leave the keyboard where it was.
  5. Extend `clicking_the_ribbon_keeps_typing_in_the_note` and check `the_ribbon_fits_at_every_window_width` in English and French.

## The page

The page is one free-form canvas with no edges: it reaches one view past its furthest content, down and to the right, and has nothing above or left of (0, 0). Its geometry is in `model.rs` and `canvas.rs`, in page units at 100 %:

| Part | Value |
| --- | --- |
| Title | starts at x = 48 (`PAGE_LEFT`), y = 40 (`TITLE_TOP`), in a 36 px box |
| Title rule | 1 px of `line` at y = 86, at least 400 px wide (`TITLE_RULE_MIN`), wider under a long title |
| Date and time | at y = 94, Selawik 13 in `faint`: the long date, then the time 28 px after it (`TIME_GAP`). Each opens a picker. |
| Notes | start at y = 136 (`PAGE_CONTENT_TOP`). The first note's text, 10 px inside its frame (`NOTE_PAD_X`), lines up under the title's first letter: an untagged note's frame is at x = 38, and a tagged one's reaches further left by its tags' hang (see Note containers). |
| Highest item | y = 16 (`ITEM_TOP`), so every grab bar stays on the page |

View › Hide Page Title hides the title, the rule, the date and the time.

**Rule lines** (View › Rule Lines, `model::RuleLines`):

- Lines: Narrow 21 px, College 27, Standard 33, Wide 40. Grids: Small 12 px, Medium 24, Large 36, Very Large 48.
- The lines start under the date (y = 120), or at the top when the title is hidden. They fall where typed text sits: the first is at y = 155 (`FIRST_RULE`), under the first baseline of the default note, and every other is a whole number of spacings from it.
- Ruled pages get the red margin line at x = 28 (`RULE_MARGIN`).
- When zoomed so far out that lines would be under 6 px apart on screen, every other line is dropped.

**Page colour** (View › Page Color) fills the whole page: one of sixteen pale colours in a 4 × 4 menu of 40 × 34 px swatches, No Color, or More Colors…. The page's colour does not change the section's. View › Switch Background draws the page card in the other theme's tokens and leaves the rest of the window alone.

**Scroll bars** are 10 px and show only on an axis whose content reaches past the view, or that is scrolled away from the start. Options › Display can move the vertical bar to the left. The panes use egui's floating bars, 8 px wide, which narrow to 4 px when idle.

**Zoom** runs from 20 % to 400 %:

- Zoom In and Zoom Out step by a factor of 1/0.85, about 18 %.
- 100 % returns to actual size.
- Page Width fits the widest content (at least the title rule) plus 48 px.
- `Ctrl`+wheel zooms too. Each page remembers where it was scrolled.

## Note containers and outlines

**Containers.** A note is a container of text and other blocks, transparent until the pointer comes near or the caret is in it (`text_item`, `item_chrome` in `canvas.rs`).

- **Frame:** 1 px of `container_border`, with square corners, around the note and its grab bar.
- **Grab bar:** 10 px high (`NOTE_BAR`), flush with the frame's top. It is `grab_bar` at rest, `container_border` under the pointer or while dragged, and `line_strong` when the note is selected as a whole (`Esc` while typing, or a click on the bar). Its grip is four dots in the middle, in `muted` (`ink` when hovered or selected). There is no × on a note: Delete, Cut and the menu remove it.
- **Width handle:** the bar's right 16 px hold the ◂▸ handle, two small triangles. A 6 px strip along the note's right edge also drags its width.
- **Padding:** the text is 10 px from the frame's sides, 4 px from its top and 6 px from its bottom.
- **Auto-width:** a new note starts 96 px wide (`NOTE_MIN_WIDTH`) and widens as you type, up to the edge of the page in view, then wraps. A width set by hand stays. The height always follows the content.
- **Merging:** a note Shift+dragged onto another merges with it; the target shows a 2 px `accent` frame.
- **Other page items** (pictures, shapes, file cards, recordings, frises, equations on the page) keep Folio's selection: a 14 px bar 2 px above the item, tinted with the accent, with a × at its right, and a round accent handle at the bottom-right corner.

**Pictures in notes** are drawn bare, with no frame. **Tables** are blocks in notes:

- **Lines:** 1 px of `container_border`. Hidden borders show dashed in `faint` while the caret is in the table.
- **Header row:** shaded `sunken`. The cell shadings are the ten tints of `table::SHADES`.
- **Cells:** 6 × 3 px of padding. A column fits its text from 40 to 320 px (`TABLE_MIN_COLUMN`, `TABLE_AUTO_MAX`), and dragging its right border sets its width.

**Paragraph handles** (`editor::outline_handles`):

- **The handle:** in a 16 px gutter left of the note (`HANDLE_GUTTER`), the paragraph under the pointer shows a 10 × 12 px handle: a light tab with a pointed right side, filled with `hover` and edged in `container_border` (filled with `container_border` and edged in `muted` under the pointer).
- **Click, double-click, drag:** a click selects the paragraph with its subparagraphs, and a double-click folds them. A folded paragraph keeps a 9 px square with a "+" in the margin. Dragging the handle moves the family, with a 2 px `accent` line where it will land.
- **Right-click:** opens Cut, Copy, Delete and Select › Level 1–5 / All Levels.

**Outline levels:**

- A note has nine levels (0–8), each indented 24 px (`LEVEL_INDENT`).
- Bullets change with the level, • ◦ ▪ and round again (a level-1 bullet has a 2.5 px radius). Numbers do too: 1. a. i. and round again (`ListMarker::for_level`).
- The Bullets ▾ and Numbering ▾ libraries hold eight bullets and seven number styles.
- The marker takes a 20 px column before the text (`MARKER_WIDTH`), wider for a long number.

**Tags hang in the left margin**, before the marker, as desktop notebooks do:

- Each tag or To Do box takes an 18 px slot (`TAG_SLOT`), and the To Do box is 12 px (`CHECKBOX_SIZE`). A ticked To Do stays plain.
- A paragraph's text starts where it would with no tags; its tags take the slots to its left. A note's hang (`editor::tag_lead`) is the most any paragraph's tags reach past that paragraph's level indent, so an indented paragraph's tags hang inside its indent.
- The note's frame, grab bar, handles and hit area reach left by the hang, so its text starts under the page title whether it is tagged or not. When the hang changes, the note moves its frame by the difference and the text stays put (`CanvasState::note_leads`); undo brings back both. The frame never goes past the page's left edge: a note near it stops there, and its text makes the rest of the room. A note saved before this only records its hang when first seen.
- Tag glyphs are Fluent icons in each tag's colour.
- Remember for later and Highlight are drawn as a yellow highlight behind the text, and Definition as a green one.

## Menus, dialogs, task panes

**Menus** are white cards with a 6 px radius, a 1 px `line_strong` border and a soft shadow (4 px down, 12 px blur), with 6 px of margin:

- **Rows** (`widgets::menu_item`, `menu_row`, `tinted_menu_item`) are 28 px high, with a glyph column and a label column in Selawik 12.5. Every Fluent glyph has the same advance, so a menu's glyphs make one column and its labels another. A tag's glyph takes the tag's colour.
- **Shortcuts** sit right-aligned in a column of their own, Selawik 11.5 in `muted`, and grey out with their row when it cannot run.
- **The current choice** is ticked at the row's left, in the glyph column, as a desktop notebook ticks it (`widgets::checked_menu_item`, `checked_menu_row`): a 12 px ✓ in `ink`, with the label where a glyph row's label goes, so ticked and unticked rows line up. A checked row with a glyph of its own shows the glyph on a 22 px square of `nav_selected` edged in `container_border`. Section Color frames the current swatch in a 2 px `ink` ring. The Tag menu still ticks at the right, after the shortcut.
- **Submenus** open beside their row.
- **Hover** fills the row in `sunken`.
- **Wording:** English rows are in Title Case; French rows are in sentence case.
- **Context menus** follow the familiar order for each object (note text, paragraph handle, container, page row, section, section group, notebook, picture, ink, file).
- **The text menu** is at least 230 px wide. The Mini Toolbar floats on a card of its own above it: 72 px tall (`MINI_CARD_HEIGHT`: a row with the font and size boxes, then a row of 28 px format buttons), 6 px (`MINI_CARD_GAP`) above the menu's card, whose top-left corner is at the pointer. Both cards are one popup, so a click on the toolbar never closes the menu.
- **The Mini Toolbar on a selection** shows after text is selected with the mouse (a drag, a double or a triple click), when Options › General › Show Mini Toolbar on selection is on (the default): the same card, 24 px left of and 86 px above where the selecting drag or click ended (`show_mini_bar`). It is drawn at 0.55 opacity, fading to nothing as the pointer moves 160 px away (`MINI_FAR`), and solid with the pointer within 8 px (`mini_bar_opacity`). A key, typing, a scroll, a press elsewhere on the page, the end of the selection or the pointer going past 160 px puts it away. Its picks run as their ribbon twins do, on the selection, which stays; a click on it that picks nothing leaves the keyboard in the note.

**Dialogs** are centred modals over a dark backdrop (black at alpha 76; 150 in dark mode):

- **Card:** white, 6 px radius, 1 px `line_strong` border, 18 px margin.
- **Width:** most are 420 px; the range is 320–540.
- **Heading** (`dialog_heading`): a Selawik Semibold 20 title over a line of explanation in `muted` 12 px.
- **Buttons:**
  - Primary (`widgets::primary_button`): filled with `accent`, semibold 12 px text in `theme::on_fill(accent)` (white, or near-black on the dark theme's light purple), 30 px high, 4 px radius.
  - Secondary and text buttons, and a red `danger_button` for what cannot be undone, its text also by `on_fill`.
- **Progress** (inserting a printout, importing a notebook) is a hand-painted 6 px bar: a `line` track with the done part in `accent`, and a sweeping segment while the total is unknown. `egui::ProgressBar` is not used.
- **Options** is 800 × 400 px: a 200 px list of pages (General, Display, Proofing, Save & Backup, Language, Advanced) boxed in a `line` hairline on `panel`, as a desktop dialog boxes its list, beside a 560 px page with 200 px labels and 220 px combo boxes. Changes apply at once, so it has no Cancel.

**Printed and exported pages** (`export::print_css`) wear the same look in the browser: a white page on the window grey; the title at 26.67 px in a light face, its `line` hairline and the date in `faint` under it, on the page at `PAGE_LEFT`, `TITLE_TOP` where the canvas draws them (a `.page-header` div inside `.sheet-inner`), the title on one line on a sheet wide enough for it; notes in Carlito or Calibri at 14.67 px on 18 px lines, their tags hanging left of the text (and of a list item's bullet or number) as on the page; and blue headings, link blue (#0563C1) and hairline cards for frises, files and recordings.

**Buttons and boxes** follow the desktop convention (`theme::style`): white with a 1 px `line_strong` hairline and a 4 px radius, `sunken` with a `container_border` edge on hover, and `accent_soft` with an `accent` edge while pressed. Icon buttons (`widgets::icon_button_sized`) are frameless: the translucent `hover` fill, `sunken` while pressed, a 1 px `accent` ring when focused, and their glyph in `muted` (`ink` when hovered).

**Task panes** sit in the right-hand card (see Layout), with 12 × 10 px margins: Find Tags, Page Versions, Spelling, and Search Results or Recent Edits. One shows at a time, beside the page, and the page stays editable.

**Tooltips** are white cards like menus, after 0.35 s. **The toast** is a notification card: a rounded rectangle in `ink` with its glyph and 12.5 px text in `raised` (dark with white text in the light theme, the reverse in the dark one), 24 px above the window's bottom edge, with a check or warning glyph, for 2.6 s.

## Motion and accessibility

**Motion is limited to state.**

- Menus, dialogs, tooltips and the toast fade in over 0.16 s (`style.animation_time`, egui's area fade).
- Hover fills appear at once.
- The panes' floating scroll bars widen under the pointer.
- The import's progress bar sweeps while its total is unknown.
- The Mini Toolbar on a selection fades with the pointer's distance (see Menus).
- Pages do not slide, panes do not animate open, and nothing loops.

**Every control has an accessible name.** eframe's AccessKit support is on:

- Ribbon tabs are `Role::Tab` with their selected state, ribbon controls and menu rows are `Role::Button` with their label (or tooltip, for icon-only ones), page rows are `Role::ListItem`, and the page's scroll bars are `Role::ScrollBar`.
- An icon-only control always has a tooltip with its name. A Split's arrow is named "{label} ▾", or more precisely where that would not say what it opens.
- The UI tests find controls by these names, as a screen reader would.

**Keyboard paths.**

- Search: `Ctrl+E` opens the search box (↑ ↓, `Enter` and `Esc` in its results; `Alt+O` opens the results pane), and `Ctrl+F` finds on the page.
- Window: `Alt+F` opens Backstage and `Esc` closes it; `Ctrl+,` opens Options; `F7` checks spelling; `F11` switches Full Page View; `Ctrl+F1` shows the tabs only.
- Pages and notebooks: `Ctrl+Shift+T` goes to the page title; `Ctrl+N`, `Ctrl+T` and `Ctrl+Shift+N` make a page, a section and a notebook.
- Notes: the outline keys (`Tab`, `Alt+Shift`+arrows and digits) shape them; `Esc` steps from typing to the selected note and then to nothing, and the arrow keys nudge selected items.
- Help › Keyboard shortcuts lists every key in the interface language.
- **Missing:** KeyTips, `F6` between panes and `Shift+F10` for menus; context menus open with a right-click.

**Contrast**, light theme, on white:

| Text | Ratio |
| --- | --- |
| `ink` | 15.5:1 |
| `text2` | 10.1:1 |
| `muted` | 6.2:1 |
| `accent` (or white on `accent`) | 8.4:1 |
| `faint` | 2.7:1 |

`faint` is kept for the date, greyed controls and placeholders. In the dark theme, `ink` on a card is 13.3:1 and `muted` 6.5:1.

Text on a fill is picked by `theme::on_fill`, so it reads at 4.5:1 or more in both themes: white on `accent` (8.4:1) and `danger` (5.7:1) in light, `#1B1B1B` on the dark theme's `accent` (6.6:1) and `danger` (8.5:1). Backstage's white text is 8.4:1 on `#7719AA` and 9.9:1 on `#5B2A86`. The unit tests `buttons_and_ticks_read_on_their_fill_in_both_themes` and `backstage_column_reads_white_on_purple` keep them there.
