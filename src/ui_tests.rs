//! End-to-end UI tests that drive the real app with pointer and keyboard events.
//!
//! Set `FOLIO_SHOTS=/some/dir` to also save a screenshot at each `shot(..)`, which is
//! handy when changing the canvas UI.

use std::sync::{Mutex, MutexGuard};

use eframe::egui::{self, Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use super::FolioApp;
use crate::canvas::{CanvasTool, EraserMode, EraserSize};
use crate::find::{MatchPlace, MilestoneField};
use crate::i18n::{t, Lang};
use crate::model::{CanvasItem, CanvasKind, Milestone, Rgb, ShapeKind};
use crate::ribbon::RibbonTab;

/// UI tests share `FOLIO_DATA_DIR` and the GPU, so they run one at a time.
static UI_LOCK: Mutex<()> = Mutex::new(());

pub(crate) struct Ui {
    pub h: Harness<'static, FolioApp>,
    _dir: tempfile::TempDir,
    _lock: MutexGuard<'static, ()>,
}

impl Ui {
    pub fn start() -> Self {
        Self::start_in(Lang::En)
    }

    /// Starts the app in `lang`, whatever the machine's locale.
    pub fn start_in(lang: Lang) -> Self {
        let lock = UI_LOCK.lock().unwrap_or_else(|poison| poison.into_inner());
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("FOLIO_DATA_DIR", dir.path());
        // The app reads FOLIO_LANG at startup; the pin keeps other test threads English.
        std::env::set_var("FOLIO_LANG", lang.code());
        crate::i18n::pin_thread_lang(lang);
        // A file queued by a test that failed before its dialog opened is not this test's.
        crate::canvas::PICKS
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clear();
        let mut h = Harness::builder()
            .with_size(egui::vec2(1320.0, 900.0))
            .with_pixels_per_point(1.0)
            .with_max_steps(500)
            // A real frame time, so quick clicks count as double-clicks.
            .with_step_dt(1.0 / 60.0)
            .wgpu()
            .build_eframe(|cc| FolioApp::new(cc).expect("app"));
        h.step();
        h.step();
        Self {
            h,
            _dir: dir,
            _lock: lock,
        }
    }

    /// Opens a fresh page and types its title, leaving the caret in the title.
    pub fn new_page(&mut self, title: &str) {
        self.h.key_press_modifiers(Modifiers::COMMAND, Key::N);
        self.steps(3);
        self.type_text(title);
    }

    pub fn steps(&mut self, count: usize) {
        for _ in 0..count {
            self.h.step();
        }
    }

    fn button(&mut self, pos: Pos2, pressed: bool) {
        self.button_with(PointerButton::Primary, pos, pressed);
    }

    fn button_with(&mut self, button: PointerButton, pos: Pos2, pressed: bool) {
        self.h.event(Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: Modifiers::NONE,
        });
    }

    pub fn click(&mut self, pos: Pos2) {
        self.h.event(Event::PointerMoved(pos));
        self.h.step();
        self.button(pos, true);
        self.h.step();
        self.button(pos, false);
        self.steps(3);
    }

    pub fn double_click(&mut self, pos: Pos2) {
        self.h.event(Event::PointerMoved(pos));
        self.h.step();
        for _ in 0..2 {
            self.button(pos, true);
            self.h.step();
            self.button(pos, false);
            self.h.step();
        }
        self.steps(2);
    }

    pub fn drag(&mut self, from: Pos2, to: Pos2) {
        self.drag_with(PointerButton::Primary, from, to);
    }

    /// Drags with `button` (the middle or right button pans).
    pub fn drag_with(&mut self, button: PointerButton, from: Pos2, to: Pos2) {
        self.h.event(Event::PointerMoved(from));
        self.h.step();
        self.button_with(button, from, true);
        self.h.step();
        for i in 1..=10 {
            let t = i as f32 / 10.0;
            self.h.event(Event::PointerMoved(from + (to - from) * t));
            self.h.step();
        }
        self.button_with(button, to, false);
        self.steps(3);
    }

    /// Turns the mouse wheel over `pos` (positive y scrolls the page up, as egui's events
    /// do), holding `modifiers`.
    pub fn wheel(&mut self, pos: Pos2, delta: egui::Vec2, modifiers: Modifiers) {
        self.h.event(Event::PointerMoved(pos));
        self.h.event(Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta,
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
            modifiers,
        });
        // egui spreads wheel scrolling over a few frames (more for a long scroll): wait
        // until it is over, so none of it reaches what the test does next.
        self.steps(12);
        for _ in 0..120 {
            if !self.h.ctx.input(|input| input.is_scrolling()) {
                break;
            }
            self.h.step();
        }
    }

    /// Holds `modifiers` (Shift, Ctrl…) while doing `action`.
    pub fn with_modifiers(&mut self, modifiers: Modifiers, action: impl FnOnce(&mut Self)) {
        self.h.event(Event::ModifiersChanged(modifiers));
        self.h.step();
        action(self);
        self.h.event(Event::ModifiersChanged(Modifiers::NONE));
        self.h.step();
    }

    /// Holds a key down (Space to pan) while doing `action`.
    pub fn holding(&mut self, key: Key, action: impl FnOnce(&mut Self)) {
        self.h.key_down(key);
        self.h.step();
        action(self);
        self.h.key_up(key);
        self.h.step();
    }

    /// Drags through every point of `path` (for the lasso).
    pub fn drag_path(&mut self, path: &[Pos2]) {
        let (first, rest) = path.split_first().expect("a path");
        self.h.event(Event::PointerMoved(*first));
        self.h.step();
        self.button(*first, true);
        self.h.step();
        for point in rest {
            self.h.event(Event::PointerMoved(*point));
            self.h.step();
        }
        self.button(*path.last().unwrap(), false);
        self.steps(3);
    }

    pub fn selection(&self) -> crate::canvas::Selection {
        self.app().canvas.selection.clone()
    }

    pub fn ink(&self) -> Vec<crate::model::InkStroke> {
        self.app()
            .page
            .as_ref()
            .map(|page| page.canvas.ink.clone())
            .unwrap_or_default()
    }

    pub fn type_text(&mut self, text: &str) {
        self.h.event(Event::Text(text.into()));
        self.steps(2);
    }

    pub fn press(&mut self, key: Key) {
        self.h.key_press(key);
        self.steps(2);
    }

    pub fn press_with(&mut self, modifiers: Modifiers, key: Key) {
        self.h.key_press_modifiers(modifiers, key);
        self.steps(2);
    }

    /// Clicks a ribbon or toolbar button by its accessible label.
    pub fn click_label(&mut self, label: &str) {
        let rect = self.h.get_by_label(label).rect();
        self.click(rect.center());
    }

    /// Clicks the button whose label contains `text` (labels may include an icon). When
    /// several match, the last one wins: menus and dialogs are drawn last.
    pub fn click_label_containing(&mut self, text: &str) {
        let rect = self
            .h
            .get_all_by_label_contains(text)
            .last()
            .unwrap_or_else(|| panic!("no widget labelled {text:?}"))
            .rect();
        self.click(rect.center());
    }

    pub fn app(&self) -> &FolioApp {
        self.h.state()
    }

    pub fn items(&self) -> Vec<CanvasItem> {
        self.app()
            .page
            .as_ref()
            .map(|page| page.canvas.items.clone())
            .unwrap_or_default()
    }

    pub fn item(&self, id: u64) -> CanvasItem {
        self.items()
            .into_iter()
            .find(|item| item.id == id)
            .expect("canvas item")
    }

    pub fn screen(&self, scene: Pos2) -> Pos2 {
        self.app().canvas.scene_to_screen(scene)
    }

    /// A ribbon tab's rect, found by its label (sentence case, as shown) and its tab role,
    /// so a group or button with the same name ("Insert") never matches.
    pub fn ribbon_tab_rect(&self, name: &str) -> egui::Rect {
        self.h.get_by_role_and_label(egui::Role::Tab, name).rect()
    }

    /// Clicks a ribbon tab by its label (sentence case, as shown).
    pub fn ribbon_tab(&mut self, name: &str) {
        let rect = self.ribbon_tab_rect(name);
        self.click(rect.center());
    }

    /// A ribbon control by its label: the one inside the ribbon (below the header strip,
    /// above the panels), so a page or menu item with the same name never matches.
    pub fn ribbon_button_rect(&self, label: &str) -> egui::Rect {
        let bottom = 48.0 + crate::ribbon::panel_height(&self.app().ribbon) + 4.0;
        self.h
            .get_all_by_role_and_label(egui::Role::Button, label)
            .map(|node| node.rect())
            .find(|rect| rect.top() >= 48.0 && rect.bottom() <= bottom)
            .unwrap_or_else(|| panic!("no ribbon control labelled {label:?}"))
    }

    pub fn ribbon_button(&mut self, label: &str) {
        let rect = self.ribbon_button_rect(label);
        self.click(rect.center());
    }

    /// Shows the open Options dialog's page named `label` ("Proofing", "Language"), by
    /// its tab in the dialog's list.
    pub fn options_page(&mut self, label: &str) {
        let rect = self.h.get_by_role_and_label(egui::Role::Tab, label).rect();
        self.click(rect.center());
    }

    /// Opens Home's Tags menu: by the gallery's ▾ when the band shows the gallery whole,
    /// else by its one button (Tags drop to their icons first as the window narrows).
    pub fn open_tags_menu(&mut self) {
        let tags = t!("ribbon-group-tags").into_owned();
        let arrow = format!("{tags} ▾");
        let whole = self
            .h
            .query_all_by_role_and_label(egui::Role::Button, &arrow)
            .next()
            .is_some();
        self.ribbon_button(if whole { &arrow } else { &tags });
    }

    /// Insert › Table, then the grid cell for a `cols` × `rows` table.
    pub fn insert_table(&mut self, cols: u8, rows: u8) {
        self.ribbon_tab(&t!("ribbon-tab-insert"));
        self.ribbon_button(&t!("ribbon-table"));
        let cell = self
            .h
            .get_by_label(&format!("{cols} × {rows}"))
            .rect()
            .center();
        self.click(cell);
    }

    /// The first table on the open page (in any note), and that note's id.
    pub fn table(&self) -> (u64, crate::model::TableBlock) {
        self.items()
            .into_iter()
            .find_map(|item| {
                let id = item.id;
                item.doc()?
                    .blocks
                    .iter()
                    .find_map(|block| match &block.kind {
                        crate::model::BlockKind::Table(table) => Some((id, table.clone())),
                        _ => None,
                    })
            })
            .expect("a table on the page")
    }

    /// The text of the first table's cells, row by row.
    pub fn cells(&self) -> Vec<Vec<String>> {
        self.table()
            .1
            .rows
            .iter()
            .map(|row| row.iter().map(|cell| cell.text.text.clone()).collect())
            .collect()
    }

    /// The doc id, note and block of the first table on the page.
    fn table_ids(&self) -> (String, u64, u64) {
        let page = self.app().page.as_ref().expect("a page");
        for item in &page.canvas.items {
            if let Some(doc) = item.doc() {
                if let Some(block) = doc
                    .blocks
                    .iter()
                    .find(|block| matches!(block.kind, crate::model::BlockKind::Table(_)))
                {
                    return (format!("{}:{}", page.id, item.id), item.id, block.id);
                }
            }
        }
        panic!("no table on the page");
    }

    /// The screen rect of a table cell's text, as last drawn.
    pub fn cell_rect(&self, row: usize, col: usize) -> egui::Rect {
        let (doc_id, _, block) = self.table_ids();
        let id = crate::editor::table_cell_id(&doc_id, block, row, col);
        let rect = self
            .h
            .ctx
            .read_response(id)
            .expect("the cell was drawn")
            .rect;
        egui::Rect::from_min_max(self.screen(rect.min), self.screen(rect.max))
    }

    /// Fills the table holding the caret, row by row, with Tab between cells.
    pub fn type_cells(&mut self, cells: &[&str]) {
        for (n, text) in cells.iter().enumerate() {
            if n > 0 {
                self.press(Key::Tab);
            }
            self.type_text(text);
        }
    }

    /// What the next frame puts on the clipboard.
    fn copied_now(&mut self) -> Vec<String> {
        self.h.step();
        copied(&[self.h.output().clone()])
    }

    /// Draw › the `index`th pen preset of the gallery.
    pub fn pick_pen(&mut self, index: usize) {
        self.ribbon_tab(&t!("ribbon-tab-draw"));
        let label = crate::canvas::pen_label(&self.app().canvas.pens[index]);
        self.ribbon_button(&label);
    }

    /// Clicks a ribbon control and returns every frame's output (clipboard and viewport
    /// commands), like `click_label_outputs`.
    pub fn ribbon_button_outputs(&mut self, label: &str) -> Vec<egui::FullOutput> {
        let pos = self.ribbon_button_rect(label).center();
        let mut outputs = Vec::new();
        self.h.event(Event::PointerMoved(pos));
        self.h.step();
        outputs.push(self.h.output().clone());
        for pressed in [true, false] {
            self.button(pos, pressed);
            self.h.step();
            outputs.push(self.h.output().clone());
        }
        for _ in 0..3 {
            self.h.step();
            outputs.push(self.h.output().clone());
        }
        outputs
    }

    /// A point on the page, in screen coordinates, at the same place in the scene the
    /// tests have always used (204, 254 from the page's top-left at zoom 1: where the old
    /// fixed window point 760, 520 fell), so it falls clear of the sample content whatever
    /// the panels around the page.
    pub fn empty_spot(&self, dx: f32, dy: f32) -> Pos2 {
        let view = self.app().canvas.viewport();
        egui::pos2(view.left() + 204.0 + dx, view.top() + 254.0 + dy)
    }

    /// A point on empty paper of the open page, in screen coordinates, for tests that need
    /// one wherever the items are and however the page is scrolled or zoomed. It lies
    /// below the page header and right of the first note's column (at least
    /// `PAGE_CONTENT_TOP + 200` down and `PAGE_LEFT + 450` across, in page units), 40 px
    /// clear of every item, and inside the part of the page in view, away from its edges
    /// and the scrollbars along its right and bottom. Panics when there is none.
    pub fn empty_paper_point(&self) -> Pos2 {
        use crate::canvas::{PAGE_CONTENT_TOP, PAGE_LEFT};
        let canvas = &self.app().canvas;
        let visible = canvas.visible().expect("the page was drawn");
        let view = canvas.viewport();
        let area = egui::Rect::from_min_max(
            view.min + egui::vec2(20.0, 20.0),
            view.max - egui::vec2(40.0, 40.0),
        );
        let corner = visible
            .min
            .max(egui::pos2(PAGE_LEFT + 450.0, PAGE_CONTENT_TOP + 200.0));
        let start = self.screen(corner).max(area.min);
        let taken: Vec<egui::Rect> = self
            .items()
            .iter()
            .map(|item| {
                let rect = item.rect();
                egui::Rect::from_min_max(self.screen(rect.min), self.screen(rect.max)).expand(40.0)
            })
            .collect();
        let rows = ((area.bottom() - start.y) / 50.0).max(-1.0) as usize + 1;
        let columns = ((area.right() - start.x) / 60.0).max(-1.0) as usize + 1;
        (0..rows)
            .flat_map(|row| (0..columns).map(move |column| (row, column)))
            .map(|(row, column)| start + egui::vec2(column as f32 * 60.0, row as f32 * 50.0))
            .find(|point| area.contains(*point) && !taken.iter().any(|rect| rect.contains(*point)))
            .unwrap_or_else(|| panic!("no empty paper in view ({visible:?} on {view:?})"))
    }

    /// The middle of an item's grab bar (shown on hover or selection).
    pub fn bar(&self, id: u64) -> Pos2 {
        let rect = self.item(id).rect();
        self.screen(egui::pos2(rect.center().x, rect.top() - 9.0))
    }

    /// The ◂▸ width handle at the right end of a note's grab bar.
    pub fn width_handle(&self, id: u64) -> Pos2 {
        let rect = self.item(id).rect();
        self.screen(egui::pos2(rect.right() - 8.0, rect.top() - 5.0))
    }

    /// The doc id of a note on the open page (as the editor keys its rows).
    fn doc_id(&self, note: u64) -> String {
        let page = self.app().page.as_ref().expect("page");
        format!("{}:{}", page.id, note)
    }

    /// Where paragraph `block` of `note` was laid out last frame.
    pub fn row(&self, note: u64, block: u64) -> crate::editor::Row {
        self.app().editor.rows[&self.doc_id(note)]
            .iter()
            .find(|row| row.block == block)
            .cloned()
            .expect("the paragraph was drawn")
    }

    /// A screen point on paragraph `block`'s text, `at` chars in.
    pub fn text_point(&self, note: u64, block: u64, at: usize) -> Pos2 {
        let (galley, origin) = self.row(note, block).text.expect("a text paragraph");
        let caret = galley.pos_from_cursor(egui::text::CCursor::new(at));
        self.screen(origin + caret.center().to_vec2())
    }

    /// Paragraph `block`'s handle, in the margin left of its note.
    pub fn handle_point(&self, note: u64, block: u64) -> Pos2 {
        let rect = self.item(note).rect();
        let row = self.row(note, block);
        self.screen(egui::pos2(
            rect.left() - crate::editor::HANDLE_GUTTER / 2.0,
            row.head.center(),
        ))
    }

    /// The blocks of the first note on the page.
    pub fn blocks(&self) -> Vec<crate::model::Block> {
        self.items()[0].doc().expect("a note").blocks.clone()
    }

    /// Steps until a printout being inserted is placed (its pages render on a thread),
    /// or until it asks for a password.
    pub fn wait_for_printout(&mut self) {
        for _ in 0..1000 {
            self.h.step();
            let canvas = &self.app().canvas;
            if !canvas.printout_busy() || canvas.printout_asks_password() {
                self.steps(2);
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("the printout never finished");
    }

    /// Steps until a widget labelled `label` shows, for up to 5 s: the desktop notebook import reads
    /// on another thread.
    pub fn wait_for_label(&mut self, label: &str) {
        for _ in 0..500 {
            if self.h.query_by_label(label).is_some() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
            self.h.step();
        }
        panic!("{label:?} never showed");
    }

    /// Writes a Letter PDF with one page per string and drops it on empty paper.
    /// Returns the folder holding it, which must outlive the insert.
    pub fn drop_pdf(&mut self, name: &str, pages: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        std::fs::write(&path, crate::pdf::letter_pdf(pages)).unwrap();
        self.drop_file(path);
        dir
    }

    /// Drops a file from the file manager onto empty paper.
    pub fn drop_file(&mut self, path: std::path::PathBuf) {
        let spot = self.empty_paper_point();
        self.h.event(Event::PointerMoved(spot));
        self.h
            .input_mut()
            .dropped_files
            .push(std::sync::Arc::new(DroppedPath(path)));
        self.steps(3);
    }

    pub fn shot(&mut self, name: &str) {
        let Some(dir) = std::env::var_os("FOLIO_SHOTS") else {
            return;
        };
        // A few frames, so pictures waiting their turn to load are drawn.
        self.steps(4);
        let image = self.h.render().expect("render");
        let dir = std::path::Path::new(&dir);
        std::fs::create_dir_all(dir).expect("create the FOLIO_SHOTS folder");
        image
            .save(dir.join(format!("{name}.png")))
            .expect("save shot");
    }
}

fn text_of(item: &CanvasItem) -> String {
    item.kind.plain_text()
}

#[test]
fn type_anywhere_and_format_with_the_ribbon() {
    let mut ui = Ui::start();
    ui.new_page("Biology notes");
    ui.press(Key::Enter);
    ui.type_text("Cells are the basic unit of life.");
    ui.shot("01-first-note");
    assert_eq!(ui.items().len(), 1);

    // Clicking empty paper starts a second note right there, ready for typing.
    let spot = ui.empty_spot(0.0, 0.0);
    ui.click(spot);
    ui.type_text("A second note");
    ui.shot("02-second-note");
    let items = ui.items();
    assert_eq!(items.len(), 2, "{items:#?}");
    assert_eq!(text_of(&items[1]), "A second note");

    // The ribbon's Bold button applies to what is typed next, without losing focus.
    ui.type_text(" plain ");
    ui.click_label("Bold");
    ui.type_text("bold");
    ui.shot("03-bold");
    let note = &ui.items()[1];
    let block = &note.doc().unwrap().blocks[0];
    assert_eq!(block.text.text, "A second note plain bold");
    let start = block.text.text.find("bold").unwrap();
    assert!(block.text.style_at(start).bold);
    assert!(
        block.text.style_at(start + 3).bold,
        "every typed char is bold"
    );
    assert!(!block.text.style_at(start - 2).bold);

    // Ctrl+1 makes the line a to-do, as in the desktop notebook.
    ui.press_with(Modifiers::COMMAND, Key::Num1);
    let note = &ui.items()[1];
    assert!(matches!(
        note.doc().unwrap().blocks[0].kind,
        crate::model::BlockKind::Todo { .. }
    ));
}

#[test]
fn empty_notes_disappear_and_escape_selects_then_delete_removes() {
    let mut ui = Ui::start();
    ui.new_page("Scratch");
    let spot = ui.empty_spot(0.0, 0.0);
    ui.click(spot);
    assert_eq!(ui.items().len(), 1);
    // Clicking somewhere else without typing drops the empty note.
    let elsewhere = ui.empty_spot(-150.0, 160.0);
    ui.click(elsewhere);
    assert_eq!(ui.items().len(), 1, "only the new, focused note remains");
    ui.type_text("Keep me");
    let id = ui.items()[0].id;

    ui.press(Key::Escape);
    assert_eq!(ui.app().canvas.selection.primary(), Some(id));
    assert_eq!(ui.app().canvas.active_text, None);
    ui.shot("04-selected");
    ui.press(Key::Delete);
    assert!(ui.items().is_empty());
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert_eq!(text_of(&ui.items()[0]), "Keep me", "undo brings it back");
}

#[test]
fn notes_move_by_their_grab_bar_and_space_drag_pans_the_page() {
    let mut ui = Ui::start();
    ui.new_page("Move");
    let spot = ui.empty_spot(0.0, 0.0);
    ui.click(spot);
    ui.type_text("Drag me");
    let id = ui.items()[0].id;
    let before = ui.item(id).rect();
    let bar = ui.bar(id);
    ui.drag(bar, bar + egui::vec2(60.0, 80.0));
    ui.shot("05-moved");
    let after = ui.item(id).rect();
    assert_eq!(after.min - before.min, egui::vec2(60.0, 80.0));
    assert_eq!(
        after.min,
        after.min.round(),
        "positions snap to whole points"
    );

    // Dragging empty paper draws a selection band (as in the desktop notebook): the view stays put.
    // The view is its top-left corner: leaving the note swaps the Home band's text
    // toolbar for the selection bar, a pixel shorter, so the page area grows by 1 px.
    let view = |ui: &Ui| ui.app().canvas.scene_rect.map(|rect| rect.min);
    let scene_before = view(&ui);
    let from = ui.empty_spot(-180.0, 200.0);
    ui.drag(from, from + egui::vec2(100.0, 60.0));
    assert_eq!(view(&ui), scene_before, "paper drags select, not pan");
    assert!(ui.selection().is_empty(), "the band covered nothing");

    // Space+drag pans with any tool, without moving anything.
    let from = ui.empty_spot(-150.0, 200.0);
    ui.holding(Key::Space, |ui| {
        ui.drag(from, from + egui::vec2(-100.0, -50.0))
    });
    assert_ne!(view(&ui), scene_before, "Space+drag pans");
    assert_eq!(ui.item(id).rect(), after);

    // So does the Pan tool, even starting on top of the note.
    ui.h.state_mut().canvas.tool = CanvasTool::Pan;
    let panned = view(&ui);
    let on_note = ui.screen(ui.item(id).rect().center());
    ui.drag(on_note, on_note + egui::vec2(-80.0, 40.0));
    assert_ne!(view(&ui), panned, "the Pan tool pans");
    assert_eq!(ui.item(id).rect(), after, "and never moves the note");
}

#[test]
fn shift_click_and_rubber_band_select_items_that_move_and_delete_together() {
    let mut ui = Ui::start();
    ui.new_page("Group");
    let mut shapes = Vec::new();
    for dx in [-160.0, -10.0, 140.0] {
        ui.h.state_mut().canvas.tool = CanvasTool::Shape(ShapeKind::Rect);
        let from = ui.empty_spot(dx, -60.0);
        ui.drag(from, from + egui::vec2(100.0, 70.0));
        shapes.push(ui.items().last().unwrap().id);
    }
    let middle = |ui: &Ui, id: u64| ui.screen(ui.item(id).rect().center());
    let first = middle(&ui, shapes[0]);
    ui.click(first);
    let second = middle(&ui, shapes[1]);
    ui.with_modifiers(Modifiers::SHIFT, |ui| ui.click(second));
    assert_eq!(ui.selection().items, vec![shapes[0], shapes[1]]);
    ui.shot("m6-01-shift-click");

    // Dragging one of them moves both, as one undo step.
    let before: Vec<egui::Rect> = shapes.iter().map(|id| ui.item(*id).rect()).collect();
    ui.drag(second, second + egui::vec2(40.0, 30.0));
    for (index, id) in shapes.iter().enumerate() {
        let moved = ui.item(*id).rect().min - before[index].min;
        let expected = if index < 2 {
            egui::vec2(40.0, 30.0)
        } else {
            egui::Vec2::ZERO
        };
        assert_eq!(moved, expected, "shape {index}");
    }
    ui.press_with(Modifiers::COMMAND, Key::Z);
    for (index, id) in shapes.iter().enumerate() {
        assert_eq!(
            ui.item(*id).rect(),
            before[index],
            "undo puts shape {index} back"
        );
    }

    // A band over empty paper picks every shape whose middle it covers.
    ui.press(Key::Escape);
    let all = shapes.iter().fold(egui::Rect::NOTHING, |area, id| {
        area.union(ui.item(*id).rect())
    });
    let scene_before = ui.app().canvas.scene_rect;
    let (from, to) = (
        ui.screen(all.min - egui::vec2(25.0, 25.0)),
        ui.screen(all.max + egui::vec2(25.0, 25.0)),
    );
    ui.drag(from, to);
    let mut picked = ui.selection().items;
    picked.sort_unstable();
    let mut expected = shapes.clone();
    expected.sort_unstable();
    assert_eq!(picked, expected);
    assert_eq!(ui.app().canvas.scene_rect, scene_before);
    // The three are selected together, and no ink with them.
    assert_eq!(ui.selection().items.len(), 3);
    assert!(ui.selection().strokes.is_empty());
    ui.shot("m6-02-rubber-band");

    ui.press(Key::Delete);
    assert!(ui.items().is_empty());
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert_eq!(ui.items().len(), 3, "one undo brings the whole group back");
}

#[test]
fn lasso_selects_ink_and_items_that_copy_and_paste_together() {
    let mut ui = Ui::start();
    ui.new_page("Lasso");
    ui.h.state_mut().canvas.tool = CanvasTool::Pen;
    let from = ui.empty_spot(-120.0, 0.0);
    ui.drag(from, from + egui::vec2(120.0, 0.0));
    ui.h.state_mut().canvas.tool = CanvasTool::Shape(ShapeKind::Ellipse);
    let corner = ui.empty_spot(60.0, -40.0);
    ui.drag(corner, corner + egui::vec2(90.0, 70.0));
    let ellipse = ui.items()[0].id;
    // A stroke far below stays outside the loop.
    ui.h.state_mut().canvas.tool = CanvasTool::Pen;
    let far = ui.empty_spot(-150.0, 220.0);
    ui.drag(far, far + egui::vec2(60.0, 0.0));
    assert_eq!(ui.ink().len(), 2);

    ui.h.state_mut().ribbon.tab = RibbonTab::Draw;
    ui.steps(2);
    ui.ribbon_button("Lasso");
    let centre = ui.empty_spot(30.0, 0.0);
    let path: Vec<Pos2> = (0..=24)
        .map(|i| {
            let angle = i as f32 / 24.0 * std::f32::consts::TAU;
            centre + egui::vec2(angle.cos() * 200.0, angle.sin() * 110.0)
        })
        .collect();
    ui.drag_path(&path);
    let selection = ui.selection();
    assert_eq!(selection.items, vec![ellipse]);
    assert_eq!(selection.strokes, vec![0]);
    ui.shot("m6-03-lasso");

    // Copy, then paste what went to the clipboard: both come back together.
    ui.h.event(Event::Copy);
    ui.h.step();
    let copied =
        ui.h.output()
            .platform_output
            .commands
            .iter()
            .find_map(|command| match command {
                egui::OutputCommand::CopyText(text) => Some(text.clone()),
                _ => None,
            })
            .expect("the selection went to the clipboard");
    ui.steps(1);
    ui.h.event(Event::Paste(copied));
    ui.steps(3);
    let items = ui.items();
    let ink = ui.ink();
    assert_eq!((items.len(), ink.len()), (2, 3));
    let shift = items[1].rect().min - items[0].rect().min;
    let stroke_shift = egui::vec2(
        ink[2].points[0][0] - ink[0].points[0][0],
        ink[2].points[0][1] - ink[0].points[0][1],
    );
    assert_eq!(shift, egui::vec2(24.0, 24.0));
    assert_eq!(
        stroke_shift, shift,
        "the pasted ink keeps its place beside the shape"
    );
    assert_eq!(ui.selection().items, vec![items[1].id]);
    assert_eq!(ui.selection().strokes, vec![2]);
}

#[test]
fn select_all_and_a_burst_of_nudges_is_one_undo_step() {
    let mut ui = Ui::start();
    ui.new_page("Nudge");
    for dx in [-150.0, 50.0] {
        ui.h.state_mut().canvas.tool = CanvasTool::Shape(ShapeKind::Rect);
        let from = ui.empty_spot(dx, 0.0);
        ui.drag(from, from + egui::vec2(80.0, 60.0));
    }
    ui.press(Key::Escape);
    ui.press_with(Modifiers::COMMAND, Key::A);
    assert_eq!(ui.selection().items.len(), 2);
    let before: Vec<egui::Rect> = ui.items().iter().map(CanvasItem::rect).collect();
    for _ in 0..3 {
        ui.press(Key::ArrowRight);
    }
    for (item, start) in ui.items().iter().zip(&before) {
        assert_eq!(item.rect().min - start.min, egui::vec2(3.0, 0.0));
    }
    ui.press_with(Modifiers::COMMAND, Key::Z);
    let after: Vec<egui::Rect> = ui.items().iter().map(CanvasItem::rect).collect();
    assert_eq!(after, before, "one undo takes back the whole burst");
}

#[test]
fn nudging_another_selection_is_its_own_undo_step() {
    let mut ui = Ui::start();
    ui.new_page("Two nudges");
    let mut shapes = Vec::new();
    for dx in [-150.0, 50.0] {
        ui.h.state_mut().canvas.tool = CanvasTool::Shape(ShapeKind::Rect);
        let from = ui.empty_spot(dx, 0.0);
        ui.drag(from, from + egui::vec2(80.0, 60.0));
        shapes.push(ui.items().last().unwrap().id);
    }
    let before: Vec<egui::Rect> = ui.items().iter().map(CanvasItem::rect).collect();
    for id in &shapes {
        let centre = ui.screen(ui.item(*id).rect().center());
        ui.click(centre);
        assert_eq!(ui.selection().items, vec![*id]);
        ui.press(Key::ArrowRight);
    }
    ui.press(Key::Escape);
    ui.press_with(Modifiers::COMMAND, Key::Z);
    let rects: Vec<egui::Rect> = ui.items().iter().map(CanvasItem::rect).collect();
    assert_eq!(
        rects[1], before[1],
        "the first undo takes back the second nudge"
    );
    assert_eq!(rects[0].min - before[0].min, egui::vec2(1.0, 0.0), "only");
    ui.press_with(Modifiers::COMMAND, Key::Z);
    let rects: Vec<egui::Rect> = ui.items().iter().map(CanvasItem::rect).collect();
    assert_eq!(rects, before);
}

#[test]
fn middle_and_right_drags_pan_with_every_tool_and_keep_the_selection() {
    let mut ui = Ui::start();
    ui.new_page("Pan buttons");
    let mut shapes = Vec::new();
    for dx in [-160.0, 40.0] {
        ui.h.state_mut().canvas.tool = CanvasTool::Shape(ShapeKind::Rect);
        let from = ui.empty_spot(dx, -60.0);
        ui.drag(from, from + egui::vec2(100.0, 70.0));
        shapes.push(ui.items().last().unwrap().id);
    }
    let rects = |ui: &Ui| -> Vec<egui::Rect> { ui.items().iter().map(CanvasItem::rect).collect() };
    let view = |ui: &Ui| ui.app().canvas.scene_rect.unwrap().min;
    let before = rects(&ui);
    let depth = ui.app().canvas.undo_depth();
    // Each button drags the page one way and back (the second from where the first left
    // the spot), so the items stay in view. The view never goes above or left of the
    // page's corner, so each check starts from a view scrolled into the page, with room
    // toward the corner for the first drag, and the items still in view.
    let check = |ui: &mut Ui,
                 from: &dyn Fn(&Ui) -> Pos2,
                 selected: &crate::canvas::Selection,
                 place: &str| {
        let size = ui.app().canvas.scene_rect.unwrap().size();
        ui.h.state_mut().canvas.scene_rect =
            Some(egui::Rect::from_min_size(egui::pos2(150.0, 100.0), size));
        ui.steps(1);
        assert_eq!(
            view(ui),
            egui::pos2(150.0, 100.0),
            "{place}: inside the page"
        );
        for (button, sign) in [
            (PointerButton::Middle, 1.0),
            (PointerButton::Secondary, -1.0),
        ] {
            let start = view(ui);
            let at = from(ui);
            let by = egui::vec2(120.0, 60.0) * sign;
            ui.drag_with(button, at, at + by);
            let moved = (view(ui) - start) * -sign;
            assert!(
                moved.x > 60.0 && moved.y > 20.0,
                "{button:?} {place}: {moved:?}"
            );
            assert_eq!(&ui.selection(), selected, "{button:?} {place}");
            assert_eq!(rects(ui), before, "{button:?} {place} moves nothing");
            assert_eq!(ui.app().canvas.undo_depth(), depth, "{button:?} {place}");
            assert!(ui.ink().is_empty(), "{button:?} {place} draws nothing");
            assert!(!egui::Popup::is_any_open(&ui.h.ctx), "{button:?} {place}");
        }
    };

    // On empty paper with the Type tool: the band is for the left button only.
    let one = ui.selection();
    assert_eq!(one.items, vec![shapes[1]]);
    let paper = |ui: &Ui| ui.empty_spot(-150.0, 200.0);
    check(&mut ui, &paper, &one, "on paper");

    // Inside a group's frame, between its items.
    ui.press_with(Modifiers::COMMAND, Key::A);
    let group = ui.selection();
    assert_eq!(group.items.len(), 2);
    let between = |ui: &Ui| {
        let (a, b) = (ui.item(shapes[0]).rect(), ui.item(shapes[1]).rect());
        ui.screen(a.right_center() + (b.left_center() - a.right_center()) / 2.0)
    };
    check(&mut ui, &between, &group, "in the frame");

    // With the lasso, the pen and both kinds of eraser.
    for (tool, eraser) in [
        (CanvasTool::Lasso, EraserMode::Stroke),
        (CanvasTool::Pen, EraserMode::Stroke),
        (CanvasTool::Eraser, EraserMode::Stroke),
        (CanvasTool::Eraser, EraserMode::Point(EraserSize::Large)),
    ] {
        ui.h.state_mut().canvas.tool = tool;
        ui.h.state_mut().canvas.eraser = eraser;
        ui.steps(1);
        check(&mut ui, &paper, &group, &format!("{tool:?} {eraser:?}"));
    }
    ui.h.state_mut().canvas.eraser = EraserMode::Stroke;

    // Right-clicking inside the frame opens the group's menu, not the paper's.
    ui.h.state_mut().canvas.tool = CanvasTool::Select;
    ui.steps(1);
    let inside = between(&ui);
    ui.right_click(inside);
    assert!(egui::Popup::is_any_open(&ui.h.ctx), "the group menu opened");
    assert!(ui.h.query_by_label_contains("Text box here").is_none());
    // Order ▸ Bring to Front, as in the desktop notebook's menu.
    ui.click_label_containing(&format!("    {}", t!("canvas-menu-order")));
    ui.click_label_containing(&t!("picture-bring-to-front"));
    assert_eq!(ui.selection(), group);
    let zs: Vec<i32> = ui.items().iter().map(|item| item.z).collect();
    assert!(zs.iter().all(|z| *z > 0), "{zs:?}");
    ui.right_click(inside);
    click_menu_row(&mut ui, &t!("common-delete"), &t!("key-del"));
    assert!(ui.items().is_empty(), "the menu deleted the whole group");
}

#[test]
fn erasing_ink_keeps_the_selection_on_the_strokes_it_picked() {
    let mut ui = Ui::start();
    ui.new_page("Erase selected");
    ui.h.state_mut().canvas.tool = CanvasTool::Pen;
    for dy in [-100.0, 0.0, 100.0] {
        let from = ui.empty_spot(-120.0, dy);
        ui.drag(from, from + egui::vec2(240.0, 0.0));
    }
    let ink = ui.ink();
    assert_eq!(ink.len(), 3);
    let (middle, bottom) = (ink[1].clone(), ink[2].clone());

    // Lasso the middle stroke.
    ui.h.state_mut().canvas.tool = CanvasTool::Lasso;
    let centre = ui.empty_spot(0.0, 0.0);
    let path: Vec<Pos2> = (0..=24)
        .map(|i| {
            let angle = i as f32 / 24.0 * std::f32::consts::TAU;
            centre + egui::vec2(angle.cos() * 170.0, angle.sin() * 45.0)
        })
        .collect();
    ui.drag_path(&path);
    assert_eq!(ui.selection().strokes, vec![1]);

    // Drag the eraser across the top stroke: the selection follows the middle one.
    ui.h.state_mut().canvas.tool = CanvasTool::Eraser;
    let top = ui.empty_spot(0.0, -130.0);
    ui.drag(top, top + egui::vec2(0.0, 60.0));
    assert_eq!(ui.ink(), vec![middle.clone(), bottom.clone()]);
    assert_eq!(ui.selection().strokes, vec![0]);

    // Delete takes the selected stroke, not the one after it.
    ui.h.state_mut().canvas.tool = CanvasTool::Select;
    ui.steps(1);
    ui.press(Key::Delete);
    assert_eq!(ui.ink(), vec![bottom.clone()]);

    // Erasing a selected stroke takes it out of the selection.
    ui.press_with(Modifiers::COMMAND, Key::A);
    assert_eq!(ui.selection().strokes, vec![0]);
    ui.h.state_mut().canvas.tool = CanvasTool::Eraser;
    let low = ui.empty_spot(0.0, 70.0);
    ui.drag(low, low + egui::vec2(0.0, 60.0));
    assert!(ui.ink().is_empty());
    assert!(ui.selection().strokes.is_empty());
}

#[test]
fn typing_in_a_table_or_a_shape_label_is_one_undo_step() {
    let mut ui = Ui::start();
    ui.new_page("Sessions");
    // The caret goes into a note, and Insert › Table puts a table at it.
    ui.press(Key::Enter);
    ui.insert_table(2, 2);
    // Past the editor's 0.8 s merge window, so typing gets its own undo step.
    ui.steps(60);
    let inserted = ui.cells();
    // Two cells, several keystrokes each.
    for text in ["Pa", "r", "t"] {
        ui.type_text(text);
    }
    ui.press(Key::Tab);
    for text in ["Jo", "b"] {
        ui.type_text(text);
    }
    assert_eq!(ui.cells()[0], ["Part", "Job"]);
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert_eq!(
        ui.cells(),
        inserted,
        "one undo takes back the whole session"
    );
    ui.press_with(Modifiers::COMMAND, Key::Z);
    let tables = ui
        .items()
        .iter()
        .filter_map(CanvasItem::doc)
        .flat_map(|doc| &doc.blocks)
        .filter(|block| matches!(block.kind, crate::model::BlockKind::Table(_)))
        .count();
    assert_eq!(tables, 0, "the next one takes back the table");
    ui.press(Key::Escape);

    ui.h.state_mut().canvas.tool = CanvasTool::Shape(ShapeKind::Ellipse);
    let from = ui.empty_spot(-150.0, 80.0);
    ui.drag(from, from + egui::vec2(220.0, 100.0));
    let ellipse = ui.items()[0].id;
    let label = |ui: &Ui| match &ui.item(ellipse).kind {
        CanvasKind::Shape { label, .. } => label.clone(),
        other => panic!("expected a shape, got {other:?}"),
    };
    ui.press(Key::Enter);
    for text in ["Nu", "cle", "us"] {
        ui.type_text(text);
    }
    assert_eq!(label(&ui), "Nucleus");
    ui.press(Key::Escape);
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert_eq!(label(&ui), "", "one undo takes back the whole label");
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert!(ui.items().is_empty(), "the next one takes back the shape");
}

#[test]
fn shapes_draw_straight_and_typing_a_label_never_deletes_them() {
    let mut ui = Ui::start();
    ui.new_page("Shapes");
    ui.h.state_mut().canvas.tool = CanvasTool::Shape(ShapeKind::Arrow);
    let from = ui.empty_spot(-150.0, 0.0);
    ui.drag(from, from + egui::vec2(200.0, 0.0));
    let arrow = ui.items()[0].clone();
    assert_eq!(arrow.height, 0.0, "a horizontal arrow stays horizontal");
    assert_eq!(ui.app().canvas.tool, CanvasTool::Select);

    ui.h.state_mut().canvas.tool = CanvasTool::Shape(ShapeKind::Ellipse);
    let from = ui.empty_spot(-150.0, 80.0);
    ui.drag(from, from + egui::vec2(220.0, 100.0));
    let ellipse = ui.items()[1].id;
    ui.press(Key::Enter);
    ui.type_text("Nucleus");
    ui.press(Key::Backspace);
    ui.shot("06-shape-label");
    let item = ui.item(ellipse);
    match &item.kind {
        CanvasKind::Shape { label, .. } => assert_eq!(label, "Nucleu"),
        other => panic!("expected a shape, got {other:?}"),
    }
    assert_eq!(ui.items().len(), 2, "Backspace edited the label only");

    // Escape finishes the label; double-clicking the shape edits it again.
    ui.press(Key::Escape);
    assert_eq!(ui.app().canvas.selection.primary(), Some(ellipse));
    let center = ui.screen(ui.item(ellipse).rect().center());
    ui.double_click(center);
    ui.press(Key::End);
    ui.type_text("s");
    match &ui.item(ellipse).kind {
        CanvasKind::Shape { label, .. } => assert_eq!(label, "Nucleus"),
        other => panic!("expected a shape, got {other:?}"),
    }

    // Enter starts a second line instead of ending the label.
    ui.press(Key::Enter);
    ui.type_text("and protons");
    ui.shot("06-shape-label-two-lines");
    match &ui.item(ellipse).kind {
        CanvasKind::Shape { label, .. } => assert_eq!(label, "Nucleus\nand protons"),
        other => panic!("expected a shape, got {other:?}"),
    }
}

#[test]
fn frise_editor_stays_open_and_saves() {
    let mut ui = Ui::start();
    ui.new_page("History");
    ui.h.state_mut().ribbon.tab = RibbonTab::Insert;
    ui.steps(2);
    ui.ribbon_button("Frise");
    ui.steps(4);
    ui.shot("07-frise-dialog");
    ui.click_label_containing("Add milestone");
    ui.click_label_containing("Save frise");
    let frise = ui
        .items()
        .into_iter()
        .find(|item| matches!(item.kind, CanvasKind::Timeline { .. }))
        .expect("frise");
    match frise.kind {
        CanvasKind::Timeline { milestones, .. } => assert_eq!(milestones.len(), 4),
        _ => unreachable!(),
    }
}

#[test]
fn a_table_block_draws_and_takes_the_caret() {
    use crate::model::{Block, BlockKind, Document, TableBlock};
    let mut ui = Ui::start();
    ui.new_page("Week");
    let note = {
        let page = ui.h.state_mut().page.as_mut().unwrap();
        page.canvas.add(CanvasItem::new(
            CanvasKind::Text {
                doc: Document {
                    blocks: vec![
                        Block::paragraph("Plan"),
                        Block::new(BlockKind::Table(TableBlock::new(2, 2))),
                    ],
                },
            },
            120.0,
            400.0,
            300.0,
            40.0,
        ))
    };
    ui.steps(2);
    // The note grew to hold the table under its paragraph.
    let rect = ui.item(note).rect();
    assert!(rect.height() > 60.0, "{rect:?}");
    // A click in the first cell puts the caret there; typing fills it.
    ui.click(ui.screen(rect.min + egui::vec2(20.0, 40.0)));
    ui.type_text("Mon");
    let item = ui.item(note);
    let table = item
        .doc()
        .and_then(|doc| {
            doc.blocks.iter().find_map(|block| match &block.kind {
                BlockKind::Table(table) => Some(table.clone()),
                _ => None,
            })
        })
        .expect("the table");
    assert_eq!(table.rows[0][0].text.text, "Mon");
    assert_eq!(table.rows[0][1].text.text, "");
    assert_eq!(ui.app().editor.active_cell, Some((0, 0)));
}

#[test]
fn tables_tab_between_cells_and_grow() {
    let mut ui = Ui::start();
    ui.new_page("Cell parts");
    ui.press(Key::Enter);
    ui.insert_table(3, 3);
    ui.steps(2);
    // The first cell has the caret; type in it and tab across the table.
    assert_eq!(ui.app().editor.active_cell, Some((0, 0)));
    ui.type_text("Part");
    ui.press(Key::Tab);
    ui.type_text("Job");
    // From the second cell, seven Tabs reach the last cell and the eighth adds a row.
    for _ in 0..8 {
        ui.press(Key::Tab);
    }
    ui.type_text("Wall");
    ui.shot("08-table");
    let cells = ui.cells();
    assert_eq!(cells[0][..2], ["Part", "Job"]);
    assert_eq!(cells.len(), 4, "tabbing past the last cell adds a row");
    assert_eq!(cells[3][0], "Wall");
    // Typing Backspace in a cell edits the cell, not the table.
    ui.press(Key::Backspace);
    assert_eq!(
        ui.cells()[3][0],
        "Wal",
        "Backspace edits the cell, not the table"
    );
}

#[test]
fn a_table_is_typed_in_like_notebook() {
    let mut ui = Ui::start();
    ui.new_page("Timetable");
    ui.press(Key::Enter);
    ui.type_text("Monday");
    ui.press(Key::Enter);
    ui.insert_table(2, 2);
    assert_eq!(
        ui.app().editor.active_cell,
        Some((0, 0)),
        "the caret goes to the first cell"
    );
    ui.type_cells(&["Day", "Task", "Mon", "Maths"]);
    ui.press(Key::Tab); // the last cell: Tab adds a row
    ui.type_text("Tue");
    assert_eq!(ui.cells(), [["Day", "Task"], ["Mon", "Maths"], ["Tue", ""]]);
    ui.press_with(Modifiers::SHIFT, Key::Tab);
    assert_eq!(ui.app().editor.active_cell, Some((1, 1)));
    // Shift+Tab selected the cell's text: Ctrl+B makes it bold, as in a paragraph.
    ui.press_with(Modifiers::COMMAND, Key::B);
    assert!(ui.table().1.rows[1][1].text.style_at(0).bold);
    // The table sits in the note under the line typed before it, and the note is wide enough.
    let (note, _) = ui.table();
    let item = ui.item(note);
    assert_eq!(item.doc().unwrap().blocks[0].text.text, "Monday");
    let last = ui.cell_rect(1, 1);
    assert!(
        last.right() <= ui.screen(item.rect().right_top()).x + 1.0,
        "the note fits its table"
    );
    ui.shot("p6-table");
}

#[test]
fn enter_in_the_last_cell_adds_a_row_and_again_leaves_the_table() {
    let mut ui = Ui::start();
    ui.new_page("Rows");
    ui.press(Key::Enter);
    // One row of two columns (the helper takes columns first).
    ui.insert_table(2, 1);
    ui.type_cells(&["a", "b"]);
    ui.press(Key::Enter);
    assert_eq!(
        ui.cells().len(),
        2,
        "Enter at the end of the last cell adds a row"
    );
    assert_eq!(ui.app().editor.active_cell, Some((1, 0)));
    ui.press(Key::Enter);
    assert_eq!(
        ui.cells().len(),
        1,
        "Enter in the empty new row takes it back…"
    );
    assert_eq!(ui.app().editor.active_cell, None, "…and leaves the table");
    ui.type_text("after");
    let (note, _) = ui.table();
    let blocks = ui.item(note).doc().unwrap().blocks.clone();
    assert!(matches!(blocks[0].kind, crate::model::BlockKind::Table(_)));
    assert_eq!(blocks[1].text.text, "after");
    assert_eq!(
        blocks.len(),
        2,
        "the caret went to the empty line Insert › Table left, adding none"
    );
    // Up at the start of the line under a table goes into it (Home first, as in the
    // paragraph tests); Ctrl+Enter adds a row below; Ctrl+Alt+R a column to the right.
    ui.press(Key::Home);
    ui.press(Key::ArrowUp);
    assert!(
        ui.app().editor.active_cell.is_some(),
        "Up from the line under a table goes into it"
    );
    ui.press_with(Modifiers::COMMAND, Key::Enter);
    assert_eq!(ui.cells().len(), 2);
    ui.press_with(Modifiers::COMMAND | Modifiers::ALT, Key::R);
    assert_eq!(ui.cells()[0].len(), 3);
}

#[test]
fn alt_enter_and_shift_enter_break_the_line_in_the_last_cell() {
    let mut ui = Ui::start();
    ui.new_page("Lines");
    ui.press(Key::Enter);
    ui.insert_table(1, 1);
    ui.type_text("one");
    // the desktop notebook's Alt+Enter is a paragraph inside the cell, even at the end of the last
    // one; Shift+Enter a line break. Neither adds a row.
    ui.press_with(Modifiers::ALT, Key::Enter);
    ui.type_text("two");
    ui.press_with(Modifiers::SHIFT, Key::Enter);
    ui.type_text("three");
    assert_eq!(ui.cells(), [["one\ntwo\nthree"]]);
    // Plain Enter still adds a row there.
    ui.press(Key::Enter);
    assert_eq!(ui.cells().len(), 2);
}

/// The keyboard focus, as egui holds it.
fn focused_widget(ui: &Ui) -> Option<egui::Id> {
    ui.h.ctx.memory(|memory| memory.focused())
}

#[test]
fn undo_in_a_table_keeps_the_keyboard_in_it() {
    let mut ui = Ui::start();
    ui.new_page("Undo rows");
    ui.press(Key::Enter);
    ui.insert_table(2, 1);
    ui.type_cells(&["a", "b"]);
    // Tab in the last cell of a one-row table adds a column; Ctrl+Z takes it back, and the
    // caret goes back to the cell it came from instead of nowhere.
    ui.press(Key::Tab);
    assert_eq!(ui.cells()[0].len(), 3);
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert_eq!(ui.cells()[0].len(), 2, "Ctrl+Z took the column back");
    assert!(
        focused_widget(&ui).is_some(),
        "the keyboard is still in the table"
    );
    assert_eq!(ui.app().editor.active_cell, Some((0, 1)));
    ui.type_text("x");
    assert_eq!(ui.cells(), [["a", "xb"]], "the typing landed in the table");
    // The same after Ctrl+Enter and Ctrl+Alt+R.
    ui.press_with(Modifiers::COMMAND, Key::Enter);
    assert_eq!(ui.cells().len(), 2);
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert_eq!(ui.cells().len(), 1);
    ui.press_with(Modifiers::COMMAND | Modifiers::ALT, Key::R);
    assert_eq!(ui.cells()[0].len(), 3);
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert_eq!(ui.cells()[0].len(), 2);
    assert!(focused_widget(&ui).is_some());
    ui.type_text("y");
    assert_eq!(ui.cells().concat().concat().matches('y').count(), 1);
}

#[test]
fn undo_after_leaving_a_table_puts_the_caret_back_in_it() {
    use crate::model::{Block, BlockKind, Document, TableBlock};
    let mut ui = Ui::start();
    ui.new_page("Leave");
    ui.press(Key::Escape);
    // A note holding only a table: leaving it with Enter twice adds the line under it.
    let table = TableBlock::from_strings(&[vec!["a".into(), "b".into()]], false, 0.0);
    ui.h.state_mut()
        .page
        .as_mut()
        .unwrap()
        .canvas
        .add(CanvasItem::new(
            CanvasKind::Text {
                doc: Document {
                    blocks: vec![Block::new(BlockKind::Table(table))],
                },
            },
            100.0,
            300.0,
            400.0,
            80.0,
        ));
    ui.steps(3);
    ui.click(ui.cell_rect(0, 1).center());
    ui.press(Key::End);
    ui.press(Key::Enter);
    ui.press(Key::Enter);
    let (note, _) = ui.table();
    assert_eq!(
        ui.item(note).doc().unwrap().blocks.len(),
        2,
        "a line under it"
    );
    assert_eq!(ui.app().editor.active_cell, None);
    // Ctrl+Z takes that line back: the caret goes back to the table's last row, where it
    // was, not to nothing (the note's first block is the table).
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert_eq!(ui.item(note).doc().unwrap().blocks.len(), 1);
    assert!(
        focused_widget(&ui).is_some(),
        "the keyboard is still in the note"
    );
    assert_eq!(ui.app().editor.active_cell, Some((1, 0)));
    ui.type_text("x");
    assert_eq!(ui.cells(), [["a", "b"], ["x", ""]]);
}

#[test]
fn the_table_tab_appears_in_a_table_and_its_commands_work() {
    use crate::table::TableCmd;
    let mut ui = Ui::start();
    ui.new_page("Marks");
    ui.press(Key::Enter);
    assert!(
        ui.h.query_by_role_and_label(egui::Role::Tab, "Table")
            .is_none(),
        "no Table tab outside a table"
    );
    ui.insert_table(2, 2);
    ui.type_cells(&["Name", "Mark", "Zoé", "9"]);
    ui.press(Key::Tab);
    ui.type_cells(&["Adam", "12"]);
    assert!(ui
        .h
        .query_by_role_and_label(egui::Role::Tab, "Table")
        .is_some());
    ui.ribbon_tab("Table");
    ui.shot("p6-table-tab");

    // Header Row, then Sort Ascending by the column holding the caret (the first).
    ui.click(ui.cell_rect(1, 0).center());
    ui.ribbon_button("Sort");
    ui.click_label_containing("Header Row");
    ui.ribbon_button("Sort");
    ui.click_label_containing("Sort Ascending");
    let names: Vec<String> = ui.cells().iter().map(|row| row[0].clone()).collect();
    assert_eq!(names, ["Name", "Adam", "Zoé"], "the header stays on top");

    ui.ribbon_button("Insert Below");
    assert_eq!(ui.cells().len(), 4);
    ui.ribbon_button("Delete Rows");
    assert_eq!(ui.cells().len(), 3);

    ui.h.state_mut().canvas.ribbon_cmd = Some(crate::ribbon::Cmd::TableEdit(TableCmd::Shading(
        Some(crate::table::SHADES[2]),
    )));
    ui.steps(3);
    let (_, table) = ui.table();
    assert!(table
        .rows
        .iter()
        .flatten()
        .any(|cell| cell.shading == Some(crate::table::SHADES[2])));

    ui.ribbon_button("Hide Borders");
    assert!(ui.table().1.hide_borders);

    ui.ribbon_button("Select Columns");
    ui.ribbon_button("Delete Columns");
    assert_eq!(ui.cells()[0].len(), 1, "the selected column went");

    ui.ribbon_button("Delete Table");
    assert!(ui.items().iter().all(|item| item.doc().is_none_or(|doc| doc
        .blocks
        .iter()
        .all(|block| !matches!(block.kind, crate::model::BlockKind::Table(_))))));
    assert!(
        ui.h.query_by_role_and_label(egui::Role::Tab, "Table")
            .is_none(),
        "the tab goes with the table"
    );
}

#[test]
fn column_borders_drag_to_set_a_width() {
    let mut ui = Ui::start();
    ui.new_page("Widths");
    ui.press(Key::Enter);
    // One row of two columns (the helper takes columns first).
    ui.insert_table(2, 1);
    ui.type_text("x");
    assert_eq!(ui.table().1.widths[0], 0.0, "a new column fits its text");
    let border = ui.cell_rect(0, 0).right_center() + egui::vec2(6.0, 0.0); // CELL_PAD.x past the text
    ui.drag(border, border + egui::vec2(80.0, 0.0));
    let width = ui.table().1.widths[0];
    assert!(width >= 110.0, "dragged to about 120 px, got {width}");
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert_eq!(ui.table().1.widths[0], 0.0, "one undo puts it back");
}

#[test]
fn dragging_across_cells_selects_them_and_copy_pastes_a_table() {
    let mut ui = Ui::start();
    ui.new_page("Grid");
    ui.press(Key::Enter);
    ui.insert_table(2, 2);
    ui.type_cells(&["a", "b", "c", "d"]);
    let (from, to) = (ui.cell_rect(0, 0).center(), ui.cell_rect(1, 1).center());
    ui.drag(from, to);
    let (_, _, block) = ui.table_ids();
    assert_eq!(
        ui.app().editor.cell_selection,
        Some((
            block,
            crate::table::CellRange {
                top: 0,
                left: 0,
                bottom: 1,
                right: 1
            }
        ))
    );
    ui.h.event(Event::Copy);
    assert_eq!(ui.copied_now(), ["a\tb\nc\td"]);
    // Pasted into another note, it is a table again.
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    ui.click(ui.empty_paper_point());
    ui.h.event(Event::Paste("a\tb\nc\td".into()));
    ui.steps(3);
    let tables = ui
        .items()
        .iter()
        .filter_map(|item| item.doc().cloned())
        .flat_map(|doc| doc.blocks)
        .filter(|block| matches!(block.kind, crate::model::BlockKind::Table(_)))
        .count();
    assert_eq!(tables, 2);
}

#[test]
fn selected_cells_copy_from_the_ribbon_and_the_cell_menu() {
    let mut ui = Ui::start();
    ui.new_page("Grid copy");
    ui.press(Key::Enter);
    ui.insert_table(2, 2);
    ui.type_cells(&["a", "b", "c", "d"]);
    let (from, to) = (ui.cell_rect(0, 0).center(), ui.cell_rect(1, 1).center());
    ui.drag(from, to);
    assert!(ui.app().editor.cell_selection.is_some());
    // Home › Copy is not greyed with cells selected, and takes them all.
    ui.ribbon_tab("Home");
    assert!(!ui.ribbon_button_disabled("Copy"), "Copy is on for cells");
    assert!(!ui.ribbon_button_disabled("Cut"));
    let outputs = ui.ribbon_button_outputs("Copy");
    assert_eq!(copied(&outputs), ["a\tb\nc\td"]);
    // So does the cell menu's Cut, opened on one of them; one undo brings them back.
    ui.right_click(ui.cell_rect(0, 0).center());
    let outputs = ui.click_label_outputs("Cut");
    assert_eq!(copied(&outputs), ["a\tb\nc\td"]);
    assert_eq!(ui.cells(), [["", ""], ["", ""]], "Cut cleared the cells");
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert_eq!(ui.cells(), [["a", "b"], ["c", "d"]]);
}

#[test]
fn the_table_tab_reads_in_french_and_fits() {
    let mut ui = Ui::start_in(Lang::Fr);
    ui.new_page("Emploi du temps");
    ui.press(Key::Enter);
    ui.insert_table(2, 2);
    ui.ribbon_tab("Tableau");
    for label in [
        "Insérer au-dessus",
        "Insérer en dessous",
        "Insérer à gauche",
        "Insérer à droite",
        "Masquer les bordures",
        "Trame de fond",
        "Trier",
    ] {
        let rect = ui.ribbon_button_rect(label);
        assert!(
            rect.width() > 30.0 && rect.right() <= 1320.0,
            "{label}: {rect:?}"
        );
    }
    ui.shot("fr-p6-table-tab");
}

/// Pastes a small red picture onto the open page and returns its item id.
fn paste_picture(ui: &mut Ui) -> u64 {
    let image = egui::ColorImage::filled([40, 20], egui::Color32::RED);
    ui.h.event(Event::PasteImage(std::sync::Arc::new(image)));
    ui.steps(4);
    ui.items()
        .iter()
        .rev()
        .find(|item| matches!(item.kind, CanvasKind::Image { .. }))
        .expect("a picture")
        .id
}

fn picture_meta(ui: &Ui, id: u64) -> crate::model::PictureMeta {
    match &ui.item(id).kind {
        CanvasKind::Image { meta, .. } => meta.clone(),
        other => panic!("not a picture: {other:?}"),
    }
}

#[test]
fn pictures_turn_flip_and_keep_their_alt_text() {
    let mut ui = Ui::start();
    ui.new_page("Flags");
    ui.press(Key::Escape);
    let id = paste_picture(&mut ui);
    let before = ui.item(id).rect();
    ui.click(ui.screen(before.center()));
    assert!(ui
        .h
        .query_by_role_and_label(egui::Role::Tab, "Picture")
        .is_some());
    ui.ribbon_tab("Picture");
    ui.shot("p6-picture-tab");
    ui.ribbon_button("Rotate");
    ui.click_label_containing("Rotate Right 90°");
    let after = ui.item(id).rect();
    assert_eq!(
        (after.width(), after.height()),
        (before.height(), before.width()),
        "a quarter turn swaps the box"
    );
    assert_eq!(picture_meta(&ui, id).turns, 1);
    ui.ribbon_button("Alt Text");
    ui.shot("p6-alt-text");
    ui.type_text("A red flag");
    ui.click_label("OK");
    assert_eq!(picture_meta(&ui, id).alt, "A red flag");
    let html = {
        let app = ui.app();
        crate::export::canvas_export_html(app.page.as_ref().unwrap(), &app.library)
    };
    assert!(
        html.contains("alt=\"A red flag\""),
        "exports carry the alt text"
    );
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert_eq!(
        picture_meta(&ui, id).alt,
        "",
        "setting the alt text is one undo step"
    );
}

#[test]
fn a_background_picture_stays_put_under_the_notes() {
    let mut ui = Ui::start();
    ui.new_page("Worksheet");
    ui.press(Key::Escape);
    let id = paste_picture(&mut ui);
    let rect = ui.item(id).rect();
    ui.click(ui.screen(rect.center()));
    ui.ribbon_tab("Picture");
    ui.ribbon_button("Set Picture as Background");
    assert!(picture_meta(&ui, id).background);
    assert!(
        ui.selection().is_empty(),
        "a background picture is not selected"
    );
    // A click on it types on the page, as on empty paper.
    let count = ui.items().len();
    ui.click(ui.screen(rect.center()));
    assert_eq!(
        ui.items().len(),
        count + 1,
        "a note starts over the picture"
    );
    ui.type_text("Answer");
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    ui.press_with(Modifiers::COMMAND, Key::A);
    assert!(
        !ui.selection().contains(id),
        "Select All leaves the background alone"
    );
    ui.press(Key::Escape);
    // The page menu over it takes it back off the background.
    ui.right_click(ui.screen(rect.left_top() + egui::vec2(4.0, 4.0)));
    ui.click_label_containing("Set Picture as Background");
    assert!(!picture_meta(&ui, id).background);
}

#[test]
fn a_picture_in_a_note_resizes_from_its_corner_and_deletes() {
    use crate::model::{Block, BlockKind, Document, PictureMeta};
    let mut ui = Ui::start();
    ui.new_page("Diagram");
    ui.press(Key::Escape);
    let note = {
        let page = ui.h.state_mut().page.as_mut().unwrap();
        let image = Block::new(BlockKind::Image {
            attachment: "missing".into(),
            width: 1.0,
            meta: PictureMeta::default(),
        });
        page.canvas.add(CanvasItem::new(
            CanvasKind::Text {
                doc: Document {
                    blocks: vec![Block::paragraph("Cell"), image],
                },
            },
            100.0,
            300.0,
            400.0,
            200.0,
        ))
    };
    ui.steps(3);
    // The picture (a missing file draws a 100-high placeholder) as last drawn.
    let rect = ui
        .app()
        .editor
        .last_picture_rect
        .expect("the picture was drawn");
    ui.click(ui.screen(rect.center()));
    assert!(
        ui.h.query_by_role_and_label(egui::Role::Tab, "Picture")
            .is_some(),
        "the Picture tab follows a picture in a note"
    );
    let rect = ui.app().editor.last_picture_rect.unwrap();
    let corner = ui.screen(rect.right_bottom());
    ui.drag(corner, corner - egui::vec2(150.0, 0.0));
    let width = match &ui.item(note).doc().unwrap().blocks[1].kind {
        BlockKind::Image { width, .. } => *width,
        _ => unreachable!(),
    };
    assert!(width < 0.8, "the corner handle made it smaller: {width}");
    ui.click(ui.screen(ui.app().editor.last_picture_rect.unwrap().center()));
    ui.press(Key::Delete);
    assert_eq!(
        ui.item(note).doc().unwrap().blocks.len(),
        1,
        "Delete removes the clicked picture"
    );
}

/// A note holding a paragraph "Cell" and a picture (a missing file: a placeholder).
fn note_with_picture(ui: &mut Ui) -> u64 {
    use crate::model::{Block, BlockKind, Document, PictureMeta};
    let page = ui.h.state_mut().page.as_mut().unwrap();
    let image = Block::new(BlockKind::Image {
        attachment: "missing".into(),
        width: 1.0,
        meta: PictureMeta::default(),
    });
    let note = page.canvas.add(CanvasItem::new(
        CanvasKind::Text {
            doc: Document {
                blocks: vec![Block::paragraph("Cell"), image],
            },
        },
        100.0,
        300.0,
        400.0,
        200.0,
    ));
    ui.steps(3);
    note
}

fn picture_block(ui: &Ui, note: u64) -> (f32, crate::model::PictureMeta) {
    match &ui.item(note).doc().unwrap().blocks[1].kind {
        crate::model::BlockKind::Image { width, meta, .. } => (*width, meta.clone()),
        other => panic!("not a picture: {other:?}"),
    }
}

#[test]
fn undoing_a_picture_change_in_a_note_keeps_the_typing_before_it() {
    let mut ui = Ui::start();
    ui.new_page("Wall");
    ui.press(Key::Escape);
    let note = note_with_picture(&mut ui);
    let first = ui.item(note).doc().unwrap().blocks[0].id;
    ui.click(ui.text_point(note, first, 4));
    ui.press(Key::End);
    ui.type_text(" wall");
    // The corner drag is one step of its own: Ctrl+Z (the page's, the press took the
    // keyboard from the text) takes back the new size, not the typing with it.
    let rect = ui.app().editor.last_picture_rect.unwrap();
    ui.click(ui.screen(rect.center()));
    let corner = ui.screen(ui.app().editor.last_picture_rect.unwrap().right_bottom());
    ui.drag(corner, corner - egui::vec2(150.0, 0.0));
    assert!(picture_block(&ui, note).0 < 0.8, "the drag made it smaller");
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert_eq!(picture_block(&ui, note).0, 1.0, "Ctrl+Z took the size back");
    assert_eq!(
        block_texts(&ui, note)[0],
        "Cell wall",
        "…and kept the typing"
    );
    // A turn from the Picture tab, taken back by the ribbon's Undo: the same.
    ui.ribbon_tab("Picture");
    ui.ribbon_button("Rotate");
    ui.click_label_containing("Rotate Right 90°");
    assert_eq!(picture_block(&ui, note).1.turns, 1);
    ui.ribbon_tab("Home");
    assert!(!ui.ribbon_button_disabled("Undo"), "Undo can take it back");
    ui.ribbon_button("Undo");
    assert_eq!(
        picture_block(&ui, note).1.turns,
        0,
        "Undo took the turn back"
    );
    assert_eq!(block_texts(&ui, note)[0], "Cell wall");
    // The typing is the step before.
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert_eq!(block_texts(&ui, note)[0], "Cell");
    // Back in the text, its undo never crosses a picture change made since.
    ui.press_with(Modifiers::COMMAND, Key::Y);
    ui.click(ui.text_point(note, first, 4));
    ui.press(Key::End);
    ui.type_text("s");
    let rect = ui.app().editor.last_picture_rect.unwrap();
    ui.click(ui.screen(rect.center()));
    ui.ribbon_tab("Picture");
    ui.ribbon_button("Rotate");
    ui.click_label_containing("Rotate Right 90°");
    ui.click(ui.text_point(note, first, 4));
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert_eq!(
        picture_block(&ui, note).1.turns,
        1,
        "an undo in the text leaves the turn made after the typing"
    );
}

#[test]
fn a_column_dragged_in_another_note_stays_out_of_the_typed_notes_undo() {
    use crate::model::{Block, BlockKind, Document, TableBlock};
    let mut ui = Ui::start();
    ui.new_page("Two notes");
    let spot = ui.empty_spot(-150.0, -100.0);
    ui.click(spot);
    ui.type_text("Hello");
    let typed = focused_note(&ui);
    let table = TableBlock::from_strings(&[vec!["a".into(), "b".into()]], false, 0.0);
    let other =
        ui.h.state_mut()
            .page
            .as_mut()
            .unwrap()
            .canvas
            .add(CanvasItem::new(
                CanvasKind::Text {
                    doc: Document {
                        blocks: vec![Block::new(BlockKind::Table(table)), Block::paragraph("")],
                    },
                },
                100.0,
                420.0,
                400.0,
                80.0,
            ));
    ui.steps(3);
    let border = ui.cell_rect(0, 0).right_center() + egui::vec2(6.0, 0.0);
    ui.drag(border, border + egui::vec2(80.0, 0.0));
    assert!(ui.table().1.widths[0] > 0.0, "the border was dragged");
    // The caret is back in the note typed in; its Ctrl+Z takes back its own typing, and
    // never brings the other note's table into it.
    assert_eq!(focused_note(&ui), typed);
    ui.press_with(Modifiers::COMMAND, Key::Z);
    let blocks = ui.item(typed).doc().unwrap().blocks.clone();
    assert!(
        blocks
            .iter()
            .all(|block| !matches!(block.kind, BlockKind::Table(_))),
        "the typed note got the other note's table: {blocks:?}"
    );
    assert_eq!(ui.table().0, other);
}

/// Selects the last `chars` characters before the caret.
fn select_back(ui: &mut Ui, chars: usize) {
    for _ in 0..chars {
        ui.press_with(Modifiers::SHIFT, Key::ArrowLeft);
    }
}

fn block_texts(ui: &Ui, note: u64) -> Vec<String> {
    ui.item(note)
        .doc()
        .unwrap()
        .blocks
        .iter()
        .map(|block| block.text.text.clone())
        .collect()
}

fn focused_note(ui: &Ui) -> u64 {
    ui.app().canvas.active_text.expect("a note being typed in")
}

/// The Paste Options button under a paste, if it shows: the one below the ribbon (the
/// ribbon's Paste ▾ carries the same name).
fn paste_options(ui: &Ui) -> Option<egui::Rect> {
    let bottom = 48.0 + crate::ribbon::panel_height(&ui.app().ribbon) + 4.0;
    ui.h.query_all_by_label(&t!("paste-options"))
        .map(|node| node.rect())
        .find(|rect| rect.top() > bottom)
}

/// Opens the Paste Options menu under the paste.
fn open_paste_options(ui: &mut Ui) {
    let rect = paste_options(ui).expect("the Paste Options button");
    ui.click(rect.center());
}

/// Enter at the end of bold text carries the bold into the new line, as in the desktop notebook: this
/// turns it off, so the line takes on no formatting a paste would merge with.
fn plain_caret(ui: &mut Ui) {
    ui.press_with(Modifiers::COMMAND, Key::B);
}

#[test]
fn pasting_copied_text_keeps_its_formatting_and_offers_the_options() {
    let mut ui = Ui::start();
    ui.new_page("Paste");
    ui.press(Key::Enter);
    ui.type_text("Hello world");
    select_back(&mut ui, 5);
    ui.press_with(Modifiers::COMMAND, Key::B);
    ui.h.event(Event::Copy);
    assert_eq!(ui.copied_now(), ["world"]);
    ui.press(Key::End);
    ui.press(Key::Enter);
    plain_caret(&mut ui);
    ui.h.event(Event::Paste("world".into()));
    ui.steps(3);
    let note = focused_note(&ui);
    assert_eq!(block_texts(&ui, note), ["Hello world", "world"]);
    let bold = |ui: &Ui| ui.item(note).doc().unwrap().blocks[1].text.style_at(0).bold;
    assert!(bold(&ui), "Ctrl+V keeps the source formatting");
    assert!(
        paste_options(&ui).is_some(),
        "the Paste Options button shows"
    );
    open_paste_options(&mut ui);
    ui.shot("p6-paste-options");
    ui.click_label_containing("Keep Text Only");
    assert!(!bold(&ui));
    open_paste_options(&mut ui);
    ui.click_label_containing("Merge Formatting");
    assert!(bold(&ui), "Merge Formatting keeps bold");
    assert_eq!(
        block_texts(&ui, note),
        ["Hello world", "world"],
        "switching repastes in place"
    );
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert_eq!(
        block_texts(&ui, note)[1],
        "",
        "one undo takes the paste back"
    );
    ui.type_text("x");
    assert!(paste_options(&ui).is_none(), "typing ends the session");
}

#[test]
fn the_paste_options_button_waits_while_the_paste_is_scrolled_away() {
    let mut ui = Ui::start();
    ui.new_page("Scrolled paste");
    ui.press(Key::Enter);
    // The ribbon's Paste ▾ carries the same name: count the buttons so named.
    let named = |ui: &Ui| ui.h.query_all_by_label(&t!("paste-options")).count();
    let ribbon = named(&ui);
    ui.h.event(Event::Paste("one\ntwo".into()));
    ui.steps(3);
    assert_eq!(named(&ui), ribbon + 1, "the button shows under the paste");
    // Scrolled until the pasted lines are above the page in view, the button does not
    // float over the ribbon; it comes back with them.
    let over_page = ui.empty_paper_point();
    ui.wheel(over_page, egui::vec2(0.0, -700.0), Modifiers::NONE);
    let note = focused_note(&ui);
    let last = ui.item(note).doc().unwrap().blocks.last().unwrap().id;
    let line = ui.text_point(note, last, 0);
    assert!(
        line.y < ui.app().canvas.viewport().top(),
        "the paste scrolled out of view"
    );
    assert!(ui.app().editor.paste_session.is_some());
    assert_eq!(named(&ui), ribbon, "no button over the ribbon");
    ui.wheel(over_page, egui::vec2(0.0, 700.0), Modifiers::NONE);
    assert_eq!(named(&ui), ribbon + 1, "back with the paste");
}

#[test]
fn pasting_several_lines_into_a_note_splits_the_line() {
    let mut ui = Ui::start();
    ui.new_page("Lines");
    ui.press(Key::Enter);
    ui.type_text("Start end");
    for _ in 0..3 {
        ui.press(Key::ArrowLeft);
    }
    ui.h.event(Event::Paste("one\ntwo".into()));
    ui.steps(3);
    assert_eq!(block_texts(&ui, focused_note(&ui)), ["Start one", "twoend"]);
}

/// Puts HTML on the test clipboard for the length of a test.
struct TestHtml;

impl TestHtml {
    fn set(html: &str) -> Self {
        crate::clipboard::TEST_HTML.with(|slot| *slot.borrow_mut() = Some(html.to_string()));
        TestHtml
    }
}

impl Drop for TestHtml {
    fn drop(&mut self) {
        crate::clipboard::TEST_HTML.with(|slot| *slot.borrow_mut() = None);
    }
}

#[test]
fn html_from_another_app_pastes_with_its_formatting() {
    use crate::model::BlockKind;
    let _html = TestHtml::set(
        "<p><b>Bold</b> text</p><ul><li>Item</li></ul><table><tr><td>a</td><td>b</td></tr></table>",
    );
    let mut ui = Ui::start();
    ui.new_page("Web");
    ui.press(Key::Enter);
    ui.h.event(Event::Paste("Bold text\n• Item\na\tb".into()));
    ui.steps(3);
    let doc = ui.item(focused_note(&ui)).doc().unwrap().clone();
    assert_eq!(doc.blocks[0].text.text, "Bold text");
    assert!(doc.blocks[0].text.style_at(0).bold);
    assert!(matches!(doc.blocks[1].kind, BlockKind::Bullet));
    assert!(doc
        .blocks
        .iter()
        .any(|block| matches!(block.kind, BlockKind::Table(_))));
}

#[test]
fn ctrl_shift_v_pastes_text_only() {
    let mut ui = Ui::start();
    ui.new_page("Plain");
    ui.press(Key::Enter);
    ui.type_text("Hello world");
    select_back(&mut ui, 5);
    ui.press_with(Modifiers::COMMAND, Key::B);
    ui.h.event(Event::Copy);
    ui.steps(2);
    ui.press(Key::End);
    ui.press(Key::Enter);
    plain_caret(&mut ui);
    ui.with_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, |ui| {
        ui.h.event(Event::Paste("world".into()));
        ui.steps(2);
    });
    let note = focused_note(&ui);
    let block = ui.item(note).doc().unwrap().blocks[1].clone();
    assert_eq!(block.text.text, "world");
    assert!(!block.text.style_at(0).bold, "Ctrl+Shift+V keeps text only");
}

#[test]
fn the_format_painter_paints_once_or_until_escape() {
    let mut ui = Ui::start();
    ui.new_page("Painter");
    ui.press(Key::Enter);
    ui.type_text("Red plain words here");
    ui.press(Key::Home);
    for _ in 0..3 {
        ui.press_with(Modifiers::SHIFT, Key::ArrowRight);
    }
    ui.press_with(Modifiers::COMMAND, Key::B);
    ui.press(Key::ArrowLeft); // caret inside the bold word
    ui.ribbon_tab("Home");
    ui.ribbon_button("Format Painter");
    assert!(ui
        .app()
        .editor
        .painter
        .as_ref()
        .is_some_and(|painter| !painter.sticky));
    let note = focused_note(&ui);
    let paint = |ui: &mut Ui, from: usize, len: usize| {
        ui.press(Key::Home);
        for _ in 0..from {
            ui.press(Key::ArrowRight);
        }
        for _ in 0..len {
            ui.press_with(Modifiers::SHIFT, Key::ArrowRight);
        }
        // A mouse release over the text applies the painter to the selection.
        let rect = ui.item(note).rect();
        let pos = ui.screen(rect.min + egui::vec2(20.0, 14.0));
        ui.h.event(Event::PointerMoved(pos));
        ui.h.event(Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        });
        ui.steps(3);
    };
    paint(&mut ui, 4, 5); // "plain"
    let style_at = |ui: &Ui, at: usize| {
        let text = ui.item(note).doc().unwrap().blocks[0].text.clone();
        text.style_at(text.char_to_byte(at))
    };
    assert!(style_at(&ui, 5).bold, "the painter painted \"plain\"");
    assert!(ui.app().editor.painter.is_none(), "one click paints once");

    let button = ui.ribbon_button_rect("Format Painter").center();
    ui.double_click(button);
    assert!(
        ui.app()
            .editor
            .painter
            .as_ref()
            .is_some_and(|painter| painter.sticky),
        "a double-click keeps it on"
    );
    paint(&mut ui, 10, 5); // "words"
    paint(&mut ui, 16, 4); // "here"
    assert!(style_at(&ui, 11).bold && style_at(&ui, 17).bold);
    assert!(ui.app().editor.painter.is_some());
    ui.press(Key::Escape);
    assert!(ui.app().editor.painter.is_none(), "Esc puts it away");
}

#[test]
fn the_format_painter_paints_a_drag_across_paragraphs() {
    let mut ui = Ui::start();
    ui.new_page("Painter range");
    ui.press(Key::Enter);
    ui.type_text("Bold");
    ui.press(Key::Enter);
    ui.type_text("aaa");
    ui.press(Key::Enter);
    ui.type_text("bbb");
    let note = focused_note(&ui);
    let ids: Vec<u64> = ui.blocks().iter().map(|block| block.id).collect();
    // "Bold" bold and right-aligned, then the painter picks that up.
    ui.click(ui.text_point(note, ids[0], 2));
    ui.press(Key::Home);
    ui.press_with(Modifiers::SHIFT, Key::End);
    ui.press_with(Modifiers::COMMAND, Key::B);
    ui.press_with(Modifiers::COMMAND, Key::R);
    ui.press(Key::ArrowLeft);
    ui.press(Key::ArrowRight); // inside "Bold"
    ui.ribbon_tab("Home");
    ui.ribbon_button("Format Painter");
    assert!(ui.app().editor.painter.is_some());
    ui.drag(
        ui.text_point(note, ids[1], 0),
        ui.text_point(note, ids[2], 3),
    );
    let blocks = ui.blocks();
    for block in &blocks[1..] {
        let text = &block.text;
        assert!(
            (0..3).all(|at| text.style_at(text.char_to_byte(at)).bold),
            "{:?} is painted whole",
            text.text
        );
        assert_eq!(block.align, crate::model::ParaAlign::Right);
    }
    assert!(ui.app().editor.painter.is_none(), "one drag paints once");
    // One undo step takes the painting back.
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert!(!ui.blocks()[2].text.style_at(0).bold);
    assert!(!ui.blocks()[1].text.style_at(0).bold);
}

#[test]
fn ctrl_alt_c_and_ctrl_alt_v_copy_and_paint_formatting() {
    let mut ui = Ui::start();
    ui.new_page("Format keys");
    ui.press(Key::Enter);
    ui.type_text("Bold plain more");
    ui.press(Key::Home);
    for _ in 0..4 {
        ui.press_with(Modifiers::SHIFT, Key::ArrowRight);
    }
    ui.press_with(Modifiers::COMMAND, Key::B);
    ui.press(Key::ArrowLeft);
    ui.press(Key::ArrowRight); // inside "Bold"
    ui.press_with(Modifiers::COMMAND | Modifiers::ALT, Key::C);
    assert!(
        ui.app().editor.painter.is_none(),
        "the keys leave the painter off"
    );
    let note = focused_note(&ui);
    let bold_at = |ui: &Ui, at: usize| {
        let text = ui.item(note).doc().unwrap().blocks[0].text.clone();
        text.style_at(text.char_to_byte(at)).bold
    };
    // "plain" selected, then Ctrl+Alt+V; again on "more" (the copy stays).
    let select = |ui: &mut Ui, from: usize, len: usize| {
        ui.press(Key::Home);
        for _ in 0..from {
            ui.press(Key::ArrowRight);
        }
        for _ in 0..len {
            ui.press_with(Modifiers::SHIFT, Key::ArrowRight);
        }
    };
    select(&mut ui, 5, 5);
    ui.press_with(Modifiers::COMMAND | Modifiers::ALT, Key::V);
    assert!(bold_at(&ui, 6), "Ctrl+Alt+V painted \"plain\"");
    select(&mut ui, 11, 4);
    ui.press_with(Modifiers::COMMAND | Modifiers::ALT, Key::V);
    assert!(bold_at(&ui, 12), "…and again \"more\"");
    assert_eq!(
        ui.item(note).doc().unwrap().blocks[0].text.text,
        "Bold plain more",
        "the keys typed nothing"
    );
}

#[test]
fn pen_draws_and_the_eraser_is_undoable() {
    let mut ui = Ui::start();
    ui.new_page("Ink");
    ui.h.state_mut().canvas.tool = CanvasTool::Pen;
    let from = ui.empty_spot(-150.0, 0.0);
    ui.drag(from, from + egui::vec2(240.0, 0.0));
    assert_eq!(ui.app().page.as_ref().unwrap().canvas.ink.len(), 1);
    let start = ui.ink()[0].points[0];
    assert!(
        ui.screen(egui::pos2(start[0], start[1])).distance(from) < 0.5,
        "the stroke starts where the button went down"
    );
    ui.h.state_mut().canvas.tool = CanvasTool::Eraser;
    // Click the middle of the stroke, away from its sample points.
    ui.click(from + egui::vec2(125.0, 2.0));
    assert!(ui.app().page.as_ref().unwrap().canvas.ink.is_empty());
    ui.press(Key::Escape);
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert_eq!(ui.app().page.as_ref().unwrap().canvas.ink.len(), 1);
}

#[test]
fn the_point_eraser_cuts_a_stroke_in_one_undo_step() {
    let mut ui = Ui::start();
    ui.new_page("Rubbing out");
    ui.h.state_mut().canvas.tool = CanvasTool::Pen;
    let from = ui.empty_spot(-150.0, 0.0);
    ui.drag(from, from + egui::vec2(240.0, 0.0));
    let original = ui.app().page.as_ref().unwrap().canvas.ink.clone();
    assert_eq!(original.len(), 1);
    ui.h.state_mut().canvas.tool = CanvasTool::Eraser;
    ui.h.state_mut().canvas.eraser = EraserMode::Point(EraserSize::Medium);
    let middle = from + egui::vec2(120.0, 0.0);
    ui.drag(
        middle - egui::vec2(0.0, 40.0),
        middle + egui::vec2(0.0, 40.0),
    );
    assert_eq!(
        ui.app().page.as_ref().unwrap().canvas.ink.len(),
        2,
        "rubbing across the middle leaves two pieces"
    );
    ui.press(Key::Escape);
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert_eq!(
        ui.app().page.as_ref().unwrap().canvas.ink,
        original,
        "one undo brings it all back"
    );
}

#[test]
fn ink_to_shape_tidies_a_rough_rectangle() {
    let mut ui = Ui::start();
    ui.new_page("Neat shapes");
    ui.h.state_mut().ribbon.tab = RibbonTab::Draw;
    ui.steps(2);
    ui.ribbon_button("Ink to Shape");
    assert!(ui.app().canvas.ink_to_shape);
    assert_eq!(ui.app().canvas.tool, CanvasTool::Pen);
    let c = ui.empty_spot(0.0, 0.0);
    let corners = [
        c + egui::vec2(-100.0, -60.0),
        c + egui::vec2(100.0, -57.0),
        c + egui::vec2(97.0, 60.0),
        c + egui::vec2(-100.0, 62.0),
        c + egui::vec2(-96.0, -54.0),
    ];
    let mut path = Vec::new();
    for pair in corners.windows(2) {
        for step in 0..8 {
            path.push(pair[0] + (pair[1] - pair[0]) * (step as f32 / 8.0));
        }
    }
    path.push(corners[4]);
    ui.drag_path(&path);
    let page = ui.app().page.as_ref().unwrap();
    assert!(page.canvas.ink.is_empty(), "the stroke became a shape");
    let shapes: Vec<_> = page
        .canvas
        .items
        .iter()
        .filter(|item| {
            matches!(
                item.kind,
                CanvasKind::Shape {
                    shape: ShapeKind::Rect,
                    fill: None,
                    ..
                }
            )
        })
        .collect();
    assert_eq!(shapes.len(), 1);
    assert!(shapes[0].width > shapes[0].height && shapes[0].height > 60.0);
    ui.press(Key::Escape);
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert!(ui.items().is_empty(), "the first undo removes the shape…");
    assert_eq!(
        ui.app().page.as_ref().unwrap().canvas.ink.len(),
        1,
        "…and brings the ink back"
    );
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert!(ui.app().page.as_ref().unwrap().canvas.ink.is_empty());

    // The highlighter's strokes stay ink, whatever they look like (the gallery's yellow
    // highlighter).
    ui.pick_pen(4);
    assert_eq!(ui.app().canvas.tool, CanvasTool::Highlighter);
    ui.drag_path(&path);
    assert_eq!(ui.ink().len(), 1);
    assert!(ui.ink()[0].highlighter);
    assert!(ui.items().is_empty(), "no shape");
}

#[test]
fn a_shape_drawn_around_a_note_leaves_its_text_clickable() {
    let mut ui = Ui::start();
    ui.new_page("Circled");
    ui.click(ui.empty_spot(0.0, 0.0));
    ui.type_text("circled words");
    ui.press(Key::Escape);
    let note = ui.note_with("circled words");
    let area = egui::Rect::from_min_max(ui.screen(note.rect().min), ui.screen(note.rect().max));
    {
        let canvas = &mut ui.h.state_mut().canvas;
        canvas.tool = CanvasTool::Pen;
        canvas.ink_to_shape = true;
        canvas.ink_width = 6.0;
    }
    ui.steps(1);
    // A loop round the note, a little past where it started.
    let (center, rx, ry) = (
        area.center(),
        area.width() / 2.0 + 40.0,
        area.height() / 2.0 + 40.0,
    );
    let path: Vec<Pos2> = (0..=48)
        .map(|i| {
            let a = i as f32 / 48.0 * 1.04 * std::f32::consts::TAU;
            center + egui::vec2(rx * a.cos(), ry * a.sin())
        })
        .collect();
    ui.drag_path(&path);
    assert!(ui.ink().is_empty(), "the loop became a shape");
    let circle = ui
        .items()
        .into_iter()
        .find(|item| item.id != note.id)
        .expect("a shape");
    assert!(
        matches!(
            circle.kind,
            CanvasKind::Shape {
                shape: ShapeKind::Ellipse,
                fill: None,
                thickness: Some(width),
                ..
            } if width == 6.0
        ),
        "an empty ellipse as thick as the pen: {:?}",
        circle.kind
    );
    assert!(circle.z > note.z, "on top of the note");
    ui.shot("ink-to-shape-around-a-note");

    // With the Type tool, a click on the words puts the caret in the note…
    ui.h.state_mut().canvas.tool = CanvasTool::Select;
    ui.steps(1);
    ui.click(ui.text_spot(&note));
    assert_eq!(ui.app().canvas.active_text, Some(note.id));
    assert_eq!(ui.selection().items, vec![note.id]);
    // …and a click on the outline picks the shape up.
    ui.click(ui.screen(circle.rect().left_center()));
    assert_eq!(ui.app().canvas.active_text, None);
    assert_eq!(ui.selection().items, vec![circle.id]);
}

#[test]
fn the_eraser_size_menu_picks_a_point_eraser() {
    let mut ui = Ui::start();
    ui.new_page("Erasers");
    ui.h.state_mut().ribbon.tab = RibbonTab::Draw;
    ui.steps(2);
    assert_eq!(ui.app().canvas.eraser, EraserMode::Stroke);
    ui.ribbon_button("Eraser size");
    ui.click_label_containing("Small eraser");
    assert_eq!(ui.app().canvas.eraser, EraserMode::Point(EraserSize::Small));
    assert_eq!(ui.app().canvas.tool, CanvasTool::Eraser);
}

#[test]
fn rubbing_out_the_same_spot_again_adds_no_undo_step() {
    let mut ui = Ui::start();
    ui.new_page("Rubbing twice");
    ui.h.state_mut().canvas.tool = CanvasTool::Pen;
    let from = ui.empty_spot(-150.0, 0.0);
    ui.drag(from, from + egui::vec2(240.0, 0.0));
    let depth = ui.app().canvas.undo_depth();
    ui.h.state_mut().canvas.tool = CanvasTool::Eraser;
    ui.h.state_mut().canvas.eraser = EraserMode::Point(EraserSize::Medium);
    let spot = from + egui::vec2(120.0, 0.0);
    ui.click(spot);
    assert_eq!(ui.ink().len(), 2, "the click cuts the stroke in two");
    assert_eq!(ui.app().canvas.undo_depth(), depth + 1);
    let cut = ui.ink();
    ui.click(spot);
    assert_eq!(ui.ink(), cut, "nothing is left there to rub out");
    assert_eq!(
        ui.app().canvas.undo_depth(),
        depth + 1,
        "so there is no undo step that changes nothing"
    );
    // Holding the eraser still in a drag changes nothing either.
    ui.drag(spot, spot);
    assert_eq!(ui.ink(), cut);
    assert_eq!(ui.app().canvas.undo_depth(), depth + 1);
}

#[test]
fn pasting_text_on_the_page_makes_a_note_and_items_copy_paste() {
    let mut ui = Ui::start();
    ui.new_page("Clipboard");
    ui.press(Key::Escape);
    let spot = ui.empty_spot(0.0, 0.0);
    ui.click(spot);
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    assert!(ui.items().is_empty());
    ui.h.event(Event::Paste("First line\nSecond line".into()));
    ui.steps(3);
    let items = ui.items();
    assert_eq!(items.len(), 1);
    assert_eq!(text_of(&items[0]), "First line\nSecond line");

    ui.press(Key::Escape);
    ui.h.event(Event::Copy);
    ui.steps(2);
    let copied = ui.h.output().platform_output.clone();
    let _ = copied;
    ui.h.event(Event::Paste("First line\nSecond line".into()));
    ui.steps(3);
    assert_eq!(ui.items().len(), 2, "pasting the copied note duplicates it");
}

/// The tabs every gallery shoots, in the desktop notebook's order (File is a menu, shot on its own).
const GALLERY_TABS: [RibbonTab; 7] = [
    RibbonTab::Home,
    RibbonTab::Insert,
    RibbonTab::Draw,
    RibbonTab::History,
    RibbonTab::Review,
    RibbonTab::View,
    RibbonTab::Help,
];

/// Readies a gallery shot: no pointer, so nothing is hovered, and no toast (they last
/// 2.6 s of real time, which the shots outrun).
fn settle(ui: &mut Ui) {
    ui.h.state_mut().toast = super::Toast::default();
    ui.h.event(Event::PointerGone);
    ui.steps(2);
}

/// Picks the open page's colour (`None` is No Color) from View › Page Color, shooting the
/// menu open as `shot` when given. The View tab must be showing.
fn pick_page_color(ui: &mut Ui, color: Option<crate::model::PageColor>, shot: Option<&str>) {
    ui.ribbon_button(&t!("ribbon-page-color"));
    if let Some(name) = shot {
        ui.shot(name);
    }
    let label = color.map_or_else(|| t!("ribbon-no-color"), |color| color.label());
    ui.click_label(&label);
    assert_eq!(ui.app().page.as_ref().unwrap().color, color);
}

/// Picks the open page's rule lines from View › Rule Lines, shooting the menu open as
/// `shot` when given. The View tab must be showing.
fn pick_rule_lines(ui: &mut Ui, rule: crate::model::RuleLines, shot: Option<&str>) {
    ui.ribbon_button(&t!("ribbon-rule-lines"));
    if let Some(name) = shot {
        ui.shot(name);
    }
    ui.click_label(&rule.label());
    assert_eq!(ui.app().page.as_ref().unwrap().rule, rule);
}

/// the desktop notebook's page (P3), named `{prefix}-…`, for the gates to set beside the desktop notebook's: a new
/// page titled `title` with `note` typed under it (`-page-header`), View's Page Color and
/// Rule Lines menus open, a lavender college-ruled page and a blue medium grid, the page
/// date's calendar, the page with its title hidden, and a long page scrolled halfway down
/// with both scrollbars (`-long-page`). Each style is undone after its shot, the long page
/// is scrolled back to its top, and it ends on the sample's page again, on Home. Returns
/// the new page's id.
fn page_shots(ui: &mut Ui, prefix: &str, title: &str, note: &str) -> String {
    use crate::canvas::PAGE_LEFT;
    use crate::model::{Block, Document, GridSize, PageColor, RuleLines, RuleSize};
    let sample = ui.app().page.as_ref().expect("a page is open").id.clone();

    // The title, the rule and the date and time on the page, and a note under them.
    ui.new_page(title);
    ui.press(Key::Enter);
    ui.type_text(note);
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    save(ui);
    let page = ui.app().page.as_ref().unwrap().id.clone();
    assert!(ui.selection().is_empty() && !ui.app().editor.title_focused);
    settle(ui);
    ui.shot(&format!("{prefix}-page-header"));

    // View › Page Setup: each menu open, then a ruled page and a grid page.
    ui.ribbon_tab(&RibbonTab::View.label());
    pick_page_color(
        ui,
        Some(PageColor::Lavender),
        Some(&format!("{prefix}-page-color-menu")),
    );
    pick_rule_lines(
        ui,
        RuleLines::Lines(RuleSize::College),
        Some(&format!("{prefix}-rule-lines-menu")),
    );
    settle(ui);
    ui.shot(&format!("{prefix}-ruled-page"));
    pick_page_color(ui, Some(PageColor::Blue), None);
    pick_rule_lines(ui, RuleLines::Grid(GridSize::Medium), None);
    settle(ui);
    ui.shot(&format!("{prefix}-grid-page"));
    pick_page_color(ui, None, None);
    pick_rule_lines(ui, RuleLines::None, None);

    // The page date's calendar.
    let date = t!("canvas-date-tip");
    ui.click_label(&date);
    settle(ui);
    assert!(ui
        .h
        .query_by_label(&t!("canvas-calendar-previous"))
        .is_some());
    ui.shot(&format!("{prefix}-date-picker"));
    ui.press(Key::Escape);
    assert!(ui
        .h
        .query_by_label(&t!("canvas-calendar-previous"))
        .is_none());

    // View › Hide Page Title, then shown again.
    ui.ribbon_button(&t!("ribbon-hide-page-title"));
    assert!(ui.app().page.as_ref().unwrap().title_hidden);
    settle(ui);
    ui.shot(&format!("{prefix}-title-hidden"));
    ui.ribbon_button(&t!("ribbon-hide-page-title"));
    assert!(!ui.app().page.as_ref().unwrap().title_hidden);
    assert!(ui.h.query_by_label(&date).is_some());

    // A note far down the page, and the view halfway to it: the page reaches its content
    // plus one view, so the vertical scrollbar shows. Nothing reaches past the view's right
    // edge, so there is no bar along the bottom, as in the desktop notebook.
    ui.h.state_mut().ribbon.tab = RibbonTab::Home;
    let far = CanvasItem::new(
        CanvasKind::Text {
            doc: Document {
                blocks: vec![Block::paragraph(note)],
            },
        },
        PAGE_LEFT,
        3000.0,
        400.0,
        40.0,
    );
    ui.h.state_mut().page.as_mut().unwrap().canvas.add(far);
    ui.steps(2);
    let bottom = ui
        .items()
        .iter()
        .map(|item| item.rect().bottom())
        .fold(0.0, f32::max);
    let size = ui.app().canvas.scene_rect.expect("a view").size();
    ui.h.state_mut().canvas.scene_rect = Some(egui::Rect::from_min_size(
        egui::pos2(0.0, bottom / 2.0),
        size,
    ));
    settle(ui);
    let top = ui.app().canvas.scene_rect.unwrap().top();
    assert!((top - bottom / 2.0).abs() < 1.0, "halfway down: {top}");
    assert!(ui
        .h
        .query_by_label(&t!("canvas-scrollbar-vertical"))
        .is_some());
    assert!(ui
        .h
        .query_by_label(&t!("canvas-scrollbar-horizontal"))
        .is_none());
    ui.shot(&format!("{prefix}-long-page"));
    ui.h.state_mut().canvas.scene_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size));
    ui.steps(2);
    save(ui);

    // Back to the sample's page.
    ui.h.state_mut().open_page(&sample, true);
    ui.steps(1);
    settle(ui);
    page
}

/// The shell's shots (C8), named `{prefix}-…`, for the gates to set beside the desktop notebook's: each
/// band at 1320 px (`-tab-home`…) and at 980 and 1620 (`-980-home`…), Backstage, the
/// Simplified ribbon, Show tabs only (closed and open), Full Page View, the sort menu, the
/// hidden navigation pane, the Table tab, and the Recycle Bin with a deleted page open.
/// `draft` names the page the last two make; it ends in the bin, so the section's list is
/// as it was. Starts and ends on the sample's page, at 1320 px, on Home.
fn shell_shots(ui: &mut Ui, prefix: &str, draft: &str) {
    use egui::Role;
    let sample = ui.app().page.as_ref().expect("a page is open").id.clone();
    settle(ui);
    for width in [1320.0, 980.0, 1620.0] {
        ui.h.set_size(egui::vec2(width, 900.0));
        ui.steps(3);
        for tab in GALLERY_TABS {
            ui.h.state_mut().ribbon.tab = tab;
            ui.steps(2);
            let name = format!("{tab:?}").to_lowercase();
            if width == 1320.0 {
                ui.shot(&format!("{prefix}-tab-{name}"));
            } else {
                ui.shot(&format!("{prefix}-{width}-{name}"));
            }
        }
    }
    ui.h.set_size(egui::vec2(1320.0, 900.0));
    ui.h.state_mut().ribbon.tab = RibbonTab::Home;
    ui.steps(3);

    // File: Backstage over the window, as in the desktop notebook; Esc leaves it. (The shot keeps the
    // name it had when File was a menu.)
    ui.ribbon_tab(&RibbonTab::File.label());
    assert_eq!(ui.app().ribbon.tab, RibbonTab::Home, "File is not a band");
    assert!(ui.app().backstage.is_some(), "File opens Backstage");
    ui.shot(&format!("{prefix}-file-menu"));
    ui.press(Key::Escape);
    assert!(ui.app().backstage.is_none(), "Esc leaves Backstage");

    // The Simplified ribbon, then Classic again.
    ui.ribbon_button(&t!("ribbon-display-options"));
    ui.click_label(&t!("ribbon-simplified"));
    settle(ui);
    ui.shot(&format!("{prefix}-simplified"));
    ui.ribbon_button(&t!("ribbon-display-options"));
    ui.click_label(&t!("ribbon-classic"));
    settle(ui);

    // Show tabs only (Ctrl+F1): the tabs alone, then Home's band over the page.
    ui.press_with(Modifiers::COMMAND, Key::F1);
    assert!(ui.app().ribbon.collapsed);
    ui.shot(&format!("{prefix}-tabs-only"));
    ui.ribbon_tab(&RibbonTab::Home.label());
    assert!(ui.app().ribbon.overlay);
    ui.shot(&format!("{prefix}-tabs-only-open"));
    ui.press_with(Modifiers::COMMAND, Key::F1);
    assert!(!ui.app().ribbon.collapsed);
    settle(ui);

    // Full Page View (F11).
    ui.press(Key::F11);
    assert!(ui.app().full_page);
    ui.shot(&format!("{prefix}-full-page"));
    ui.press(Key::Escape);
    assert!(!ui.app().full_page);

    // The page list's sort menu.
    ui.click_label(&t!("app-sort-pages"));
    ui.shot(&format!("{prefix}-sort-menu"));
    ui.press(Key::Escape);

    // ≡ hides the navigation pane; the page list's own ≡ brings it back.
    ui.click_label(&t!("app-hide-navigation"));
    assert!(!ui.app().show_library);
    settle(ui);
    ui.shot(&format!("{prefix}-nav-hidden"));
    ui.click_label(&t!("app-show-navigation"));
    assert!(ui.app().show_library);

    // A new table, selected: the Table tab. (Saved first, so the list shows the title.)
    ui.new_page(draft);
    ui.press(Key::Escape);
    save(ui);
    ui.insert_table(3, 3);
    ui.ribbon_tab(&RibbonTab::Table.label());
    assert_eq!(ui.app().ribbon.tab, RibbonTab::Table);
    settle(ui);
    ui.shot(&format!("{prefix}-table-tab"));
    save(ui);

    // That page deleted (its row's Delete sends it to the bin), then opened from History ›
    // Notebook Recycle Bin.
    let row = ui.h.get_by_role_and_label(Role::ListItem, draft).rect();
    ui.right_click(row.center());
    click_menu_row(ui, &t!("common-delete"), "");
    ui.ribbon_tab(&RibbonTab::History.label());
    ui.ribbon_button(&t!("app-recycle-bin"));
    assert_eq!(ui.app().view, super::View::Trash);
    let row = ui.h.get_by_role_and_label(Role::ListItem, draft).rect();
    ui.click(row.center());
    assert!(ui.app().page.as_ref().is_some_and(|page| page.trashed));
    settle(ui);
    ui.shot(&format!("{prefix}-recycle-bin"));

    // Back to the sample's page, on Home.
    ui.h.state_mut().open_page(&sample, true);
    ui.h.state_mut().ribbon.tab = RibbonTab::Home;
    ui.steps(1);
    settle(ui);
    assert_eq!(ui.app().view, super::View::Section);
}

/// Closes the ribbon menu open with Esc.
fn close_menu(ui: &mut Ui) {
    ui.press(Key::Escape);
    assert!(!egui::Popup::is_any_open(&ui.h.ctx), "Esc closed the menu");
}

/// Opens the ribbon menu of the control named `label` and shoots it as `name`.
fn menu_shot(ui: &mut Ui, label: &str, name: &str) {
    ui.ribbon_button(label);
    assert!(
        egui::Popup::is_any_open(&ui.h.ctx),
        "{label:?} opened its menu"
    );
    ui.shot(name);
    close_menu(ui);
}

/// Home, Insert and Draw as P5 fills them, and what Insert puts in a note (P5), named
/// `{prefix}-…`, for the gates to set beside the desktop notebook's: the three bands at 1320 and 980 px
/// while a note is typed in (`-home-1320`…), Home's paste options, marker libraries and
/// Styles menu, Insert's table grid (4 × 3 hovered), Page Templates and Symbol menus,
/// Draw's pen settings, Add Pen and Rotate menus, the note holding a picture, a file, a
/// recording and an equation under its first line (`-blocks`), Insert Space's guide line,
/// and a page made from the Lecture notes template. Typed on a page of its own titled
/// `title`, whose note starts with `line`; ends on the page it started from, at 1320 px, on
/// Home.
fn p5_shots(ui: &mut Ui, prefix: &str, title: &str, line: &str) {
    use crate::model::ShapeKind;
    use crate::templates::PageTemplate;
    let start = ui.app().page.as_ref().expect("a page is open").id.clone();
    ui.new_page(title);
    ui.press(Key::Enter);
    ui.type_text(line);
    let note = ui.items()[0].id;
    // Esc closes a menu and may leave the note too: a click at the end of its first line
    // puts the caret back.
    let type_in = |ui: &mut Ui| {
        if ui.app().canvas.active_text != Some(note) {
            let first = ui.item(note).doc().expect("a note").blocks[0].clone();
            ui.click(ui.text_point(note, first.id, first.text.char_count()));
        }
        assert_eq!(
            ui.app().canvas.active_text,
            Some(note),
            "typing in the note"
        );
    };

    // The three bands, at the default width and the app's narrowest.
    for width in [1320.0, 980.0] {
        ui.h.set_size(egui::vec2(width, 900.0));
        ui.steps(3);
        for tab in [RibbonTab::Home, RibbonTab::Insert, RibbonTab::Draw] {
            ui.h.state_mut().ribbon.tab = tab;
            settle(ui);
            type_in(ui);
            let name = format!("{tab:?}").to_lowercase();
            ui.shot(&format!("{prefix}-{name}-{width}"));
        }
    }
    ui.h.set_size(egui::vec2(1320.0, 900.0));
    ui.h.state_mut().ribbon.tab = RibbonTab::Home;
    ui.steps(3);

    // Home: the paste options, the bullet and numbering libraries, every style. The last
    // three only open while typing. Each opens from its arrow (Paste's is named Paste
    // Options, as the desktop notebook's), or from the one button a gallery folds to where the band is
    // short of room (French Styles).
    for (control, name) in [
        (t!("ribbon-paste"), "paste-menu"),
        (t!("ribbon-bullets"), "bullets"),
        (t!("ribbon-numbering"), "numbering"),
        (t!("ribbon-group-styles"), "styles"),
    ] {
        type_in(ui);
        let arrow = if name == "paste-menu" {
            t!("paste-options").into_owned()
        } else {
            format!("{control} ▾")
        };
        let shown =
            ui.h.query_all_by_role_and_label(egui::Role::Button, &arrow)
                .next()
                .is_some();
        let label = if shown { arrow } else { control.into_owned() };
        menu_shot(ui, &label, &format!("{prefix}-{name}"));
    }

    // Insert: the table grid with 4 × 3 hovered, the page templates and the symbols.
    type_in(ui);
    ui.ribbon_tab(&RibbonTab::Insert.label());
    ui.ribbon_button(&t!("ribbon-table"));
    let cell = ui.h.get_by_label("4 × 3").rect().center();
    ui.h.event(Event::PointerMoved(cell));
    ui.steps(2);
    assert!(
        ui.h.query_all_by_label_contains("4 × 3").count() > 1,
        "the grid names the size hovered"
    );
    ui.shot(&format!("{prefix}-table-grid"));
    close_menu(ui);
    for (control, name) in [
        (t!("ribbon-page-templates"), "templates"),
        (t!("ribbon-symbol"), "symbols"),
    ] {
        type_in(ui);
        menu_shot(ui, &control, &format!("{prefix}-{name}"));
    }

    // The note's blocks: under its first line, a picture, a file, a recording (silence:
    // tests have no microphone) and an equation, each put in at the caret.
    type_in(ui);
    let picture = ui._dir.path().join("sky.png");
    image::RgbaImage::from_fn(240, 150, |x, y| {
        let (x, y) = (x as f32 / 240.0, y as f32 / 150.0);
        let sun = (x - 0.72).hypot(y - 0.3) < 0.12;
        if sun {
            image::Rgba([255, 214, 90, 255])
        } else if y > 0.78 {
            image::Rgba([70, 140, 80, 255])
        } else {
            image::Rgba([
                (90.0 + 80.0 * y) as u8,
                (150.0 + 60.0 * y) as u8,
                (230.0 - 20.0 * x) as u8,
                255,
            ])
        }
    })
    .save(&picture)
    .expect("write the picture");
    ui.queue_pick(picture);
    ui.ribbon_button(&t!("ribbon-picture"));
    ui.queue_file("notes.txt", &[b'x'; 2048]);
    ui.ribbon_button(&t!("ribbon-file"));
    ui.ribbon_button(&t!("ribbon-audio"));
    ui.ribbon_tab(&RibbonTab::Recording.label());
    ui.ribbon_button(&t!("ribbon-stop"));
    ui.ribbon_tab(&RibbonTab::Insert.label());
    ui.ribbon_button(&t!("ribbon-equation"));
    ui.type_text("x = \\frac{-b \\pm \\sqrt{b^2 - 4ac}}{2a}");
    ui.press(Key::Enter);
    let blocks = ui.item(note).doc().expect("a note").blocks.clone();
    assert_eq!(
        outline_of(&blocks)[..5],
        [
            line.to_owned(),
            "[picture]".to_owned(),
            "[file notes.txt 2048]".to_owned(),
            "[recording]".to_owned(),
            "[equation x = \\frac{-b \\pm \\sqrt{b^2 - 4ac}}{2a}]".to_owned(),
        ],
        "{blocks:#?}"
    );
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    settle(ui);
    ui.shot(&format!("{prefix}-blocks"));

    // Insert Space's guide, under the note.
    ui.ribbon_button(&t!("ribbon-insert-space"));
    assert_eq!(ui.app().canvas.tool, CanvasTool::InsertSpace);
    let visible = ui.app().canvas.visible().expect("the page was drawn");
    let rect = ui.item(note).rect();
    let below = egui::pos2(
        rect.left() + 120.0,
        (rect.bottom() + 40.0).min(visible.bottom() - 60.0),
    );
    ui.h.event(Event::PointerMoved(ui.screen(below)));
    ui.steps(2);
    ui.shot(&format!("{prefix}-insert-space"));
    ui.h.state_mut().canvas.tool = CanvasTool::Select;
    ui.steps(1);

    // Draw: the red pen's settings, Add Pen, and Rotate for a rectangle just drawn.
    ui.pick_pen(2);
    menu_shot(
        ui,
        &t!("ribbon-pen-settings"),
        &format!("{prefix}-pen-settings"),
    );
    menu_shot(ui, &t!("ribbon-add-pen"), &format!("{prefix}-add-pen"));
    ui.ribbon_button(&ShapeKind::Rect.label());
    let from = ui.empty_paper_point();
    ui.drag(from, from + egui::vec2(140.0, 90.0));
    assert!(
        ui.app().canvas.selection.primary().is_some(),
        "the rectangle drawn is selected"
    );
    menu_shot(ui, &t!("ribbon-rotate"), &format!("{prefix}-rotate"));
    ui.press(Key::Escape);
    ui.h.state_mut().canvas.tool = CanvasTool::Select;
    save(ui);

    // Insert › Page Templates › Lecture notes.
    let pages = ui.app().pages.len();
    ui.ribbon_tab(&RibbonTab::Insert.label());
    ui.ribbon_button(&t!("ribbon-page-templates"));
    let lecture = PageTemplate::LectureNotes;
    ui.click_label(&menu_row_label(lecture.glyph(), &lecture.label()));
    assert_eq!(ui.app().pages.len(), pages + 1, "a page from the template");
    assert_eq!(ui.items().len(), 1, "the template's note");
    settle(ui);
    ui.shot(&format!("{prefix}-template-page"));
    save(ui);

    // Back to the page it started from.
    ui.h.state_mut().open_page(&start, true);
    ui.h.state_mut().ribbon.tab = RibbonTab::Home;
    ui.steps(1);
    settle(ui);
}

/// What `p6_shots` types, in one language: the page's title, the line above the table, the
/// table's nine cells row by row (a header row, then two days), the picture's alt text,
/// and the word at the start of `line` that is copied and pasted.
struct P6Words {
    title: &'static str,
    line: &'static str,
    cells: [&'static str; 9],
    alt: &'static str,
    word: &'static str,
}

/// Tables, pictures and paste options as P6 makes them, named `{prefix}-p6-…`, for the
/// gates to set beside the desktop notebook's: a 3 × 3 timetable under a line of text in one note, its
/// header row on and shaded, the caret in its second row (`-p6-table`, on Home), the same
/// on the Table tab (`-p6-table-tab`) and with its Shading menu open
/// (`-p6-table-shading`); a picture pasted on the page and turned right on the Picture tab
/// (`-p6-picture-tab`), then its Alt Text dialog filled in (`-p6-alt-text`); last, the
/// line's first word made bold, copied and pasted on a new line under it, with the Paste
/// Options menu open (`-p6-paste-options`). Typed on a page of its own; ends on the page
/// it started from, on Home.
fn p6_shots(ui: &mut Ui, prefix: &str, words: &P6Words) {
    use crate::ribbon::Cmd;
    use crate::table::{TableCmd, SHADES};
    let start = ui.app().page.as_ref().expect("a page is open").id.clone();
    ui.new_page(words.title);
    ui.press(Key::Enter);
    ui.type_text(words.line);
    ui.press(Key::Enter);
    ui.insert_table(3, 3);
    ui.type_cells(&words.cells);

    // The header row: Sort › Header Row from a cell in it, then the whole row shaded;
    // then the caret in the second row's middle cell.
    let table_tab = t!("ribbon-tab-table").into_owned();
    ui.click(ui.cell_rect(0, 0).center());
    ui.ribbon_tab(&table_tab);
    ui.ribbon_button(&t!("table-sort"));
    ui.click_label_containing(&t!("table-header-row"));
    ui.ribbon_button(&t!("table-select-rows"));
    ui.h.state_mut().canvas.ribbon_cmd = Some(Cmd::TableEdit(TableCmd::Shading(Some(SHADES[2]))));
    ui.steps(3);
    let (note, table) = ui.table();
    assert!(
        table.header
            && table.rows[0]
                .iter()
                .all(|cell| cell.shading == Some(SHADES[2])),
        "{table:#?}"
    );
    let typed: Vec<&str> = words.cells.to_vec();
    assert_eq!(ui.cells().concat(), typed, "every cell typed");
    ui.click(ui.cell_rect(1, 1).center());
    assert_eq!(ui.app().editor.active_cell, Some((1, 1)));
    ui.h.state_mut().ribbon.tab = RibbonTab::Home;
    settle(ui);
    ui.shot(&format!("{prefix}-p6-table"));
    ui.ribbon_tab(&table_tab);
    settle(ui);
    ui.shot(&format!("{prefix}-p6-table-tab"));
    menu_shot(
        ui,
        &t!("table-shading"),
        &format!("{prefix}-p6-table-shading"),
    );

    // A picture pasted on the page, selected and turned right, then its alt text.
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    let picture = paste_picture(ui);
    ui.click(ui.screen(ui.item(picture).rect().center()));
    assert_eq!(ui.app().canvas.selection.primary(), Some(picture));
    ui.ribbon_tab(&t!("ribbon-tab-picture"));
    ui.ribbon_button(&t!("picture-rotate"));
    ui.click_label_containing(&t!("ribbon-rotate-right"));
    assert_eq!(picture_meta(ui, picture).turns, 1);
    settle(ui);
    ui.shot(&format!("{prefix}-p6-picture-tab"));
    ui.ribbon_button(&t!("picture-alt-text"));
    ui.type_text(words.alt);
    settle(ui);
    ui.shot(&format!("{prefix}-p6-alt-text"));
    ui.click_label(&t!("common-ok"));
    assert_eq!(picture_meta(ui, picture).alt, words.alt);

    // The line's first word made bold and copied, then pasted on a new line under it.
    let line = ui.item(note).doc().expect("a note").blocks[0].clone();
    assert_eq!(line.text.text, words.line);
    let chars = words.word.chars().count();
    ui.click(ui.text_point(note, line.id, chars));
    assert_eq!(
        ui.app().canvas.active_text,
        Some(note),
        "typing in the note"
    );
    select_back(ui, chars);
    ui.press_with(Modifiers::COMMAND, Key::B);
    ui.h.event(Event::Copy);
    ui.steps(2);
    ui.press(Key::End);
    ui.press(Key::Enter);
    ui.h.event(Event::Paste(words.word.into()));
    ui.steps(3);
    assert_eq!(block_texts(ui, note)[..2], [words.line, words.word]);
    assert!(
        ui.item(note).doc().expect("a note").blocks[1]
            .text
            .style_at(0)
            .bold,
        "the paste kept the bold"
    );
    open_paste_options(ui);
    settle(ui);
    ui.shot(&format!("{prefix}-p6-paste-options"));
    close_menu(ui);
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    save(ui);

    // Back to the page it started from.
    ui.h.state_mut().open_page(&start, true);
    ui.h.state_mut().ribbon.tab = RibbonTab::Home;
    ui.steps(1);
    settle(ui);
}

/// What `p7_shots` types and looks for, in the gallery's language: the title of a page of
/// its own, its note's first line (spelled right), a second line whose first misspelling
/// is `misspelled` in the dictionary `language`, and a word the sample's pages hold, to
/// search for.
struct P7Words {
    title: &'static str,
    line: &'static str,
    second: &'static str,
    misspelled: &'static str,
    language: &'static str,
    search: &'static str,
}

/// A notebook's name, by id.
fn notebook_name(ui: &Ui, id: &str) -> String {
    ui.app()
        .workspace
        .notebooks
        .iter()
        .find(|notebook| notebook.id == id)
        .unwrap_or_else(|| panic!("no notebook {id}"))
        .name
        .clone()
}

/// Ticks or unticks Options › Display › "Move page list to the left" (Horizontal Tabs).
fn set_page_list_left(ui: &mut Ui, on: bool) {
    ui.h.state_mut().dialog = Some(super::Dialog::Settings);
    ui.steps(3);
    // By their place in the dialog: the View tab is "Affichage" too in French.
    click_in_dialog(ui, &t!("app-options-display"));
    ui.click_label(&t!("app-options-page-list-left"));
    assert_eq!(ui.app().page_list_left, on);
    click_in_dialog(ui, &t!("app-options-general"));
    ui.click_label(&t!("common-ok"));
    assert!(ui.app().dialog.is_none(), "OK closes Options");
}

/// Backstage, Options, the History, Review, View and Help tabs, the desktop notebook's menus, the Move
/// or Copy and Merge dialogs, the search box and Horizontal Tabs as P7 makes them, named
/// `{prefix}-…`, for the gates to set beside the desktop notebook's:
/// - each Backstage page (`-backstage-info`…) and each Options page (`-options-general`…);
/// - History's versions pane on a page saved twice, the Recent Edits menu, and its Today
///   results in the docked pane;
/// - Review's Spelling pane on a misspelled word, its suggestions loaded;
/// - View's Tabs Layout menu and the page under Switch Background, then Help › About;
/// - the right-click menus of a notebook, a section (Section Color › open), a page row, a
///   note, selected text (the mini toolbar) and a picture pasted on the page;
/// - Move or Copy for that page and Merge for a section, each with a section picked;
/// - the search dropdown and its scope menu;
/// - Horizontal Tabs at 1320 and 980 px, with the page list on the left, the notebook
///   picker open, and the notebooks pinned as a pane.
///
/// Typed on a page of its own; ends on the page it started from, in Vertical Tabs, at
/// 1320 px, on Home.
fn p7_shots(ui: &mut Ui, prefix: &str, words: &P7Words) {
    use super::{BackstagePage, OptionsPage, TaskPane};
    use egui::Role;
    let start = ui.app().page.as_ref().expect("a page is open").id.clone();
    // Every save keeps a version, so the versions pane has some to list.
    ui.h.state_mut().library.set_version_interval_ms(0);
    ui.new_page(words.title);
    save(ui);
    ui.press(Key::Enter);
    ui.type_text(words.line);
    save(ui);
    ui.press(Key::Enter);
    ui.type_text(words.second);
    save(ui);
    let note = ui.items()[0].id;
    let page = ui.app().page.as_ref().unwrap().id.clone();
    assert_eq!(block_texts(ui, note), [words.line, words.second]);
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    settle(ui);

    // File: each Backstage page, then Options over it, each page in turn.
    ui.ribbon_tab(&RibbonTab::File.label());
    for each in BackstagePage::ALL {
        let tab = ui.h.get_by_role_and_label(Role::Tab, &each.label()).rect();
        ui.click(tab.center());
        assert_eq!(
            ui.app().backstage.as_ref().map(|backstage| backstage.page),
            Some(each)
        );
        settle(ui);
        let name = format!("{each:?}").to_lowercase();
        ui.shot(&format!("{prefix}-backstage-{name}"));
    }
    ui.click_label(&t!("app-backstage-options"));
    assert!(matches!(ui.app().dialog, Some(super::Dialog::Settings)));
    for each in OptionsPage::ALL {
        ui.options_page(&each.label());
        assert_eq!(ui.app().options_page, each);
        settle(ui);
        let name = format!("{each:?}").to_lowercase();
        ui.shot(&format!("{prefix}-options-{name}"));
    }
    // Options opens on General next time, as the other galleries' shots expect.
    ui.options_page(&OptionsPage::General.label());
    ui.click_label(&t!("common-ok"));
    assert!(ui.app().dialog.is_none(), "OK closes Options");
    assert!(
        ui.app().backstage.is_some(),
        "Backstage stays behind Options"
    );
    ui.press(Key::Escape);
    assert!(ui.app().backstage.is_none(), "Esc leaves Backstage");

    // History: the page's versions, the Recent Edits menu, then Today's edits.
    ui.ribbon_tab(&RibbonTab::History.label());
    ui.ribbon_button(&t!("common-page-versions"));
    assert!(
        matches!(&ui.app().task_pane, Some(TaskPane::Versions(pane))
            if pane.page_id == page && !pane.versions.is_empty()),
        "the versions pane lists the page's versions"
    );
    settle(ui);
    ui.shot(&format!("{prefix}-history-versions-pane"));
    ui.ribbon_button(&t!("ribbon-recent-edits"));
    assert!(egui::Popup::is_any_open(&ui.h.ctx), "Recent Edits' menu");
    settle(ui);
    assert!(egui::Popup::is_any_open(&ui.h.ctx), "the menu stays open");
    ui.shot(&format!("{prefix}-history-recent-edits"));
    click_named(ui, &t!("ribbon-recent-today"));
    assert!(
        matches!(&ui.app().task_pane, Some(TaskPane::Results(pane))
            if pane.hits.iter().any(|hit| hit.page_id == page)),
        "today's edits list the page"
    );
    settle(ui);
    ui.shot(&format!("{prefix}-history-results-pane"));
    ui.h.state_mut().close_task_pane();
    ui.steps(2);

    // Review › Spelling, on the second line's misspelling with its suggestions shown; then
    // the gallery's own spelling setting again.
    let setting = ui.app().editor.spell.setting().to_string();
    ui.h.state_mut().editor.spell.set_language(words.language);
    ui.h.state_mut().editor.spell.wait_until_ready();
    ui.ribbon_tab(&RibbonTab::Review.label());
    ui.ribbon_button(&t!("ribbon-spelling"));
    let hit = match &ui.app().task_pane {
        Some(TaskPane::Spelling(pane)) => pane.current.clone().expect("a misspelling"),
        _ => panic!("the Spelling pane is not open"),
    };
    assert_eq!(hit.word, words.misspelled);
    let suggestions =
        ui.h.state_mut()
            .editor
            .spell
            .wait_for_suggestions_in(&hit.word, hit.language);
    assert!(!suggestions.is_empty(), "suggestions for {:?}", hit.word);
    ui.steps(3);
    settle(ui);
    ui.shot(&format!("{prefix}-review-spelling-pane"));
    ui.h.state_mut().close_task_pane();
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    ui.h.state_mut().editor.spell.set_language(&setting);
    ui.steps(2);

    // View: the Tabs Layout menu, then the page in the other theme's colours and back.
    ui.ribbon_tab(&RibbonTab::View.label());
    ui.ribbon_button(&t!("ribbon-tabs-layout"));
    settle(ui);
    assert!(egui::Popup::is_any_open(&ui.h.ctx), "Tabs Layout's menu");
    ui.shot(&format!("{prefix}-view-tabs-layout-menu"));
    close_menu(ui);
    ui.ribbon_button(&t!("ribbon-switch-background"));
    assert!(ui.app().switch_background);
    settle(ui);
    ui.shot(&format!("{prefix}-view-switch-background"));
    ui.ribbon_button(&t!("ribbon-switch-background"));
    assert!(!ui.app().switch_background);

    // Help › About Folio.
    ui.ribbon_tab(&RibbonTab::Help.label());
    ui.ribbon_button(&t!("ribbon-about"));
    assert!(matches!(ui.app().dialog, Some(super::Dialog::About)));
    settle(ui);
    ui.shot(&format!("{prefix}-help-about"));
    ui.click_label(&t!("common-ok"));
    assert!(ui.app().dialog.is_none());
    ui.h.state_mut().ribbon.tab = RibbonTab::Home;
    ui.steps(2);

    // The menus: a notebook's, a section's with Section Color › open, the page's row's.
    let school = notebook_name(ui, "notebook-school");
    let row = ui.h.get_by_role_and_label(Role::Button, &school).rect();
    ui.right_click(row.center());
    assert!(
        menu_item_rect(ui, &t!("app-move-up")).is_some(),
        "the notebook's menu"
    );
    settle(ui);
    assert!(egui::Popup::is_any_open(&ui.h.ctx), "the menu stays open");
    ui.shot(&format!("{prefix}-menu-notebook"));
    close_menu(ui);
    let history = section_of(ui, "section-classes").expect("History").name;
    let row = ui.h.get_by_role_and_label(Role::Button, &history).rect();
    ui.right_click(row.center());
    ui.click_label_containing(&format!("    {}", t!("app-section-color")));
    ui.h.get_by_role_and_label(Role::Button, &t!("app-color-none"));
    ui.shot(&format!("{prefix}-menu-section"));
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    assert!(!egui::Popup::is_any_open(&ui.h.ctx), "Esc closed the menu");
    let row =
        ui.h.get_by_role_and_label(Role::ListItem, words.title)
            .rect();
    ui.right_click(row.center());
    assert!(
        menu_row_rect(ui, &t!("app-move-or-copy"), &ctrl_alt_hint(Key::M)).is_some(),
        "the row's menu"
    );
    settle(ui);
    assert!(egui::Popup::is_any_open(&ui.h.ctx), "the menu stays open");
    ui.shot(&format!("{prefix}-menu-page-row"));
    close_menu(ui);

    // A note's (on its grab bar), then the text menu with its mini toolbar over the first
    // line, selected.
    let first = ui.item(note).doc().expect("a note").blocks[0].id;
    let end = words.line.chars().count();
    ui.click(ui.text_point(note, first, end));
    assert_eq!(ui.app().canvas.active_text, Some(note));
    ui.right_click(ui.bar(note));
    assert!(
        menu_row_rect(ui, &t!("common-cut"), &ctrl_hint(Key::X)).is_some(),
        "the note's menu"
    );
    settle(ui);
    assert!(egui::Popup::is_any_open(&ui.h.ctx), "the menu stays open");
    ui.shot(&format!("{prefix}-menu-note"));
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    assert!(!egui::Popup::is_any_open(&ui.h.ctx), "Esc closed the menu");
    ui.click(ui.text_point(note, first, end));
    select_back(ui, end);
    ui.right_click(ui.text_point(note, first, end / 2));
    assert!(
        ui.h.query_all_by_label_contains(&format!("    {}", t!("common-cut")))
            .next()
            .is_some(),
        "the text menu"
    );
    settle(ui);
    assert!(egui::Popup::is_any_open(&ui.h.ctx), "the menu stays open");
    ui.shot(&format!("{prefix}-menu-text"));
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    assert!(!egui::Popup::is_any_open(&ui.h.ctx), "Esc closed the menu");

    // A picture pasted on the page (the sample's pages have none).
    let picture = paste_picture(ui);
    let rect = ui.item(picture).rect();
    ui.right_click(ui.screen(rect.center()));
    assert!(
        ui.h.query_all_by_label_contains(&format!("    {}", t!("canvas-menu-order")))
            .next()
            .is_some(),
        "the picture's menu"
    );
    settle(ui);
    assert!(egui::Popup::is_any_open(&ui.h.ctx), "the menu stays open");
    ui.shot(&format!("{prefix}-menu-picture"));
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    assert!(!egui::Popup::is_any_open(&ui.h.ctx), "Esc closed the menu");
    save(ui);

    // Move or Copy for the page (Ctrl+Alt+M), Science picked; Merge for Maths, Science
    // picked.
    ui.h.state_mut().open_page(&page, false);
    ui.steps(2);
    ui.press_with(Modifiers::COMMAND | Modifiers::ALT, Key::M);
    assert!(matches!(
        &ui.app().dialog,
        Some(super::Dialog::MoveOrCopy { .. })
    ));
    ui.steps(2);
    let science = section_of(ui, "section-science").expect("Science").name;
    click_in_dialog(ui, &science);
    settle(ui);
    ui.shot(&format!("{prefix}-dialog-move-or-copy"));
    ui.h.state_mut().dialog = None;
    ui.steps(2);
    let maths = section_of(ui, "section-maths").expect("Maths").name;
    let row = ui.h.get_by_role_and_label(Role::Button, &maths).rect();
    ui.right_click(row.center());
    click_menu_row(ui, &t!("app-merge-section"), "");
    ui.steps(2);
    click_in_dialog(ui, &science);
    settle(ui);
    ui.shot(&format!("{prefix}-dialog-merge"));
    ui.h.state_mut().dialog = None;
    ui.steps(2);
    assert!(section_of(ui, "section-maths").is_some(), "nothing merged");

    // The search box (Ctrl+E): its dropdown, then its scope menu.
    ui.h.state_mut().open_page(&start, true);
    ui.steps(2);
    ui.press_with(Modifiers::COMMAND, Key::E);
    assert!(ui.app().search.open);
    ui.type_text(words.search);
    ui.steps(3);
    assert!(
        !ui.app().search.results.is_empty(),
        "{:?} is found",
        words.search
    );
    settle(ui);
    ui.shot(&format!("{prefix}-search-dropdown"));
    ui.click_label(&t!("app-search-scope"));
    settle(ui);
    assert!(egui::Popup::is_any_open(&ui.h.ctx), "the menu stays open");
    ui.shot(&format!("{prefix}-search-scope-menu"));
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    assert!(!ui.app().search.open, "Esc closed the search");

    // Horizontal Tabs at 1320 and 980 px, the page list on the left, the notebook picker
    // open, the notebooks pinned; then Vertical Tabs again.
    horizontal_tabs(ui);
    ui.h.state_mut().ribbon.tab = RibbonTab::Home;
    ui.steps(2);
    for width in [1320.0, 980.0] {
        ui.h.set_size(egui::vec2(width, 900.0));
        ui.steps(4);
        settle(ui);
        ui.shot(&format!("{prefix}-horizontal-{width}"));
    }
    ui.h.set_size(egui::vec2(1320.0, 900.0));
    ui.steps(4);
    set_page_list_left(ui, true);
    settle(ui);
    // The list meets the page on its left, wide enough for "Add page" whole.
    let add =
        ui.h.get_by_role_and_label(Role::Button, &t!("app-add-page"))
            .rect();
    let page_left = ui.app().canvas.viewport().left();
    assert!(add.right() <= page_left, "{add:?} is left of the page");
    let label = t!("app-add-page").into_owned();
    assert!(
        texts_in(ui, add).iter().any(|(text, _)| *text == label),
        "{label:?} is cut: {:?}",
        texts_in(ui, add)
    );
    ui.shot(&format!("{prefix}-horizontal-page-list-left"));
    set_page_list_left(ui, false);
    let picker =
        ui.h.get_by_role_and_label(Role::Button, &t!("app-notebook-picker"))
            .rect();
    ui.click(picker.center());
    settle(ui);
    assert!(egui::Popup::is_any_open(&ui.h.ctx), "the menu stays open");
    ui.shot(&format!("{prefix}-horizontal-picker"));
    ui.click_label(&t!("app-pin-notebooks"));
    assert!(ui.app().notebooks_pinned);
    settle(ui);
    ui.shot(&format!("{prefix}-horizontal-pinned"));
    ui.click_label(&t!("app-unpin-notebooks"));
    assert!(!ui.app().notebooks_pinned);
    ui.ribbon_tab(&RibbonTab::View.label());
    ui.ribbon_button(&t!("ribbon-tabs-layout"));
    click_named(ui, &t!("ribbon-vertical-tabs"));
    assert_eq!(ui.app().tabs_layout, crate::ribbon::TabsLayout::Vertical);

    // Back to the page it started from.
    ui.h.state_mut().open_page(&start, true);
    ui.h.state_mut().ribbon.tab = RibbonTab::Home;
    ui.steps(2);
    settle(ui);
}

/// S1's look, named `{prefix}-…`, for the gates to set beside the desktop notebook's: Backstage › Info
/// with its coloured column, Horizontal Tabs with the search box at the end of the tab row,
/// Draw with the second pen in hand and then its menu (`-draw-pen-menu`), Home at 1320
/// (Styles previews) and at 980 px (`-home-980`: Tags folded into one button), the
/// Simplified ribbon (Styles labelled), the page list's sort menu (ticked at the left),
/// Home › Paste ▾ open (Paste pressed, its chevron on a line of its own), a word selected
/// with the mouse under its Mini Toolbar (`-selection-bar`), and the Mini Toolbar's card
/// over the text menu. The pointer leaves before each shot that needs no hover. Starts
/// and ends on the sample's My week, at 1320 px, on Home, with the Select tool.
fn s1_shots(ui: &mut Ui, prefix: &str) {
    use super::BackstagePage;
    ui.h.state_mut().open_page("page-school-week", true);
    ui.h.state_mut().ribbon.tab = RibbonTab::Home;
    ui.steps(3);
    settle(ui);

    // File: Backstage › Info.
    ui.ribbon_tab(&RibbonTab::File.label());
    assert_eq!(
        ui.app().backstage.as_ref().map(|backstage| backstage.page),
        Some(BackstagePage::Info)
    );
    settle(ui);
    ui.shot(&format!("{prefix}-backstage"));
    ui.press(Key::Escape);
    assert!(ui.app().backstage.is_none(), "Esc leaves Backstage");

    // Horizontal Tabs on the sample, then Vertical Tabs again.
    horizontal_tabs(ui);
    ui.h.state_mut().ribbon.tab = RibbonTab::Home;
    ui.steps(2);
    settle(ui);
    ui.shot(&format!("{prefix}-horizontal-search"));
    ui.ribbon_tab(&RibbonTab::View.label());
    ui.ribbon_button(&t!("ribbon-tabs-layout"));
    click_named(ui, &t!("ribbon-vertical-tabs"));
    assert_eq!(ui.app().tabs_layout, crate::ribbon::TabsLayout::Vertical);

    // Draw: the second pen in hand, then clicked again for its menu.
    ui.pick_pen(1);
    settle(ui);
    ui.shot(&format!("{prefix}-draw-pens"));
    let pen = crate::canvas::pen_label(&ui.app().canvas.pens[1]);
    ui.ribbon_button(&pen);
    assert!(egui::Popup::is_any_open(&ui.h.ctx), "the pen's menu");
    settle(ui);
    ui.shot(&format!("{prefix}-draw-pen-menu"));
    close_menu(ui);
    ui.h.state_mut().canvas.tool = CanvasTool::Select;
    ui.h.state_mut().ribbon.tab = RibbonTab::Home;
    ui.steps(2);

    // Home at 1320 and 980 px.
    for (width, name) in [(1320.0, "home-styles"), (980.0, "home-980")] {
        ui.h.set_size(egui::vec2(width, 900.0));
        ui.steps(3);
        settle(ui);
        ui.shot(&format!("{prefix}-{name}"));
    }
    ui.h.set_size(egui::vec2(1320.0, 900.0));
    ui.steps(3);

    // The Simplified ribbon, then Classic again.
    ui.ribbon_button(&t!("ribbon-display-options"));
    ui.click_label(&t!("ribbon-simplified"));
    settle(ui);
    ui.shot(&format!("{prefix}-simplified"));
    ui.ribbon_button(&t!("ribbon-display-options"));
    ui.click_label(&t!("ribbon-classic"));
    settle(ui);

    // The page list's sort menu.
    ui.click_label(&t!("app-sort-pages"));
    assert!(egui::Popup::is_any_open(&ui.h.ctx), "the sort menu");
    settle(ui);
    ui.shot(&format!("{prefix}-sort-menu"));
    close_menu(ui);

    // Home › Paste ▾ (named Paste Options, as the desktop notebook's).
    ui.ribbon_button(&t!("paste-options"));
    assert!(egui::Popup::is_any_open(&ui.h.ctx), "Paste's menu");
    settle(ui);
    ui.shot(&format!("{prefix}-paste-menu"));
    close_menu(ui);

    // A word of the first note's second line, selected with the mouse: the Mini Toolbar
    // shows over it, faded with the pointer where the drag ended. The note is clicked
    // into first, and the drag waits so it is not taken for a double-click.
    let note = ui.items()[0].clone();
    let line = note.doc().expect("a note").blocks[1].clone();
    let chars: Vec<char> = line.text.text.chars().collect();
    let start = chars.iter().position(|c| *c == ' ').expect("two words") + 1;
    let end = chars[start..]
        .iter()
        .position(|c| !c.is_alphanumeric())
        .map_or(chars.len(), |len| start + len);
    ui.click(ui.text_point(note.id, line.id, chars.len()));
    ui.steps(30);
    ui.drag(
        ui.text_point(note.id, line.id, start),
        ui.text_point(note.id, line.id, end),
    );
    ui.steps(2);
    assert_eq!(ui.app().editor.selection, Some(start..end));
    assert!(ui.app().editor.mini_bar.is_some(), "the Mini Toolbar shows");
    ui.h.state_mut().toast = super::Toast::default();
    ui.shot(&format!("{prefix}-selection-bar"));

    // The text menu on that word: the Mini Toolbar's card above it.
    ui.right_click(ui.text_point(note.id, line.id, start + 1));
    assert!(egui::Popup::is_any_open(&ui.h.ctx), "the text menu");
    settle(ui);
    ui.shot(&format!("{prefix}-text-menu"));
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    assert!(!egui::Popup::is_any_open(&ui.h.ctx), "Esc closed the menu");
    ui.h.state_mut().open_page("page-school-week", true);
    ui.steps(2);
    settle(ui);
}

/// S4's galleries and dialogs, named `{prefix}-…`, typed on a page of their own titled
/// `title` whose note reads `word`: Home's Font Color and Text Highlight Color menus and
/// the font's More Colors…, the pen's menu with a recent colour and the custom thickness,
/// Insert's Equation and Symbol menus and More Symbols…, a page picture being cropped, and
/// Options with OK and Cancel. Ends on the page it started from, on Home.
fn s4_shots(ui: &mut Ui, prefix: &str, title: &str, word: &str) {
    ui.new_page(title);
    ui.press(Key::Enter);
    ui.type_text(word);
    ui.press_with(Modifiers::SHIFT, Key::Home);
    ui.h.state_mut().ribbon.tab = RibbonTab::Home;
    ui.steps(2);

    // Home: Font Color ▾, Text Highlight Color ▾ and the font's More Colors….
    for (name, shot) in [
        (t!("editor-text-color"), "font-color-menu"),
        (t!("editor-highlight"), "highlight-menu"),
    ] {
        // A second click on its ▾ closes each menu.
        let arrow = format!("{name} ▾");
        ui.steps(30);
        ui.ribbon_button(&arrow);
        assert!(egui::Popup::is_any_open(&ui.h.ctx), "{arrow:?} opened");
        settle(ui);
        ui.shot(&format!("{prefix}-{shot}"));
        ui.steps(30);
        ui.ribbon_button(&arrow);
        assert!(!egui::Popup::is_any_open(&ui.h.ctx), "{arrow:?} closed");
    }
    ui.steps(30);
    ui.ribbon_button(&format!("{} ▾", t!("editor-text-color")));
    ui.click_label(&format!(
        "{}    {}",
        crate::icons::PALETTE,
        t!("ribbon-more-colors")
    ));
    assert!(matches!(
        ui.app().dialog,
        Some(super::Dialog::MoreColors { .. })
    ));
    settle(ui);
    ui.shot(&format!("{prefix}-more-colors"));
    // The dialog's Cancel: French names Undo "Annuler" too.
    click_in_dialog(ui, &t!("common-cancel"));
    assert!(ui.app().dialog.is_none());

    // Draw: the pen in hand clicked again, with a colour of its own in Recent Colors and
    // the custom thickness open.
    ui.h.state_mut().canvas.recent_colors = vec![Rgb(0x12, 0x34, 0x56)];
    ui.pick_pen(1);
    let pen = crate::canvas::pen_label(&ui.app().canvas.pens[1]);
    ui.ribbon_button(&pen);
    // The slider stays open from one opening of the menu to the next.
    if ui.h.query_by_role(egui::Role::Slider).is_none() {
        let custom =
            ui.h.get_all_by_role_and_label(egui::Role::Button, &t!("ribbon-custom-thickness"))
                .last()
                .expect("the custom thickness")
                .rect();
        ui.click(custom.center());
    }
    settle(ui);
    ui.shot(&format!("{prefix}-pen-menu"));
    close_menu(ui);
    ui.h.state_mut().canvas.tool = CanvasTool::Select;

    // Insert: Equation ▾, then Symbol ▾ with recent symbols and More Symbols….
    ui.ribbon_tab(&RibbonTab::Insert.label());
    ui.steps(30);
    menu_shot(
        ui,
        &format!("{} ▾", t!("ribbon-equation")),
        &format!("{prefix}-equation-gallery"),
    );
    ui.h.state_mut().recent_symbols = vec!['€', 'π', '→'];
    ui.steps(30);
    menu_shot(ui, &t!("ribbon-symbol"), &format!("{prefix}-symbol-menu"));
    ui.steps(30);
    ui.ribbon_button(&t!("ribbon-symbol"));
    ui.click_label_containing(&t!("editor-more-symbols"));
    ui.steps(3);
    ui.type_text("delta");
    settle(ui);
    ui.shot(&format!("{prefix}-more-symbols"));
    ui.h.state_mut().dialog = None;
    ui.steps(2);

    // A picture on the page being cropped: the left handle drawn in a quarter.
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    let pixels: Vec<u8> = (0..100)
        .flat_map(|_| {
            (0..200).flat_map(|x| {
                if x < 100 {
                    [0xED, 0x7D, 0x31, 255]
                } else {
                    [0x44, 0x72, 0xC4, 255]
                }
            })
        })
        .collect();
    let image = egui::ColorImage::from_rgba_unmultiplied([200, 100], &pixels);
    ui.h.event(Event::PasteImage(std::sync::Arc::new(image)));
    ui.steps(4);
    let picture = ui
        .items()
        .iter()
        .rev()
        .find(|item| matches!(item.kind, CanvasKind::Image { .. }))
        .expect("a picture")
        .rect();
    ui.click(ui.screen(picture.center()));
    ui.ribbon_tab(&RibbonTab::Picture.label());
    ui.steps(30);
    ui.ribbon_button(&t!("picture-crop"));
    assert!(ui.app().canvas.cropping.is_some());
    let left = egui::pos2(picture.left(), picture.center().y);
    ui.drag(ui.screen(left), ui.screen(left + egui::vec2(50.0, 0.0)));
    settle(ui);
    ui.shot(&format!("{prefix}-crop"));
    ui.press(Key::Escape);
    assert!(ui.app().canvas.cropping.is_none());

    // Options, with OK and Cancel.
    ui.h.state_mut().dialog = Some(super::Dialog::Settings);
    ui.steps(3);
    settle(ui);
    ui.shot(&format!("{prefix}-options"));
    click_in_dialog(ui, &t!("common-cancel"));
    assert!(ui.app().dialog.is_none());

    ui.h.state_mut().open_page("page-school-week", true);
    ui.h.state_mut().ribbon.tab = RibbonTab::Home;
    ui.steps(2);
    settle(ui);
}

/// S5's menus, dialogs and panes, named `{prefix}-…`, on a page of its own titled `title`
/// with a numbered list of `items`: the text menu of a numbered item, Home's Numbering and
/// Paragraph Alignment menus, Customize Numbering…, Paragraph Spacing Options…, the Page
/// Versions pane with Delete Version, and the Recycle Bin holding a deleted section (put
/// back after). Ends on the sample's page, on Home.
fn s5_shots(ui: &mut Ui, prefix: &str, title: &str, items: [&str; 3]) {
    ui.h.state_mut().library.set_version_interval_ms(0);
    ui.new_page(title);
    ui.press_with(Modifiers::COMMAND, Key::S);
    ui.press(Key::Enter);
    ui.type_text("1. ");
    ui.type_text(items[0]);
    ui.press_with(Modifiers::COMMAND, Key::S);
    ui.press(Key::Enter);
    ui.type_text(items[1]);
    ui.press_with(Modifiers::COMMAND, Key::S);
    ui.press(Key::Enter);
    ui.type_text(items[2]);
    ui.press_with(Modifiers::COMMAND, Key::S);
    ui.h.state_mut().ribbon.tab = RibbonTab::Home;
    ui.steps(2);

    // The second item's text menu: Restart Numbering, Continue Numbering and Copy Link to
    // Paragraph.
    let note = ui.items()[0].id;
    let second = ui.blocks()[1].id;
    ui.right_click(ui.text_point(note, second, 2));
    settle(ui);
    ui.shot(&format!("{prefix}-text-menu-numbered"));
    close_menu(ui);

    // Home: Numbering ▾ and Paragraph Alignment ▾, each with its new last row.
    ui.steps(30);
    menu_shot(
        ui,
        &format!("{} ▾", t!("ribbon-numbering")),
        &format!("{prefix}-numbering-menu"),
    );
    ui.steps(30);
    menu_shot(
        ui,
        &t!("editor-paragraph-alignment"),
        &format!("{prefix}-alignment-menu"),
    );

    // Customize Numbering… and Paragraph Spacing Options…, each closed with Cancel.
    for (menu, row, shot) in [
        (
            format!("{} ▾", t!("ribbon-numbering")),
            t!("ribbon-customize-numbering"),
            "customize-numbering",
        ),
        (
            t!("editor-paragraph-alignment").into_owned(),
            t!("editor-paragraph-spacing"),
            "paragraph-spacing",
        ),
    ] {
        ui.steps(30);
        ui.ribbon_button(&menu);
        ui.click_label_containing(&row);
        ui.steps(3);
        assert!(ui.app().dialog.is_some(), "{row:?} opened its dialog");
        settle(ui);
        ui.shot(&format!("{prefix}-{shot}"));
        click_in_dialog(ui, &t!("common-cancel"));
        assert!(ui.app().dialog.is_none());
    }

    // History › Page Versions: the versions kept while the list was typed.
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    ui.ribbon_tab(&RibbonTab::History.label());
    ui.ribbon_button(&t!("common-page-versions"));
    assert!(matches!(
        ui.app().task_pane,
        Some(super::TaskPane::Versions(_))
    ));
    settle(ui);
    ui.shot(&format!("{prefix}-versions"));
    ui.steps(30);
    ui.ribbon_button(&t!("common-page-versions"));

    // History › Notebook Recycle Bin, with a deleted section above the deleted pages.
    ui.h.state_mut()
        .library
        .trash_section("section-science")
        .unwrap();
    ui.h.state_mut().refresh_workspace();
    ui.steps(30);
    ui.ribbon_button(&t!("app-recycle-bin"));
    assert_eq!(ui.app().view, super::View::Trash);
    assert_eq!(ui.app().trashed_sections.len(), 1);
    settle(ui);
    ui.shot(&format!("{prefix}-section-bin"));
    ui.h.state_mut()
        .library
        .restore_section("section-science")
        .unwrap();
    ui.h.state_mut().refresh_workspace();

    ui.h.state_mut().open_page("page-school-week", true);
    ui.h.state_mut().ribbon.tab = RibbonTab::Home;
    ui.steps(2);
    settle(ui);
}

/// Screenshots of the sample notebook in both themes, for setting beside the desktop notebook's: the
/// page with nothing selected, hovered and being typed in, the skin on menus, dialogs and
/// tooltips, the desktop notebook's containers and outlines (`outline_shots`), the desktop notebook's page and View's
/// page options (`page_shots`), the shell (`shell_shots`), P5's ribbon and blocks
/// (`p5_shots`), P6's tables, pictures and paste options (`p6_shots`), then P7's Backstage,
/// Options, tabs, menus, dialogs, search and Horizontal Tabs (`p7_shots`), S1's look
/// (`s1_shots`, named `gallery-{theme}-s1-…`), S4's galleries and dialogs (`s4_shots`,
/// `gallery-{theme}-s4-…`) and S5's menus, dialogs and panes (`s5_shots`,
/// `gallery-{theme}-s5-…`). Each theme starts from a fresh sample.
/// Last, in light, P8's dropped-PDF question and a printout, then P9's the desktop notebook import
/// report. Run with `FOLIO_SHOTS=dir cargo test gallery -- --ignored`.
#[test]
#[ignore]
fn gallery() {
    for (theme, name) in [
        (egui::ThemePreference::Light, "light"),
        (egui::ThemePreference::Dark, "dark"),
    ] {
        let mut ui = Ui::start();
        ui.h.ctx.set_theme(theme);
        ui.steps(3);
        assert!(ui.selection().is_empty() && ui.app().canvas.active_text.is_none());
        assert_eq!(ui.app().ribbon.tab, RibbonTab::Home);
        ui.shot(&format!("gallery-{name}-sample"));
        // Every sample page, as a new library has it.
        for id in crate::seed::PAGE_IDS {
            ui.h.state_mut().open_page(id, true);
            ui.h.event(Event::PointerGone);
            ui.steps(6);
            ui.shot(&format!("gallery-{name}-page-{id}"));
        }
        ui.h.state_mut().open_page("page-school-week", true);
        ui.steps(3);
        // Hover the first note to show its container chrome.
        let first = ui.items()[0].clone();
        let inside = ui.screen(first.rect().min + egui::vec2(30.0, 12.0));
        ui.h.event(Event::PointerMoved(inside));
        ui.steps(2);
        ui.shot(&format!("gallery-{name}-hover"));
        ui.click(inside);
        ui.shot(&format!("gallery-{name}-editing"));
        // The skin on menus, dialogs and tooltips: the right-click text menu, Options
        // (its General page: boxes, captions) and a ribbon tooltip.
        let text = ui.screen(first.rect().min + egui::vec2(30.0, 40.0));
        ui.right_click(text);
        ui.steps(2);
        ui.shot(&format!("gallery-{name}-text-menu"));
        ui.press(Key::Escape);
        ui.h.state_mut().dialog = Some(super::Dialog::Settings);
        ui.steps(3);
        ui.shot(&format!("gallery-{name}-settings"));
        ui.h.state_mut().dialog = None;
        ui.steps(2);
        // The tabs carry no tooltip, so hover the Insert band's Table button.
        ui.h.state_mut().ribbon.tab = RibbonTab::Insert;
        ui.steps(2);
        let table = ui.ribbon_button_rect(&t!("ribbon-table"));
        ui.h.event(Event::PointerMoved(table.center()));
        ui.steps(30);
        ui.shot(&format!("gallery-{name}-tooltip"));
        ui.h.state_mut().ribbon.tab = RibbonTab::Home;
        ui.steps(2);
        ui.click(inside);
        ui.press(Key::Escape);
        ui.press(Key::Escape);
        assert!(ui.selection().is_empty());
        outline_shots(&mut ui, &format!("gallery-{name}"));
        page_shots(
            &mut ui,
            &format!("gallery-{name}"),
            "Rings of Saturn",
            "Saturn has rings",
        );
        shell_shots(&mut ui, &format!("gallery-{name}"), "Old notes");
        p5_shots(
            &mut ui,
            &format!("gallery-{name}"),
            "Physics lecture",
            "Projectile motion",
        );
        p6_shots(
            &mut ui,
            &format!("gallery-{name}"),
            &P6Words {
                title: "Timetable",
                line: "Week 1",
                cells: [
                    "Day", "Subject", "Room", "Mon", "Maths", "B12", "Tue", "History", "A3",
                ],
                alt: "A red flag",
                word: "Week",
            },
        );
        p7_shots(
            &mut ui,
            &format!("gallery-{name}"),
            &P7Words {
                title: "Field trip",
                line: "Bring a notebook and a pencil",
                second: "Meet at the musuem at nine",
                misspelled: "musuem",
                language: "en_US",
                search: "castle",
            },
        );
        s1_shots(&mut ui, &format!("gallery-{name}-s1"));
        s4_shots(
            &mut ui,
            &format!("gallery-{name}-s4"),
            "Organelles",
            "Mitochondria",
        );
        s5_shots(
            &mut ui,
            &format!("gallery-{name}-s5"),
            "Lab steps",
            ["Weigh the salt", "Add the water", "Stir until clear"],
        );
    }

    // File printouts: the question, then a two-page printout under its icon. The loop
    // above ends in dark with its `Ui` gone, so these start from a fresh sample in light.
    let mut ui = Ui::start();
    ui.h.ctx.set_theme(egui::ThemePreference::Light);
    ui.steps(3);
    ui.new_page("Printout");
    let _printout = ui.drop_pdf(
        "worksheet.pdf",
        &["Fractions page one", "Fractions page two"],
    );
    ui.shot("gallery-printout-choice");
    ui.click_label("Insert printout");
    ui.wait_for_printout();
    ui.shot("gallery-printout");

    // P9's the desktop notebook import: the report over the new notebook's first page, still in light.
    ui.h.state_mut()
        .start_notebook_import(notebook_fixture("mpl/notebook"));
    ui.wait_for_label(&t!("import-notebook-done-title"));
    ui.h.event(Event::PointerGone);
    ui.shot("gallery-light-import-report");
    ui.h.get_by_label(&t!("common-close")).click();
    ui.steps(3);
}

/// The README's pictures, on the sample library, in English (light and dark) and French.
/// Run with `FOLIO_SHOTS=dir cargo test --bin folio readme_shots -- --ignored`, then save
/// them in docs/images as RGB (README › Tests says how).
#[test]
#[ignore]
fn readme_shots() {
    let open = |ui: &mut Ui, id: &str| {
        ui.h.state_mut().open_page(id, true);
        ui.h.event(Event::PointerGone);
        ui.steps(6);
    };
    let mut ui = Ui::start();
    // No squiggles in the pictures.
    ui.h.state_mut().editor.spell.set_language("off");
    let ctx = ui.h.ctx.clone();
    ui.h.state_mut()
        .set_appearance(super::Appearance::Light, &ctx);
    ui.h.set_size(egui::vec2(1440.0, 900.0));
    ui.steps(3);
    ui.shot("folio-light");

    // The pointer leaves the tab it clicked, so the tab is not drawn hovered.
    open(&mut ui, "page-school-diagram");
    ui.ribbon_tab(&t!("ribbon-tab-insert"));
    ui.h.event(Event::PointerGone);
    ui.steps(3);
    ui.shot("folio-insert");
    open(&mut ui, "page-school-pythagoras");
    ui.ribbon_tab(&t!("ribbon-tab-draw"));
    ui.h.event(Event::PointerGone);
    ui.steps(3);
    ui.shot("folio-draw");
    ui.ribbon_tab(&t!("ribbon-tab-home"));

    // An outline: the pointer over the note shows its frame and paragraph handles, and
    // "Feudalism" is folded.
    open(&mut ui, "page-school-frise");
    let note = ui.items()[0].clone();
    let over = ui.screen(note.rect().min + egui::vec2(40.0, 60.0));
    ui.h.event(Event::PointerMoved(over));
    ui.steps(3);
    ui.shot("folio-outline");

    if crate::seed::PAGE_IDS.contains(&"page-school-worksheet") {
        open(&mut ui, "page-school-worksheet");
        ui.shot("folio-printout");
    }

    open(&mut ui, "page-school-week");
    ui.press_with(Modifiers::COMMAND, Key::E);
    ui.type_text("castle");
    ui.steps(3);
    ui.shot("folio-search");
    ui.press(Key::Escape);

    // Dark: The Middle Ages, then the Immersive Reader on Figures of speech.
    ui.h.state_mut()
        .set_appearance(super::Appearance::Dark, &ctx);
    open(&mut ui, "page-school-frise");
    ui.shot("folio-dark");
    open(&mut ui, "page-school-figures");
    ui.ribbon_tab(&t!("ribbon-tab-view"));
    ui.ribbon_button(&t!("common-immersive-reading"));
    ui.h.event(Event::PointerGone);
    ui.steps(3);
    ui.shot("folio-reader");
    ui.press(Key::Escape);

    // Last, back in light, the report after importing a the desktop notebook (P9): the new
    // notebook would show in the navigation pane of every picture after it.
    ui.h.state_mut()
        .set_appearance(super::Appearance::Light, &ctx);
    ui.h.state_mut().ribbon.tab = RibbonTab::Home;
    ui.h.state_mut()
        .start_notebook_import(notebook_fixture("mpl/notebook"));
    ui.wait_for_label(&t!("import-notebook-done-title"));
    ui.h.event(Event::PointerGone);
    ui.shot("folio-import");
    ui.h.get_by_label(&t!("common-close")).click();
    ui.steps(3);
    drop(ui);

    // French, light: Ma semaine.
    let mut ui = Ui::start_in(Lang::Fr);
    ui.h.state_mut().editor.spell.set_language("off");
    let ctx = ui.h.ctx.clone();
    ui.h.state_mut()
        .set_appearance(super::Appearance::Light, &ctx);
    ui.h.set_size(egui::vec2(1440.0, 900.0));
    ui.steps(3);
    ui.shot("folio-fr");
}

/// The French interface on the French sample library (light), for reviewing the
/// translation: the shell (`shell_shots`, named `fr-…`), the desktop notebook's page and View's page
/// options (`page_shots`), containers and outlines (`outline_shots`), the ribbon's menus,
/// P5's bands, menus and blocks (`p5_shots`), the dialogs, the panes, the reader, the
/// trash, and the canvas states the other French tests do not picture; then a few of them
/// in dark (`fr-dark-…`); last, in light again, P6's tables, pictures and paste options
/// (`p6_shots`), P7's Backstage, Options, tabs, menus, dialogs, search and Horizontal
/// Tabs (`p7_shots`), S1's look (`s1_shots`, named `fr-s1-…`), S4's galleries and dialogs
/// in light and dark (`s4_shots`, `fr-s4-…` and `fr-dark-s4-…`), S5's menus, dialogs and
/// panes in dark and light (`s5_shots`, `fr-dark-s5-…` and `fr-s5-…`), P8's dropped-PDF
/// question and a printout, and P9's the desktop notebook import report and refusal. Run with
/// `FOLIO_SHOTS=dir cargo test gallery_fr -- --ignored`.
#[test]
#[ignore]
fn gallery_fr() {
    let mut ui = Ui::start_in(Lang::Fr);
    ui.h.ctx.set_theme(egui::ThemePreference::Light);
    // Squiggles would hide the words being reviewed.
    ui.h.state_mut().editor.spell.set_language("off");
    // Every save keeps a version, so the versions pane has some to list.
    ui.h.state_mut().library.set_version_interval_ms(0);
    ui.steps(3);
    ui.shot("fr-sample");
    // Every sample page, as a new library has it.
    for id in crate::seed::PAGE_IDS {
        ui.h.state_mut().open_page(id, true);
        ui.h.event(Event::PointerGone);
        ui.steps(6);
        ui.shot(&format!("fr-page-{id}"));
    }
    ui.h.state_mut().open_page("page-school-week", true);
    ui.steps(3);
    shell_shots(&mut ui, "fr", "Anciennes notes");
    let saturn = page_shots(&mut ui, "fr", "Anneaux de Saturne", "Saturne a des anneaux");
    outline_shots(&mut ui, "fr");

    // Hovering and typing in a note, the right-click text menu, the Tags gallery's menu,
    // and the menus of the Home groups folded to one button (the old toolbar's "More
    // formatting" held what did not fit), in a window narrower than the app allows: P2's
    // bands fit its 980 px minimum without folding.
    let first = ui.items()[0].clone();
    let inside = ui.screen(first.rect().min + egui::vec2(30.0, 40.0));
    ui.h.event(Event::PointerMoved(inside));
    ui.steps(2);
    ui.shot("fr-hover");
    ui.click(inside);
    ui.shot("fr-editing");
    ui.right_click(inside);
    ui.steps(2);
    ui.shot("fr-text-menu");
    ui.press(Key::Escape);
    ui.open_tags_menu();
    ui.shot("fr-tag-menu");
    ui.press(Key::Escape);
    ui.h.set_size(egui::vec2(FOLDED_WIDTH, 900.0));
    ui.steps(3);
    // A folded group is one Large button named after the group.
    let mut folded = 0;
    for group in ui.ribbon_groups(RibbonTab::Home) {
        let button = ui
            .band_controls()
            .into_iter()
            .find(|(label, rect)| *label == group.label && rect.height() > 40.0);
        if let Some((_, button)) = button {
            ui.click(button.center());
            ui.shot(&format!("fr-folded-{}", group.id));
            ui.press(Key::Escape);
            folded += 1;
        }
    }
    assert!(
        folded > 0,
        "Home folds a group at {FOLDED_WIDTH} px in French"
    );
    ui.h.set_size(egui::vec2(1320.0, 900.0));
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    ui.press(Key::Escape);

    // Dialogs.
    for (dialog, name) in [
        (super::Dialog::Settings, "settings"),
        (super::Dialog::Shortcuts, "shortcuts"),
    ] {
        ui.h.state_mut().dialog = Some(dialog);
        ui.steps(3);
        ui.shot(&format!("fr-{name}"));
        ui.h.state_mut().dialog = None;
        ui.steps(2);
    }
    ui.h.state_mut().open_new_notebook();
    ui.steps(3);
    ui.shot("fr-new-notebook");
    ui.h.state_mut().dialog = None;
    let notebook = ui.app().workspace.notebooks[0].id.clone();
    ui.h.state_mut()
        .handle_library_action(super::LibraryAction::DeleteNotebook(notebook));
    ui.steps(3);
    ui.shot("fr-delete-notebook");
    ui.h.state_mut().dialog = None;
    ui.steps(2);

    // The search dropdown, find on page (a match, then none), the Find tags pane.
    ui.press_with(Modifiers::COMMAND, Key::E);
    ui.type_text("cellule");
    ui.steps(3);
    ui.shot("fr-search");
    ui.press(Key::Escape);
    ui.press_with(Modifiers::COMMAND, Key::F);
    ui.type_text("semaine");
    ui.steps(3);
    ui.shot("fr-find");
    ui.press(Key::Escape);
    ui.press_with(Modifiers::COMMAND, Key::F);
    ui.type_text("zzzz");
    ui.steps(3);
    ui.shot("fr-find-none");
    ui.press(Key::Escape);
    ui.h.state_mut().canvas.open_find_tags = true;
    ui.steps(4);
    ui.shot("fr-find-tags");
    ui.h.state_mut().close_tags_pane();
    ui.steps(2);

    // A page row's menu and its Move or Copy… dialog.
    let title = ui.app().page.as_ref().unwrap().title.clone();
    let row =
        ui.h.get_by_role_and_label(egui::Role::ListItem, &title)
            .rect();
    ui.right_click(row.center());
    ui.shot("fr-page-row-menu");
    click_menu_row(&mut ui, &t!("app-move-or-copy"), &ctrl_alt_hint(Key::M));
    ui.steps(2);
    ui.shot("fr-move-or-copy");
    ui.press(Key::Escape);
    ui.press(Key::Escape);

    // The reader (View › Lecteur immersif), at the default and the smallest window.
    ui.ribbon_tab(&RibbonTab::View.label());
    ui.ribbon_button(&t!("common-immersive-reading"));
    ui.steps(3);
    ui.shot("fr-reader");
    ui.h.set_size(egui::vec2(980.0, 900.0));
    ui.steps(4);
    ui.shot("fr-reader-980");
    ui.press(Key::Escape);
    ui.h.set_size(egui::vec2(1320.0, 900.0));
    ui.h.state_mut().ribbon.tab = RibbonTab::Home;
    ui.steps(3);

    // The other sample pages.
    ui.h.state_mut().open_page("page-school-diagram", true);
    ui.steps(3);
    ui.shot("fr-diagram");
    ui.h.state_mut().open_page("page-school-frise", true);
    ui.steps(3);
    ui.shot("fr-frise");

    // A shape and then everything selected, the page menu, the files dialog and the
    // frise editor, on a page of their own.
    ui.new_page("Brouillon");
    ui.press(Key::Escape);
    ui.h.state_mut().canvas.tool = CanvasTool::Shape(ShapeKind::Ellipse);
    let corner = ui.empty_spot(-200.0, -40.0);
    ui.drag(corner, corner + egui::vec2(90.0, 70.0));
    ui.h.state_mut().canvas.tool = CanvasTool::Select;
    ui.press(Key::Escape);
    let ellipse = ui.items()[0].id;
    ui.click(ui.screen(ui.item(ellipse).rect().center()));
    ui.steps(2);
    assert_eq!(ui.app().canvas.selection.primary(), Some(ellipse));
    ui.shot("fr-shape-selected");
    ui.insert_table(3, 3);
    ui.press(Key::Escape);
    ui.h.state_mut().ribbon.tab = RibbonTab::Home;
    ui.press_with(Modifiers::COMMAND, Key::A);
    ui.steps(2);
    ui.shot("fr-group-selected");
    ui.press(Key::Escape);
    ui.right_click(ui.empty_spot(0.0, 200.0));
    ui.shot("fr-page-menu");
    ui.press(Key::Escape);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("notes.txt");
    std::fs::write(&path, b"x").unwrap();
    ui.h.event(Event::Paste(format!("file://{}", path.display())));
    ui.steps(3);
    ui.shot("fr-files-dialog");
    ui.press(Key::Escape);
    ui.h.state_mut().ribbon.tab = RibbonTab::Insert;
    ui.steps(2);
    ui.ribbon_button(&t!("ribbon-frise"));
    ui.steps(4);
    ui.shot("fr-frise-editor");
    ui.press(Key::Escape);
    ui.h.state_mut().ribbon.tab = RibbonTab::Home;
    ui.steps(2);

    // The Home band around a numbered list at four widths, the link dialog, the text menu
    // with its link rows, and the versions pane.
    ui.new_page("Mise en forme");
    ui.press(Key::Enter);
    ui.type_text("1. ");
    ui.type_text("Premier point");
    for width in [1000.0, 1200.0, 1320.0, 1700.0] {
        ui.h.set_size(egui::vec2(width, 900.0));
        ui.steps(3);
        ui.shot(&format!("fr-home-{width}"));
    }
    ui.h.set_size(egui::vec2(1320.0, 900.0));
    ui.steps(3);
    ui.press_with(Modifiers::COMMAND, Key::S);
    ui.press_with(Modifiers::COMMAND, Key::A);
    ui.press_with(Modifiers::COMMAND, Key::K);
    ui.steps(2);
    ui.shot("fr-link-dialog");
    ui.type_text("https://docs.rs");
    ui.press(Key::Enter);
    let note = ui.items()[0].clone();
    ui.right_click(ui.text_spot(&note));
    ui.shot("fr-text-menu-link");
    ui.press(Key::Escape);
    ui.press_with(Modifiers::COMMAND, Key::S);
    ui.h.state_mut().open_versions_pane();
    ui.steps(3);
    ui.shot("fr-versions");
    ui.h.state_mut().close_task_pane();
    ui.steps(2);

    // An equation that does not parse.
    ui.h.state_mut().ribbon.tab = RibbonTab::Insert;
    ui.steps(2);
    ui.ribbon_button(&t!("ribbon-equation"));
    ui.type_text("\\frac{1}{");
    ui.steps(3);
    ui.shot("fr-equation-error");
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    ui.h.state_mut().ribbon.tab = RibbonTab::Home;
    ui.steps(2);

    // P5's bands, menus and blocks. Its `fr-home-1320` (a note typed in) takes the place
    // of the numbered list's above.
    p5_shots(
        &mut ui,
        "fr",
        "Cours de physique",
        "Mouvement d’un projectile",
    );

    // The trash: a trashed page's header at the default and the smallest window.
    let id = ui.app().page.as_ref().unwrap().id.clone();
    {
        let app = ui.h.state_mut();
        app.handle_page_list_action(super::PageListAction::Trash(id.clone()));
        app.select_view(super::View::Trash);
        app.open_page(&id, false);
    }
    ui.steps(3);
    for width in [1320.0, 980.0] {
        ui.h.set_size(egui::vec2(width, 900.0));
        ui.steps(3);
        ui.shot(&format!("fr-trash-{width}"));
    }

    // The dark theme: the sample page's bands, Options, the reader and the text menu.
    ui.h.set_size(egui::vec2(1320.0, 900.0));
    ui.h.ctx.set_theme(egui::ThemePreference::Dark);
    ui.h.state_mut().select_view(super::View::Section);
    ui.h.state_mut().open_page("page-school-week", true);
    // The trash's toast would still show: the shots outrun its 2.6 s.
    ui.h.state_mut().toast = super::Toast::default();
    ui.steps(3);
    // Every sample page again, in dark.
    for id in crate::seed::PAGE_IDS {
        ui.h.state_mut().open_page(id, true);
        ui.h.event(Event::PointerGone);
        ui.steps(6);
        ui.shot(&format!("fr-dark-page-{id}"));
    }
    ui.h.state_mut().open_page("page-school-week", true);
    ui.steps(3);
    for tab in GALLERY_TABS {
        ui.h.state_mut().ribbon.tab = tab;
        ui.steps(2);
        ui.shot(&format!("fr-dark-{tab:?}").to_lowercase());
    }
    // P3's page in dark: View › Page Color open, then a lavender college-ruled page, each
    // undone after, and back to the sample's page.
    {
        use crate::model::{PageColor, RuleLines, RuleSize};
        ui.h.state_mut().open_page(&saturn, true);
        ui.steps(3);
        ui.ribbon_tab(&RibbonTab::View.label());
        pick_page_color(
            &mut ui,
            Some(PageColor::Lavender),
            Some("fr-dark-page-color-menu"),
        );
        pick_rule_lines(&mut ui, RuleLines::Lines(RuleSize::College), None);
        settle(&mut ui);
        ui.shot("fr-dark-ruled-page");
        pick_page_color(&mut ui, None, None);
        pick_rule_lines(&mut ui, RuleLines::None, None);
        ui.h.state_mut().open_page("page-school-week", true);
        ui.steps(3);
    }
    ui.h.state_mut().ribbon.tab = RibbonTab::Home;
    ui.h.state_mut().dialog = Some(super::Dialog::Settings);
    ui.steps(3);
    ui.shot("fr-dark-settings");
    ui.h.state_mut().dialog = None;
    ui.steps(2);
    ui.ribbon_tab(&RibbonTab::View.label());
    ui.ribbon_button(&t!("common-immersive-reading"));
    ui.steps(3);
    ui.shot("fr-dark-reader");
    ui.press(Key::Escape);
    ui.steps(2);
    let first = ui.items()[0].clone();
    let inside = ui.screen(first.rect().min + egui::vec2(30.0, 40.0));
    ui.click(inside);
    ui.right_click(inside);
    ui.steps(2);
    ui.shot("fr-dark-text-menu");
    ui.press(Key::Escape);

    // P6's tables, pictures and paste options, in light as the other `fr-` shots, after
    // every shot above so none of them changes.
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    ui.h.ctx.set_theme(egui::ThemePreference::Light);
    ui.steps(3);
    p6_shots(
        &mut ui,
        "fr",
        &P6Words {
            title: "Emploi du temps",
            line: "Semaine 1",
            cells: [
                "Jour", "Matière", "Salle", "Lun", "Maths", "B12", "Mar", "Histoire", "A3",
            ],
            alt: "Un drapeau rouge",
            word: "Semaine",
        },
    );
    // P7's Backstage, Options, tabs, menus, dialogs, search and Horizontal Tabs, in light.
    p7_shots(
        &mut ui,
        "fr",
        &P7Words {
            title: "Sortie scolaire",
            line: "Apporter un cahier et un crayon",
            second: "Rendez-vous devant le musee à neuf heures",
            misspelled: "musee",
            language: "fr",
            search: "cellule",
        },
    );
    // S1's look, in light.
    s1_shots(&mut ui, "fr-s1");
    // S4's galleries and dialogs, in light, then in dark.
    s4_shots(&mut ui, "fr-s4", "Organites", "Mitochondries");
    ui.h.ctx.set_theme(egui::ThemePreference::Dark);
    ui.steps(3);
    s4_shots(&mut ui, "fr-dark-s4", "Organites sombres", "Mitochondries");
    // S5's menus, dialogs and panes, in dark, then in light.
    let steps = [
        "Peser le sel",
        "Ajouter l’eau",
        "Remuer jusqu’à dissolution",
    ];
    s5_shots(&mut ui, "fr-dark-s5", "Étapes sombres", steps);
    ui.h.ctx.set_theme(egui::ThemePreference::Light);
    ui.steps(3);
    s5_shots(&mut ui, "fr-s5", "Étapes", steps);
    // File printouts: the question, then a two-page printout under its icon, in light.
    ui.new_page("Impression");
    let _printout = ui.drop_pdf(
        "fiche.pdf",
        &["Les fractions page un", "Les fractions page deux"],
    );
    ui.shot("fr-printout-choice");
    ui.click_label("Insérer une impression");
    ui.wait_for_printout();
    ui.shot("fr-printout");
    // P9's the desktop notebook import: the report, then a password-protected section's long refusal,
    // the sentence most likely to wrap badly.
    ui.h.state_mut()
        .start_notebook_import(notebook_fixture("mpl/notebook"));
    ui.wait_for_label(&t!("import-notebook-done-title"));
    ui.h.event(Event::PointerGone);
    ui.shot("fr-import-report");
    ui.h.get_by_label(&t!("common-close")).click();
    ui.steps(3);
    ui.h.state_mut()
        .start_notebook_import(notebook_fixture("mit/Encryption.one"));
    ui.wait_for_label(&t!("import-notebook-failed-title"));
    ui.h.event(Event::PointerGone);
    ui.shot("fr-import-failed");
    ui.h.get_by_label(&t!("common-close")).click();
    ui.steps(3);
}

#[test]
fn tab_indents_and_keeps_typing_in_the_note() {
    let mut ui = Ui::start();
    ui.new_page("Tabs");
    ui.press(Key::Enter);
    ui.type_text("Parent");
    ui.press(Key::Enter);
    ui.press(Key::Tab);
    ui.type_text("Child");
    let doc = ui.items()[0].doc().unwrap().clone();
    assert_eq!(doc.plain_text(), "Parent\nChild", "{doc:#?}");
    assert_eq!(doc.blocks[1].indent, 1);
    ui.press_with(Modifiers::SHIFT, Key::Tab);
    let doc = ui.items()[0].doc().unwrap().clone();
    assert_eq!(doc.blocks[1].indent, 0);
}

#[test]
fn typing_list_prefixes_and_inserting_the_date() {
    let mut ui = Ui::start();
    ui.new_page("Lists");
    ui.press(Key::Enter);
    ui.type_text("* ");
    ui.type_text("milk");
    ui.press(Key::Enter);
    ui.type_text("eggs");
    ui.press(Key::Enter);
    ui.press(Key::Enter);
    ui.type_text("1. ");
    ui.type_text("first");
    ui.press(Key::Enter);
    ui.type_text("[] ");
    ui.type_text("call ");
    ui.h.key_press_modifiers(Modifiers::ALT | Modifiers::SHIFT, Key::D);
    ui.steps(3);
    let doc = ui.items()[0].doc().unwrap().clone();
    use crate::model::BlockKind;
    let kinds: Vec<_> = doc
        .blocks
        .iter()
        .map(|b| (b.kind.clone(), b.text.text.clone()))
        .collect();
    assert_eq!(
        kinds[0],
        (BlockKind::Bullet, "milk".to_string()),
        "{kinds:#?}"
    );
    assert_eq!(kinds[1], (BlockKind::Bullet, "eggs".to_string()));
    // Enter on the empty third bullet turned it back into text, where "1. " began a list.
    assert_eq!(kinds[2], (BlockKind::Numbered, "first".to_string()));
    assert!(
        matches!(kinds[3].0, BlockKind::Numbered),
        "Enter continues the list"
    );
    assert!(
        kinds[3].1.starts_with("[] call "),
        "a prefix inside a list is just text"
    );
    assert!(kinds[3].1.ends_with(&crate::editor::today()));
}

impl Ui {
    pub fn right_click(&mut self, pos: Pos2) {
        self.h.event(Event::PointerMoved(pos));
        self.h.step();
        for pressed in [true, false] {
            self.h.event(Event::PointerButton {
                pos,
                button: PointerButton::Secondary,
                pressed,
                modifiers: Modifiers::NONE,
            });
            self.h.step();
        }
        self.steps(2);
    }
}

/// A right-click menu's Paste puts what it pastes where the menu was opened (the copied
/// item's top-left corner, a pasted text's note), as the paper's Paste did before it asked
/// the platform for the clipboard; Ctrl+V still pastes in the middle of the view.
#[test]
fn a_menu_paste_lands_where_the_menu_was_opened() {
    let mut ui = Ui::start();
    ui.new_page("Paste here");
    ui.h.state_mut().canvas.tool = CanvasTool::Shape(ShapeKind::Rect);
    let from = ui.empty_spot(-150.0, -100.0);
    ui.drag(from, from + egui::vec2(120.0, 80.0));
    let shape = ui.items()[0].id;
    ui.right_click(ui.screen(ui.item(shape).rect().center()));
    let copy = menu_row_rect(&ui, &t!("common-copy"), &ctrl_hint(Key::C))
        .expect("the shape's menu has Copy");
    let clipboard = copied(&ui.click_outputs(copy.center()));
    assert_eq!(clipboard.len(), 1, "Copy put the shape on the clipboard");
    let menu_paste = |ui: &mut Ui, at: Pos2, text: &str| {
        ui.right_click(at);
        let paste = menu_row_rect(ui, &t!("common-paste"), &ctrl_hint(Key::V))
            .expect("the paper's menu has Paste");
        assert!(asks_to_paste(&ui.click_outputs(paste.center())));
        ui.h.event(Event::Paste(text.into()));
        ui.steps(3);
    };
    let near = |a: Pos2, b: Pos2| (a - b).length() < 2.0;

    // The shape: its copy's corner where the paper was right-clicked.
    let spot = ui.empty_paper_point();
    menu_paste(&mut ui, spot, &clipboard[0]);
    let items = ui.items();
    assert_eq!(items.len(), 2, "the menu pasted the shape");
    let copy = items.iter().find(|item| item.id != shape).unwrap();
    let corner = ui.screen(copy.rect().min);
    assert!(
        near(corner, spot),
        "pasted at {corner:?}, clicked at {spot:?}"
    );

    // Text from another app: a new note there.
    let spot = ui.empty_paper_point();
    menu_paste(&mut ui, spot, "Pasted words");
    let note = ui.note_with("Pasted words");
    let corner = ui.screen(note.rect().min);
    assert!(
        near(corner, spot),
        "the note at {corner:?}, clicked at {spot:?}"
    );

    // Ctrl+V right after has no menu point: the middle of the view, as before.
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    ui.h.event(Event::Paste("Centred words".into()));
    ui.steps(3);
    let note = ui.note_with("Centred words");
    let view = ui.app().canvas.visible().expect("a view");
    // A pasted note is 420 wide, centred across the view.
    assert!(
        (note.rect().left() - (view.center().x - 210.0)).abs() < 2.0,
        "Ctrl+V's note sits in the middle of the view: {:?} / {view:?}",
        note.rect()
    );
}

#[test]
fn right_click_menus_and_the_view_follows_long_notes() {
    let mut ui = Ui::start();
    ui.new_page("Menus");
    ui.h.state_mut().canvas.tool = CanvasTool::Shape(ShapeKind::Rect);
    let from = ui.empty_spot(-150.0, -100.0);
    ui.drag(from, from + egui::vec2(120.0, 80.0));
    let rect_id = ui.items()[0].id;
    let center = ui.screen(ui.item(rect_id).rect().center());
    // the desktop notebook's menu has no Duplicate: Copy, then Paste. Paste asks the platform for the
    // clipboard, which kittest never answers, so the copy comes back as a paste event.
    ui.right_click(center);
    let copy = menu_row_rect(&ui, &t!("common-copy"), &ctrl_hint(Key::C))
        .expect("the shape's menu has Copy");
    let clipboard = copied(&ui.click_outputs(copy.center()));
    assert_eq!(clipboard.len(), 1, "Copy put the shape on the clipboard");
    ui.right_click(center);
    let paste = menu_row_rect(&ui, &t!("common-paste"), &ctrl_hint(Key::V))
        .expect("the shape's menu has Paste");
    assert!(
        asks_to_paste(&ui.click_outputs(paste.center())),
        "Paste asks the platform for the clipboard"
    );
    ui.h.event(Event::Paste(clipboard[0].clone()));
    ui.steps(3);
    assert_eq!(ui.items().len(), 2, "the menu copied and pasted the shape");

    // "Text box here" on empty paper, clear of both shapes.
    let spot = ui.empty_paper_point();
    ui.right_click(spot);
    ui.click_label_containing("Text box here");
    ui.type_text("From the menu");
    assert!(ui
        .items()
        .iter()
        .any(|item| text_of(item) == "From the menu"));

    // Typing many lines keeps the note's last line on screen. (the desktop notebook's 18 px lines: it
    // takes 40 of them to outgrow the view.)
    for _ in 0..40 {
        ui.press(Key::Enter);
        ui.type_text("line");
    }
    let note = ui
        .items()
        .into_iter()
        .find(|item| text_of(item).starts_with("From the menu"))
        .unwrap();
    let visible = ui.app().canvas.scene_rect.unwrap();
    assert!(
        note.rect().bottom() <= visible.bottom() + 1.0 && note.rect().top() < visible.top(),
        "the last line (note bottom {}) should be in the view ending at {}",
        note.rect().bottom(),
        visible.bottom()
    );
}

#[test]
fn undo_in_one_note_never_brings_in_another_notes_text() {
    let mut ui = Ui::start();
    ui.new_page("Two notes");
    let first = ui.empty_spot(-180.0, -120.0);
    ui.click(first);
    ui.type_text("Alpha alpha");
    let second = ui.empty_spot(-180.0, 120.0);
    ui.click(second);
    ui.type_text("Beta");
    // Back into the first note through its padding, then undo there.
    let alpha = ui
        .items()
        .into_iter()
        .find(|item| text_of(item) == "Alpha alpha")
        .unwrap();
    let padding = ui.screen(alpha.rect().right_bottom() - egui::vec2(8.0, 3.0));
    ui.click(padding);
    ui.press_with(Modifiers::COMMAND, Key::Z);
    // The first note's history starts fresh when it is re-entered, so there is nothing
    // to undo there; it must not pick up the second note's earlier (empty) state.
    let texts: Vec<String> = ui.items().iter().map(text_of).collect();
    assert_eq!(texts, ["Alpha alpha", "Beta"]);
}

#[test]
fn emptying_a_note_can_be_undone_and_insert_has_no_stickies() {
    let mut ui = Ui::start();
    ui.new_page("Undo");
    let spot = ui.empty_spot(-150.0, -100.0);
    ui.click(spot);
    ui.type_text("Keep this");
    let elsewhere = ui.empty_spot(-150.0, 150.0);
    ui.click(elsewhere);
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    assert_eq!(ui.items().len(), 1);

    // Empty the existing note, then click away: it goes, but undo brings it back.
    let id = ui.items()[0].id;
    let text = ui.screen(ui.item(id).rect().left_top() + egui::vec2(20.0, 14.0));
    ui.click(text);
    ui.press_with(Modifiers::COMMAND, Key::A);
    ui.press(Key::Backspace);
    let away = ui.empty_spot(100.0, 150.0);
    ui.click(away);
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    assert!(ui.items().is_empty());
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert_eq!(
        ui.items().iter().map(text_of).collect::<Vec<_>>(),
        ["Keep this"]
    );

    // the desktop notebook has no sticky notes: Insert offers none, and every note is a container that
    // goes when it is left empty (old stickies load as such notes).
    ui.h.state_mut().ribbon.tab = RibbonTab::Insert;
    ui.steps(2);
    assert!(!ui
        .band_controls()
        .iter()
        .any(|(label, _)| label.to_lowercase().contains("sticky")));
}

#[test]
fn moving_a_note_while_typing_undoes_in_order() {
    let mut ui = Ui::start();
    ui.new_page("Order");
    let spot = ui.empty_spot(-150.0, -100.0);
    ui.click(spot);
    ui.type_text("Old");
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    let id = ui.items()[0].id;
    let start = ui.item(id).rect().min;
    // Type more, then move the note by its bar without clicking elsewhere first.
    let text = ui.screen(ui.item(id).rect().left_top() + egui::vec2(60.0, 14.0));
    ui.click(text);
    ui.press(Key::End);
    ui.type_text(" new");
    let bar = ui.bar(id);
    ui.drag(bar, bar + egui::vec2(50.0, 40.0));
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    assert_eq!(text_of(&ui.item(id)), "Old new");
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert_eq!(ui.item(id).rect().min, start, "first undo moves it back");
    assert_eq!(text_of(&ui.item(id)), "Old new", "and keeps the typing");
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert_eq!(
        text_of(&ui.item(id)),
        "Old",
        "second undo takes back the typing"
    );
}

#[test]
fn page_shortcuts_do_nothing_behind_a_dialog() {
    let mut ui = Ui::start();
    ui.new_page("Covered");
    ui.h.state_mut().canvas.tool = CanvasTool::Shape(ShapeKind::Rect);
    let from = ui.empty_spot(-150.0, 0.0);
    ui.drag(from, from + egui::vec2(120.0, 80.0));
    assert_eq!(ui.app().canvas.selection.primary(), Some(ui.items()[0].id));
    ui.h.state_mut().dialog = Some(super::Dialog::Shortcuts);
    ui.steps(2);
    ui.press(Key::Backspace);
    ui.h.event(Event::Paste("hello".into()));
    // Pictures are pasted even while typing, so they rely on this guard alone.
    let image = egui::ColorImage::filled([4, 4], egui::Color32::RED);
    ui.h.event(Event::PasteImage(std::sync::Arc::new(image)));
    ui.steps(2);
    assert_eq!(ui.items().len(), 1);

    // With the dialog gone, the same paste does insert a picture.
    ui.h.state_mut().dialog = None;
    ui.steps(2);
    let image = egui::ColorImage::filled([4, 4], egui::Color32::RED);
    ui.h.event(Event::PasteImage(std::sync::Arc::new(image)));
    ui.steps(2);
    assert!(ui
        .items()
        .iter()
        .any(|item| matches!(item.kind, CanvasKind::Image { .. })));
}

#[test]
fn armed_bold_is_dropped_when_the_caret_moves() {
    let mut ui = Ui::start();
    ui.new_page("Caret");
    ui.press(Key::Enter);
    ui.type_text("abc def");
    // Bold with nothing selected applies to the text typed next...
    ui.press_with(Modifiers::COMMAND, Key::B);
    // ...but only right there: moving the caret drops it, as in the desktop notebook.
    for _ in 0..4 {
        ui.press(Key::ArrowLeft);
    }
    ui.type_text("X");
    let text = ui.items()[0].doc().unwrap().blocks[0].text.clone();
    assert_eq!(text.text, "abcX def");
    assert!(!text.style_at(3).bold, "{text:#?}");
    // Arming and typing straight away still works.
    ui.press_with(Modifiers::COMMAND, Key::B);
    ui.type_text("Y");
    let text = ui.items()[0].doc().unwrap().blocks[0].text.clone();
    assert_eq!(text.text, "abcXY def");
    assert!(text.style_at(4).bold, "{text:#?}");
}

#[test]
fn fonts_sizes_alignment_and_scripts_from_the_ribbon_and_keys() {
    use crate::model::{FontFace, ParaAlign};
    let mut ui = Ui::start();
    ui.new_page("Fonts");
    ui.press(Key::Enter);
    ui.type_text("Hello world");
    let id = ui.items()[0].id;
    let short = ui.item(id).height;
    ui.press_with(Modifiers::COMMAND, Key::A);
    ui.click_label("Font");
    ui.click_label_containing("Atkinson Hyperlegible");
    ui.click_label("Font size");
    ui.click_label("36");
    let block = ui.item(id).doc().unwrap().blocks[0].clone();
    for byte in [0, 6, 10] {
        let style = block.text.style_at(byte);
        assert_eq!(
            (style.font, style.size),
            (Some(FontFace::Readable), Some(36)),
            "{block:#?}"
        );
    }
    ui.steps(2);
    assert!(
        ui.item(id).height > short + 20.0,
        "the note grows with its text"
    );

    // Ctrl+Shift+> and Ctrl+Shift+< step through the desktop notebook's sizes.
    ui.press_with(Modifiers::COMMAND | Modifiers::SHIFT, Key::Period);
    let size = |ui: &Ui| ui.item(id).doc().unwrap().blocks[0].text.style_at(0).size;
    assert_eq!(size(&ui), Some(48));
    ui.press_with(Modifiers::COMMAND | Modifiers::SHIFT, Key::Comma);
    ui.press_with(Modifiers::COMMAND | Modifiers::SHIFT, Key::Comma);
    assert_eq!(size(&ui), Some(28));
    assert!(ui.app().dialog.is_none(), "Ctrl+Shift+, is not Settings");

    // Ctrl+R / Ctrl+L align; the ribbon menu also centers.
    let align = |ui: &Ui| ui.item(id).doc().unwrap().blocks[0].align;
    ui.press_with(Modifiers::COMMAND, Key::R);
    assert_eq!(align(&ui), ParaAlign::Right);
    ui.press_with(Modifiers::COMMAND, Key::L);
    assert_eq!(align(&ui), ParaAlign::Left);
    ui.click_label("Paragraph alignment");
    ui.click_label_containing("Center");
    assert_eq!(align(&ui), ParaAlign::Center);

    // Superscript (Ctrl+Shift+=) and subscript (Ctrl+=) apply to what is typed next.
    ui.press(Key::End);
    ui.press_with(Modifiers::COMMAND | Modifiers::SHIFT, Key::Equals);
    ui.type_text("2");
    ui.press_with(Modifiers::COMMAND, Key::Equals);
    ui.type_text("i");
    let text = ui.item(id).doc().unwrap().blocks[0].text.clone();
    assert_eq!(text.text, "Hello world2i");
    assert!(
        text.style_at(11).superscript && !text.style_at(11).subscript,
        "{text:#?}"
    );
    assert!(
        text.style_at(12).subscript && !text.style_at(12).superscript,
        "{text:#?}"
    );
    assert_eq!(
        text.style_at(12).font,
        Some(FontFace::Readable),
        "typing keeps the font"
    );
}

#[test]
fn ctrl_shift_n_clears_formatting_while_typing() {
    let mut ui = Ui::start();
    ui.new_page("Plain");
    ui.press(Key::Enter);
    ui.type_text("Loud words");
    ui.press_with(Modifiers::COMMAND, Key::A);
    ui.press_with(Modifiers::COMMAND, Key::B);
    ui.press_with(Modifiers::COMMAND | Modifiers::SHIFT, Key::Period);
    let text = ui.items()[0].doc().unwrap().blocks[0].text.clone();
    assert!(
        text.style_at(0).bold && text.style_at(0).size == Some(12),
        "{text:#?}"
    );
    ui.press_with(Modifiers::COMMAND | Modifiers::SHIFT, Key::N);
    let text = ui.items()[0].doc().unwrap().blocks[0].text.clone();
    assert!(text.runs.is_empty(), "{text:#?}");
    assert!(
        ui.app().dialog.is_none(),
        "no New notebook dialog while typing"
    );

    // With no note focused, egui's keyboard zoom must not scale the whole app.
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    assert!(
        !ui.h.ctx.text_edit_focused(),
        "the note let go of the caret"
    );
    ui.press_with(Modifiers::COMMAND, Key::Minus);
    assert_eq!(ui.h.ctx.zoom_factor(), 1.0, "Ctrl+- never zooms the app");
    ui.press_with(Modifiers::COMMAND, Key::Num0);
    assert_eq!(ui.h.ctx.zoom_factor(), 1.0, "Ctrl+0 never zooms the app");
}

#[test]
fn ticking_a_box_in_another_note_stays_out_of_this_notes_undo() {
    let mut ui = Ui::start();
    ui.new_page("Boxes");
    let first = ui.empty_spot(-180.0, -120.0);
    ui.click(first);
    ui.type_text("Alpha");
    let second = ui.empty_spot(-180.0, 120.0);
    ui.click(second);
    ui.type_text("[] ");
    ui.type_text("Beta");
    let find = |ui: &Ui, text: &str| {
        ui.items()
            .into_iter()
            .find(|item| text_of(item) == text)
            .unwrap()
    };
    let alpha = find(&ui, "Alpha");
    let beta = find(&ui, "Beta");
    // Type in the first note, tick the second note's box, then undo in the first note.
    let padding = ui.screen(alpha.rect().right_bottom() - egui::vec2(8.0, 3.0));
    ui.click(padding);
    ui.type_text(" one");
    // The to-do box hangs in its 18 px slot just left of the text, as the desktop notebook's does.
    let start = ui.text_point(beta.id, beta.doc().unwrap().blocks[0].id, 0);
    ui.click(start - egui::vec2(crate::editor::TAG_SLOT / 2.0, 0.0));
    assert!(
        ui.item(beta.id).doc().unwrap().blocks[0].checked(),
        "the box was ticked"
    );
    ui.click(padding);
    ui.press_with(Modifiers::COMMAND, Key::Z);
    let text = text_of(&ui.item(alpha.id));
    assert!(
        text.starts_with("Alpha") && !text.contains("Beta"),
        "undo in the first note must not bring in the second: {text:?}"
    );
    // ...and the first note keeps its own history: undo takes back its typing.
    assert_eq!(text, "Alpha", "the tick must not wipe this note's undo");
    assert!(
        ui.item(beta.id).doc().unwrap().blocks[0].checked(),
        "undo here leaves the other note alone"
    );
}

#[test]
fn dividers_in_notes_still_show_and_export() {
    use crate::model::{Block, BlockKind};
    let mut ui = Ui::start();
    ui.new_page("Divided");
    ui.press(Key::Enter);
    ui.type_text("Above the line");
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    let note = ui.items()[0].id;
    // Insert has no Divider any more (the desktop notebook has none), but a note that holds one (from
    // an older page) still shows it and exports it.
    {
        let app = ui.h.state_mut();
        let page = app.page.as_mut().expect("a page");
        let doc = page
            .canvas
            .item_mut(note)
            .and_then(CanvasItem::doc_mut)
            .expect("a note");
        doc.blocks.push(Block::new(BlockKind::Divider));
        doc.blocks.push(Block::paragraph("Below"));
    }
    ui.steps(3);
    let blocks = ui.item(note).doc().unwrap().blocks.clone();
    let kinds: Vec<_> = blocks.iter().map(|block| block.kind.clone()).collect();
    assert_eq!(
        kinds,
        [
            BlockKind::Paragraph,
            BlockKind::Divider,
            BlockKind::Paragraph
        ]
    );
    // A horizontal line across the note, between the two paragraphs.
    let row = ui.row(note, blocks[1].id).rect;
    let (top, bottom) = (ui.screen(row.left_top()), ui.screen(row.right_bottom()));
    let line = painted(&ui).iter().any(|shape| match shape {
        egui::Shape::LineSegment { points: [a, b], .. } => {
            a.y == b.y && (b.x - a.x).abs() > 40.0 && a.y > top.y && a.y < bottom.y
        }
        _ => false,
    });
    assert!(line, "the divider is drawn");
    ui.shot("divider");
    let app = ui.app();
    let html = crate::export::canvas_export_html(app.page.as_ref().unwrap(), &app.library);
    assert!(html.contains("<hr>"), "the divider is exported: {html}");
}

#[test]
fn enter_keeps_the_font_and_size_for_the_next_paragraph() {
    use crate::model::FontFace;
    let mut ui = Ui::start();
    ui.new_page("Hand");
    ui.press(Key::Enter);
    // Nothing selected: the face and size apply to what is typed next...
    ui.click_label("Font");
    ui.click_label_containing("Patrick Hand");
    ui.click_label("Font size");
    ui.click_label("20");
    ui.type_text("Hand line");
    // ...and carry on into the next paragraph, as in the desktop notebook.
    ui.press(Key::Enter);
    ui.type_text("next");
    let doc = ui.items()[0].doc().unwrap().clone();
    assert_eq!(doc.blocks.len(), 2, "{doc:#?}");
    for block in &doc.blocks {
        let style = block.text.style_at(0);
        assert_eq!(
            (style.font, style.size),
            (Some(FontFace::Hand), Some(20)),
            "{doc:#?}"
        );
    }
    assert_eq!(doc.blocks[1].text.text, "next");
}

/// Home › Delete takes only the selected text, as the desktop notebook's Delete takes the selection;
/// with a caret, or the whole paragraph selected, the paragraph goes (above).
#[test]
fn deleting_from_the_ribbon_takes_only_the_selected_text() {
    let mut ui = Ui::start();
    ui.new_page("Trim");
    ui.press(Key::Enter);
    ui.type_text("Hello brave world");
    let note = ui.items()[0].id;
    // " world" selected, from the end.
    for _ in 0..6 {
        ui.press_with(Modifiers::SHIFT, Key::ArrowLeft);
    }
    ui.ribbon_button("Delete");
    let text = |ui: &Ui| {
        let doc = ui.item(note).doc().unwrap().clone();
        doc.blocks
            .iter()
            .map(|block| block.text.text.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(text(&ui), ["Hello brave"]);
    // The caret stayed where the text was.
    ui.type_text(" new world");
    assert_eq!(text(&ui), ["Hello brave new world"]);
}

#[test]
fn ctrl_shift_n_and_ctrl_comma_open_dialogs_outside_note_text() {
    let mut ui = Ui::start();
    // Right after Ctrl+N the caret is in the page title, not in a note.
    ui.new_page("Title");
    assert!(ui.h.ctx.text_edit_focused(), "the title has the caret");
    ui.press_with(Modifiers::COMMAND | Modifiers::SHIFT, Key::N);
    assert!(
        matches!(ui.app().dialog, Some(super::Dialog::NewNotebook { .. })),
        "Ctrl+Shift+N in the title is New notebook"
    );
    ui.h.state_mut().dialog = None;
    ui.steps(2);
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    assert!(!ui.h.ctx.text_edit_focused(), "nothing is being typed in");
    ui.press_with(Modifiers::COMMAND | Modifiers::SHIFT, Key::N);
    assert!(
        matches!(ui.app().dialog, Some(super::Dialog::NewNotebook { .. })),
        "Ctrl+Shift+N with nothing focused is New notebook"
    );
    ui.h.state_mut().dialog = None;
    ui.steps(2);
    ui.press_with(Modifiers::COMMAND, Key::Comma);
    assert!(
        matches!(ui.app().dialog, Some(super::Dialog::Settings)),
        "Ctrl+, opens Settings"
    );
}

impl Ui {
    /// A screen point on the first character of a note's first block.
    pub fn text_spot(&self, note: &CanvasItem) -> Pos2 {
        let page = self.app().page.as_ref().expect("page");
        let doc_id = format!("{}:{}", page.id, note.id);
        let block = note.doc().expect("a note").blocks[0].id;
        let rect = self
            .h
            .ctx
            .read_response(crate::editor::block_id(&doc_id, block))
            .expect("the block was drawn")
            .rect;
        self.screen(rect.left_center() + egui::vec2(4.0, 0.0))
    }

    /// Clicks while holding `modifiers` (e.g. Ctrl+click on a link).
    pub fn click_with(&mut self, modifiers: Modifiers, pos: Pos2) {
        self.h.event(Event::PointerMoved(pos));
        self.h.event(Event::ModifiersChanged(modifiers));
        self.h.step();
        for pressed in [true, false] {
            self.h.event(Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed,
                modifiers,
            });
            self.h.step();
        }
        self.h.event(Event::ModifiersChanged(Modifiers::NONE));
        self.steps(3);
    }

    /// Clicks the last widget labelled with `text`, returning every frame's output, so
    /// clipboard and viewport commands can be checked.
    pub fn click_label_outputs(&mut self, text: &str) -> Vec<egui::FullOutput> {
        let pos = self
            .h
            .get_all_by_label_contains(text)
            .last()
            .unwrap_or_else(|| panic!("no widget labelled {text:?}"))
            .rect()
            .center();
        self.click_outputs(pos)
    }

    /// Clicks at `pos` and returns every frame's output, like `click_label_outputs`.
    pub fn click_outputs(&mut self, pos: Pos2) -> Vec<egui::FullOutput> {
        let mut outputs = Vec::new();
        self.h.event(Event::PointerMoved(pos));
        self.h.step();
        outputs.push(self.h.output().clone());
        for pressed in [true, false] {
            self.button(pos, pressed);
            self.h.step();
            outputs.push(self.h.output().clone());
        }
        for _ in 0..3 {
            self.h.step();
            outputs.push(self.h.output().clone());
        }
        outputs
    }

    pub fn note_with(&self, text: &str) -> CanvasItem {
        self.items()
            .into_iter()
            .find(|item| text_of(item) == text)
            .unwrap_or_else(|| panic!("no note reading {text:?}"))
    }
}

fn copied(outputs: &[egui::FullOutput]) -> Vec<String> {
    outputs
        .iter()
        .flat_map(|output| output.platform_output.commands.iter())
        .filter_map(|command| match command {
            egui::OutputCommand::CopyText(text) => Some(text.clone()),
            _ => None,
        })
        .collect()
}

/// Whether any of these frames asked the platform for the clipboard (a menu's Paste),
/// which kittest never answers: tests follow it with an `Event::Paste` of their own.
fn asks_to_paste(outputs: &[egui::FullOutput]) -> bool {
    outputs.iter().any(|output| {
        output.viewport_output.values().any(|viewport| {
            viewport
                .commands
                .contains(&egui::ViewportCommand::RequestPaste)
        })
    })
}

fn link_of(note: &CanvasItem, char_index: usize) -> Option<String> {
    link_in(note, 0, char_index)
}

fn link_in(note: &CanvasItem, block: usize, char_index: usize) -> Option<String> {
    let text = &note.doc().expect("a note").blocks[block].text;
    text.style_at(text.char_to_byte(char_index))
        .link
        .map(|url| url.to_string())
}

#[test]
fn ctrl_k_links_text_and_ctrl_click_follows_it() {
    let mut ui = Ui::start();
    ui.new_page("Links");
    ui.press(Key::Enter);
    ui.type_text("guide");
    ui.press_with(Modifiers::COMMAND, Key::A);
    ui.press_with(Modifiers::COMMAND, Key::K);
    assert!(
        matches!(ui.app().dialog, Some(super::Dialog::Link { .. })),
        "Ctrl+K in a note opens the link dialog"
    );
    assert!(!ui.app().search.open, "and does not search");
    ui.type_text("example.com");
    ui.press(Key::Enter);
    assert!(ui.app().dialog.is_none());
    let note = ui.note_with("guide");
    assert_eq!(link_of(&note, 0).as_deref(), Some("https://example.com"));
    assert_eq!(link_of(&note, 4).as_deref(), Some("https://example.com"));

    let spot = ui.text_spot(&note);
    ui.click(spot);
    assert_eq!(
        ui.app().last_opened_url,
        None,
        "a plain click only places the caret"
    );
    ui.click_with(Modifiers::COMMAND, spot);
    assert_eq!(
        ui.app().last_opened_url.as_deref(),
        Some("https://example.com")
    );

    // Away from note text Ctrl+K searches; Ctrl+E always does (the desktop notebook).
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    ui.press_with(Modifiers::COMMAND, Key::K);
    assert!(ui.app().search.open);
    ui.press(Key::Escape);
    assert!(!ui.app().search.open);
    ui.press_with(Modifiers::COMMAND, Key::E);
    assert!(ui.app().search.open);
}

#[test]
fn ctrl_k_never_deletes_text_and_cancel_keeps_typing() {
    let mut ui = Ui::start();
    ui.new_page("Keep");
    ui.press(Key::Enter);
    ui.type_text("abc def");
    for _ in 0..3 {
        ui.press(Key::ArrowLeft);
    }
    ui.press_with(Modifiers::COMMAND, Key::K);
    ui.press(Key::Escape);
    assert!(ui.app().dialog.is_none());
    ui.type_text("X");
    assert!(
        ui.items().iter().any(|item| text_of(item) == "abc Xdef"),
        "egui's Ctrl+K must not delete the rest of the line, and focus returns: {:?}",
        ui.items().iter().map(text_of).collect::<Vec<_>>()
    );

    // Applying a link gives the caret back too, after the link, and what follows is plain.
    ui.press_with(Modifiers::COMMAND, Key::A);
    ui.press_with(Modifiers::COMMAND, Key::K);
    ui.type_text("example.com");
    ui.press(Key::Enter);
    ui.type_text("!");
    let note = ui.note_with("abc Xdef!");
    assert_eq!(link_of(&note, 0).as_deref(), Some("https://example.com"));
    assert_eq!(link_of(&note, 8), None);
}

#[test]
fn typed_addresses_link_themselves() {
    let mut ui = Ui::start();
    ui.new_page("Auto");
    ui.press(Key::Enter);
    ui.type_text("see");
    ui.type_text(" ");
    for ch in "www.example.com".chars() {
        ui.type_text(&ch.to_string());
    }
    ui.type_text(" ");
    ui.type_text("x");
    let note = ui.note_with("see www.example.com x");
    assert_eq!(
        link_of(&note, 4).as_deref(),
        Some("https://www.example.com")
    );
    assert_eq!(link_of(&note, 0), None);
    assert_eq!(link_of(&note, 20), None, "text after the address is plain");

    // Enter links an address too, and the next paragraph does not continue the link.
    ui.press(Key::Enter);
    for ch in "www.rust-lang.org".chars() {
        ui.type_text(&ch.to_string());
    }
    ui.press(Key::Enter);
    ui.type_text("after");
    let note = ui.item(note.id);
    assert_eq!(
        text_of(&note),
        "see www.example.com x\nwww.rust-lang.org\nafter"
    );
    assert_eq!(
        link_in(&note, 1, 0).as_deref(),
        Some("https://www.rust-lang.org")
    );
    assert_eq!(link_in(&note, 2, 0), None, "Enter never carries a link");
}

#[test]
fn right_click_menu_formats_cuts_and_keeps_the_selection() {
    let mut ui = Ui::start();
    ui.new_page("Menu");
    ui.press(Key::Enter);
    ui.type_text("Hello world");
    ui.press_with(Modifiers::COMMAND, Key::A);
    let spot = ui.text_spot(&ui.note_with("Hello world"));
    ui.right_click(spot);
    ui.shot("text-menu");
    ui.click_label_containing("Bold");
    let note = ui.note_with("Hello world");
    let text = &note.doc().unwrap().blocks[0].text;
    assert!(
        text.style_at(0).bold && text.style_at(10).bold,
        "the right-click kept the whole selection"
    );

    ui.right_click(spot);
    let outputs = ui.click_label_outputs("Cut");
    assert_eq!(copied(&outputs), ["Hello world"]);
    assert!(ui.items().iter().any(|item| text_of(item).is_empty()));
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert!(
        ui.items().iter().any(|item| text_of(item) == "Hello world"),
        "cut is undoable"
    );

    ui.right_click(spot);
    let outputs = ui.click_label_outputs("Paste");
    assert!(
        outputs.iter().any(
            |output| output.viewport_output.values().any(|viewport| viewport
                .commands
                .contains(&egui::ViewportCommand::RequestPaste))
        ),
        "Paste asks the platform for the clipboard"
    );
}

#[test]
fn the_text_and_tag_menus_fit_in_french() {
    use crate::icons;
    let mut ui = Ui::start_in(Lang::Fr);
    ui.new_page("Menus");
    ui.press(Key::Enter);
    // A link, so the menu has its link rows too.
    ui.type_text("Bonjour");
    ui.press_with(Modifiers::COMMAND, Key::A);
    ui.press_with(Modifiers::COMMAND, Key::K);
    ui.type_text("https://docs.rs");
    ui.press(Key::Enter);
    let spot = ui.text_spot(&ui.note_with("Bonjour"));
    ui.right_click(spot);
    // Each row's glyph, label and shortcut fit in the menu's measured width, and every row
    // is that wide: no label pushes its row past the menu.
    let menu = crate::editor::text_menu_width(&ui.h.ctx);
    let spacing = ui.h.ctx.global_style().spacing.clone();
    let fits = |ui: &Ui, glyph: &str, label: &str, hint: &str| {
        let measure = |text: &str, font: egui::FontId| {
            ui.h.ctx.fonts_mut(|fonts| {
                fonts
                    .layout_no_wrap(text.to_owned(), font, egui::Color32::BLACK)
                    .size()
                    .x
            })
        };
        let text = format!("{glyph}    {label}");
        // A missing row fails here.
        let row =
            ui.h.get_by_label_contains(format!("{text} {hint}").trim_end())
                .rect();
        let mut needed = 2.0 * spacing.button_padding.x + measure(&text, crate::theme::ui(12.5));
        if !hint.is_empty() {
            needed += 2.0 * spacing.icon_spacing + measure(hint, crate::theme::ui(11.5));
        }
        assert!(
            needed <= menu,
            "{label} {hint}: needs {needed}, menu {menu}"
        );
        assert!(
            (row.width() - menu).abs() < 1.0,
            "{label}: {row:?} in a {menu} menu"
        );
    };
    let key = |key: Key| {
        crate::i18n::shortcut_text(&egui::KeyboardShortcut::new(Modifiers::COMMAND, key), false)
    };
    // (Bold, Italic, Underline, Highlight and Tag are the mini toolbar's, checked below;
    // Select All is Ctrl+A alone, as in the desktop notebook.)
    let rows = [
        (icons::SCISSORS, t!("common-cut"), key(Key::X)),
        (icons::COPY, t!("common-copy"), key(Key::C)),
        (icons::CLIPBOARD_TEXT, t!("common-paste"), key(Key::V)),
        (icons::LINK, t!("editor-edit-link"), key(Key::K)),
        (
            icons::ARROW_SQUARE_OUT,
            t!("editor-open-link"),
            t!("key-ctrl-click").into_owned(),
        ),
        (
            icons::COPY_LINK,
            t!("editor-copy-paragraph-link"),
            String::new(),
        ),
        (icons::COPY, t!("editor-copy-link"), String::new()),
        (icons::LINK_BREAK, t!("editor-remove-link"), String::new()),
    ];
    for (glyph, label, hint) in rows {
        fits(&ui, glyph, &label, &hint);
    }
    // The Mini Toolbar on its own card above the menu's: every control, named as its ribbon
    // twin is, lies above the menu's rows, from the same left edge, with a gap between the
    // cards. (The ribbon has the same names; the menu, drawn last, is the last one.)
    let cut =
        ui.h.get_by_label_contains(&format!("{}    {}", icons::SCISSORS, t!("common-cut")))
            .rect();
    for label in [
        t!("editor-font"),
        t!("editor-font-size"),
        t!("editor-bold"),
        t!("editor-italic"),
        t!("editor-underline"),
        t!("editor-highlight"),
        t!("editor-text-color"),
        t!("ribbon-bullets"),
        t!("ribbon-numbering"),
        t!("ribbon-decrease-indent"),
        t!("ribbon-increase-indent"),
        t!("ribbon-group-styles"),
        t!("editor-tag"),
    ] {
        let control =
            ui.h.get_all_by_role_and_label(egui::Role::Button, &label)
                .last()
                .unwrap_or_else(|| panic!("no {label:?} in the mini toolbar"))
                .rect();
        assert!(
            control.left() >= cut.left() - 0.5 && control.bottom() + 6.0 <= cut.top(),
            "{label}: {control:?} is not on the card above {cut:?}"
        );
    }
    ui.press(Key::Escape);
    // A numbered paragraph's menu adds Restart Numbering and Continue Numbering.
    let note = ui.note_with("Bonjour").id;
    ui.press(Key::End);
    ui.press(Key::Enter);
    ui.type_text("1. ");
    ui.type_text("Un");
    let item = ui.blocks()[1].id;
    ui.right_click(ui.text_point(note, item, 1));
    for (glyph, label) in [
        (icons::ARROW_RESET, t!("editor-restart-numbering")),
        (icons::ARROW_RIGHT, t!("editor-continue-numbering")),
    ] {
        fits(&ui, glyph, &label, "");
    }
    ui.press(Key::Escape);
    // The tag rows paint their names and shortcut hints inside their row: the ribbon's
    // full tag menu, from the Tags gallery ("Indicateurs").
    ui.open_tags_menu();
    let question = crate::model::LineTag::RememberForLater.label().into_owned();
    // (The gallery shows this tag inline too; the menu, drawn last, is the last one.)
    let row = ui.h.get_all_by_label(&question).last().unwrap().rect();
    let needed = ui.h.ctx.fonts_mut(|fonts| {
        fonts
            .layout_no_wrap(
                question.clone(),
                crate::theme::ui(12.5),
                egui::Color32::BLACK,
            )
            .size()
            .x
    }) + 28.0
        + 16.0
        + 26.0
        + 40.0; // name, gap, hint (about 40 px), right margin
    assert!(row.width() >= needed, "{question}: {row:?} needs {needed}");
}

#[test]
fn link_menu_opens_copies_and_removes_links() {
    let mut ui = Ui::start();
    ui.new_page("Link menu");
    ui.press(Key::Enter);
    ui.type_text("docs");
    ui.press_with(Modifiers::COMMAND, Key::A);
    ui.press_with(Modifiers::COMMAND, Key::K);
    ui.type_text("https://docs.rs");
    ui.press(Key::Enter);
    let spot = ui.text_spot(&ui.note_with("docs"));

    ui.right_click(spot);
    let outputs = ui.click_label_outputs("Copy link");
    assert_eq!(copied(&outputs), ["https://docs.rs"]);
    ui.right_click(spot);
    ui.click_label_containing("Open link");
    assert_eq!(ui.app().last_opened_url.as_deref(), Some("https://docs.rs"));
    ui.right_click(spot);
    ui.click_label_containing("Remove link");
    assert_eq!(link_of(&ui.note_with("docs"), 0), None);
}

#[test]
fn page_links_open_the_page_and_rows_copy_them() {
    let mut ui = Ui::start();
    ui.new_page("Target");
    let target = ui.app().page.as_ref().unwrap().id.clone();
    ui.new_page("Source");
    ui.press(Key::Enter);
    ui.type_text("go there");
    ui.press_with(Modifiers::COMMAND, Key::A);
    ui.press_with(Modifiers::COMMAND, Key::K);
    ui.type_text(&crate::model::page_link(&target));
    ui.press(Key::Enter);
    let spot = ui.text_spot(&ui.note_with("go there"));
    ui.click_with(Modifiers::COMMAND, spot);
    assert_eq!(
        ui.app().page.as_ref().unwrap().id,
        target,
        "the link opened the page"
    );

    let row =
        ui.h.get_by_role_and_label(egui::accesskit::Role::ListItem, "Target")
            .rect()
            .center();
    ui.right_click(row);
    let outputs = ui.click_label_outputs("Copy Link to Page");
    assert_eq!(copied(&outputs), [crate::model::page_link(&target)]);
}

impl Ui {
    /// A screen point on the last character of a note's first block (for right-aligned text).
    pub fn text_spot_right(&self, note: &CanvasItem) -> Pos2 {
        let page = self.app().page.as_ref().expect("page");
        let doc_id = format!("{}:{}", page.id, note.id);
        let block = note.doc().expect("a note").blocks[0].id;
        let rect = self
            .h
            .ctx
            .read_response(crate::editor::block_id(&doc_id, block))
            .expect("the block was drawn")
            .rect;
        self.screen(rect.right_center() - egui::vec2(4.0, 0.0))
    }

    /// A click whose press and release are a few frames apart, as with a real mouse.
    pub fn slow_click(&mut self, pos: Pos2) {
        self.h.event(Event::PointerMoved(pos));
        self.h.step();
        self.button(pos, true);
        self.steps(3);
        self.button(pos, false);
        self.steps(3);
    }
}

#[test]
fn the_ribbon_link_button_links_the_selection() {
    let mut ui = Ui::start();
    ui.new_page("Ribbon link");
    ui.press(Key::Enter);
    ui.type_text("read the guide");
    for _ in 0..5 {
        ui.press_with(Modifiers::SHIFT, Key::ArrowLeft);
    }
    ui.ribbon_tab("Insert");
    let link = ui.ribbon_button_rect("Link").center();
    ui.slow_click(link);
    match &ui.app().dialog {
        Some(super::Dialog::Link { edit, .. }) => {
            assert_eq!(edit.range, 9..14, "{edit:?}");
            assert_eq!(edit.text, "guide");
        }
        _ => panic!("Insert → Link opens the link dialog"),
    }
    ui.type_text("example.com");
    ui.press(Key::Enter);
    let note = ui.note_with("read the guide");
    assert_eq!(link_of(&note, 9).as_deref(), Some("https://example.com"));
    assert_eq!(link_of(&note, 13).as_deref(), Some("https://example.com"));
    assert_eq!(link_of(&note, 8), None);
}

#[test]
fn links_in_right_aligned_paragraphs_open_with_ctrl_click() {
    let mut ui = Ui::start();
    ui.new_page("Aligned");
    ui.press(Key::Enter);
    ui.type_text("guide");
    ui.press_with(Modifiers::COMMAND, Key::A);
    ui.press_with(Modifiers::COMMAND, Key::K);
    ui.type_text("example.com");
    ui.press(Key::Enter);
    ui.press_with(Modifiers::COMMAND, Key::R);
    let note = ui.note_with("guide");
    assert_eq!(
        note.doc().unwrap().blocks[0].align,
        crate::model::ParaAlign::Right
    );
    let spot = ui.text_spot_right(&note);
    ui.click_with(Modifiers::COMMAND, spot);
    assert_eq!(
        ui.app().last_opened_url.as_deref(),
        Some("https://example.com")
    );
}

#[test]
fn pasted_and_fast_typed_addresses_link_whole() {
    let mut ui = Ui::start();
    ui.new_page("Paste");
    ui.press(Key::Enter);
    // Two keystrokes in one frame never link half an address.
    for ch in "www.example".chars() {
        ui.type_text(&ch.to_string());
    }
    ui.type_text(".co");
    ui.type_text("m");
    ui.type_text(" ");
    let note = ui.items().into_iter().next().expect("a note");
    assert_eq!(note.doc().unwrap().blocks[0].text.text, "www.example.com ");
    assert_eq!(
        link_of(&note, 14).as_deref(),
        Some("https://www.example.com"),
        "{:#?}",
        note.doc().unwrap().blocks[0].text
    );
    assert_eq!(
        link_of(&note, 0).as_deref(),
        Some("https://www.example.com")
    );

    // A pasted address links at once, wherever it sits in the pasted text.
    ui.h.event(Event::Paste("see www.rust-lang.org now".into()));
    ui.steps(2);
    let note = ui.note_with("www.example.com see www.rust-lang.org now");
    assert_eq!(
        link_of(&note, 20).as_deref(),
        Some("https://www.rust-lang.org")
    );
    assert_eq!(
        link_of(&note, 36).as_deref(),
        Some("https://www.rust-lang.org")
    );
    assert_eq!(link_of(&note, 16), None);
    assert_eq!(link_of(&note, 38), None);
    ui.h.event(Event::Paste(" https://docs.rs".into()));
    ui.steps(2);
    let note = ui.note_with("www.example.com see www.rust-lang.org now https://docs.rs");
    assert_eq!(link_of(&note, 45).as_deref(), Some("https://docs.rs"));
}

#[test]
fn the_link_dialog_picks_pages_and_page_actions_copy_links() {
    let mut ui = Ui::start();
    ui.new_page("Target");
    let target = ui.app().page.as_ref().unwrap().id.clone();
    ui.new_page("Source");
    ui.press(Key::Enter);
    ui.type_text("over there");
    ui.press_with(Modifiers::COMMAND, Key::A);
    ui.press_with(Modifiers::COMMAND, Key::K);
    ui.shot("link-dialog");
    ui.click_label_containing(&format!("{}    Target", crate::icons::FILE_TEXT));
    ui.click_label_containing("Insert link");
    assert!(ui.app().dialog.is_none());
    let note = ui.note_with("over there");
    assert_eq!(
        link_of(&note, 0),
        Some(crate::model::page_link(&target)),
        "picking a page links to it and keeps the selected text"
    );

    // The page's actions are on its row's right-click menu (saved, so the row has its
    // title).
    save(&mut ui);
    let row =
        ui.h.get_by_role_and_label(egui::Role::ListItem, "Source")
            .rect();
    ui.right_click(row.center());
    let outputs = ui.click_label_outputs("Copy Link to Page");
    let source = ui.app().page.as_ref().unwrap().id.clone();
    assert_eq!(copied(&outputs), [crate::model::page_link(&source)]);
}

#[test]
fn link_menu_edits_the_link_under_the_pointer() {
    let mut ui = Ui::start();
    ui.new_page("Edit link");
    ui.press(Key::Enter);
    ui.type_text("docs");
    ui.press_with(Modifiers::COMMAND, Key::A);
    ui.press_with(Modifiers::COMMAND, Key::K);
    ui.type_text("https://docs.rs");
    ui.press(Key::Enter);
    // The caret lands before the link: the menu still edits that link.
    let spot = ui.text_spot(&ui.note_with("docs")) - egui::vec2(3.0, 0.0);
    ui.right_click(spot);
    ui.click_label_containing("Edit link");
    match &ui.app().dialog {
        Some(super::Dialog::Link { edit, .. }) => {
            assert!(edit.existing, "{edit:?}");
            assert_eq!(edit.range, 0..4);
            assert_eq!(edit.url, "https://docs.rs");
        }
        _ => panic!("Edit link… opens the link dialog"),
    }
    // Saving it untouched changes nothing and leaves no empty undo step: one Ctrl+Z
    // undoes the link itself.
    ui.press(Key::Enter);
    assert!(ui.app().dialog.is_none());
    assert_eq!(
        link_of(&ui.note_with("docs"), 0).as_deref(),
        Some("https://docs.rs")
    );
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert_eq!(link_of(&ui.note_with("docs"), 0), None);
}

#[test]
fn an_empty_link_at_the_caret_keeps_typing() {
    let mut ui = Ui::start();
    ui.new_page("Empty link");
    ui.press(Key::Enter);
    ui.type_text("abc");
    ui.press_with(Modifiers::COMMAND, Key::K);
    ui.press(Key::Enter);
    assert!(ui.app().dialog.is_none());
    ui.type_text("d");
    assert!(
        ui.items().iter().any(|item| text_of(item) == "abcd"),
        "{:?}",
        ui.items().iter().map(text_of).collect::<Vec<_>>()
    );
}

#[test]
fn escape_closes_the_text_menu_first() {
    let mut ui = Ui::start();
    ui.new_page("Escape menu");
    ui.press(Key::Enter);
    ui.type_text("Hello");
    let spot = ui.text_spot(&ui.note_with("Hello"));
    ui.right_click(spot);
    assert!(egui::Popup::is_any_open(&ui.h.ctx), "the menu opened");
    let menu =
        ui.h.get_by_label_contains(&format!(
            "{}    {}",
            crate::icons::CLIPBOARD_TEXT,
            t!("common-paste")
        ))
        .rect()
        .width();
    let width = crate::editor::text_menu_width(&ui.h.ctx);
    assert!(
        (menu - width).abs() < 1.0 && menu < 360.0,
        "the text menu is no wider than its mini toolbar needs: {menu} ({width})"
    );
    ui.press(Key::Escape);
    assert!(
        !egui::Popup::is_any_open(&ui.h.ctx),
        "Escape closed the menu"
    );
    assert!(
        ui.app().canvas.active_text.is_some(),
        "the first Escape only closes the menu"
    );
    ui.type_text("!");
    assert!(
        ui.items().iter().any(|item| text_of(item).contains('!')),
        "typing goes on after the menu closes: {:?}",
        ui.items().iter().map(text_of).collect::<Vec<_>>()
    );
}

#[test]
fn tag_shortcuts_and_find_tags_go_to_the_line() {
    use crate::model::{BlockKind, LineTag, TagMark};
    let mut ui = Ui::start();
    ui.new_page("Tagged");
    ui.press(Key::Enter);
    ui.type_text("Photosynthesis needs light");
    let tags = |ui: &Ui| -> Vec<Vec<LineTag>> {
        ui.items()[0]
            .doc()
            .unwrap()
            .blocks
            .iter()
            .map(|block| block.tags.clone())
            .collect()
    };
    ui.press_with(Modifiers::COMMAND, Key::Num2);
    assert_eq!(tags(&ui)[0], [LineTag::Important]);
    ui.press_with(Modifiers::COMMAND, Key::Num2);
    assert!(
        tags(&ui)[0].is_empty(),
        "the same shortcut takes the tag off"
    );
    ui.press_with(Modifiers::COMMAND, Key::Num3);
    ui.press(Key::Enter);
    ui.type_text("Why is the sky blue");
    assert_eq!(
        tags(&ui),
        [vec![LineTag::Question], vec![LineTag::Question]],
        "Enter carries the tag"
    );
    ui.press(Key::Enter);
    ui.press(Key::Enter);
    ui.type_text("Buy a notebook");
    assert!(
        tags(&ui)[2].is_empty(),
        "Enter on the empty tagged line dropped the tag"
    );
    ui.press_with(Modifiers::COMMAND, Key::Num1);
    ui.press_with(Modifiers::COMMAND, Key::Num5);
    ui.press_with(Modifiers::COMMAND, Key::Num0);
    let block = ui.items()[0].doc().unwrap().blocks[2].clone();
    assert!(
        block.tags.is_empty() && block.kind == BlockKind::Paragraph,
        "Ctrl+0 removes To Do too"
    );
    let note = ui.items()[0].id;
    let question = ui.items()[0].doc().unwrap().blocks[1].id;

    // The full tag menu in the ribbon: the Tags gallery's ▾ (Idea is past the tags the
    // gallery shows inline).
    ui.ribbon_button("Tags ▾");
    ui.click_label("Idea");
    assert_eq!(tags(&ui)[2], [LineTag::Idea]);
    ui.shot("tags-on-the-page");

    // From another page, Find Tags lists the lines and opens the right one.
    ui.new_page("Elsewhere");
    ui.ribbon_button("Find Tags");
    ui.steps(2);
    // The sample library's own to-dos share the section; this page's lines are the
    // Idea and the two Question lines, the tags' names in alphabetical order.
    let listed: Vec<TagMark> = ui
        .app()
        .tags_pane
        .as_ref()
        .expect("the Find Tags pane opened")
        .lines
        .iter()
        .filter(|line| line.page_title == "Tagged")
        .map(|line| line.mark)
        .collect();
    assert_eq!(
        listed,
        [
            TagMark::Line(LineTag::Idea),
            TagMark::Line(LineTag::Question),
            TagMark::Line(LineTag::Question)
        ]
    );
    ui.shot("find-tags-pane");
    // Group tags by, Show only unchecked items and Create Summary Page take room from
    // the list: the line is scrolled into view first.
    ui.h.get_all_by_label_contains("Question: Why is the sky")
        .last()
        .expect("the Why is the sky line")
        .scroll_to_me();
    ui.steps(4);
    ui.click_label_containing("Question: Why is the sky");
    ui.steps(6);
    let app = ui.app();
    assert_eq!(app.page.as_ref().unwrap().title, "Tagged");
    assert_eq!(app.canvas.active_text, Some(note));
    assert_eq!(
        app.editor.active_block,
        Some(question),
        "the caret is in the tagged line"
    );
}

impl Ui {
    /// A key press as a real keyboard reports it: `key` is what the layout types and
    /// `physical` is the key's position (AZERTY's 6 key types "-").
    pub fn press_physical(&mut self, modifiers: Modifiers, key: Key, physical: Key) {
        self.h.event(Event::ModifiersChanged(modifiers));
        for pressed in [true, false] {
            self.h.event(Event::Key {
                key,
                physical_key: Some(physical),
                pressed,
                repeat: false,
                modifiers,
            });
        }
        self.h.event(Event::ModifiersChanged(Modifiers::NONE));
        self.steps(2);
    }

    fn tagged_titles(&self) -> Vec<String> {
        self.app()
            .tags_pane
            .as_ref()
            .expect("the Find Tags pane is open")
            .lines
            .iter()
            .map(|line| line.page_title.clone())
            .collect()
    }
}

#[test]
fn tag_shortcuts_follow_the_digit_keys_on_azerty() {
    use crate::model::LineTag;
    let mut ui = Ui::start();
    ui.new_page("Azerty");
    ui.press(Key::Enter);
    ui.type_text("Surligner ceci");
    let block = |ui: &Ui| ui.items()[0].doc().unwrap().blocks[0].clone();
    // AZERTY's 6 key types "-", so egui reports Ctrl+6 as Ctrl+Minus (strikethrough), and
    // its 4 key types "'".
    ui.press_with(Modifiers::COMMAND, Key::A);
    ui.press_physical(Modifiers::COMMAND, Key::Minus, Key::Num6);
    ui.press_physical(Modifiers::COMMAND, Key::Quote, Key::Num4);
    let tagged = block(&ui);
    assert_eq!(
        tagged.tags,
        [LineTag::RememberForLater, LineTag::Highlight],
        "{tagged:#?}"
    );
    assert!(
        !tagged.text.style_at(0).strike,
        "Ctrl+6 is a tag, not strikethrough: {tagged:#?}"
    );
    // The minus key itself still strikes through.
    ui.press_physical(Modifiers::COMMAND, Key::Minus, Key::Minus);
    assert!(block(&ui).text.style_at(0).strike);
    ui.press_physical(Modifiers::COMMAND, Key::Num0, Key::Num0);
    assert!(block(&ui).tags.is_empty(), "Ctrl+0 removes the tags");

    // The right-click menu tags too, from its mini toolbar's Tag button. (The ribbon's
    // Tags gallery also shows a Question item: the menu, drawn last, is the last one.)
    let spot = ui.text_spot(&ui.note_with("Surligner ceci"));
    ui.right_click(spot);
    let tag =
        ui.h.get_all_by_role_and_label(egui::Role::Button, &t!("editor-tag"))
            .last()
            .expect("the mini toolbar's Tag")
            .rect();
    ui.click(tag.center());
    let question = ui.h.get_all_by_label("Question").last().unwrap().rect();
    ui.click(question.center());
    assert_eq!(block(&ui).tags, [LineTag::Question]);
}

#[test]
fn find_tags_keeps_its_section_and_drops_trashed_pages() {
    let mut ui = Ui::start();
    ui.new_page("Here");
    ui.press(Key::Enter);
    ui.type_text("Listed line");
    ui.press_with(Modifiers::COMMAND, Key::Num2);
    let here = ui.app().page.as_ref().unwrap().clone();
    ui.h.state_mut().open_tags_pane();
    ui.steps(2);
    assert!(ui.tagged_titles().contains(&"Here".to_string()));

    // A line tagged in another section stays out of this section's summary, even once it
    // is saved while the pane is open.
    let other = ui
        .app()
        .workspace
        .notebooks
        .iter()
        .flat_map(|notebook| notebook.sections.iter())
        .find(|section| section.id != here.section_id)
        .expect("a second section")
        .id
        .clone();
    ui.h.state_mut().select_section(&other, true);
    ui.steps(2);
    ui.new_page("Elsewhere");
    ui.press(Key::Enter);
    ui.type_text("Another section");
    ui.press_with(Modifiers::COMMAND, Key::Num2);
    assert!(ui.h.state_mut().flush_save());
    ui.steps(2);
    let titles = ui.tagged_titles();
    assert!(!titles.contains(&"Elsewhere".to_string()), "{titles:?}");
    assert!(titles.contains(&"Here".to_string()), "{titles:?}");

    // Trashing a listed page takes its lines out of the open pane.
    ui.h.state_mut()
        .handle_page_list_action(super::PageListAction::Trash(here.id.clone()));
    ui.steps(2);
    let titles = ui.tagged_titles();
    assert!(!titles.contains(&"Here".to_string()), "{titles:?}");

    // Refresh lists the section of the page that is open now.
    ui.click_label("Refresh tags");
    let titles = ui.tagged_titles();
    assert!(titles.contains(&"Elsewhere".to_string()), "{titles:?}");
}

#[test]
fn find_tags_keeps_the_page_zoom() {
    let mut ui = Ui::start();
    ui.new_page("Zoom");
    ui.press(Key::Enter);
    ui.type_text("Some text");
    ui.steps(3);
    let before = ui.app().canvas.zoom();
    ui.h.state_mut().open_tags_pane();
    ui.steps(4);
    let open = ui.app().canvas.zoom();
    // Every task pane's header has the same Close button.
    ui.click_label("Close");
    assert!(ui.app().tags_pane.is_none(), "Close closed the Tags pane");
    ui.steps(4);
    let after = ui.app().canvas.zoom();
    assert!(
        (open - before).abs() < 0.01 && (after - before).abs() < 0.01,
        "zoom {before} -> {open} with the pane -> {after} after it closed"
    );
}

#[test]
fn a_results_pane_takes_the_tags_panes_place_and_opens_its_pages() {
    use super::{ResultsKind, ResultsPane, ScopeKind, TaskPane};
    let mut ui = Ui::start();
    ui.new_page("Comet notes");
    ui.press(Key::Enter);
    ui.type_text("A comet went by");
    assert!(ui.h.state_mut().flush_save());
    let comet = ui.app().page.as_ref().unwrap().id.clone();
    ui.new_page("Elsewhere");
    ui.steps(2);
    // In a narrow window the Tags pane folds the navigation pane away.
    ui.h.set_size(egui::vec2(980.0, 900.0));
    ui.h.state_mut().open_tags_pane();
    ui.steps(3);
    assert!(!ui.app().show_library, "the Tags pane folds the navigation");

    // The pane holds one thing at a time: a list of pages takes the summary's place and
    // keeps the navigation folded.
    let hits = ui.app().library.search("comet", 10).unwrap();
    assert_eq!(hits.len(), 1, "{hits:?}");
    ui.h.state_mut()
        .open_task_pane(TaskPane::Results(ResultsPane {
            kind: ResultsKind::Search {
                query: "comet".into(),
                scope: ScopeKind::All,
            },
            hits,
        }));
    ui.steps(3);
    assert!(ui.app().tags_pane.is_none());
    assert!(!ui.app().show_library);
    ui.h.get_by_label("Search results: comet");
    ui.h.get_by_label("1 page");
    ui.shot("results-pane");

    // A row opens its page with the words searched for found on it; the pane stays.
    let row =
        ui.h.get_by_role_and_label(egui::Role::Button, "Comet notes")
            .rect();
    let pane = ui.h.get_by_label("Search results: comet").rect();
    assert!(
        row.top() > pane.bottom() && row.height() >= 50.0,
        "the row {row:?} sits under the header {pane:?}"
    );
    ui.click(row.center());
    assert_eq!(ui.app().page.as_ref().unwrap().id, comet);
    assert!(matches!(ui.app().task_pane, Some(TaskPane::Results(_))));
    let find = ui.app().canvas.find.as_ref().expect("find on page opened");
    assert_eq!(find.query, "comet");

    // Close brings back what the panes folded away. (The find bar on the page has a
    // Close of its own; the pane's is the rightmost.)
    let close =
        ui.h.get_all_by_label("Close")
            .map(|node| node.rect())
            .max_by(|a, b| a.right().total_cmp(&b.right()))
            .expect("the pane's Close");
    ui.click(close.center());
    assert!(ui.app().task_pane.is_none());
    assert!(ui.app().show_library, "the navigation pane came back");
    ui.h.set_size(egui::vec2(1320.0, 900.0));
    ui.steps(2);
}

#[test]
fn find_tags_scrolls_a_long_note_to_the_line() {
    use crate::model::{Block, Document, LineTag, PAGE_CONTENT_TOP, PAGE_LEFT};
    let mut ui = Ui::start();
    ui.new_page("Long note");
    let blocks: Vec<Block> = (0..120)
        .map(|i| Block::paragraph(format!("Paragraph {i}")))
        .collect();
    let ids: Vec<u64> = blocks.iter().map(|block| block.id).collect();
    let note = {
        let app = ui.h.state_mut();
        let page = app.page.as_mut().unwrap();
        let mut doc = Document { blocks };
        doc.blocks[110].tags = vec![LineTag::Important];
        let note = page.canvas.add(CanvasItem::new(
            CanvasKind::Text { doc },
            PAGE_LEFT,
            PAGE_CONTENT_TOP,
            400.0,
            40.0,
        ));
        app.dirty = true;
        assert!(app.flush_save());
        note
    };
    ui.new_page("Elsewhere");
    ui.h.state_mut().open_tags_pane();
    ui.steps(2);
    ui.click_label_containing("Important: Paragraph 110");
    ui.steps(10);
    let app = ui.app();
    let page = app.page.as_ref().unwrap();
    assert_eq!(page.title, "Long note");
    assert_eq!(app.canvas.active_text, Some(note));
    assert_eq!(app.editor.active_block, Some(ids[110]));
    let rect =
        ui.h.ctx
            .read_response(crate::editor::block_id(
                &format!("{}:{note}", page.id),
                ids[110],
            ))
            .expect("the line was drawn")
            .rect;
    // The note is drawn in the page's scene; on screen the line sits below the ribbon.
    let rect = egui::Rect::from_min_max(ui.screen(rect.min), ui.screen(rect.max));
    let top = ui.ribbon_card().bottom();
    let bottom = ui.h.ctx.content_rect().bottom();
    assert!(
        rect.top() >= top && rect.bottom() <= bottom,
        "the tagged line is on screen, between {top} and {bottom}: {rect:?}"
    );
}

#[test]
fn find_on_page_steps_through_matches_and_escape_returns_to_the_note() {
    let mut ui = Ui::start();
    ui.new_page("Fruit");
    ui.press(Key::Enter);
    ui.type_text("Apple pie");
    ui.press(Key::Enter);
    ui.type_text("more apple");
    let spot = ui.empty_spot(0.0, 120.0);
    ui.click(spot);
    ui.type_text("APPLES everywhere");

    // Ctrl+F works while typing in a note, and never types an "f" into it.
    ui.press_with(Modifiers::COMMAND, Key::F);
    ui.type_text("apple");
    ui.steps(2);
    ui.shot("find-01-matches");
    let find = ui.app().canvas.find.as_ref().expect("find bar");
    assert_eq!(find.matches.len(), 3);
    assert_eq!(find.current, 0);
    assert!(!ui.app().editor.find_marks.is_empty());

    ui.press(Key::Enter);
    assert_eq!(ui.app().canvas.find.as_ref().unwrap().current, 1);
    ui.press_with(Modifiers::SHIFT, Key::Enter);
    assert_eq!(ui.app().canvas.find.as_ref().unwrap().current, 0);
    ui.press(Key::F3);
    assert_eq!(ui.app().canvas.find.as_ref().unwrap().current, 1);
    ui.press_with(Modifiers::SHIFT, Key::F3);
    assert_eq!(ui.app().canvas.find.as_ref().unwrap().current, 0);
    let mut texts: Vec<_> = ui.items().iter().map(text_of).collect();
    texts.sort();
    assert_eq!(texts, vec!["APPLES everywhere", "Apple pie\nmore apple"]);

    // With nothing found, Escape closes the bar (not the note) and gives the note its
    // caret back.
    ui.type_text("zz");
    assert_eq!(
        ui.app().canvas.find.as_ref().unwrap().status(),
        "No matches"
    );
    ui.press(Key::Escape);
    assert!(ui.app().canvas.find.is_none());
    ui.steps(2);
    assert!(ui.app().editor.find_marks.is_empty());
    assert!(ui.app().canvas.active_text.is_some());
    ui.type_text("!");
    assert!(ui
        .items()
        .iter()
        .any(|item| text_of(item) == "APPLES everywhere!"));

    // Otherwise Escape leaves the caret on the current match, selected, as the desktop notebook and
    // Word leave a found word: typing replaces it.
    ui.press_with(Modifiers::COMMAND, Key::F);
    ui.type_text("apple");
    ui.press(Key::Enter);
    ui.press(Key::Escape);
    assert!(ui.app().canvas.find.is_none());
    ui.steps(2);
    ui.type_text("pear");
    let mut texts: Vec<_> = ui.items().iter().map(text_of).collect();
    texts.sort();
    assert_eq!(texts, vec!["APPLES everywhere!", "Apple pie\nmore pear"]);
}

#[test]
fn find_on_page_brings_far_matches_into_view_and_leaves_the_caret_there() {
    use crate::model::{Block, Document};
    let mut ui = Ui::start();
    ui.new_page("Far");
    ui.press(Key::Enter);
    ui.type_text("Start here");
    let far = {
        let page = ui.h.state_mut().page.as_mut().unwrap();
        page.canvas.add(CanvasItem::new(
            CanvasKind::Text {
                doc: Document {
                    blocks: vec![Block::paragraph("A distant comet")],
                },
            },
            120.0,
            4000.0,
            300.0,
            40.0,
        ))
    };
    ui.steps(2);
    let in_view = |ui: &Ui| {
        let visible = ui.app().canvas.scene_rect.expect("a view");
        visible.contains(ui.item(far).rect().center())
    };
    assert!(!in_view(&ui));

    // Ctrl+F opens the bar with the caret in its field.
    ui.press_with(Modifiers::COMMAND, Key::F);
    ui.steps(2);
    assert!(
        ui.app()
            .canvas
            .find
            .as_ref()
            .expect("find bar")
            .field_focused
    );

    // Finding a word on the far note brings it into view; the caret stays in the field.
    ui.type_text("comet");
    ui.steps(4);
    ui.shot("find-03-far");
    assert!(
        in_view(&ui),
        "{:?} shows {:?}",
        ui.app().canvas.scene_rect,
        ui.item(far).rect()
    );
    let find = ui.app().canvas.find.as_ref().unwrap();
    assert_eq!(find.matches.len(), 1);
    assert!(find.field_focused);
    assert_ne!(ui.app().canvas.active_text, Some(far));

    // The bar hides, keeping its state, while the search dialog covers the page.
    ui.h.state_mut().open_search();
    ui.steps(2);
    assert!(ui.h.query_by_label("Close find").is_none());
    ui.press(Key::Escape);
    ui.steps(2);
    assert!(!ui.app().search.open);
    assert!(ui.h.query_by_label("Close find").is_some());
    assert_eq!(ui.app().canvas.find.as_ref().unwrap().query, "comet");

    // Closing leaves the caret on the match, not in the note now out of view.
    ui.click_label("Close find");
    assert!(ui.app().canvas.find.is_none());
    assert_eq!(ui.app().canvas.active_text, Some(far));
    ui.steps(1);
    ui.shot("find-04-closed-on-the-match");
    assert_eq!(ui.app().editor.selection, Some(10..15));
    ui.type_text("star");
    assert_eq!(text_of(&ui.item(far)), "A distant star");
    assert!(ui.items().iter().any(|item| text_of(item) == "Start here"));
}

#[test]
fn ctrl_f_searches_the_selected_words_in_notes_labels_cells_and_frises() {
    use crate::model::{Block, BlockKind, Document, TableBlock};
    let mut ui = Ui::start();
    ui.new_page("World");
    ui.press(Key::Enter);
    ui.type_text("hello world");
    let table_block_id;
    {
        let page = ui.h.state_mut().page.as_mut().unwrap();
        page.canvas.add(CanvasItem::new(
            CanvasKind::Shape {
                shape: ShapeKind::Rect,
                stroke: Rgb::INKS[0],
                fill: None,
                label: "World".into(),
                flip_x: false,
                flip_y: false,
                thickness: None,
            },
            120.0,
            300.0,
            160.0,
            80.0,
        ));
        // A table in a note, as Insert › Table and old page tables make them.
        let table = Block::new(BlockKind::Table(TableBlock::from_strings(
            &[vec!["World map".into(), "x".into()]],
            false,
            0.0,
        )));
        table_block_id = table.id;
        page.canvas.add(CanvasItem::new(
            CanvasKind::Text {
                doc: Document {
                    blocks: vec![table],
                },
            },
            120.0,
            420.0,
            300.0,
            60.0,
        ));
        page.canvas.add(CanvasItem::new(
            CanvasKind::Timeline {
                color: Rgb::INKS[2],
                milestones: vec![Milestone {
                    title: "World war".into(),
                    date: "1914".into(),
                    note: String::new(),
                }],
            },
            120.0,
            520.0,
            600.0,
            200.0,
        ));
    }
    // Select "world" in the note, then Ctrl+F searches for it.
    for _ in 0..5 {
        ui.press_with(Modifiers::SHIFT, Key::ArrowLeft);
    }
    ui.press_with(Modifiers::COMMAND, Key::F);
    ui.steps(2);
    ui.shot("find-02-everywhere");
    let find = ui.app().canvas.find.as_ref().expect("find bar");
    assert_eq!(find.query, "world");
    let places: Vec<_> = find.matches.iter().map(|found| found.place).collect();
    assert!(matches!(places[0], MatchPlace::Block(_)));
    assert_eq!(
        places[1..],
        [
            MatchPlace::Label,
            MatchPlace::Cell {
                block: table_block_id,
                row: 0,
                col: 0
            },
            MatchPlace::Milestone(0, MilestoneField::Title),
        ]
    );

    // The buttons step too, and the caret stays in the find field.
    ui.click_label("Next match");
    ui.steps(2);
    let find = ui.app().canvas.find.as_ref().unwrap();
    assert_eq!(find.current, 1);
    assert!(find.field_focused);
    ui.click_label("Close find");
    assert!(ui.app().canvas.find.is_none());
}

#[test]
fn search_results_open_with_the_words_highlighted() {
    let mut ui = Ui::start();
    ui.new_page("Zoo");
    ui.press(Key::Enter);
    ui.type_text("The zebra sleeps");
    ui.new_page("Other");

    ui.h.state_mut().open_search();
    ui.steps(2);
    ui.type_text("zebra");
    ui.press(Key::Enter);
    ui.steps(3);
    assert_eq!(ui.app().page.as_ref().unwrap().title, "Zoo");
    let find = ui.app().canvas.find.as_ref().expect("find bar from search");
    assert_eq!(find.query, "zebra");
    assert_eq!(find.matches.len(), 1);

    // Closing the bar while typing in a note leaves the caret in that note.
    let spot = ui.empty_spot(0.0, 120.0);
    ui.click(spot);
    ui.type_text("Giraffe");
    ui.click_label("Close find");
    assert!(ui.app().canvas.find.is_none());
    ui.type_text("s");
    let mut texts: Vec<_> = ui.items().iter().map(text_of).collect();
    texts.sort();
    assert_eq!(texts, vec!["Giraffes", "The zebra sleeps"]);

    // A hit on the title alone opens the page without an empty find bar.
    ui.h.state_mut().open_search();
    ui.steps(2);
    ui.type_text("Other");
    ui.press(Key::Enter);
    ui.steps(3);
    assert_eq!(ui.app().page.as_ref().unwrap().title, "Other");
    assert!(ui.app().canvas.find.is_none());
}

#[test]
fn new_link_text_moves_the_find_highlights() {
    let mut ui = Ui::start();
    ui.new_page("Linked");
    ui.press(Key::Enter);
    ui.type_text("an apple a day");
    let note = ui.items()[0].clone();
    let block = note.doc().unwrap().blocks[0].id;
    let page_id = ui.app().page.as_ref().unwrap().id.clone();
    ui.press_with(Modifiers::COMMAND, Key::F);
    ui.type_text("apple");
    ui.steps(2);
    assert_eq!(
        ui.app().canvas.find.as_ref().unwrap().matches[0].range,
        3..8
    );

    // The link dialog gives "an" new text: the match (and its highlight) move with it.
    let edit = crate::editor::LinkEdit {
        doc_id: format!("{page_id}:{}", note.id),
        block,
        range: 0..2,
        text: "one big".into(),
        url: "https://example.com".into(),
        existing: false,
    };
    let ctx = ui.h.ctx.clone();
    ui.h.state_mut().apply_link(edit, &ctx);
    ui.steps(2);
    assert_eq!(text_of(&ui.item(note.id)), "one big apple a day");
    assert_eq!(
        ui.app().canvas.find.as_ref().unwrap().matches[0].range,
        8..13
    );
}

impl Ui {
    /// The red wavy lines painted in the last frame.
    fn squiggles(&self) -> usize {
        let danger = egui::epaint::ColorMode::Solid(crate::theme::tokens(&self.h.ctx).danger);
        self.h
            .output()
            .shapes
            .iter()
            .filter(|clipped| {
                matches!(&clipped.shape, egui::Shape::Path(path)
                    if !path.closed && path.points.len() > 3 && path.stroke.color == danger)
            })
            .count()
    }

    /// A screen point on char `at` of a one-line note's first block.
    fn char_spot(&self, note: &CanvasItem, at: usize) -> Pos2 {
        let page = self.app().page.as_ref().expect("page");
        let doc_id = format!("{}:{}", page.id, note.id);
        let block = note.doc().expect("a note").blocks[0].id;
        let rect = self
            .h
            .ctx
            .read_response(crate::editor::block_id(&doc_id, block))
            .expect("the block was drawn")
            .rect;
        let before: String = text_of(note).chars().take(at).collect();
        let width = self.h.ctx.fonts_mut(|fonts| {
            fonts
                .layout_no_wrap(
                    before,
                    crate::theme::body(crate::editor::BODY_SIZE),
                    egui::Color32::BLACK,
                )
                .size()
                .x
        });
        self.screen(rect.left_center() + egui::vec2(width + 3.0, 0.0))
    }

    /// Closes the app and opens it again on the same data folder.
    fn reopen(&mut self) {
        let fresh = Harness::builder()
            .with_size(egui::vec2(1320.0, 900.0))
            .with_pixels_per_point(1.0)
            .with_max_steps(500)
            .with_step_dt(1.0 / 60.0)
            .wgpu()
            .build_eframe(|cc| FolioApp::new(cc).expect("app"));
        drop(std::mem::replace(&mut self.h, fresh));
        self.steps(2);
    }
}

#[test]
fn misspelled_words_offer_suggestions_and_can_be_added_to_the_dictionary() {
    let mut ui = Ui::start();
    // Fixed language, whatever the machine's locale.
    ui.h.state_mut().editor.spell.set_language("en_US");
    ui.h.state_mut().editor.spell.wait_until_ready();
    ui.new_page("Spelling");
    let spot = ui.empty_spot(-200.0, 0.0);
    ui.click(spot);
    ui.type_text("We recieve");
    assert_eq!(ui.squiggles(), 0, "the word being typed is not flagged yet");
    ui.type_text(" letters ");
    assert_eq!(
        ui.squiggles(),
        1,
        "a finished misspelled word is underlined"
    );
    ui.shot("spelling-squiggle");

    // Putting the caret in the word keeps its squiggle; editing it hides it until the
    // caret moves on, and typing elsewhere never makes it blink.
    let note = ui.items()[0].clone();
    ui.click(ui.char_spot(&note, 6));
    assert_eq!(
        ui.squiggles(),
        1,
        "a caret in a flagged word keeps the squiggle"
    );
    ui.type_text("x");
    assert_eq!(ui.squiggles(), 0, "the word being edited is not flagged");
    ui.press(Key::Backspace);
    ui.press(Key::End);
    assert_eq!(ui.squiggles(), 1, "leaving the word flags it again");
    ui.h.event(Event::Text("a".into()));
    ui.h.step();
    assert_eq!(ui.squiggles(), 1, "no blink on the frame of a keystroke");
    ui.press(Key::Backspace);
    // Wait out the double-click time, so the right-click is a click of its own.
    ui.steps(30);

    // Right-click the word: the squiggle stays and the menu offers the fix.
    let note = ui.items()[0].clone();
    ui.right_click(ui.char_spot(&note, 6));
    assert_eq!(ui.app().editor.selection, None);
    assert_eq!(
        ui.squiggles(),
        1,
        "the squiggle stays while the menu is open"
    );
    ui.h.state_mut()
        .editor
        .spell
        .wait_for_suggestions("recieve");
    ui.steps(3);
    ui.shot("spelling-menu");
    ui.click_label_containing("receive");
    // `text_of` trims the end of each paragraph.
    assert_eq!(text_of(&ui.item(note.id)), "We receive letters");
    ui.steps(2);
    assert_eq!(ui.squiggles(), 0);

    // "Add to dictionary" accepts the word and saves it.
    let spot = ui.empty_spot(-200.0, 140.0);
    ui.click(spot);
    ui.type_text("Folioz rocks ");
    let note = ui.note_with("Folioz rocks");
    ui.right_click(ui.char_spot(&note, 3));
    ui.click_label_containing("Add to dictionary");
    ui.steps(2);
    assert!(ui
        .h
        .state_mut()
        .editor
        .spell
        .misspelled_words("Folioz")
        .is_empty());
    assert_eq!(
        ui.app()
            .library
            .setting("spell_personal")
            .unwrap()
            .as_deref(),
        Some("Folioz")
    );

    // "Ignore all" accepts the word for this session only.
    let spot = ui.empty_spot(-200.0, -140.0);
    ui.click(spot);
    ui.type_text("Some blorft here ");
    assert_eq!(ui.squiggles(), 1);
    let note = ui.note_with("Some blorft here");
    ui.right_click(ui.char_spot(&note, 7));
    ui.click_label_containing("Ignore all");
    ui.steps(2);
    assert_eq!(ui.squiggles(), 0);
    assert_eq!(
        ui.app()
            .library
            .setting("spell_personal")
            .unwrap()
            .as_deref(),
        Some("Folioz"),
        "ignored words are not added to the dictionary"
    );

    // The language is chosen in Options › Proofing and saved.
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    ui.h.state_mut().dialog = Some(super::Dialog::Settings);
    ui.steps(2);
    ui.options_page(&t!("app-options-proofing"));
    // The language box reads as its current value.
    let rect = ui.h.get_by_value("English (US)").rect();
    ui.click(rect.center());
    ui.click_label("Off");
    assert_eq!(
        ui.app()
            .library
            .setting("spell_language")
            .unwrap()
            .as_deref(),
        Some("off")
    );
    let rect = ui.h.get_by_value("Off").rect();
    ui.click(rect.center());
    ui.shot("spelling-languages");
    ui.click_label("Français");
    assert_eq!(
        ui.app()
            .library
            .setting("spell_language")
            .unwrap()
            .as_deref(),
        Some("fr")
    );

    // Folio starts again with the saved language and words.
    ui.reopen();
    let spell = &mut ui.h.state_mut().editor.spell;
    assert_eq!(spell.setting(), "fr");
    assert_eq!(spell.active_codes(), ["fr"]);
    assert!(spell.has_personal("Folioz"));
    spell.wait_until_ready();
    assert!(spell.misspelled_words("Folioz est là").is_empty());
}

fn titles(ui: &Ui) -> Vec<(String, u8)> {
    ui.app()
        .pages
        .iter()
        .map(|page| (page.title.clone(), page.level))
        .collect()
}

/// Saves the open page so its title shows in the page list.
fn save(ui: &mut Ui) {
    ui.h.state_mut().save_now();
    ui.steps(2);
}

#[test]
fn subpages_by_shortcut_fold_and_drag_to_reorder() {
    let mut ui = Ui::start();
    ui.new_page("Alpha");
    save(&mut ui);
    ui.new_page("Beta");
    save(&mut ui);
    let list = titles(&ui);
    let alpha = list.iter().position(|(title, _)| title == "Alpha").unwrap();
    assert_eq!(
        list[alpha + 1].0,
        "Beta",
        "new pages go to the end of the section"
    );

    // Ctrl+Alt+] makes the open page (Beta) a subpage of Alpha.
    ui.press_with(Modifiers::COMMAND | Modifiers::ALT, Key::CloseBracket);
    assert!(titles(&ui).contains(&("Beta".into(), 1)));
    ui.shot("m8-subpages");

    // Folding Alpha hides Beta's row; unfolding shows it again.
    ui.click_label("Collapse Subpages");
    assert!(ui
        .h
        .query_by_role_and_label(egui::Role::ListItem, "Beta")
        .is_none());
    ui.click_label("Expand Subpages");
    assert!(ui
        .h
        .query_by_role_and_label(egui::Role::ListItem, "Beta")
        .is_some());

    // Ctrl+Alt+[ promotes it again.
    ui.press_with(Modifiers::COMMAND | Modifiers::ALT, Key::OpenBracket);
    assert!(titles(&ui).contains(&("Beta".into(), 0)));

    // Drag Beta's row onto the top half of Alpha's: Beta now comes first.
    let alpha_row =
        ui.h.get_by_role_and_label(egui::Role::ListItem, "Alpha")
            .rect();
    let beta_row =
        ui.h.get_by_role_and_label(egui::Role::ListItem, "Beta")
            .rect();
    ui.drag(
        beta_row.center(),
        alpha_row.center_top() + egui::vec2(0.0, 10.0),
    );
    let list = titles(&ui);
    let beta = list.iter().position(|(title, _)| title == "Beta").unwrap();
    let alpha = list.iter().position(|(title, _)| title == "Alpha").unwrap();
    assert!(beta < alpha, "dragged above: {list:?}");
}

#[test]
fn section_groups_hold_sections_dragged_onto_them() {
    let mut ui = Ui::start();
    ui.h.state_mut()
        .handle_library_action(super::LibraryAction::NewGroup(
            "notebook-school".into(),
            None,
        ));
    ui.steps(3);
    ui.type_text("Term 1");
    ui.press(Key::Enter);
    let group_id = ui
        .app()
        .workspace
        .notebooks
        .iter()
        .find(|notebook| notebook.id == "notebook-school")
        .unwrap()
        .groups
        .iter()
        .find(|group| group.name == "Term 1")
        .expect("the new group, after the sample's Last year")
        .id
        .clone();

    // Drag the "History" section onto the group row.
    let history =
        ui.h.get_by_role_and_label(egui::Role::Button, "History")
            .rect();
    let group =
        ui.h.get_by_role_and_label(egui::Role::Button, "Term 1")
            .rect();
    ui.drag(history.center(), group.center());
    let section = ui
        .app()
        .workspace
        .notebooks
        .iter()
        .flat_map(|notebook| notebook.sections.iter())
        .find(|section| section.id == "section-classes")
        .unwrap()
        .clone();
    assert_eq!(section.group_id.as_deref(), Some(group_id.as_str()));
    ui.shot("m8-section-group");

    // Folding the group hides its sections.
    ui.click_label("Term 1");
    assert!(ui
        .h
        .query_by_role_and_label(egui::Role::Button, "History")
        .is_none());

    // A section created in a folded group unfolds it, so the new section shows.
    ui.h.state_mut()
        .handle_library_action(super::LibraryAction::NewSectionIn(
            "notebook-school".into(),
            Some(group_id.clone()),
        ));
    ui.steps(3);
    ui.type_text("Biology");
    ui.press(Key::Enter);
    ui.steps(2);
    assert!(ui
        .h
        .query_by_role_and_label(egui::Role::Button, "Biology")
        .is_some());
    assert!(ui
        .h
        .query_by_role_and_label(egui::Role::Button, "History")
        .is_some());

    // "Move to" menus name a grouped section by its group, in sidebar order.
    let school = |ui: &Ui| {
        ui.app()
            .workspace
            .notebooks
            .iter()
            .find(|notebook| notebook.id == "notebook-school")
            .unwrap()
            .clone()
    };
    let labels: Vec<String> = super::section_choices(&school(&ui))
        .into_iter()
        .map(|(_, label)| label)
        .collect();
    assert_eq!(
        labels[labels.len() - 2..],
        [
            "Term 1 / History".to_string(),
            "Term 1 / Biology".to_string()
        ]
    );

    // Dropped on its notebook's row, a section leaves its group…
    let history =
        ui.h.get_by_role_and_label(egui::Role::Button, "History")
            .rect();
    let notebook =
        ui.h.get_by_role_and_label(egui::Role::Button, "School")
            .rect();
    ui.drag(history.center(), notebook.center());
    let top_level = |ui: &Ui| -> Vec<String> {
        school(ui)
            .sections
            .iter()
            .filter(|section| section.group_id.is_none())
            .map(|section| section.name.clone())
            .collect()
    };
    assert!(top_level(&ui).contains(&"History".to_string()));

    // …and dropped on the top half of another section, it goes above it.
    let history =
        ui.h.get_by_role_and_label(egui::Role::Button, "History")
            .rect();
    let week =
        ui.h.get_by_role_and_label(egui::Role::Button, "This week")
            .rect();
    ui.drag(history.center(), week.center_top() + egui::vec2(0.0, 4.0));
    assert_eq!(top_level(&ui)[0], "History", "{:?}", top_level(&ui));
}

fn page_id(ui: &Ui, title: &str) -> String {
    ui.app()
        .pages
        .iter()
        .find(|page| page.title == title)
        .unwrap_or_else(|| panic!("no page {title:?}"))
        .id
        .clone()
}

fn shows_row(ui: &Ui, title: &str) -> bool {
    ui.h.query_by_role_and_label(egui::Role::ListItem, title)
        .is_some()
}

/// Opens `title`'s page, presses a page shortcut and names the new page `name`.
fn new_page_by_keys(ui: &mut Ui, on: &str, modifiers: Modifiers, name: &str) {
    let id = page_id(ui, on);
    ui.h.state_mut().open_page(&id, false);
    ui.steps(2);
    ui.press_with(modifiers, Key::N);
    ui.steps(1);
    ui.type_text(name);
    save(ui);
}

#[test]
fn ctrl_alt_n_adds_pages_below_and_a_folded_parent_unfolds_for_a_new_subpage() {
    let mut ui = Ui::start();
    ui.new_page("Alpha");
    save(&mut ui);
    ui.new_page("Beta");
    save(&mut ui);
    let below = Modifiers::COMMAND | Modifiers::ALT;
    let subpage = below | Modifiers::SHIFT;

    // Ctrl+Alt+N: a page right below Alpha, at its level. Ctrl+Alt+Shift+N: its first subpage.
    new_page_by_keys(&mut ui, "Alpha", below, "Gamma");
    new_page_by_keys(&mut ui, "Alpha", subpage, "Delta");
    let list = titles(&ui);
    let at = list.iter().position(|(title, _)| title == "Alpha").unwrap();
    let expected: Vec<(String, u8)> = [("Alpha", 0), ("Delta", 1), ("Gamma", 0), ("Beta", 0)]
        .iter()
        .map(|&(title, level)| (title.to_string(), level))
        .collect();
    assert_eq!(list[at..at + 4], expected[..]);

    // A new subpage of a folded page unfolds it, so its row shows (as in the desktop notebook).
    let alpha = page_id(&ui, "Alpha");
    ui.h.state_mut().open_page(&alpha, false);
    ui.steps(2);
    ui.click_label("Collapse Subpages");
    assert!(!shows_row(&ui, "Delta"));
    ui.press_with(subpage, Key::N);
    ui.steps(1);
    ui.type_text("Epsilon");
    save(&mut ui);
    assert!(shows_row(&ui, "Epsilon"));
    assert!(shows_row(&ui, "Delta"));

    // Trashing the open page opens the nearest row that shows above it, never one
    // folded away under a page.
    ui.click_label("Collapse Subpages");
    assert!(!shows_row(&ui, "Delta"));
    let gamma = page_id(&ui, "Gamma");
    ui.h.state_mut().open_page(&gamma, false);
    ui.steps(2);
    ui.h.state_mut()
        .handle_page_list_action(super::PageListAction::Trash(gamma));
    ui.steps(2);
    assert_eq!(
        ui.app().page.as_ref().map(|page| page.id.clone()),
        Some(alpha.clone())
    );

    // Trashing a folded page takes its subpages along.
    ui.h.state_mut()
        .handle_page_list_action(super::PageListAction::Trash(alpha));
    ui.steps(2);
    let list = titles(&ui);
    assert!(
        !list
            .iter()
            .any(|(title, _)| ["Alpha", "Delta", "Epsilon"].contains(&title.as_str())),
        "{list:?}"
    );
    assert!(list.contains(&("Beta".to_string(), 0)));
}

#[test]
fn the_page_list_follows_a_new_page_and_rows_drop_on_sidebar_sections() {
    let mut ui = Ui::start();
    let section = ui.app().page.as_ref().unwrap().section_id.clone();
    {
        let app = ui.h.state_mut();
        for n in 0..14 {
            app.library
                .create_canvas_page(&section, &format!("Filler {n}"))
                .unwrap();
        }
        app.refresh_page_list();
    }
    ui.steps(2);

    // Ctrl+N adds the page at the end of a long section: the list scrolls to its row.
    ui.new_page("Fresh");
    save(&mut ui);
    ui.steps(30);
    let row =
        ui.h.get_by_role_and_label(egui::Role::ListItem, "Fresh")
            .rect();
    assert!(
        row.top() >= 0.0 && row.bottom() <= 900.0,
        "the new page's row is off-screen: {row:?}"
    );

    // Dropping a page row on a section in the sidebar moves the page there.
    let history =
        ui.h.get_by_role_and_label(egui::Role::Button, "History")
            .rect();
    let fresh = page_id(&ui, "Fresh");
    ui.drag(row.center(), history.center());
    assert_eq!(
        ui.app().library.page_section_id(&fresh).unwrap().as_deref(),
        Some("section-classes")
    );
}

/// History › Page Versions opens the desktop notebook's Page Versions task pane beside the page (not a
/// dialog). The pane follows the page opened, and Restore brings back an earlier page.
#[test]
fn versions_open_in_the_task_pane_and_follow_the_page() {
    use super::TaskPane;
    let mut ui = Ui::start();
    let sample = ui.app().page.as_ref().expect("the sample page").id.clone();
    ui.h.state_mut().library.set_version_interval_ms(0);
    ui.new_page("Draft");
    ui.press_with(Modifiers::COMMAND, Key::S); // a blank page is not kept
    ui.press(Key::Enter);
    ui.type_text("First idea");
    ui.press_with(Modifiers::COMMAND, Key::S); // keeps the titled, empty page
    ui.type_text(" and more");
    ui.press_with(Modifiers::COMMAND, Key::S); // keeps "First idea"
    let draft = ui.app().page.as_ref().unwrap().id.clone();

    // History › Page Versions lists them in the task pane.
    ui.ribbon_tab("History");
    ui.ribbon_button("Page Versions");
    let listed = |ui: &Ui| match &ui.app().task_pane {
        Some(TaskPane::Versions(pane)) => (pane.page_id.clone(), pane.versions.len()),
        _ => panic!("the Versions pane is not open"),
    };
    assert_eq!(listed(&ui), (draft.clone(), 2));
    assert!(ui.app().dialog.is_none(), "a pane, not a dialog");
    ui.h.get_by_label(&t!("app-versions-explain"));
    ui.shot("versions-dialog");

    // Another page opened: the pane lists that page's versions instead, and comes back
    // with the draft.
    ui.h.state_mut().open_page(&sample, true);
    ui.steps(3);
    let kept = ui.app().library.page_versions(&sample).unwrap().len();
    assert_eq!(
        listed(&ui),
        (sample.clone(), kept),
        "the pane follows the page"
    );
    ui.h.state_mut().open_page(&draft, true);
    ui.steps(3);
    assert_eq!(listed(&ui), (draft.clone(), 2));

    ui.click_label_containing(&t!("app-restore-version"));
    assert!(ui.app().dialog.is_none());
    // The pane stays open and lists the page as it was before the restore too.
    assert!(matches!(
        &ui.app().task_pane,
        Some(super::TaskPane::Versions(pane)) if pane.versions.len() == 3
    ));
    let page = ui.app().page.as_ref().unwrap();
    let text = page.canvas.plain_text();
    assert!(
        text.contains("First idea") && !text.contains("and more"),
        "{text}"
    );
    let versions = ui.app().library.page_versions(&page.id).unwrap();
    assert_eq!(versions.len(), 3, "the page before the restore is kept too");
}

/// The Versions pane's Delete Version forgets the version picked and keeps the others; the
/// pane stays open. A version row's own menu has Delete Version too.
#[test]
fn deleting_one_version_from_the_pane() {
    use super::TaskPane;
    let mut ui = Ui::start();
    ui.h.state_mut().library.set_version_interval_ms(0);
    ui.new_page("Draft");
    ui.press_with(Modifiers::COMMAND, Key::S); // a blank page is not kept
    ui.press(Key::Enter);
    ui.type_text("One");
    ui.press_with(Modifiers::COMMAND, Key::S); // keeps the titled, empty page
    ui.type_text(" two");
    ui.press_with(Modifiers::COMMAND, Key::S); // keeps "One"
    ui.type_text(" three");
    ui.press_with(Modifiers::COMMAND, Key::S); // keeps "One two"
    ui.type_text(" four");
    ui.press_with(Modifiers::COMMAND, Key::S); // keeps "One two three"
    let draft = ui.app().page.as_ref().unwrap().id.clone();
    ui.ribbon_tab("History");
    ui.ribbon_button("Page Versions");
    let versions = |ui: &Ui| match &ui.app().task_pane {
        Some(TaskPane::Versions(pane)) => pane.versions.clone(),
        _ => panic!("the Versions pane is not open"),
    };
    let previewed = |ui: &Ui| match &ui.app().task_pane {
        Some(TaskPane::Versions(pane)) => pane.versions[pane.selected].id,
        _ => panic!("the Versions pane is not open"),
    };
    // A version's row, as the pane names it.
    let row = |ui: &Ui, version: &crate::model::PageVersion| {
        let label = format!(
            "{} · {}",
            crate::util::relative_time(version.saved_at),
            t!("common-word-count", count = version.word_count)
        );
        ui.h.get_by_role_and_label(egui::Role::Button, &label)
            .rect()
    };
    let kept = versions(&ui);
    assert_eq!(kept.len(), 4);

    // The second picked, then Delete Version: the next one is previewed.
    ui.click(row(&ui, &kept[1]).center());
    ui.steps(30);
    ui.click_label_containing(&t!("app-delete-version"));
    let left: Vec<i64> = versions(&ui).iter().map(|version| version.id).collect();
    assert_eq!(left, [kept[0].id, kept[2].id, kept[3].id]);
    let stored = ui.app().library.page_versions(&draft).unwrap();
    assert_eq!(stored.len(), 3);
    assert_eq!(ui.app().toast.message, t!("toast-version-deleted"));
    assert_eq!(previewed(&ui), kept[2].id);

    // The newest row's menu › Delete Version: the version previewed stays previewed.
    ui.steps(30);
    ui.right_click(row(&ui, &kept[0]).center());
    click_menu_row(&mut ui, &t!("app-delete-version"), "");
    let left: Vec<i64> = versions(&ui).iter().map(|version| version.id).collect();
    assert_eq!(left, [kept[2].id, kept[3].id]);
    assert_eq!(previewed(&ui), kept[2].id, "the preview is not swapped");

    // The oldest row's menu › Delete Version.
    ui.steps(30);
    ui.right_click(row(&ui, &kept[3]).center());
    click_menu_row(&mut ui, &t!("app-delete-version"), "");
    let left: Vec<i64> = versions(&ui).iter().map(|version| version.id).collect();
    assert_eq!(left, [kept[2].id]);
    assert_eq!(ui.app().page.as_ref().unwrap().id, draft, "the page stays");
}

/// A file dropped from the file manager, as egui-winit hands it over.
#[derive(Debug)]
struct DroppedPath(std::path::PathBuf);

impl egui::DroppedFile for DroppedPath {
    fn path(&self) -> &std::path::Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|error| error.to_string())
    }
}

#[test]
fn dropped_files_become_cards_with_open_and_save() {
    let mut ui = Ui::start();
    ui.new_page("Homework");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("worksheet.pdf");
    std::fs::write(&path, b"%PDF-1.4").unwrap();
    let spot = ui.empty_spot(0.0, 0.0);
    ui.h.event(Event::PointerMoved(spot));
    ui.h.input_mut()
        .dropped_files
        .push(std::sync::Arc::new(DroppedPath(path)));
    ui.steps(3);
    // A PDF asks first, as in the desktop notebook: attach it, or insert it as a printout.
    ui.click_label("Attach file");
    ui.steps(2);
    let file = ui
        .items()
        .into_iter()
        .find(|item| matches!(item.kind, CanvasKind::File { .. }))
        .expect("a file card");
    assert_eq!(text_of(&file), "worksheet.pdf");

    // Its menu offers Open and Save as…; Open hands a copy, under the file's own name,
    // to the system's app (which tests only record) and says edits stay out of the page.
    let opened = || -> Vec<String> {
        std::mem::take(&mut *crate::canvas::OPENED.lock().unwrap())
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().to_string())
            .collect()
    };
    opened();
    let center = ui.screen(file.rect().center());
    ui.right_click(center);
    assert!(menu_item_rect(&ui, "Save As…").is_some());
    click_menu_row(&mut ui, "Open", "");
    ui.steps(2);
    assert_eq!(opened(), ["worksheet.pdf"]);
    assert!(
        ui.app()
            .toast
            .message
            .contains("aren't saved into the page"),
        "{}",
        ui.app().toast.message
    );
    // So do Enter, a double-click and the menu again once the card is selected.
    ui.press(Key::Enter);
    ui.steps(2);
    assert_eq!(opened(), ["worksheet.pdf"], "Enter");
    // (Long enough after the menu's click that egui doesn't count it in.)
    ui.steps(30);
    ui.double_click(center);
    ui.steps(2);
    assert_eq!(opened(), ["worksheet.pdf"], "double-click");
    ui.steps(30);
    ui.right_click(center);
    click_menu_row(&mut ui, "Open", "");
    ui.steps(2);
    assert_eq!(opened(), ["worksheet.pdf"], "the selected card's menu");
    ui.shot("m10-file-card");

    // Pasted onto another page, the card gets its own copy of the file.
    ui.h.event(Event::Copy);
    ui.h.step();
    let copied =
        ui.h.output()
            .platform_output
            .commands
            .iter()
            .find_map(|command| match command {
                egui::OutputCommand::CopyText(text) => Some(text.clone()),
                _ => None,
            })
            .expect("the card went to the clipboard");
    ui.steps(1);
    ui.new_page("Other page");
    ui.press(Key::Escape);
    ui.h.event(Event::Paste(copied));
    ui.steps(3);
    let pasted = ui
        .items()
        .into_iter()
        .find_map(|item| match item.kind {
            CanvasKind::File { attachment, .. } => Some(attachment),
            _ => None,
        })
        .expect("the pasted card");
    let CanvasKind::File { attachment, .. } = &file.kind else {
        unreachable!()
    };
    assert_ne!(&pasted, attachment, "the copy has its own attachment");
    let meta = ui.app().library.attachment(&pasted).unwrap().unwrap();
    assert_eq!(meta.page_id, ui.app().page.as_ref().unwrap().id);
    assert_eq!(meta.file_name, "worksheet.pdf");
    let stored = ui.app().library.attachment_path(&meta.stored_path);
    assert_eq!(std::fs::read(stored).unwrap(), b"%PDF-1.4");

    // The Insert ribbon has a File Attachment button, and at the default window it fits
    // with its label showing (an icon-only button is 30 px wide).
    ui.ribbon_tab("Insert");
    let button = ui.ribbon_button_rect("File Attachment");
    assert!(
        button.right() <= 1320.0 && button.width() > 40.0,
        "the Insert ribbon fits: {button:?}"
    );
}

fn printout_pages(ui: &Ui) -> Vec<CanvasItem> {
    let mut pages: Vec<CanvasItem> = ui
        .items()
        .into_iter()
        .filter(|item| item.kind.printout().is_some())
        .collect();
    pages.sort_by_key(|item| item.kind.printout().map(|printout| printout.page));
    pages
}

#[test]
fn a_dropped_pdf_becomes_a_printout_under_its_icon() {
    let mut ui = Ui::start();
    ui.new_page("Fractions");
    let attachments = ui.app().library.attachment_count().unwrap();
    let _dir = ui.drop_pdf(
        "worksheet.pdf",
        &["Fractions page one", "Fractions page two"],
    );
    assert!(
        ui.h.query_by_label("Insert printout").is_some(),
        "the desktop notebook's question"
    );
    ui.shot("p8-printout-choice");
    ui.click_label("Insert printout");
    ui.wait_for_printout();

    let items = ui.items();
    let icon = items
        .iter()
        .find(|item| matches!(&item.kind, CanvasKind::File { name, .. } if name == "worksheet.pdf"))
        .expect("the PDF's icon heads the printout");
    let pages = printout_pages(&ui);
    assert_eq!(pages.len(), 2);
    // Stacked under the icon, left edges lined up, at printed size (Letter at 96/in).
    assert_eq!(pages[0].x, icon.x);
    assert!(pages[0].y >= icon.y + icon.height);
    assert!(pages[1].y >= pages[0].y + pages[0].height);
    assert_eq!((pages[0].width, pages[0].height), (816.0, 1056.0));
    let source = pages[0].kind.printout().unwrap().source.clone();
    assert_eq!(
        Some(source.as_str()),
        icon.kind.attachment(),
        "pages point at the PDF"
    );
    // Its words are searchable, as in the desktop notebook.
    let text = ui.app().page.as_ref().unwrap().canvas.plain_text();
    assert!(text.contains("Fractions page two"), "{text}");
    assert_eq!(
        ui.app().library.attachment_count().unwrap(),
        attachments + 3,
        "the PDF and one picture per page"
    );
    ui.shot("p8-printout");

    // One undo takes the whole printout away.
    ui.press(Key::Escape);
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert!(printout_pages(&ui).is_empty());
    assert!(ui
        .items()
        .iter()
        .all(|item| !matches!(item.kind, CanvasKind::File { .. })));
}

#[test]
fn a_dropped_pdf_can_still_be_attached() {
    let mut ui = Ui::start();
    ui.new_page("Homework");
    let _dir = ui.drop_pdf("worksheet.pdf", &["one"]);
    ui.click_label("Attach file");
    ui.steps(2);
    assert!(printout_pages(&ui).is_empty());
    assert!(ui.items().iter().any(
        |item| matches!(&item.kind, CanvasKind::File { name, .. } if name == "worksheet.pdf")
    ));
}

#[test]
fn a_locked_pdf_asks_for_its_password_and_cancel_leaves_nothing() {
    let mut ui = Ui::start();
    ui.new_page("Locked");
    let attachments = ui.app().library.attachment_count().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("locked.pdf");
    std::fs::write(&path, include_bytes!("../tests/fixtures/locked-aes128.pdf")).unwrap();
    ui.drop_file(path.clone());
    ui.click_label("Insert printout");
    ui.wait_for_printout();
    assert!(ui.app().canvas.printout_asks_password());
    ui.shot("p8-printout-password");

    // A wrong password asks again and says so.
    ui.type_text("nope");
    ui.click_label("OK");
    ui.wait_for_printout();
    assert!(ui.app().canvas.printout_asks_password());
    assert!(ui.h.query_by_label_contains("didn’t work").is_some());

    // Cancel: nothing on the page, nothing kept.
    ui.click_label("Cancel");
    ui.steps(2);
    assert!(!ui.app().canvas.printout_busy());
    assert!(ui.items().is_empty());
    assert_eq!(ui.app().library.attachment_count().unwrap(), attachments);

    // The right password inserts it.
    ui.drop_file(path);
    ui.click_label("Insert printout");
    ui.wait_for_printout();
    ui.type_text("secret");
    ui.click_label("OK");
    ui.wait_for_printout();
    assert_eq!(printout_pages(&ui).len(), 1);
}

#[test]
fn insert_file_printout_from_the_ribbon() {
    let mut ui = Ui::start();
    ui.new_page("Reading");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("chapter.pdf");
    std::fs::write(
        &path,
        crate::pdf::letter_pdf(&["Chapter one", "Chapter two", "Chapter three"]),
    )
    .unwrap();
    *crate::canvas::PICKED_PDF.lock().unwrap() = Some(path);
    ui.ribbon_tab("Insert");
    ui.ribbon_button("File Printout");
    ui.wait_for_printout();
    assert_eq!(printout_pages(&ui).len(), 3);
}

/// A click on paper starts a note there; Insert › File Printout then puts the printout
/// at that spot, and the note, still empty, goes (as clicking elsewhere would end it),
/// leaving no step of its own to undo.
#[test]
fn a_printout_goes_where_the_paper_was_clicked_without_an_empty_note() {
    let mut ui = Ui::start();
    ui.new_page("Reading");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("chapter.pdf");
    std::fs::write(&path, crate::pdf::letter_pdf(&["Chapter one"])).unwrap();
    let spot = ui.empty_paper_point();
    ui.click(spot);
    let note = ui.items()[0].clone();
    assert!(note.doc().is_some(), "a click on paper starts a note");
    *crate::canvas::PICKED_PDF.lock().unwrap() = Some(path);
    ui.ribbon_tab("Insert");
    ui.ribbon_button("File Printout");
    ui.wait_for_printout();

    let items = ui.items();
    assert!(
        items.iter().all(|item| item.doc().is_none()),
        "no empty note left behind: {items:?}"
    );
    let icon = items
        .iter()
        .find(|item| matches!(item.kind, CanvasKind::File { .. }))
        .expect("the PDF's icon");
    assert_eq!(
        (icon.x, icon.y),
        ((note.x + crate::canvas::NOTE_PAD_X).round(), note.y.round()),
        "at the click"
    );
    assert_eq!(printout_pages(&ui).len(), 1);
    // One undo takes the printout away, and nothing brings the empty note back.
    ui.press(Key::Escape);
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert!(ui.items().is_empty());
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert!(ui.items().is_empty());
}

/// Restore Original Size gives a printout page back its printed size, not a picture's.
#[test]
fn a_printout_page_restores_to_its_printed_size() {
    let mut ui = Ui::start();
    ui.new_page("Sizes");
    let _dir = ui.drop_pdf("letter.pdf", &["One"]);
    ui.click_label("Insert printout");
    ui.wait_for_printout();
    let sheet = printout_pages(&ui)[0].clone();
    assert_eq!((sheet.width, sheet.height), (816.0, 1056.0));
    // Made smaller (as a corner drag would).
    let page = ui.h.state_mut().page.as_mut().unwrap();
    if let Some(item) = page.canvas.item_mut(sheet.id) {
        (item.width, item.height) = (408.0, 528.0);
    }
    ui.steps(2);
    let at = ui.screen(sheet.rect().min + egui::vec2(200.0, 150.0));
    assert!(ui.app().canvas.viewport().contains(at), "in view");
    ui.click(at);
    ui.right_click(at);
    ui.click_label_containing("Restore Original Size");
    ui.steps(2);
    let sheet = printout_pages(&ui)[0].clone();
    assert_eq!((sheet.width, sheet.height), (816.0, 1056.0));
}

/// PDFs dropped together become printouts one after the other, each below the last,
/// rather than piled over each other.
#[test]
fn pdfs_dropped_together_are_printed_one_below_the_other() {
    let mut ui = Ui::start();
    ui.new_page("Two");
    let dir = tempfile::tempdir().unwrap();
    let spot = ui.empty_paper_point();
    ui.h.event(Event::PointerMoved(spot));
    for (name, words) in [("first.pdf", "First sheet"), ("second.pdf", "Second sheet")] {
        let path = dir.path().join(name);
        std::fs::write(&path, crate::pdf::letter_pdf(&[words])).unwrap();
        ui.h.input_mut()
            .dropped_files
            .push(std::sync::Arc::new(DroppedPath(path)));
    }
    ui.steps(3);
    ui.click_label("Insert printout");
    // The first one is placed, then the second one's question comes.
    for _ in 0..1000 {
        ui.h.step();
        if ui.h.query_by_label("Insert printout").is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    ui.steps(2);
    ui.click_label("Insert printout");
    ui.wait_for_printout();

    let pages = ui.items();
    let sheet = |words: &str| {
        pages
            .iter()
            .find(|item| {
                item.kind
                    .printout()
                    .is_some_and(|page| page.text.contains(words))
            })
            .unwrap_or_else(|| panic!("{words}"))
            .clone()
    };
    let (first, second) = (sheet("First sheet"), sheet("Second sheet"));
    let second_icon = pages
        .iter()
        .find(|item| matches!(&item.kind, CanvasKind::File { name, .. } if name == "second.pdf"))
        .expect("the second PDF's icon");
    assert!(
        second_icon.y >= first.y + first.height,
        "the second printout starts below the first"
    );
    assert!(second.y > second_icon.y);
}

#[test]
fn a_printout_page_menu_copies_its_text_and_opens_the_pdf() {
    let mut ui = Ui::start();
    ui.new_page("Science");
    let _dir = ui.drop_pdf("cells.pdf", &["Cells are small", "Plants have walls"]);
    ui.click_label("Insert printout");
    ui.wait_for_printout();
    let second = printout_pages(&ui)[1].clone();
    // The second page starts below the window: scroll it into view, then right-click it.
    for _ in 0..10 {
        let view = ui.app().canvas.viewport().shrink(40.0);
        let center = ui.screen(second.rect().center());
        if view.contains(center) {
            break;
        }
        let over = view.center();
        ui.wheel(over, egui::vec2(0.0, over.y - center.y), Modifiers::NONE);
    }
    let center = ui.screen(second.rect().center());
    assert!(
        ui.app().canvas.viewport().contains(center),
        "the second page is in view"
    );
    ui.click(center);
    ui.right_click(center);
    let outputs = ui.click_label_outputs("Copy Text from This Page of the Printout");
    assert_eq!(copied(&outputs), ["Plants have walls"]);

    ui.right_click(center);
    let outputs = ui.click_label_outputs("Copy Text from All Pages of the Printout");
    assert_eq!(copied(&outputs), ["Cells are small\n\nPlants have walls"]);

    crate::canvas::OPENED.lock().unwrap().clear();
    ui.right_click(center);
    ui.click_label_containing("Open Original");
    ui.steps(2);
    let opened: Vec<String> = crate::canvas::OPENED
        .lock()
        .unwrap()
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    assert_eq!(opened, ["cells.pdf"]);
}

#[test]
fn an_attached_pdf_can_become_a_printout_later() {
    let mut ui = Ui::start();
    ui.new_page("Later");
    let _dir = ui.drop_pdf("notes.pdf", &["First", "Second"]);
    ui.click_label("Attach file");
    ui.steps(2);
    let card = ui
        .items()
        .into_iter()
        .find(|item| matches!(item.kind, CanvasKind::File { .. }))
        .unwrap();
    ui.right_click(ui.screen(card.rect().center()));
    ui.click_label_containing("Insert Printout");
    ui.wait_for_printout();
    let pages = printout_pages(&ui);
    assert_eq!(pages.len(), 2);
    assert!(pages[0].y >= card.y + card.height, "under the card");
    assert_eq!(
        ui.items()
            .iter()
            .filter(|item| matches!(item.kind, CanvasKind::File { .. }))
            .count(),
        1,
        "no second icon"
    );
}

#[test]
fn audio_notes_show_a_player_and_the_ribbon_fits() {
    let mut ui = Ui::start();
    ui.new_page("Lecture");
    let bytes = crate::audio::wav_bytes(crate::audio::TARGET_RATE, &[0; 16_000]);
    let (id, attachment) = {
        let app = ui.h.state_mut();
        let page = app.page.as_mut().unwrap();
        let attachment = app
            .library
            .save_attachment_bytes(&page.id, "wav", "audio/wav", &bytes, "clip.wav")
            .unwrap();
        let center = app.canvas.scene_rect.unwrap().center();
        let id = page.canvas.add(CanvasItem::new(
            CanvasKind::Audio {
                attachment: attachment.id.clone(),
                duration_ms: 0,
            },
            center.x - 150.0,
            center.y - 32.0,
            300.0,
            64.0,
        ));
        (id, attachment.id)
    };
    ui.steps(3);
    assert!(ui
        .h
        .get_all_by_label_contains("Play recording")
        .next()
        .is_some());
    // A clip saved without its length (Folio closed mid-recording) shows the file's.
    assert_eq!(ui.app().canvas.audio_length(&attachment), Some(1000));

    // With the shape tool, a drag that starts on the seek track draws a shape: the
    // clip's controls only take the pointer with the Type tool.
    let card = ui.item(id).rect();
    ui.h.state_mut().canvas.tool = CanvasTool::Shape(ShapeKind::Rect);
    let from = ui.screen(egui::pos2(card.left() + 100.0, card.bottom() - 9.0));
    ui.drag(from, from + egui::vec2(140.0, 90.0));
    assert!(
        ui.items()
            .iter()
            .any(|item| matches!(item.kind, CanvasKind::Shape { .. })),
        "{:?}",
        ui.items()
    );
    assert!(
        ui.app().canvas.player_attachment().is_none(),
        "nothing plays"
    );
    ui.h.state_mut().canvas.tool = CanvasTool::Select;
    ui.steps(2);

    // Its menu offers playing and saving (not clicked: no sound device in tests).
    let card = ui.item(id).rect();
    let spot = ui.screen(card.center() + egui::vec2(60.0, 0.0));
    ui.right_click(spot);
    assert!(ui
        .h
        .get_all_by_label_contains("Play / pause")
        .next()
        .is_some());
    assert!(ui.h.get_all_by_label_contains("Save As…").next().is_some());
    ui.press(Key::Escape);

    // The Insert ribbon has File Attachment and Record Audio, and both fit the default
    // window labelled.
    ui.ribbon_tab("Insert");
    ui.shot("m10-insert-band");
    for label in ["File Attachment", "Record Audio", "Link"] {
        let button = ui.ribbon_button_rect(label);
        assert!(
            button.right() <= 1320.0 && button.width() > 40.0,
            "{label} fits the Insert ribbon: {button:?}"
        );
    }
}

#[test]
fn pasted_file_paths_are_attached_only_when_confirmed() {
    let mut ui = Ui::start();
    ui.new_page("Pasted paths");
    ui.press(Key::Escape);
    let spot = ui.empty_spot(0.0, 0.0);
    ui.click(spot);
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    assert!(ui.items().is_empty());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("id_ed25519");
    std::fs::write(&path, b"-----BEGIN OPENSSH PRIVATE KEY-----").unwrap();
    let uri = format!("file://{}", path.display());
    let files = |ui: &Ui| {
        ui.items()
            .iter()
            .filter(|item| matches!(item.kind, CanvasKind::File { .. }))
            .count()
    };
    let attachments = |ui: &Ui| ui.app().library.attachment_count().unwrap();
    let asking = |ui: &Ui| {
        ui.h.query_by_label("Insert files from your computer?")
            .is_some()
    };
    let before = attachments(&ui);

    // Text that names a file asks first, showing the full path, and attaches nothing yet.
    ui.h.event(Event::Paste(uri.clone()));
    ui.steps(3);
    assert!(asking(&ui));
    assert!(ui.h.query_by_label(&path.display().to_string()).is_some());
    assert_eq!((files(&ui), attachments(&ui)), (0, before));
    ui.shot("m10-pasted-files");
    ui.click_label("Cancel");
    ui.steps(2);
    assert!(!asking(&ui));
    assert_eq!((files(&ui), attachments(&ui)), (0, before));
    // Escape cancels too.
    ui.h.event(Event::Paste(uri.clone()));
    ui.steps(3);
    assert!(asking(&ui));
    ui.press(Key::Escape);
    ui.steps(2);
    assert!(!asking(&ui));
    assert!(ui.items().is_empty());
    assert_eq!(attachments(&ui), before);

    // Attach: exactly one file card.
    ui.h.event(Event::Paste(uri.clone()));
    ui.steps(3);
    ui.click_label_containing("Attach");
    ui.steps(2);
    assert!(!asking(&ui));
    assert_eq!((files(&ui), attachments(&ui)), (1, before + 1));

    // Paste as text: the words land in a note and nothing more is attached.
    ui.press(Key::Escape);
    ui.h.event(Event::Paste(uri.clone()));
    ui.steps(3);
    ui.click_label("Paste as text");
    ui.steps(2);
    assert!(!asking(&ui));
    let notes: Vec<String> = ui
        .items()
        .iter()
        .filter(|item| matches!(item.kind, CanvasKind::Text { .. }))
        .map(text_of)
        .collect();
    assert_eq!(notes, [uri]);
    assert_eq!((files(&ui), attachments(&ui)), (1, before + 1));
}

#[test]
fn a_recording_can_be_seen_and_stopped_from_any_page() {
    let mut ui = Ui::start();
    ui.new_page("Lecture");
    ui.press(Key::Escape);
    // (Tests have no microphone: this records silence, as Insert → Audio would.)
    let start = |ui: &mut Ui| {
        let app = ui.h.state_mut();
        let page = app.page.as_mut().unwrap();
        let clip = crate::canvas::start_test_recording(page, &mut app.canvas, &mut app.library);
        app.mark_dirty(0.0);
        ui.steps(2);
        clip
    };
    let recording = |ui: &Ui| ui.app().canvas.recording_time();
    let has_clip = |ui: &Ui, clip: &str| {
        ui.items()
            .iter()
            .any(|item| item.kind.attachment() == Some(clip))
    };
    let clip = start(&mut ui);
    let lecture = ui.app().page.as_ref().unwrap().id.clone();
    // While recording, the ribbon shows the contextual Audio & Video tab, which holds the
    // recording's Pause and Stop (there is no top bar any more).
    ui.ribbon_tab("Audio & Video");
    assert_eq!(ui.app().ribbon.tab, RibbonTab::Recording);
    ui.ribbon_button_rect("Stop");
    assert!(ui.h.query_by_label("Stop recording").is_none());
    // Even at the smallest window, paused (the wider "Resume"), the tab and its buttons
    // sit inside the window, clear of each other.
    ui.h.set_size(egui::vec2(980.0, 900.0));
    ui.steps(3);
    ui.ribbon_button("Pause");
    ui.steps(2);
    assert_eq!(recording(&ui).map(|(_, paused)| paused), Some(true));
    let tab = ui.ribbon_tab_rect("Audio & Video");
    let stop = ui.ribbon_button_rect("Stop");
    let resume = ui.ribbon_button_rect("Resume");
    assert!(
        tab.right() <= 980.0 && resume.right() <= stop.left() && stop.right() <= 980.0,
        "tab {tab:?}, resume {resume:?}, stop {stop:?}"
    );
    ui.shot("m10-recording-narrow");
    ui.ribbon_button("Resume");
    ui.h.set_size(egui::vec2(1320.0, 900.0));
    ui.steps(3);
    assert_eq!(recording(&ui).map(|(_, paused)| paused), Some(false));

    // Another page shares the ribbon, so its Audio & Video tab is there too.
    let other = {
        let app = ui.h.state_mut();
        let section = app.page.as_ref().unwrap().section_id.clone();
        app.library
            .create_canvas_page(&section, "Old notes")
            .unwrap()
            .id
    };
    ui.h.state_mut().open_page(&other, false);
    ui.steps(3);
    assert_eq!(ui.app().page.as_ref().unwrap().id, other);
    assert!(recording(&ui).is_some(), "it carries on");
    ui.ribbon_tab("Audio & Video");
    ui.shot("m10-recording-other-page");
    ui.ribbon_button("Pause");
    assert_eq!(recording(&ui).map(|(_, paused)| paused), Some(true));
    ui.ribbon_button("Stop");
    assert!(recording(&ui).is_none());
    // The tab leaves with the recording.
    assert!(ui.h.query_by_label("Resume").is_none());
    assert!(ui
        .h
        .query_by_role_and_label(egui::Role::Tab, "Audio & Video")
        .is_none());
    assert_eq!(ui.app().ribbon.tab, RibbonTab::Home);
    // Its clip stays on its page, and the file is complete.
    let saved = ui.app().library.get_page(&lecture).unwrap().unwrap();
    assert!(saved
        .canvas
        .items
        .iter()
        .any(|item| item.kind.attachment() == Some(clip.as_str())));
    let meta = ui.app().library.attachment(&clip).unwrap().unwrap();
    assert_eq!(meta.byte_size, 44 + 3200);

    // A microphone that stops working ends the recording with a message, whatever is open.
    ui.h.state_mut().open_page(&lecture, false);
    ui.steps(3);
    start(&mut ui);
    ui.app()
        .canvas
        .fail_recording("the microphone was unplugged");
    ui.steps(2);
    assert!(recording(&ui).is_none());
    assert!(
        ui.app()
            .toast
            .message
            .contains("the microphone was unplugged"),
        "{}",
        ui.app().toast.message
    );

    // A clip that undo took off the page comes back when its recording stops.
    let again = start(&mut ui);
    assert!(has_clip(&ui, &again));
    ui.press_with(Modifiers::COMMAND, Key::Z);
    ui.steps(2);
    assert!(!has_clip(&ui, &again));
    ui.ribbon_tab("Audio & Video");
    ui.ribbon_button("Stop");
    assert!(recording(&ui).is_none());
    assert!(has_clip(&ui, &again), "the clip is back");

    // Sending its page to the trash stops the recording first.
    start(&mut ui);
    ui.h.state_mut()
        .handle_page_list_action(super::PageListAction::Trash(lecture.clone()));
    ui.steps(2);
    assert!(recording(&ui).is_none());
}

#[test]
fn equations_type_live_undo_as_one_step_and_vanish_when_emptied() {
    let mut ui = Ui::start();
    ui.new_page("Maths");
    ui.h.state_mut().ribbon.tab = RibbonTab::Insert;
    ui.steps(2);
    // The Insert ribbon fits the default window with Equation labelled.
    for label in ["Equation", "Link"] {
        let button = ui.ribbon_button_rect(label);
        assert!(
            button.right() <= 1320.0 && button.width() > 40.0,
            "{label} fits the Insert ribbon: {button:?}"
        );
    }
    // No note is being typed in (the caret is in the title), so Equation makes a new note
    // holding an equation block, whose source field takes the keyboard.
    ui.ribbon_button("Equation");
    ui.steps(2);
    ui.type_text("x^2+1/2");
    // What is typed shows in the block at once.
    let blocks = equation_blocks(&ui);
    assert_eq!(blocks.len(), 1);
    let note = blocks[0].0;
    assert_eq!(source_of(&blocks[0].1), "x^2+1/2");
    // While it is typed, its contextual Equation tab copies it as LaTeX.
    ui.ribbon_tab("Equation");
    let outputs = ui.ribbon_button_outputs("Copy LaTeX");
    assert_eq!(copied(&outputs), ["x^{2}+\\frac{1}{2}"]);
    ui.press(Key::Enter);
    assert_eq!(ui.items().len(), 1);
    assert_eq!(sources(&ui), ["x^2+1/2"]);
    ui.shot("equation");

    // Inserting and typing is one undo step.
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert!(equation_blocks(&ui).is_empty());
    ui.press_with(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    assert_eq!(sources(&ui), ["x^2+1/2"]);

    // A click on the formula edits it again; emptying it and pressing Enter removes it,
    // and undo brings it back.
    let block = equation_blocks(&ui)[0].1.id;
    ui.click(formula_point(&ui, note, block));
    ui.press(Key::Backspace);
    assert_eq!(sources(&ui), ["x^2+1/"], "the click opened the source");
    for _ in 1.."x^2+1/2".len() {
        ui.press(Key::Backspace);
    }
    assert_eq!(sources(&ui), [""]);
    ui.press(Key::Enter);
    assert!(
        equation_blocks(&ui).is_empty(),
        "an emptied equation is removed"
    );
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert_eq!(sources(&ui), ["x^2+1/2"]);

    // Alt+= while typing starts a new, empty equation at the caret: the "=" the same key
    // press types is kept out of it and out of the paragraph.
    let after = ui.item(note).doc().unwrap().blocks.last().unwrap().id;
    ui.click(ui.text_point(note, after, 0));
    ui.type_text("Then");
    let key = |pressed| Event::Key {
        key: Key::Equals,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: Modifiers::ALT,
    };
    ui.h.input_mut()
        .events
        .extend([key(true), Event::Text("=".into()), key(false)]);
    ui.steps(3);
    assert_eq!(sources(&ui), ["x^2+1/2", ""]);
    ui.type_text("\\alpha");
    ui.press(Key::Escape);
    assert_eq!(sources(&ui), ["x^2+1/2", "\\alpha"]);
    let texts: Vec<String> = ui
        .item(note)
        .doc()
        .unwrap()
        .blocks
        .iter()
        .map(|block| block.text.text.clone())
        .collect();
    assert!(texts.contains(&"Then".to_string()), "{texts:?}");
    // The fraction makes its formula taller than the one-line one.
    let blocks = equation_blocks(&ui);
    let height = |block: &crate::model::Block| ui.row(note, block.id).rect.height();
    assert!(
        height(&blocks[0].1) > height(&blocks[1].1) + 8.0,
        "the fraction makes the formula taller"
    );
}

fn equation_size(item: &CanvasItem) -> f32 {
    match item.kind {
        CanvasKind::Equation { size, .. } => size,
        ref other => panic!("expected an equation, got {other:?}"),
    }
}

fn equations(ui: &Ui) -> Vec<CanvasItem> {
    ui.items()
        .into_iter()
        .filter(|item| matches!(item.kind, CanvasKind::Equation { .. }))
        .collect()
}

/// Every equation block in the notes on the open page, with its note's id.
fn equation_blocks(ui: &Ui) -> Vec<(u64, crate::model::Block)> {
    ui.items()
        .iter()
        .filter_map(|item| Some((item.id, item.doc()?.blocks.clone())))
        .flat_map(|(note, blocks)| {
            blocks
                .into_iter()
                .filter(|block| matches!(block.kind, crate::model::BlockKind::Equation { .. }))
                .map(move |block| (note, block))
        })
        .collect()
}

/// An equation block's source.
fn source_of(block: &crate::model::Block) -> String {
    match &block.kind {
        crate::model::BlockKind::Equation { source, .. } => source.clone(),
        other => panic!("expected an equation block, got {other:?}"),
    }
}

/// The sources of the equation blocks on the open page, in order.
fn sources(ui: &Ui) -> Vec<String> {
    equation_blocks(ui)
        .iter()
        .map(|(_, block)| source_of(block))
        .collect()
}

/// A screen point on the formula of equation block `block` in note `note`, to click it.
fn formula_point(ui: &Ui, note: u64, block: u64) -> Pos2 {
    let row = ui.row(note, block).rect;
    ui.screen(row.left_top() + egui::vec2(16.0, 12.0))
}

#[test]
fn the_ribbon_undo_while_typing_an_equation_takes_back_only_that() {
    let mut ui = Ui::start();
    ui.new_page("Undo maths");
    let spot = ui.empty_spot(-150.0, -100.0);
    ui.click(spot);
    ui.type_text("Keep");
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    let keep = |ui: &Ui| ui.items().iter().any(|item| text_of(item) == "Keep");
    // Alt+= with no note being typed in makes a new note where the pointer is.
    let paper = ui.empty_paper_point();
    ui.h.event(Event::PointerMoved(paper));
    ui.steps(2);

    // Inserted, nothing typed yet: the click on Undo takes back the insert, not the note.
    ui.press_with(Modifiers::ALT, Key::Equals);
    assert_eq!(equation_blocks(&ui).len(), 1);
    ui.click_label("Undo");
    assert!(equation_blocks(&ui).is_empty());
    assert!(keep(&ui), "the note typed before is still there");

    // Inserted and typed in: the same.
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    ui.h.event(Event::PointerMoved(paper));
    ui.press_with(Modifiers::ALT, Key::Equals);
    ui.type_text("x+1");
    assert_eq!(sources(&ui), ["x+1"]);
    ui.click_label("Undo");
    assert!(equation_blocks(&ui).is_empty());
    assert!(keep(&ui));

    // Typed into an existing equation: Undo takes back that typing; Redo restores it.
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    ui.h.event(Event::PointerMoved(paper));
    ui.press_with(Modifiers::ALT, Key::Equals);
    ui.type_text("a");
    ui.press(Key::Enter);
    let (note, block) = equation_blocks(&ui)
        .first()
        .map(|(note, block)| (*note, block.id))
        .expect("an equation block");
    ui.click(formula_point(&ui, note, block));
    ui.type_text("+b");
    assert_eq!(sources(&ui), ["a+b"]);
    ui.click_label("Undo");
    assert_eq!(sources(&ui), ["a"]);
    assert!(keep(&ui));
    ui.click_label("Redo");
    assert_eq!(sources(&ui), ["a+b"]);

    // Deleting a new, empty one from the ribbon leaves no undo step behind.
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    let depth = ui.app().canvas.undo_depth();
    let elsewhere = ui.empty_paper_point();
    ui.h.event(Event::PointerMoved(elsewhere));
    ui.press_with(Modifiers::ALT, Key::Equals);
    assert_eq!(equation_blocks(&ui).len(), 2);
    ui.ribbon_button("Delete");
    assert_eq!(sources(&ui), ["a+b"]);
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    assert_eq!(ui.app().canvas.undo_depth(), depth);

    // Insert ▸ Equation while a new one is still empty replaces it. (The equation being
    // typed has its contextual tab, also called Equation: the button is the one clicked.)
    ui.ribbon_tab("Insert");
    ui.ribbon_button("Equation");
    ui.ribbon_button("Equation");
    assert_eq!(sources(&ui), ["a+b", ""], "one empty equation, besides a+b");
    ui.press(Key::Escape);
    assert_eq!(sources(&ui), ["a+b"], "the empty one goes");
}

#[test]
fn a_picture_pasted_while_typing_an_equation_undoes_in_order() {
    let mut ui = Ui::start();
    ui.new_page("Paste maths");
    ui.press(Key::Escape);
    ui.press_with(Modifiers::ALT, Key::Equals);
    ui.type_text("a");
    ui.press(Key::Enter);
    // Leaving the note makes its new equation one step; the next session in it is another.
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    let (note, block) = equation_blocks(&ui)
        .first()
        .map(|(note, block)| (*note, block.id))
        .expect("an equation block");
    ui.click(formula_point(&ui, note, block));
    ui.type_text("b");
    let image = egui::ColorImage::filled([4, 4], egui::Color32::RED);
    ui.h.event(Event::PasteImage(std::sync::Arc::new(image)));
    ui.steps(2);
    ui.type_text("c");
    ui.press(Key::Escape);
    let picture = |ui: &Ui| {
        ui.items()
            .iter()
            .any(|item| matches!(item.kind, CanvasKind::Image { .. }))
    };
    assert_eq!(sources(&ui), ["abc"]);
    assert!(picture(&ui));

    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert_eq!(
        sources(&ui),
        ["ab"],
        "the typing after the paste goes first"
    );
    assert!(picture(&ui));
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert!(!picture(&ui), "then the picture");
    assert_eq!(sources(&ui), ["ab"]);
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert_eq!(sources(&ui), ["a"], "then the typing before it");
}

#[test]
fn an_empty_equation_is_not_kept_when_another_page_opens() {
    let mut ui = Ui::start();
    ui.new_page("Alpha");
    save(&mut ui);
    ui.new_page("Beta");
    save(&mut ui);
    let beta = page_id(&ui, "Beta");
    // What was saved, before `get_page`'s repair (which drops blank equation blocks) reads
    // it: neither the equation nor the note Alt+= made to hold it.
    let stored_items = |ui: &Ui, page: &str| -> Vec<CanvasItem> {
        let json = ui.app().library.stored_canvas_json(page).unwrap();
        serde_json::from_str::<crate::model::CanvasDoc>(&json)
            .unwrap()
            .items
    };
    ui.press(Key::Escape);
    ui.press_with(Modifiers::ALT, Key::Equals);
    assert_eq!(equation_blocks(&ui).len(), 1);

    // Clicking another page in the list while the source field has the caret.
    let row =
        ui.h.get_by_role_and_label(egui::accesskit::Role::ListItem, "Alpha")
            .rect()
            .center();
    ui.click(row);
    assert_eq!(ui.app().page.as_ref().unwrap().id, page_id(&ui, "Alpha"));
    let stored = stored_items(&ui, &beta);
    assert!(stored.is_empty(), "{stored:?}");

    // A new page by its shortcut while typing in a new, still empty equation.
    let alpha = page_id(&ui, "Alpha");
    ui.press(Key::Escape);
    ui.press_with(Modifiers::ALT, Key::Equals);
    assert_eq!(equation_blocks(&ui).len(), 1);
    ui.press_with(Modifiers::COMMAND, Key::N);
    ui.steps(2);
    assert_ne!(ui.app().page.as_ref().unwrap().id, alpha);
    let stored = stored_items(&ui, &alpha);
    assert!(stored.is_empty(), "{stored:?}");
}

/// Puts an equation item reading `source` on the open page at page point `at`, as older
/// pages hold them (Insert › Equation now types equations into notes), and selects it.
fn add_equation_item(ui: &mut Ui, source: &str, at: Pos2) -> u64 {
    let id = {
        let app = ui.h.state_mut();
        let page = app.page.as_mut().expect("a page");
        page.canvas.add(CanvasItem::new(
            CanvasKind::Equation {
                source: source.into(),
                size: crate::model::default_equation_size(),
            },
            at.x,
            at.y,
            48.0,
            28.0,
        ))
    };
    // The box fits itself to the formula when it is drawn.
    ui.steps(3);
    let center = ui.screen(ui.item(id).rect().center());
    ui.click(center);
    id
}

#[test]
fn equations_scale_by_their_corner_only_and_copy_as_latex() {
    let mut ui = Ui::start();
    ui.new_page("Sizes");
    ui.press(Key::Escape);
    let at = egui::pos2(
        crate::canvas::PAGE_LEFT + 150.0,
        crate::canvas::PAGE_CONTENT_TOP + 100.0,
    );
    let id = add_equation_item(&mut ui, "x^2+1/2", at);
    assert_eq!(ui.app().canvas.selection.primary(), Some(id));
    let before = ui.item(id).rect();
    assert_eq!(equation_size(&ui.item(id)), 22.0);

    // Dragging the corner to twice the width doubles the size; undo puts it back.
    let corner = ui.screen(before.right_bottom());
    ui.drag(corner, corner + egui::vec2(before.width(), before.height()));
    assert_eq!(equation_size(&ui.item(id)), 44.0);
    // The padding around the formula stays the same, so the box grows a little less.
    assert!(ui.item(id).height > before.height() * 1.5);
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert_eq!(equation_size(&ui.item(id)), 22.0);
    assert_eq!(ui.item(id).rect(), before);

    // Its menu copies it as LaTeX.
    let center = ui.screen(before.center());
    ui.right_click(center);
    let outputs = ui.click_label_outputs("Copy LaTeX");
    assert_eq!(copied(&outputs), ["x^{2}+\\frac{1}{2}"]);

    // A display with another scale measures glyphs a little differently: the box
    // follows, but the equation keeps its size.
    ui.h.set_pixels_per_point(2.0);
    ui.steps(3);
    assert_ne!(
        ui.item(id).height,
        before.height(),
        "the box was refitted for the new scale"
    );
    assert_eq!(equation_size(&ui.item(id)), 22.0);
    ui.h.set_pixels_per_point(1.0);
    ui.steps(3);
    assert_eq!(equation_size(&ui.item(id)), 22.0);
    assert_eq!(ui.item(id).rect(), before);
    assert!(equations(&ui).len() == 1);
}

#[test]
fn the_source_box_of_an_equation_low_on_the_page_comes_into_view() {
    let mut ui = Ui::start();
    ui.new_page("Low");
    let low = ui.empty_spot(0.0, 340.0);
    ui.right_click(low);
    // A new note holding an equation block, at the point clicked.
    ui.click_label_containing("Equation here");
    ui.shot("equation-source-box");
    ui.type_text("y");
    let (note, block) = equation_blocks(&ui)
        .first()
        .map(|(note, block)| (*note, block.clone()))
        .expect("an equation block");
    assert_eq!(source_of(&block), "y");
    // The block's row holds the formula and, under it while it is typed, its source field.
    let row = ui.row(note, block.id).rect;
    let field_bottom = ui.screen(row.left_bottom());
    assert!(
        field_bottom.y < 890.0,
        "the source box is on screen: {field_bottom:?}"
    );
}

/// Every shape painted in the last frame (nested ones included), in screen coordinates.
fn painted(ui: &Ui) -> Vec<egui::Shape> {
    fn flatten(shape: &egui::Shape, out: &mut Vec<egui::Shape>) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| flatten(shape, out)),
            shape => out.push(shape.clone()),
        }
    }
    let mut out = Vec::new();
    for clipped in &ui.h.output().shapes {
        flatten(&clipped.shape, &mut out);
    }
    out
}

/// Where the first line of a paragraph reading `text` starts, and how far left of it each
/// glyph before it sits: a star tag, a bullet, a check box (left to right).
fn line_lead(ui: &Ui, text: &str) -> (f32, Vec<f32>, Vec<egui::Rect>) {
    let shapes = painted(ui);
    let (left, band) = shapes
        .iter()
        .find_map(|shape| match shape {
            egui::Shape::Text(shape) if shape.galley.text() == text => {
                let row = &shape.galley.rows[0];
                let y = shape.pos.y;
                Some((
                    shape.pos.x + row.pos.x,
                    egui::Rangef::new(y + row.min_y(), y + row.max_y()),
                ))
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("{text:?} was not painted"));
    let mut glyphs: Vec<egui::Rect> = shapes
        .iter()
        .filter_map(|shape| match shape {
            egui::Shape::Circle(dot) if dot.radius == crate::editor::BULLET_RADIUS => Some(
                egui::Rect::from_center_size(dot.center, egui::Vec2::splat(dot.radius * 2.0)),
            ),
            egui::Shape::Rect(check)
                if check.rect.size() == egui::Vec2::splat(crate::editor::CHECKBOX_SIZE) =>
            {
                Some(check.rect)
            }
            egui::Shape::Text(icon) if icon.galley.text() == crate::icons::STAR => {
                Some(icon.visual_bounding_rect())
            }
            _ => None,
        })
        .filter(|rect| band.contains(rect.center().y))
        .collect();
    glyphs.sort_by(|a, b| a.center().x.total_cmp(&b.center().x));
    glyphs.dedup();
    let gaps = glyphs.iter().map(|rect| left - rect.center().x).collect();
    (left, gaps, glyphs)
}

#[test]
fn list_markers_and_tags_follow_centered_and_right_aligned_text() {
    use crate::model::{BlockKind, ParaAlign};
    let mut ui = Ui::start();
    ui.new_page("Aligned lists");
    ui.press(Key::Enter);
    ui.type_text("Milk");
    ui.press_with(Modifiers::COMMAND, Key::Period);
    ui.press_with(Modifiers::COMMAND, Key::Num2);
    ui.press(Key::Enter);
    ui.type_text("Eggs");
    ui.press_with(Modifiers::COMMAND, Key::Num1);
    // A new note is only as wide as its text, so aligning would barely move it: give the
    // note a width with its ◂▸ handle first, as one would in the desktop notebook, then go back to "Eggs".
    let id = ui.items()[0].id;
    let handle = ui.width_handle(id);
    ui.drag(handle, handle + egui::vec2(320.0, 0.0));
    ui.click(ui.text_point(id, ui.blocks()[1].id, 2));
    let blocks = |ui: &Ui| ui.items()[0].doc().unwrap().blocks.clone();
    assert!(matches!(blocks(&ui)[0].kind, BlockKind::Bullet));
    assert!(matches!(
        blocks(&ui)[1].kind,
        BlockKind::Todo { checked: false }
    ));
    ui.shot("aligned-lists-left");
    let (milk_left, milk_gaps, _) = line_lead(&ui, "Milk");
    let (eggs_left, eggs_gaps, _) = line_lead(&ui, "Eggs");
    assert_eq!(milk_gaps.len(), 2, "a star, then the bullet: {milk_gaps:?}");
    assert_eq!(
        eggs_gaps.len(),
        2,
        "the star carried over, then a box: {eggs_gaps:?}"
    );

    // Right-aligned (Ctrl+R) and centred (from the ribbon), the star, the bullet and the
    // box move with the text and keep their distance from it, as in the desktop notebook.
    let close = |a: &[f32], b: &[f32]| {
        a.len() == b.len() && a.iter().zip(b).all(|(a, b)| (a - b).abs() < 0.5)
    };
    for align in [ParaAlign::Right, ParaAlign::Center] {
        for line in [1, 0] {
            if align == ParaAlign::Right {
                ui.press_with(Modifiers::COMMAND, Key::R);
            } else {
                ui.click_label("Paragraph alignment");
                ui.click_label_containing("Center");
            }
            assert_eq!(blocks(&ui)[line].align, align);
            if line == 1 {
                ui.press(Key::Home);
                ui.press(Key::ArrowUp);
            }
        }
        ui.press(Key::End);
        ui.press(Key::ArrowDown);
        ui.shot(&format!("aligned-lists-{align:?}"));
        let (left, gaps, _) = line_lead(&ui, "Milk");
        assert!(left > milk_left + 150.0, "{align:?}: the text moved");
        assert!(
            close(&gaps, &milk_gaps),
            "{align:?}: {gaps:?} vs {milk_gaps:?}"
        );
        let (left, gaps, glyphs) = line_lead(&ui, "Eggs");
        assert!(left > eggs_left + 150.0, "{align:?}: the text moved");
        assert!(
            close(&gaps, &eggs_gaps),
            "{align:?}: {gaps:?} vs {eggs_gaps:?}"
        );

        // The box ticks where it is drawn; where it used to be, a click does nothing.
        let checked = |ui: &Ui| matches!(blocks(ui)[1].kind, BlockKind::Todo { checked: true });
        let check = glyphs[1].center();
        ui.click(egui::pos2(eggs_left - eggs_gaps[1], check.y));
        assert!(!checked(&ui), "{align:?}: the old spot is empty");
        ui.click(check);
        assert!(checked(&ui), "{align:?}: the moved box ticks");
        ui.click(check);
        assert!(!checked(&ui), "{align:?}: and unticks");
        assert_eq!(
            blocks(&ui)[1].text.text,
            "Eggs",
            "clicking the box typed nothing"
        );
    }
}

/// Copy Link to Paragraph, on a paragraph's text menu and on its handle, copies a link to
/// that paragraph. Pasted on another page it reads as the paragraph's text, cut short as
/// the desktop notebook does, and Ctrl+click opens the page with the caret in that paragraph.
#[test]
fn copy_link_to_paragraph_opens_the_page_at_that_paragraph() {
    let mut ui = Ui::start();
    ui.new_page("Rivers");
    ui.press(Key::Enter);
    ui.type_text("The Nile");
    ui.press(Key::Enter);
    let amazon = "The Amazon carries more water than any other river on Earth, by far";
    ui.type_text(amazon);
    ui.press(Key::Enter);
    ui.type_text("The Yangtze");
    save(&mut ui);
    let rivers = ui.app().page.as_ref().unwrap().id.clone();
    let note = ui.items()[0].id;
    let block = ui.blocks()[1].id;
    let link = crate::model::paragraph_link(&rivers, block);

    // The second paragraph's text menu, then its handle's menu.
    ui.right_click(ui.text_point(note, block, 4));
    let outputs = ui.click_label_outputs(&t!("editor-copy-paragraph-link"));
    assert_eq!(copied(&outputs), std::slice::from_ref(&link));
    ui.steps(30);
    ui.right_click(ui.handle_point(note, block));
    let outputs = ui.click_label_outputs(&t!("editor-copy-paragraph-link"));
    assert_eq!(copied(&outputs), std::slice::from_ref(&link));

    // Pasted on "Index": a link that reads the paragraph's text, shortened.
    ui.new_page("Index");
    ui.press(Key::Enter);
    ui.type_text("See ");
    ui.h.event(Event::Paste(link.clone()));
    ui.steps(2);
    let index = ui.items()[0].clone();
    let text = text_of(&index);
    let pasted = text.strip_prefix("See ").expect("after what was typed");
    let kept = pasted.strip_suffix('…').expect("cut short with …");
    assert!(amazon.starts_with(kept), "{pasted:?}");
    assert_eq!(pasted.chars().count(), 60, "{pasted:?}");
    for at in [4, 30, 63] {
        assert_eq!(link_of(&index, at).as_deref(), Some(link.as_str()));
    }

    // Ctrl+click: "Rivers" opens with the caret in that paragraph.
    ui.steps(30);
    let spot = ui.text_point(index.id, ui.blocks()[0].id, 10);
    ui.click_with(Modifiers::COMMAND, spot);
    ui.steps(3);
    assert_eq!(ui.app().page.as_ref().unwrap().id, rivers);
    assert_eq!(ui.app().canvas.active_text, Some(note));
    assert_eq!(ui.app().editor.active_block, Some(block));
    ui.shot("s5-paragraph-link");
}

#[test]
fn a_pasted_page_link_reads_the_page_title() {
    use crate::model::page_link;
    let mut ui = Ui::start();
    ui.new_page("Target");
    let target = ui.app().page.as_ref().unwrap().id.clone();
    ui.new_page("Gone");
    let gone = ui.app().page.as_ref().unwrap().id.clone();
    ui.new_page("Source");
    ui.h.state_mut().library.trash_page(&gone).unwrap();
    assert_eq!(
        ui.app().library.page_title(&target).unwrap().as_deref(),
        Some("Target")
    );
    ui.press(Key::Enter);
    ui.type_text("See ");
    let note = |ui: &Ui| ui.items()[0].clone();
    // The note's lines as typed (trailing spaces included).
    let text = |ui: &Ui| {
        let doc = ui.items()[0].doc().unwrap().clone();
        let lines: Vec<String> = doc
            .blocks
            .into_iter()
            .map(|block| block.text.text)
            .collect();
        lines.join("\n")
    };

    // "Copy link to page", pasted: a link that reads the page's title, as in the desktop notebook.
    let link = page_link(&target);
    ui.h.event(Event::Paste(link.clone()));
    ui.steps(2);
    assert_eq!(text(&ui), "See Target");
    ui.shot("pasted-page-link");
    for at in 4..10 {
        assert_eq!(link_of(&note(&ui), at).as_deref(), Some(link.as_str()));
    }
    assert_eq!(link_of(&note(&ui), 3), None);
    // The caret is after it, and what is typed there is plain text.
    ui.steps(60);
    ui.type_text("!");
    assert_eq!(text(&ui), "See Target!");
    assert_eq!(link_of(&note(&ui), 10), None);
    // The paste is one undo step: no raw address is left behind.
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert_eq!(text(&ui), "See Target");
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert_eq!(text(&ui), "See ");
    ui.press_with(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    assert_eq!(text(&ui), "See Target");
    assert_eq!(link_of(&note(&ui), 4).as_deref(), Some(link.as_str()));

    // A page that is gone or in the trash, another address, or several lines paste as
    // they always have, here each into a new note: the text as it is (an address alone
    // is linked to itself).
    let lines = |note: &CanvasItem| {
        let doc = note.doc().unwrap().clone();
        let lines: Vec<String> = doc
            .blocks
            .into_iter()
            .map(|block| block.text.text)
            .collect();
        lines.join("\n")
    };
    for pasted in [
        page_link("no-such-page"),
        page_link(&gone),
        "https://docs.rs".to_string(),
        format!("{link}\nnext line"),
    ] {
        // Empty paper, clear of the first note under the page header and of each new one.
        let spot = ui.empty_paper_point();
        ui.click(spot);
        ui.h.event(Event::Paste(pasted.clone()));
        ui.steps(2);
        let note = ui
            .items()
            .into_iter()
            .find(|note| lines(note) == pasted)
            .unwrap_or_else(|| panic!("no note reads {pasted:?}"));
        let linked = !pasted.contains('\n');
        assert_eq!(
            link_of(&note, 0).is_some_and(|url| url == pasted),
            linked,
            "{pasted}"
        );
    }
}

#[test]
fn moving_a_page_to_its_own_section_does_nothing() {
    use egui::accesskit::Role;
    use egui_kittest::kittest::NodeT;
    let mut ui = Ui::start();
    ui.new_page("Stay");
    save(&mut ui);
    let stay = ui.app().page.as_ref().unwrap().id.clone();
    ui.new_page("Last");
    save(&mut ui);
    let page = |ui: &Ui| ui.app().library.get_page(&stay).unwrap().unwrap();
    let own = page(&ui).section_name;
    assert_eq!(own, "This week");
    let state = |ui: &Ui| {
        let page = page(ui);
        (page.section_id, page.updated_at, titles(ui))
    };
    let before = state(&ui);
    let toast = |ui: &Ui| ui.app().toast.message.clone();
    ui.h.state_mut().toast.message.clear();
    let row = |ui: &Ui| ui.h.get_by_role_and_label(Role::ListItem, "Stay").rect();
    let section = |ui: &Ui, name: &str| ui.h.get_by_role_and_label(Role::Button, name).rect();

    // Dropped on its own section in the sidebar, the page stays where it is, and nothing
    // says it moved.
    let (from, to) = (row(&ui).center(), section(&ui, &own).center());
    ui.drag(from, to);
    assert_eq!(toast(&ui), "", "no toast for a move that changes nothing");
    assert_eq!(state(&ui), before);

    // In the row's Move or Copy… dialog, Move stays greyed while its own section is the
    // one picked in the tree, and clicking it does nothing…
    // (A tree row is a button named by its section, as the navigation pane's is: the
    // dialog's, centred in the window, is the rightmost one.)
    let pick = |ui: &mut Ui, name: &str| {
        let rect =
            ui.h.get_all_by_role_and_label(Role::Button, name)
                .map(|node| node.rect())
                .max_by(|a, b| a.left().total_cmp(&b.left()))
                .expect(name);
        ui.click(rect.center());
    };
    let move_label = t!("dialog-move");
    let move_button = |ui: &Ui| {
        let node =
            ui.h.get_all_by_role_and_label(Role::Button, &move_label)
                .last()
                .expect("the dialog's Move");
        (node.rect(), node.accesskit_node().is_disabled())
    };
    let move_or_copy = |ui: &mut Ui| {
        ui.right_click(row(ui).center());
        click_menu_row(ui, &t!("app-move-or-copy"), &ctrl_alt_hint(Key::M));
        assert!(
            matches!(ui.app().dialog, Some(super::Dialog::MoveOrCopy { .. })),
            "the row's Move or Copy… opens the dialog"
        );
        // (School, the tree's first notebook, shows its sections at the top, unscrolled.)
    };
    move_or_copy(&mut ui);
    pick(&mut ui, "History");
    assert!(!move_button(&ui).1, "another section takes the page");
    pick(&mut ui, &own);
    let (here, disabled) = move_button(&ui);
    assert!(disabled, "the page's own section is not a choice");
    ui.shot("move-to-own-section");
    ui.click(here.center());
    assert!(ui.app().dialog.is_some(), "the greyed Move does nothing");
    assert_eq!((toast(&ui), state(&ui)), (String::new(), before.clone()));
    ui.press(Key::Escape);
    assert!(ui.app().dialog.is_none());

    // …and so it does from the open page's own row menu.
    ui.h.state_mut().open_page(&stay, false);
    ui.steps(3);
    move_or_copy(&mut ui);
    pick(&mut ui, &own);
    assert!(move_button(&ui).1, "the page's own section is not a choice");
    pick(&mut ui, "History");
    assert!(!move_button(&ui).1);
    ui.press(Key::Escape);
    assert!(ui.app().dialog.is_none());
    assert_eq!((toast(&ui), state(&ui)), (String::new(), before.clone()));

    // Another section is still a drop target.
    let (from, to) = (row(&ui).center(), section(&ui, "History").center());
    ui.drag(from, to);
    assert_eq!(toast(&ui), "Page moved");
    assert_eq!(page(&ui).section_name, "History");
}

/// Checks a frise as painted: each milestone's title, then its date label, then (with a
/// gap) its dot on the line, then its note, never overlapping one another or a neighbour's,
/// and all inside the frise.
fn assert_frise_rows_apart(ui: &Ui, id: u64, what: &str) {
    let item = ui.item(id);
    let CanvasKind::Timeline { color, milestones } = &item.kind else {
        panic!("not a frise");
    };
    let frame = egui::Rect::from_min_max(ui.screen(item.rect().min), ui.screen(item.rect().max));
    let shapes = painted(ui);
    let text = |text: &str| {
        shapes
            .iter()
            .find_map(|shape| match shape {
                egui::Shape::Text(shape) if shape.galley.text() == text => {
                    Some(shape.galley.rect.translate(shape.pos.to_vec2()))
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("{what}: {text:?} was not painted"))
    };
    let accent = crate::canvas::frise_accent(*color, crate::theme::tokens(&ui.h.ctx));
    let mut dots: Vec<egui::Rect> = shapes
        .iter()
        .filter_map(|shape| match shape {
            egui::Shape::Circle(dot) if dot.fill == accent => Some(egui::Rect::from_center_size(
                dot.center,
                egui::Vec2::splat(dot.radius * 2.0),
            )),
            _ => None,
        })
        .filter(|dot| frame.contains_rect(*dot))
        .collect();
    dots.sort_by(|a, b| a.center().x.total_cmp(&b.center().x));
    assert_eq!(
        dots.len(),
        milestones.len(),
        "{what}: one dot per milestone"
    );
    let mut previous: Option<[egui::Rect; 3]> = None;
    for (milestone, dot) in milestones.iter().zip(dots) {
        let rows = [
            text(&milestone.title),
            text(&milestone.date),
            text(&milestone.note),
        ];
        let [title, date, note] = rows;
        let name = &milestone.date;
        assert!(
            title.bottom() <= date.top() + 0.5,
            "{what}: {name}: title {title:?} over date {date:?}"
        );
        assert!(
            date.bottom() < dot.top(),
            "{what}: {name}: date {date:?} on the dot {dot:?}"
        );
        assert!(
            note.top() > dot.bottom(),
            "{what}: {name}: note {note:?} on the dot {dot:?}"
        );
        for row in rows {
            assert!(
                frame.contains_rect(row),
                "{what}: {name}: {row:?} outside {frame:?}"
            );
        }
        if let Some(before) = previous {
            for (a, b) in before.iter().zip(rows) {
                assert!(
                    a.right() <= b.left() + 0.5,
                    "{what}: {name} runs into its neighbour"
                );
            }
        }
        previous = Some(rows);
    }
}

#[test]
fn frise_rows_stack_without_overlapping_at_any_size_or_zoom() {
    let mut ui = Ui::start();
    let page = ui
        .app()
        .library
        .page_list(crate::db::PageFilter::All, "", 100)
        .unwrap()
        .into_iter()
        .find(|page| page.title == "The Middle Ages")
        .expect("the sample frise page")
        .id;
    ui.h.state_mut().open_page(&page, false);
    ui.steps(4);
    let id = ui
        .items()
        .iter()
        .find(|item| matches!(item.kind, CanvasKind::Timeline { .. }))
        .expect("a frise")
        .id;
    ui.shot("frise");
    assert_frise_rows_apart(&ui, id, "as placed");

    // Zoomed out, so the whole frise is on screen.
    ui.h.state_mut().ribbon.tab = RibbonTab::View;
    ui.steps(2);
    for _ in 0..4 {
        ui.ribbon_button("Zoom Out");
    }
    assert!(ui.app().canvas.zoom() < 0.6);
    assert_frise_rows_apart(&ui, id, "zoomed out");

    // Shrunk by its corner as far as it goes: the smallest frise still holds every row.
    let rect = ui.item(id).rect();
    let inside = ui.screen(rect.left_top() + egui::vec2(12.0, 12.0));
    ui.click(inside);
    assert_eq!(ui.selection().primary(), Some(id));
    let corner = ui.screen(rect.right_bottom());
    assert!(
        corner.x < 1300.0 && corner.y < 880.0,
        "the corner is on screen: {corner:?}"
    );
    ui.drag(corner, corner - egui::vec2(rect.width(), rect.height()));
    let small = ui.item(id).rect().size();
    assert_eq!(small, crate::canvas::FRISE_MIN, "{small:?}");
    ui.shot("frise-smallest");
    assert_frise_rows_apart(&ui, id, "smallest");
}

/// The text painted starting inside `row` (its left middle in it), with where it went.
fn texts_in(ui: &Ui, row: egui::Rect) -> Vec<(String, egui::Rect)> {
    painted(ui)
        .iter()
        .filter_map(|shape| match shape {
            egui::Shape::Text(text) => Some((
                text.galley.text().to_string(),
                text.galley.rect.translate(text.pos.to_vec2()),
            )),
            _ => None,
        })
        .filter(|(_, rect)| row.contains(rect.left_center()))
        .collect()
}

/// The French shell reads like the desktop notebook's French and fits, at the default window and the
/// smallest: the desktop notebook's French tab names, the header's search box, the page list's
/// "Ajouter une page" beside its sort button, the navigation pane's French rows and no
/// English left; then Options, the Shortcuts list (whole, by scrolling, under a title that
/// stays in the window) and the reader's French buttons.
#[test]
fn the_app_shell_reads_in_french_and_fits() {
    use egui::Role;
    let mut ui = Ui::start_in(Lang::Fr);
    let has = |ui: &Ui, label: &str| ui.h.query_all_by_label(label).next().is_some();
    // Every painted text starting inside `area`: as shown (a text cut to fit ends in
    // "…"), as written, and where it went.
    let shown_in = |ui: &Ui, area: egui::Rect| -> Vec<(String, String, egui::Rect)> {
        painted(ui)
            .iter()
            .filter_map(|shape| match shape {
                egui::Shape::Text(text) => Some((
                    text.galley
                        .rows
                        .iter()
                        .map(|row| row.text())
                        .collect::<String>(),
                    text.galley.text().to_string(),
                    text.galley.rect.translate(text.pos.to_vec2()),
                )),
                _ => None,
            })
            .filter(|(_, _, rect)| area.contains(rect.left_center()))
            .collect()
    };
    // A title far too long for its row, so the rows' check below means something.
    let long = "Compte rendu de la réunion trimestrielle du comité de pilotage";
    ui.new_page(long);
    save(&mut ui);

    for width in [1320.0f32, 980.0] {
        ui.h.set_size(egui::vec2(width, 900.0));
        ui.steps(3);
        ui.shot(&format!("p2-fr-shell-{width}"));

        // the desktop notebook's French tabs, left to right, inside the window.
        let mut left = 0.0;
        for tab in [
            "Fichier",
            "Accueil",
            "Insertion",
            "Dessiner",
            "Historique",
            "Révision",
            "Affichage",
            "Aide",
        ] {
            let rect = ui.ribbon_tab_rect(tab);
            assert!(
                rect.left() >= left && rect.right() <= width,
                "{width}: the {tab:?} tab {rect:?}"
            );
            left = rect.right();
        }

        // The header's search box, its placeholder painted inside it.
        let search =
            ui.h.get_by_role_and_label(Role::Button, "Rechercher")
                .rect();
        let (shown, _, text) = shown_in(&ui, search)
            .into_iter()
            .find(|(_, full, _)| full == "Rechercher")
            .unwrap_or_else(|| panic!("{width}: no placeholder in the search box"));
        assert_eq!(shown, "Rechercher");
        assert!(
            search.contains_rect(text),
            "{width}: {text:?} leaves the search box {search:?}"
        );

        // The page list's header: "Ajouter une page" whole, clear of the sort button, both
        // inside the card (whose rows span its inner width).
        let add =
            ui.h.get_by_role_and_label(Role::Button, "Ajouter une page")
                .rect();
        let sort =
            ui.h.get_by_role_and_label(Role::Button, "Trier les pages")
                .rect();
        let row = ui.h.get_by_role_and_label(Role::ListItem, long).rect();
        let (shown, _, text) = shown_in(&ui, add)
            .into_iter()
            .find(|(_, full, _)| full == "Ajouter une page")
            .unwrap_or_else(|| panic!("{width}: no label on Add page"));
        assert_eq!(shown, "Ajouter une page", "{width}: the label is cut");
        assert!(
            text.right() <= add.right() + 0.5
                && add.right() <= sort.left() + 0.5
                && add.left() >= row.left() - 0.5
                && sort.right() <= row.right() + 0.5,
            "{width}: label {text:?}, Ajouter une page {add:?}, sort {sort:?}, row {row:?}"
        );

        // The navigation pane speaks French, and no English is left in the shell.
        for label in ["Notes rapides", "Nouvelle section", "Masquer la navigation"] {
            assert!(has(&ui, label), "{width}: no {label:?}");
        }
        for english in [
            "Home",
            "Insert",
            "Draw",
            "Add page",
            "Quick notes",
            "New section",
        ] {
            assert!(!has(&ui, english), "{width}: {english:?} is still English");
        }

        // Every page row's title ends inside its row; the long one is cut to fit.
        let rows: Vec<egui::Rect> =
            ui.h.get_all_by_role(Role::ListItem)
                .map(|node| node.rect())
                .collect();
        let mut long_cut = false;
        for row in rows {
            for (shown, full, rect) in shown_in(&ui, row) {
                assert!(
                    rect.right() <= row.right() + 0.5,
                    "{width}: {shown:?} runs past its row: {rect:?} {row:?}"
                );
                long_cut |= full == long && shown.ends_with('…');
            }
        }
        assert!(long_cut, "{width}: the long title is not cut to its row");
    }

    // Options: French pages, headings and buttons.
    ui.h.set_size(egui::vec2(1320.0, 900.0));
    ui.h.state_mut().dialog = Some(super::Dialog::Settings);
    ui.steps(3);
    for (page, heading) in [
        ("Général", "Personnaliser Folio"),
        ("Vérification", "Orthographe"),
        ("Enregistrement et sauvegarde", "Vos fichiers"),
    ] {
        ui.options_page(page);
        let shown: Vec<String> = texts_in(&ui, egui::Rect::EVERYTHING)
            .into_iter()
            .map(|(text, _)| text)
            .collect();
        assert!(
            shown.iter().any(|text| text == heading),
            "{page}: no {heading:?}"
        );
    }
    for button in ["Sauvegarder", "Ouvrir le dossier"] {
        assert!(
            ui.h.query_all_by_label_contains(button).next().is_some(),
            "no {button:?}"
        );
    }

    // Keyboard shortcuts: the title stays in the window, the list scrolls to its last
    // row, and every French label ends before its keys, which use the French name for
    // Shift off macOS. At 900 px and at the smallest window, 640.
    let title = t!("common-keyboard-shortcuts").into_owned();
    // The last row: Back and Forward, which macOS leaves out.
    let last = if cfg!(target_os = "macos") {
        t!("app-shortcut-zoom")
    } else {
        t!("app-shortcut-back-forward")
    }
    .into_owned();
    for height in [900.0f32, 640.0] {
        ui.h.set_size(egui::vec2(1320.0, height));
        ui.h.state_mut().dialog = Some(super::Dialog::Shortcuts);
        ui.steps(3);
        let painted_texts = texts_in(&ui, egui::Rect::EVERYTHING);
        let (_, heading) = painted_texts
            .iter()
            .find(|(text, _)| *text == title)
            .unwrap_or_else(|| panic!("{height}: no {title:?}"));
        assert!(
            heading.top() >= 0.0 && heading.bottom() <= height,
            "{height}: the title {heading:?} leaves the window"
        );
        // Scroll the list, under the pointer, to its end.
        let over = egui::pos2(heading.left() + 120.0, heading.bottom() + 120.0);
        ui.h.event(Event::PointerMoved(over));
        ui.h.event(Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -4000.0),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Unknown,
            modifiers: Modifiers::NONE,
        });
        ui.steps(30);
        ui.shot(&format!("p2-fr-shortcuts-{height}"));
        let painted_texts = texts_in(&ui, egui::Rect::EVERYTHING);
        let (_, last_rect) = painted_texts
            .iter()
            .find(|(text, _)| *text == last)
            .unwrap_or_else(|| panic!("{height}: {last:?} cannot be reached"));
        assert!(
            last_rect.bottom() <= height,
            "{height}: {last:?} {last_rect:?} is below the window"
        );
        // The longest labels with the longest keys at the list's end, on every system.
        for label in [
            "Monter / descendre la page (hors d’une note)",
            "Première / dernière page visible",
        ] {
            let (_, label_rect) = painted_texts
                .iter()
                .find(|(text, _)| text == label)
                .unwrap_or_else(|| panic!("{height}: no {label:?}"));
            let keys = painted_texts
                .iter()
                .filter(|(_, rect)| {
                    (rect.center().y - label_rect.center().y).abs() < 4.0
                        && rect.left() > label_rect.left()
                })
                .map(|(_, rect)| rect.left())
                .fold(f32::INFINITY, f32::min);
            assert!(
                label_rect.right() + 8.0 <= keys,
                "{height}: {label:?} runs into its keys"
            );
        }
        if !cfg!(target_os = "macos") {
            assert!(
                painted_texts.iter().any(|(text, _)| text.contains("Maj")),
                "Shift is written Maj in French"
            );
        }
        ui.h.state_mut().dialog = None;
        ui.steps(2);
    }

    // The reader's buttons grow to their French labels (the text-size ones fold to
    // their glyphs: `the_reader_toolbar_fits_in_both_languages`).
    ui.h.set_size(egui::vec2(1320.0, 900.0));
    ui.steps(2);
    ui.ribbon_tab("Affichage");
    ui.ribbon_button("Lecteur immersif");
    ui.steps(3);
    for label in ["Fermer le lecteur", "Aucun", "Sombre"] {
        let button =
            ui.h.query_all_by_label_contains(label)
                .last()
                .unwrap_or_else(|| panic!("no reader button {label:?}"))
                .rect();
        // Only the button's own label: the page list painted under the reader starts
        // in the same spots.
        let texts: Vec<_> = texts_in(&ui, button)
            .into_iter()
            .filter(|(text, _)| text.contains(label))
            .collect();
        assert!(
            !texts.is_empty(),
            "{label:?} is not painted whole in its button"
        );
        for (text, rect) in texts {
            assert!(
                rect.right() <= button.right() + 0.5,
                "{text:?} is cut in its button"
            );
        }
    }
}

/// The texts painted on the editor header's row, from its first text (the first of
/// `starts`, written out whole, that is painted) to its right end, as (shown, full text,
/// where) left to right. A text cut to fit shows ending in "…".
fn header_texts(ui: &Ui, starts: &[&str]) -> Vec<(String, String, egui::Rect)> {
    let texts: Vec<_> = painted(ui)
        .iter()
        .filter_map(|shape| match shape {
            egui::Shape::Text(text) => Some((
                text.galley
                    .rows
                    .iter()
                    .map(|row| row.text())
                    .collect::<String>(),
                text.galley.text().to_string(),
                text.galley.rect.translate(text.pos.to_vec2()),
            )),
            _ => None,
        })
        .collect();
    let start = starts
        .iter()
        .find_map(|start| texts.iter().find(|(_, full, _)| full == start))
        .unwrap_or_else(|| panic!("none of {starts:?} is painted"))
        .2;
    let band = start.y_range().expand(4.0);
    let mut row: Vec<_> = texts
        .into_iter()
        .filter(|(_, _, rect)| band.contains(rect.center().y) && rect.left() >= start.left() - 0.5)
        .collect();
    row.sort_by(|a, b| a.2.left().total_cmp(&b.2.left()));
    row
}

/// The banner over a page in the Notebook Recycle Bin: its message shows whole, and
/// Restore and Delete forever sit at its right, clear of it and of each other, and end
/// inside the page card (left of its Full Page View button), at the default and the
/// smallest window, in English and in French.
#[test]
fn the_trashed_page_banner_keeps_clear_of_its_buttons() {
    use crate::i18n::Lang;
    for lang in [Lang::En, Lang::Fr] {
        let mut ui = Ui::start_in(lang);
        let id = ui.app().page.as_ref().expect("a page is open").id.clone();
        {
            let app = ui.h.state_mut();
            app.handle_page_list_action(super::PageListAction::Trash(id.clone()));
            app.open_page(&id, true);
        }
        ui.steps(3);
        assert!(ui.app().page.as_ref().unwrap().trashed);
        let message = t!("app-page-in-trash").into_owned();
        let restore = format!(
            "{} {}",
            crate::icons::ARROW_COUNTER_CLOCKWISE,
            t!("common-restore")
        );
        let delete = format!("{} {}", crate::icons::TRASH, t!("common-delete-forever"));
        for width in [1320.0f32, 980.0] {
            ui.h.set_size(egui::vec2(width, 900.0));
            ui.steps(3);
            ui.shot(&format!("trash-banner-{lang:?}-{width}").to_lowercase());
            let row = header_texts(&ui, &[&message]);
            let (shown, _, text) = row
                .iter()
                .find(|(_, full, _)| full == &message)
                .expect("the banner's message");
            assert_eq!(shown, &message, "{lang:?} at {width}: the message is cut");
            let restore = ui.h.get_by_label(&restore).rect();
            let delete = ui.h.get_by_label(&delete).rect();
            let corner = ui.h.get_by_label(&t!("app-full-page-view")).rect();
            assert!(
                text.right() <= restore.left() + 0.5
                    && restore.right() <= delete.left() + 0.5
                    && delete.right() <= corner.left() + 0.5
                    && corner.right() <= width,
                "{lang:?} at {width}: message {text:?}, restore {restore:?}, \
                 delete {delete:?}, Full Page View {corner:?}"
            );
            // The buttons share the banner's row.
            for button in [restore, delete] {
                assert!(
                    button.y_range().contains(text.center().y),
                    "{lang:?} at {width}: {button:?} is not beside {text:?}"
                );
            }
        }
    }
}

/// The texts painted on the reader's toolbar row (the one holding Read aloud), left to
/// right. Only what the reader paints: the page list under it shares those rows.
fn reader_toolbar_texts(ui: &Ui, header: &str, read_aloud: &str) -> Vec<(String, egui::Rect)> {
    let texts: Vec<(String, egui::Rect)> = painted(ui)
        .iter()
        .filter_map(|shape| match shape {
            egui::Shape::Text(text) => Some((
                text.galley.text().to_string(),
                text.galley.rect.translate(text.pos.to_vec2()),
            )),
            _ => None,
        })
        .skip_while(|(text, _)| !text.starts_with(header))
        .collect();
    let band = texts
        .iter()
        .find(|(text, _)| text.contains(read_aloud))
        .unwrap_or_else(|| panic!("no {read_aloud:?} in the reader"))
        .1
        .y_range()
        .expand(4.0);
    let mut row: Vec<_> = texts
        .into_iter()
        .filter(|(_, rect)| band.contains(rect.center().y))
        .collect();
    row.sort_by(|a, b| a.1.left().total_cmp(&b.1.left()));
    row
}

#[test]
fn the_reader_toolbar_fits_in_both_languages() {
    use crate::i18n::Lang;
    use crate::reading::ReaderTheme;
    for (lang, open, header, close, smaller, read_aloud, dark) in [
        (
            Lang::En,
            "Immersive Reader",
            "Immersive Reader",
            "Close reader",
            "Smaller text",
            "Read aloud",
            "Dark",
        ),
        (
            Lang::Fr,
            "Lecteur immersif",
            "Lecteur immersif",
            "Fermer le lecteur",
            "Texte plus petit",
            "Lire à voix haute",
            "Sombre",
        ),
    ] {
        let mut ui = Ui::start_in(lang);
        // View › Immersive Reader.
        ui.ribbon_tab(&t!("ribbon-tab-view"));
        ui.ribbon_button(open);
        ui.steps(3);
        for width in [1320.0f32, 980.0] {
            ui.h.set_size(egui::vec2(width, 900.0));
            ui.h.state_mut().reader.theme = ReaderTheme::Paper;
            ui.steps(4);
            let row = reader_toolbar_texts(&ui, header, read_aloud);
            for pair in row.windows(2) {
                assert!(
                    pair[1].1.left() >= pair[0].1.right() - 0.5,
                    "{lang:?} at {width}: the reader's toolbar overlaps: {pair:?}"
                );
            }
            // Read aloud's button clears every other control, and the row keeps to the
            // reader's width (it ends where Close reader does).
            let button = |label: &str| {
                ui.h.query_all_by_label_contains(label)
                    .last()
                    .unwrap_or_else(|| panic!("no reader button {label:?}"))
                    .rect()
            };
            let aloud = button(read_aloud);
            for (text, rect) in row.iter().filter(|(text, _)| !text.contains(read_aloud)) {
                assert!(
                    rect.right() <= aloud.left(),
                    "{lang:?} at {width}: Read aloud covers {text:?}"
                );
            }
            let reader = (width - 96.0).clamp(560.0, 980.0);
            assert!(
                button(close).right() - row[0].1.left() <= reader + 0.5,
                "{lang:?} at {width}: the toolbar widens the reader: {row:?}"
            );
            assert!((aloud.right() - button(close).right()).abs() < 0.5);
            // The text-size buttons keep their name, painted or as a tooltip; English
            // keeps it painted at the default window.
            assert!(ui.h.query_all_by_label_contains(smaller).next().is_some());
            if lang == Lang::En && width == 1320.0 {
                assert!(
                    row.iter().any(|(text, _)| text.contains(smaller)),
                    "English shows {smaller:?} at the default window: {row:?}"
                );
            }
            // And the last theme can be picked.
            ui.click_label(dark);
            ui.steps(2);
            assert_eq!(
                ui.app().reader.theme,
                ReaderTheme::Dark,
                "{lang:?} at {width}"
            );
        }
    }
}

#[test]
fn search_results_and_sidebar_names_end_inside_their_rows() {
    use egui::accesskit::Role;
    let mut ui = Ui::start();
    let long = "A page whose title goes on and on and on, well past the width of the search \
                palette, so that it has to be cut short";
    ui.new_page(long);
    ui.press(Key::Enter);
    let wide = "WWWWWWWWWWWWWWWWWWWWWWWWWWWWWW";
    ui.type_text(&wide.repeat(3));
    save(&mut ui);

    // The page list's rows are titles only, as in the desktop notebook: the row's one text is its
    // title, cut short to end inside the row.
    let row = ui.h.get_by_role_and_label(Role::ListItem, long).rect();
    let texts = texts_in(&ui, row);
    let [(title, rect)] = texts.as_slice() else {
        panic!("the row shows only its title: {texts:?}")
    };
    assert!(
        long.starts_with(title.trim_end_matches('…')),
        "{title:?} is the start of the title"
    );
    assert!(
        rect.right() <= row.right(),
        "{title:?} ends at {} past the row's {}",
        rect.right(),
        row.right()
    );

    {
        let app = ui.h.state_mut();
        app.library
            .create_section("notebook-school", wide, "#745cf5")
            .unwrap();
        app.refresh_workspace();
    }
    ui.press(Key::Escape);
    ui.press_with(Modifiers::COMMAND, Key::E);
    ui.steps(3);
    assert!(
        ui.app().search.open,
        "Ctrl+E opens the search field and its dropdown"
    );
    ui.shot("search-dropdown");

    // Every recent page's row in the dropdown: its title, its "Notebook › Section" line
    // and its excerpt line end inside the row, cut with "…" when they are too long
    // ("Pythagoras’ theorem" has a long excerpt).
    let hits = ui.app().search.recent.clone();
    assert!(
        hits.iter()
            .any(|hit| hit.title == "Pythagoras’ theorem" && hit.excerpt.chars().count() > 100),
        "{hits:?}"
    );
    assert!(hits.iter().any(|hit| hit.title == long));
    for hit in &hits {
        let title = &hit.title;
        let row =
            ui.h.query_all_by_role_and_label(Role::Button, title)
                .last()
                .unwrap_or_else(|| panic!("no dropdown row for {title:?}"))
                .rect();
        // (The page behind the dropdown is painted too; only the row's lines count.)
        let place = format!("{} › {}", hit.notebook_name, hit.section_name);
        let excerpt = hit.excerpt.as_str();
        let lines: Vec<_> = texts_in(&ui, row)
            .into_iter()
            .filter(|(text, _)| {
                text == title || *text == place || (!excerpt.is_empty() && text == excerpt)
            })
            .collect();
        let expected = if excerpt.is_empty() { 2 } else { 3 };
        assert_eq!(
            lines.len(),
            expected,
            "{title}: a title, its place and its excerpt: {lines:?}"
        );
        for (text, rect) in lines {
            assert!(
                rect.right() <= row.right(),
                "{text:?} ends at {} past the row's {}",
                rect.right(),
                row.right()
            );
        }
    }

    // The sidebar's names too, measured rather than counted (W is a wide letter).
    ui.press(Key::Escape);
    let row = ui.h.get_by_role_and_label(Role::Button, wide).rect();
    let (_, name) = texts_in(&ui, row)
        .into_iter()
        .find(|(text, _)| text == wide)
        .expect("the section's name");
    assert!(name.right() <= row.right(), "{name:?} past {row:?}");
}

#[test]
fn a_french_start_shows_french_labels() {
    let mut ui = Ui::start_in(Lang::Fr);
    assert_eq!(crate::i18n::lang(), Lang::Fr);
    ui.h.state_mut().dialog = Some(super::Dialog::Settings);
    ui.steps(2);
    assert!(
        ui.h.query_all_by_label("Langue").next().is_some(),
        "the Language page"
    );
    // General's Theme box offers the appearance choices in French.
    let theme = ui.app().appearance.label().into_owned();
    let rect = ui.h.get_by_value(&theme).rect();
    ui.click(rect.center());
    assert!(
        ui.h.query_all_by_label_contains("Clair").next().is_some(),
        "the appearance choices"
    );
    assert!(ui.h.query_all_by_label("Language").next().is_none());
}

#[test]
fn the_interface_language_switches_live_and_is_remembered() {
    use crate::i18n::{lang, system_lang};
    let mut ui = Ui::start();
    // An untitled page with a tagged line, listed in the open Find Tags pane.
    ui.new_page("");
    ui.press(Key::Enter);
    ui.type_text("Tagged without a title");
    ui.press_with(Modifiers::COMMAND, Key::Num2);
    assert!(ui.h.state_mut().flush_save());
    ui.new_page("Elsewhere");
    ui.h.state_mut().open_tags_pane();
    ui.steps(2);
    assert!(ui.tagged_titles().contains(&"Untitled page".to_string()));

    // Options › Language holds the interface language.
    ui.h.state_mut().dialog = Some(super::Dialog::Settings);
    ui.steps(2);
    ui.options_page("Language");
    // The box reads as its value: Automatic, which FOLIO_LANG makes English here.
    let rect = ui.h.get_by_value("Automatic (English)").rect();
    ui.click(rect.center());
    ui.click_label("Français");
    assert_eq!(lang(), Lang::Fr);
    assert_eq!(
        ui.app().library.setting("ui_language").unwrap().as_deref(),
        Some("fr")
    );
    assert!(ui.app().dialog.is_some(), "Options stays open");
    assert!(
        ui.h.query_all_by_label("Langue").next().is_some(),
        "the dialog is French at once"
    );
    assert!(ui.h.query_all_by_label("Général").next().is_some());
    assert!(
        texts_in(&ui, egui::Rect::EVERYTHING)
            .iter()
            .any(|(text, _)| text == "Langue d’affichage"),
        "the page it is on is French too"
    );
    // Text listed before the switch follows it too.
    let titles = ui.tagged_titles();
    assert!(
        titles.contains(&"Page sans titre".to_string())
            && !titles.contains(&"Untitled page".to_string()),
        "{titles:?}"
    );

    // Save the language the system does not speak, so only the saved choice can bring
    // it back after a restart, whatever this machine's locale.
    let system = system_lang();
    let saved = if system == Lang::Fr {
        Lang::En
    } else {
        Lang::Fr
    };
    if saved != Lang::Fr {
        let rect = ui.h.get_by_value("Français").rect();
        ui.click(rect.center());
        // (The sample's English class is a section named "English" too: the open list,
        // drawn last, holds the language.)
        let rect =
            ui.h.get_all_by_label(saved.endonym())
                .last()
                .expect("the language in the open list")
                .rect();
        ui.click(rect.center());
    }
    assert_eq!(lang(), saved);
    assert_eq!(
        ui.app().library.setting("ui_language").unwrap().as_deref(),
        Some(saved.code())
    );

    // The saved choice wins over the system language next time.
    std::env::remove_var("FOLIO_LANG");
    ui.reopen();
    assert_eq!(
        lang(),
        saved,
        "the saved choice, not the system's {system:?}"
    );
    ui.h.state_mut().dialog = Some(super::Dialog::Settings);
    ui.steps(2);
    ui.options_page(&t!("app-options-language"));
    let rect = ui.h.get_by_value(saved.endonym()).rect();
    ui.click(rect.center());
    // Back to Automatic: nothing is stored and the system's language applies.
    let automatic = crate::i18n::t!(
        "dialog-settings-language-automatic",
        language = system.endonym()
    );
    ui.click_label(&automatic);
    assert_eq!(ui.app().library.setting("ui_language").unwrap(), None);
    assert_eq!(lang(), system);
    assert_ne!(lang(), saved);
}

/// A copy made in one language pastes back as items after switching to the other (the page
/// clipboard is recognised by what was put on the system clipboard, not by relabelling it).
#[test]
fn copied_items_paste_back_after_switching_language() {
    let mut ui = Ui::start();
    ui.new_page("Clipboard");
    ui.h.state_mut().canvas.tool = CanvasTool::Shape(ShapeKind::Rect);
    let from = ui.empty_spot(0.0, 0.0);
    ui.drag(from, from + egui::vec2(80.0, 60.0));
    ui.press(Key::Escape);
    ui.press_with(Modifiers::COMMAND, Key::A);
    assert_eq!(ui.selection().items.len(), 1);
    ui.h.event(Event::Copy);
    ui.h.step();
    let copied =
        ui.h.output()
            .platform_output
            .commands
            .iter()
            .find_map(|command| match command {
                egui::OutputCommand::CopyText(text) => Some(text.clone()),
                _ => None,
            })
            .expect("the shape went to the clipboard");
    // A shape without text goes to the clipboard as its English kind name.
    assert_eq!(copied, "[Shape]");
    ui.steps(1);
    crate::i18n::set_lang(Lang::Fr);
    ui.steps(2);
    ui.h.event(Event::Paste(copied));
    ui.steps(3);
    let items = ui.items();
    assert_eq!(
        items.len(),
        2,
        "pasted as a second shape, not as a note reading [Shape]: {items:?}"
    );
    assert!(
        items
            .iter()
            .all(|item| matches!(item.kind, CanvasKind::Shape { .. })),
        "the paste is the shape, not a text box reading [Shape]: {items:?}"
    );
}

/// The canvas in French: tabs, bands, defaults written in French, gendered labels, and
/// what was written stays as written when the language changes.
#[test]
fn the_canvas_speaks_french() {
    use crate::i18n::Lang;
    let mut ui = Ui::start_in(Lang::Fr);
    ui.new_page("Cours");
    ui.press(Key::Escape);
    for tab in ["Accueil", "Insertion", "Dessiner", "Affichage"] {
        ui.ribbon_tab(tab);
    }
    ui.ribbon_tab("Insertion");
    // At the default window the French Insert band keeps its labels (an icon-only button
    // is 30 px wide).
    let labelled = |ui: &Ui, label: &str| {
        let button = ui.ribbon_button_rect(label);
        assert!(
            button.right() <= 1320.0 && button.width() > 40.0,
            "{label} is labelled at 1320 px: {button:?}"
        );
    };
    labelled(&ui, "Insérer un espace");
    ui.shot("l3-fr-insert");
    // Tableau opens the desktop notebook's grid, which names the size under the pointer in French.
    ui.ribbon_button("Tableau");
    let cell = ui.h.get_by_label("3 × 3").rect().center();
    ui.h.event(Event::PointerMoved(cell));
    ui.steps(2);
    assert!(ui.h.query_by_label_contains("Tableau 3 × 3").is_some());
    ui.click(cell);
    // As in the desktop notebook, the table starts empty, with no header row to translate, in a note.
    let (note, table) = ui.table();
    let cells = ui.cells();
    assert_eq!(cells.len(), 3);
    assert!(
        !table.header && cells[0].iter().all(String::is_empty),
        "{cells:?}"
    );
    // The caret is in the new table, so its contextual Tableau tab shows. Its buttons keep
    // their French labels, so adding and removing rows or columns stay told apart.
    ui.ribbon_tab("Tableau");
    assert_eq!(ui.app().ribbon.tab, RibbonTab::Table);
    assert_eq!(t!("table-insert-below"), "Insérer en dessous");
    for label in [
        t!("table-insert-below"),
        t!("table-insert-right"),
        t!("table-delete-rows"),
        t!("table-delete-columns"),
    ] {
        labelled(&ui, &label);
    }
    ui.shot("l3-fr-selection");
    // The right-click menu of the table's note (on its grab bar) is French: the desktop notebook's
    // Couper, Copier, Coller, Supprimer and Ordre.
    let bar = ui.bar(note);
    ui.right_click(bar);
    for row in ["Couper", "Copier", "Coller", "Supprimer", "Ordre"] {
        assert!(
            ui.h.query_all_by_label_contains(&format!("    {row}"))
                .next()
                .is_some(),
            "no {row:?} row"
        );
    }
    assert!(ui
        .h
        .query_all_by_label_contains("Dupliquer")
        .next()
        .is_none());
    ui.press(Key::Escape);
    ui.ribbon_tab("Insertion");
    ui.ribbon_button("Frise");
    let frise = ui.items().last().unwrap().clone();
    let CanvasKind::Timeline { milestones, .. } = &frise.kind else {
        panic!("a frise")
    };
    let titles: Vec<&str> = milestones.iter().map(|m| m.title.as_str()).collect();
    assert_eq!(titles, ["Avant", "Événement", "Après"]);
    // (The frise editor covers the page until it is saved.)
    ui.steps(4);
    ui.click_label_containing("Enregistrer la frise");
    // Right-click empty paper: the page menu is French.
    let spot = ui.empty_paper_point();
    ui.right_click(spot);
    assert!(ui.h.query_by_label_contains("Zone de texte ici").is_some());
    ui.press(Key::Escape);
    ui.ribbon_tab("Affichage");
    // "100 %" keeps its no-break space.
    ui.ribbon_button_rect("100\u{a0}%");
    // The French View band keeps its labels at the default window too.
    labelled(&ui, "Lecteur immersif");
    labelled(&ui, "Largeur de page");
    ui.shot("l3-fr-view");
    // Switching back to English relabels the ribbon at once; what was written stays French.
    crate::i18n::set_lang(Lang::En);
    ui.steps(2);
    ui.ribbon_tab("Insert");
    let CanvasKind::Timeline { milestones, .. } = &ui.item(frise.id).kind else {
        panic!("a frise")
    };
    assert_eq!(milestones[0].title, "Avant");
}

/// The find bar's status never runs under its buttons, short ("No matches") or long
/// ("10000 of 10000", which the field's old fixed 170 px would push under them).
#[test]
fn the_find_bar_status_stays_clear_of_its_buttons() {
    for lang in crate::i18n::Lang::ALL {
        let mut ui = Ui::start_in(lang);
        ui.new_page("Find");
        ui.press(Key::Enter);
        ui.type_text(&"x ".repeat(10_000));
        ui.press(Key::Escape);
        ui.press_with(Modifiers::COMMAND, Key::F);
        let check = |ui: &mut Ui| {
            ui.steps(2);
            let previous = ui.h.get_by_label(&t!("canvas-find-previous")).rect();
            // The status label reads `FindState::status()`, in the interface language.
            let status = ui.app().canvas.find.as_ref().unwrap().status();
            let rect = ui.h.get_by_label(&status).rect();
            assert!(
                rect.right() <= previous.left(),
                "{lang:?} {status}: {rect:?} vs {previous:?}"
            );
        };
        ui.type_text("zzzz-nowhere");
        check(&mut ui);
        ui.press_with(Modifiers::COMMAND, Key::A);
        ui.type_text("x");
        // Back from the first match to the last: the widest status this page gives.
        ui.press_with(Modifiers::SHIFT, Key::Enter);
        let status = ui.app().canvas.find.as_ref().unwrap().status();
        let widest = match lang {
            Lang::En => "10000 of 10000",
            Lang::Fr => "10000 sur 10000",
        };
        assert_eq!(status, widest);
        check(&mut ui);
    }
}

/// At the smallest window, paused (the wider "Reprendre"), the French Audio & Video tab
/// ("Audio et vidéo") and its buttons sit inside the window, as the English ones do.
#[test]
fn the_recording_indicator_fits_the_narrow_window_in_french() {
    let mut ui = Ui::start_in(Lang::Fr);
    ui.new_page("Cours");
    ui.press(Key::Escape);
    // (Tests have no microphone: this records silence, as Insert → Audio would.)
    {
        let app = ui.h.state_mut();
        let page = app.page.as_mut().unwrap();
        crate::canvas::start_test_recording(page, &mut app.canvas, &mut app.library);
        app.mark_dirty(0.0);
    }
    ui.steps(2);
    ui.h.set_size(egui::vec2(980.0, 900.0));
    ui.steps(3);
    assert_eq!(t!("ribbon-tab-recording"), "Audio et vidéo");
    ui.ribbon_tab("Audio et vidéo");
    ui.ribbon_button("Pause");
    ui.steps(2);
    assert_eq!(
        ui.app().canvas.recording_time().map(|(_, paused)| paused),
        Some(true)
    );
    let tab = ui.ribbon_tab_rect("Audio et vidéo");
    let stop = ui.ribbon_button_rect("Arrêter");
    let resume = ui.ribbon_button_rect("Reprendre");
    assert!(
        tab.right() <= 980.0 && resume.right() <= stop.left() && stop.right() <= 980.0,
        "tab {tab:?}, resume {resume:?}, stop {stop:?}"
    );
    ui.shot("l3-fr-recording-narrow");
    ui.ribbon_button("Reprendre");
    ui.ribbon_button("Arrêter");
    assert!(ui.app().canvas.recording_time().is_none());
}

#[test]
fn a_french_first_run_installs_the_french_sample_library() {
    let mut ui = Ui::start_in(crate::i18n::Lang::Fr);
    let workspace = ui.app().library.workspace().unwrap();
    // A notebook's sections at its top level (`None`) or in one of its groups.
    let names = |notebook: &crate::model::Notebook, group: Option<&str>| -> Vec<String> {
        notebook
            .sections
            .iter()
            .filter(|section| section.group_id.as_deref() == group)
            .map(|section| section.name.clone())
            .collect()
    };
    // Each notebook's own sections, then the section groups with theirs.
    let tree: Vec<(String, Vec<String>)> = workspace
        .notebooks
        .iter()
        .map(|notebook| (notebook.name.clone(), names(notebook, None)))
        .collect();
    let groups: Vec<(String, Vec<String>)> = workspace
        .notebooks
        .iter()
        .flat_map(|notebook| {
            notebook
                .groups
                .iter()
                .map(move |group| (group.name.clone(), names(notebook, Some(&group.id))))
        })
        .collect();
    let owned = |rows: Vec<(&str, Vec<&str>)>| -> Vec<(String, Vec<String>)> {
        rows.into_iter()
            .map(|(name, sections)| {
                (
                    name.into(),
                    sections.into_iter().map(String::from).collect(),
                )
            })
            .collect()
    };
    assert_eq!(
        tree,
        owned(vec![
            (
                "École",
                vec![
                    "Cette semaine",
                    "Histoire-géo",
                    "SVT",
                    "Français",
                    "Anglais",
                    "Maths"
                ]
            ),
            ("Personnel", vec!["Journal", "Recettes", "Notes rapides"]),
        ])
    );
    assert_eq!(groups, owned(vec![("L’an dernier", vec!["Géographie"])]));
    // P2 retired page tags: the sample has none.
    assert!(workspace.tags.is_empty(), "{:?}", workspace.tags);
    // The sidebar shows the French sections, and the first run opens « Ma semaine ».
    assert!(ui
        .h
        .query_by_role_and_label(egui::Role::Button, "Histoire-géo")
        .is_some());
    assert_eq!(ui.app().page.as_ref().unwrap().title, "Ma semaine");

    // The history frise opens, and its (longer) French rows still stack apart.
    ui.h.state_mut().open_page("page-school-frise", true);
    ui.steps(4);
    assert_eq!(ui.app().page.as_ref().unwrap().title, "Le Moyen Âge");
    let id = ui
        .items()
        .iter()
        .find(|item| matches!(item.kind, CanvasKind::Timeline { .. }))
        .expect("a frise")
        .id;
    ui.shot("frise-fr");
    assert_frise_rows_apart(&ui, id, "French frise");
}

/// Every page of the sample library opens, with a title and something on it, in English
/// and in French; a first run opens My week.
#[test]
fn every_sample_page_opens_in_both_languages() {
    for lang in [Lang::En, Lang::Fr] {
        let mut ui = Ui::start_in(lang);
        assert_eq!(
            ui.app()
                .page
                .as_ref()
                .map(|page| page.id.clone())
                .as_deref(),
            Some("page-school-week"),
            "a first run opens My week"
        );
        for id in crate::seed::PAGE_IDS {
            ui.h.state_mut().open_page(id, true);
            ui.steps(4);
            let page = ui.app().page.as_ref().expect("a page is open");
            assert_eq!(page.id, *id, "{lang:?}");
            assert!(!page.title.is_empty(), "{id} {lang:?}");
            assert!(!ui.items().is_empty(), "{id} {lang:?} has content");
        }
    }
}

// The ribbon: the desktop notebook's tabs and groups, scaled to the window, its Simplified layout, Show
// tabs only, and the contextual Table tab.

impl Ui {
    /// The ribbon card, under the tab row, across the window: only ribbon controls are
    /// drawn there.
    fn ribbon_card(&self) -> egui::Rect {
        let width = self.h.ctx.content_rect().width();
        let top = 48.0 + crate::ribbon::TAB_ROW;
        // The panel ends 8 px below the card.
        let bottom = 48.0 + crate::ribbon::panel_height(&self.app().ribbon) - 8.0;
        egui::Rect::from_x_y_ranges(0.0..=width, top..=bottom)
    }

    /// Every control drawn in the ribbon card (a band's buttons, a split's two parts,
    /// gallery items and the display-options chevron), by accessible name.
    fn band_controls(&self) -> Vec<(String, egui::Rect)> {
        use egui_kittest::kittest::NodeT;
        let card = self.ribbon_card();
        self.h
            .query_all_by_role(egui::Role::Button)
            .map(|node| {
                let label = node.accesskit_node().label().unwrap_or_default();
                (label, node.rect())
            })
            .filter(|(_, rect)| rect.top() >= card.top() && rect.bottom() <= card.bottom())
            .collect()
    }

    /// Whether the ribbon control labelled `label` is greyed out.
    fn ribbon_button_disabled(&self, label: &str) -> bool {
        use egui_kittest::kittest::NodeT;
        let rect = self.ribbon_button_rect(label);
        self.h
            .get_all_by_role_and_label(egui::Role::Button, label)
            .find(|node| node.rect() == rect)
            .expect("the ribbon control")
            .accesskit_node()
            .is_disabled()
    }

    /// `tab`'s groups, as the ribbon builds them for the app's current state.
    fn ribbon_groups(&self, tab: RibbonTab) -> Vec<crate::ribbon::Group> {
        let app = self.app();
        let host = super::ShellHost::new(
            app.page.as_ref(),
            &app.canvas,
            &app.editor,
            app.view == super::View::Trash,
            app.full_page,
            app.ribbon.layout,
            app.new_page_rule.is_some(),
        );
        crate::ribbon::groups(tab, &host)
    }
}

/// The names a control can be found by in the band, whatever its group's level: its own
/// (an unlabelled control goes by its tooltip), a split's arrow, and a gallery's items
/// and "▾".
fn control_names(control: &crate::ribbon::Control) -> Vec<String> {
    use crate::ribbon::ControlKind;
    let name = if control.label.is_empty() {
        control.tooltip.to_string()
    } else {
        control.label.to_string()
    };
    let mut names = vec![name.clone()];
    match &control.kind {
        ControlKind::Split(_) => names.push(match &control.arrow_label {
            Some(label) => label.to_string(),
            None => format!("{name} ▾"),
        }),
        ControlKind::Gallery { items, .. } => {
            names.push(format!("{name} ▾"));
            names.extend(
                items
                    .iter()
                    .filter(|item| !item.label.is_empty())
                    .map(|item| item.label.to_string()),
            );
        }
        _ => {}
    }
    names
}

/// The row a control becomes in a menu (a collapsed group's, Simplified's "More
/// commands"): its glyph, then its name.
fn menu_row_label(glyph: &str, name: &str) -> String {
    if glyph.is_empty() {
        name.to_owned()
    } else {
        format!("{glyph}    {name}")
    }
}

/// Home is populated as in the desktop notebook even with nothing to format: its commands show, greyed
/// out until a note is being typed in, and Bold then formats what is typed next without
/// taking the caret out of the note.
#[test]
fn home_is_populated_with_nothing_to_format() {
    let mut ui = Ui::start();
    ui.new_page("Nothing yet");
    // The caret is in the title: no note is being typed in.
    assert_eq!(ui.app().canvas.active_text, None);
    for label in [
        "Bold",
        "Italic",
        "Underline",
        "Font",
        "Font size",
        "To Do Tag",
        "Find Tags",
    ] {
        ui.ribbon_button_rect(label);
    }
    assert!(ui.ribbon_button_disabled("Bold"), "nothing to make bold");
    assert!(
        !ui.ribbon_button_disabled("Find Tags"),
        "Find Tags opens its pane with nothing typed"
    );
    ui.shot("p2-home-nothing-to-format");

    ui.press(Key::Enter);
    ui.type_text("Plain ");
    let note = ui.items()[0].id;
    assert_eq!(ui.app().canvas.active_text, Some(note));
    assert!(!ui.ribbon_button_disabled("Bold"));
    ui.ribbon_button("Bold");
    // Typing goes on in the note, in bold.
    ui.type_text("bold");
    ui.shot("p2-home-bold");
    let block = ui.item(note).doc().unwrap().blocks[0].clone();
    assert_eq!(block.text.text, "Plain bold");
    assert!(
        block.text.style_at(6).bold && block.text.style_at(9).bold,
        "{block:#?}"
    );
    assert!(!block.text.style_at(2).bold, "{block:#?}");
    assert_eq!(ui.app().canvas.active_text, Some(note), "the caret stayed");
}

/// Home opens with the desktop notebook's Clipboard group and Basic Text's lists and indents. As in
/// the desktop notebook, Paste and the Format Painter work from a caret alone, while Cut and Copy only
/// light up once there is something to take.
#[test]
fn home_has_notebook_clipboard_lists_and_indents() {
    let mut ui = Ui::start();
    ui.new_page("Clipboard");
    ui.press(Key::Enter);
    ui.type_text("First");
    assert_eq!(
        ui.app().ribbon.tab,
        RibbonTab::Home,
        "Home is the tab at start"
    );
    for label in [
        "Paste",
        "Cut",
        "Copy",
        "Format Painter",
        "Bullets",
        "Numbering",
        "Decrease Indent",
        "Increase Indent",
        "Clear formatting",
        "Bold",
        "Delete",
    ] {
        ui.ribbon_button_rect(label);
    }
    ui.shot("p5-home-clipboard");
    assert!(
        !ui.ribbon_button_disabled("Format Painter"),
        "the Format Painter picks up the formatting at the caret"
    );
    assert!(!ui.ribbon_button_disabled("Paste"));
    // A caret alone: there is nothing to cut or copy.
    assert!(ui.ribbon_button_disabled("Cut"));
    assert!(ui.ribbon_button_disabled("Copy"));
    ui.press_with(Modifiers::SHIFT, Key::Home);
    assert!(!ui.ribbon_button_disabled("Cut"), "\"First\" is selected");
    assert!(!ui.ribbon_button_disabled("Copy"), "\"First\" is selected");
    ui.shot("p5-home-clipboard-selection");
}

/// Whether one of the frames asked the platform for the clipboard (it answers with a paste
/// event the next frame).
fn requests_paste(outputs: &[egui::FullOutput]) -> bool {
    outputs.iter().any(|output| {
        output.viewport_output.values().any(|viewport| {
            viewport
                .commands
                .contains(&egui::ViewportCommand::RequestPaste)
        })
    })
}

/// Home › Cut, Copy and Paste do what Ctrl+X, Ctrl+C and Ctrl+V do: on the text selected
/// in the note being typed in, else on the page's selected items.
#[test]
fn cut_copy_and_paste_from_the_ribbon() {
    let mut ui = Ui::start();
    ui.new_page("Clipboard");
    ui.press(Key::Enter);
    ui.type_text("Hello world");
    let note = ui.items()[0].id;
    let text = |ui: &Ui| {
        ui.item(note).doc().expect("a note").blocks[0]
            .text
            .text
            .clone()
    };
    // Shift+Ctrl+← selects "world". (On Linux and Windows, Ctrl is both `ctrl`, which moves
    // egui's caret by words, and `command`.)
    ui.press_with(
        Modifiers::CTRL | Modifiers::COMMAND | Modifiers::SHIFT,
        Key::ArrowLeft,
    );

    let outputs = ui.ribbon_button_outputs("Copy");
    assert_eq!(copied(&outputs), ["world"]);
    assert_eq!(text(&ui), "Hello world", "copying changes nothing");
    let outputs = ui.ribbon_button_outputs("Cut");
    assert_eq!(copied(&outputs), ["world"]);
    assert_eq!(text(&ui), "Hello ");
    // The caret stayed in the note, so Ctrl+Z reaches it.
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert_eq!(text(&ui), "Hello world", "the cut is one undo step");

    // egui can't read the clipboard on demand: Paste asks the platform, which answers with
    // a paste event for the note next frame.
    ui.press(Key::End);
    let outputs = ui.ribbon_button_outputs("Paste");
    assert!(
        requests_paste(&outputs),
        "Paste asks the platform for the clipboard"
    );
    ui.h.event(Event::Paste(" again".into()));
    ui.steps(2);
    assert_eq!(text(&ui), "Hello world again");
    ui.shot("p5-ribbon-paste");

    // Off the note, they act on the page's selection: a rectangle drawn from the Shapes
    // gallery is selected, Copy takes it and Paste asks for the clipboard.
    ui.click(ui.empty_paper_point());
    ui.ribbon_tab("Draw");
    ui.ribbon_button("Rectangle");
    let from = ui.empty_paper_point();
    ui.drag(from, from + egui::vec2(120.0, 80.0));
    let shape = ui
        .items()
        .into_iter()
        .find(|item| matches!(item.kind, CanvasKind::Shape { .. }))
        .expect("the rectangle was drawn");
    assert_eq!(ui.app().canvas.selection.primary(), Some(shape.id));
    ui.ribbon_tab("Home");
    assert!(
        !ui.ribbon_button_disabled("Copy"),
        "the rectangle is selected"
    );
    let outputs = ui.ribbon_button_outputs("Copy");
    assert!(
        copied(&outputs).iter().any(|text| !text.is_empty()),
        "Copy put the rectangle on the clipboard"
    );
    let outputs = ui.ribbon_button_outputs("Paste");
    assert!(
        requests_paste(&outputs),
        "Paste asks the platform for the clipboard"
    );
    ui.shot("p5-ribbon-copy-items");
}

/// Home's Decrease and Increase Indent change the paragraph's outline level, as Shift+Tab
/// and Tab do, and leave the caret where it was.
#[test]
fn indent_buttons_change_the_outline_level() {
    let mut ui = Ui::start();
    ui.new_page("Levels");
    ui.press(Key::Enter);
    ui.type_text("a");
    ui.press(Key::Enter);
    ui.type_text("b");
    let level = |ui: &Ui| ui.blocks()[1].indent;
    assert_eq!(level(&ui), 0);

    ui.ribbon_button("Increase Indent");
    assert_eq!(level(&ui), 1, "\"b\" went one level under \"a\"");
    ui.type_text("c");
    assert_eq!(ui.blocks()[1].text.text, "bc", "the caret stayed in \"b\"");
    ui.shot("p5-increase-indent");

    ui.ribbon_button("Decrease Indent");
    assert_eq!(level(&ui), 0);
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert_eq!(level(&ui), 1, "Ctrl+Z takes the decrease back");
    assert_eq!(ui.blocks()[1].text.text, "bc", "and nothing more");
}

/// The arrows of Bullets and Numbering open the desktop notebook's marker libraries: the pick styles
/// the list item being typed in (and its export), and None ends the list.
#[test]
fn bullet_and_numbering_libraries_set_the_marker() {
    use crate::model::{Block, BlockKind, ListMarker};
    let mut ui = Ui::start();
    ui.new_page("Markers");
    ui.press(Key::Enter);
    ui.press_with(Modifiers::COMMAND, Key::Period);
    ui.type_text("one");
    ui.press(Key::Enter);
    ui.type_text("two");
    let active = |ui: &Ui| -> Block {
        let id = ui.app().editor.active_block.expect("typing in the note");
        ui.blocks()
            .into_iter()
            .find(|block| block.id == id)
            .expect("the paragraph being typed in")
    };
    assert_eq!(active(&ui).kind, BlockKind::Bullet);

    ui.ribbon_button("Numbering ▾");
    ui.shot("p5-numbering-library");
    let cell =
        ui.h.get_all_by_role_and_label(egui::Role::Button, "a.")
            .last()
            .expect("the a. marker")
            .rect();
    ui.click(cell.center());
    let block = active(&ui);
    assert_eq!(block.text.text, "two", "the caret stayed in \"two\"");
    assert_eq!(
        (block.kind, block.marker),
        (BlockKind::Numbered, Some(ListMarker::LowerAlpha))
    );
    ui.shot("p5-numbering-lower-alpha");
    let html = {
        let app = ui.app();
        crate::export::canvas_export_html(app.page.as_ref().expect("a page"), &app.library)
    };
    assert!(html.contains("lower-alpha"), "{html}");

    ui.ribbon_button("Bullets ▾");
    let none =
        ui.h.get_all_by_role_and_label(egui::Role::Button, "None")
            .last()
            .expect("the None row")
            .rect();
    ui.click(none.center());
    assert_eq!(active(&ui).kind, BlockKind::Paragraph, "None ends the list");
}

/// Whether `text` is a time of day as Folio writes it into notes: "18:41".
fn is_clock_time(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    chars.len() == 5
        && chars[2] == ':'
        && [0, 1, 3, 4]
            .iter()
            .all(|&index| chars[index].is_ascii_digit())
}

/// Time stamps go in at the caret of the note being typed in, from the keyboard
/// (Alt+Shift+T) as from Insert › Time Stamp.
#[test]
fn time_stamps_go_in_at_the_caret() {
    let mut ui = Ui::start();
    ui.new_page("Stamps");
    ui.press(Key::Enter);
    ui.press_with(Modifiers::ALT | Modifiers::SHIFT, Key::T);
    let time = ui.blocks()[0].text.text.clone();
    assert!(is_clock_time(&time), "{time:?}");

    ui.press(Key::Enter);
    ui.ribbon_tab("Insert");
    // The minute may turn while the button is clicked.
    let before = crate::editor::now_time();
    ui.ribbon_button("Date & Time");
    let after = crate::editor::now_time();
    let line = ui.blocks()[1].text.text.clone();
    assert!(line.starts_with(&crate::editor::today()), "{line:?}");
    assert!(
        line.ends_with(&before) || line.ends_with(&after),
        "{line:?} ends with the time"
    );
    // the desktop notebook's stamps carry the short date: "10/29/2026 18:41".
    let (date, _) = line.split_once(' ').expect("a date, then the time");
    let parts: Vec<&str> = date.split('/').collect();
    assert!(
        parts.len() == 3
            && parts
                .iter()
                .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit())),
        "{line:?} starts with a short date"
    );
    ui.shot("p5-date-and-time");
}

// Insert: what it puts goes in at the caret of the note being typed in, else into a new
// note at the last click; the table grid, Insert Space, page templates and symbols.

impl Ui {
    /// Queues `path` for the next file dialog, which takes it instead of asking.
    fn queue_pick(&self, path: std::path::PathBuf) {
        crate::canvas::PICKS
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .push(path);
    }

    /// A small red picture, 40 × 30, written into the test's temp dir and queued for the
    /// next file dialog.
    fn queue_png(&self) {
        let path = self._dir.path().join("red.png");
        image::RgbaImage::from_pixel(40, 30, image::Rgba([200, 30, 30, 255]))
            .save(&path)
            .expect("write the picture");
        self.queue_pick(path);
    }

    /// A file `name` holding `bytes`, written into the test's temp dir and queued for the
    /// next file dialog.
    fn queue_file(&self, name: &str, bytes: &[u8]) {
        let path = self._dir.path().join(name);
        std::fs::write(&path, bytes).expect("write the file");
        self.queue_pick(path);
    }

    /// The notes on the open page, by id.
    fn note_ids(&self) -> Vec<u64> {
        self.items()
            .iter()
            .filter(|item| item.doc().is_some())
            .map(|item| item.id)
            .collect()
    }
}

/// A note's blocks in short: a paragraph's text, or what else it holds in brackets.
fn outline_of(blocks: &[crate::model::Block]) -> Vec<String> {
    use crate::model::BlockKind;
    blocks
        .iter()
        .map(|block| match &block.kind {
            BlockKind::Image { .. } => "[picture]".to_owned(),
            BlockKind::File { name, size, .. } => format!("[file {name} {size}]"),
            BlockKind::Equation { source, .. } => format!("[equation {source}]"),
            BlockKind::Audio { .. } => "[recording]".to_owned(),
            _ => block.text.text.clone(),
        })
        .collect()
}

/// While you type in a note, Pictures, File Attachment and Equation put what they insert
/// into it at the caret, as the desktop notebook puts them in the container you are typing in: the
/// paragraph splits there, typing carries on after it, and the note grows round the
/// picture.
#[test]
fn pictures_files_and_equations_go_where_you_type() {
    let mut ui = Ui::start();
    ui.new_page("Inserts");
    ui.press(Key::Enter);
    ui.type_text("BeforeAfter");
    for _ in 0.."After".len() {
        ui.press(Key::ArrowLeft);
    }
    let note = ui.items()[0].id;
    let outline = |ui: &Ui| outline_of(&ui.item(note).doc().expect("a note").blocks);

    ui.queue_png();
    ui.ribbon_tab("Insert");
    ui.ribbon_button("Pictures");
    assert_eq!(outline(&ui), ["Before", "[picture]", "After"]);
    // The caret went on to the second half.
    ui.type_text("!");
    assert_eq!(outline(&ui), ["Before", "[picture]", "!After"]);

    ui.queue_file("notes.txt", b"0123456789");
    ui.ribbon_button("File Attachment");
    assert_eq!(
        outline(&ui),
        ["Before", "[picture]", "!", "[file notes.txt 10]", "After"]
    );

    // An equation goes in at the caret too, its source taking the keyboard, with the
    // contextual Equation tab while it is typed.
    ui.ribbon_button("Equation");
    ui.type_text("x^2");
    assert!(
        ui.h.query_by_role_and_label(egui::Role::Tab, "Equation")
            .is_some(),
        "the Equation tab shows while the equation is typed"
    );
    ui.press(Key::Enter);
    let with_equation = [
        "Before",
        "[picture]",
        "!",
        "[file notes.txt 10]",
        "[equation x^2]",
        "After",
    ];
    assert_eq!(outline(&ui), with_equation);

    // The note grew round the picture and keeps that width as its text changes.
    let item = ui.item(note);
    assert!(!item.auto_width, "the picture sized the note");
    assert!(
        item.width >= 40.0 + 2.0 * crate::canvas::NOTE_PAD_X,
        "{} holds the 40 px picture",
        item.width
    );
    let picture = item.doc().unwrap().blocks[1].id;
    let drawn = ui.row(note, picture).rect;
    assert!(
        drawn.left() >= item.rect().left() && drawn.right() <= item.rect().right(),
        "{drawn:?} inside {:?}",
        item.rect()
    );
    ui.shot("p5-insert-at-the-caret");

    // Inserting the equation and typing it was one step of the note's history.
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert_eq!(
        outline(&ui),
        ["Before", "[picture]", "!", "[file notes.txt 10]", "After"]
    );
}

/// With no note being typed in, what Insert puts goes into a new note at the last place
/// the paper was clicked (the desktop notebook's insertion point).
#[test]
fn with_no_note_inserts_make_one_at_the_last_click() {
    let mut ui = Ui::start();
    ui.new_page("Last click");
    let first = ui.empty_paper_point();
    ui.click(first);
    assert!(
        ui.app().canvas.active_text.is_some(),
        "a click on empty paper starts a note there"
    );
    let last = ui.empty_paper_point();
    assert!(last.distance(first) > 100.0, "{last:?} is elsewhere");
    ui.click(last);
    // Nothing typed: the note the click started ends, and the page keeps the click.
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    assert_eq!(ui.app().canvas.active_text, None);

    let before = ui.note_ids();
    ui.ribbon_tab("Insert");
    ui.ribbon_button("Date");
    let today = crate::editor::today();
    let dated = ui
        .items()
        .into_iter()
        .find(|item| text_of(item) == today)
        .expect("a note holding today's date");
    assert!(!before.contains(&dated.id), "a new note");
    let line = dated.doc().unwrap().blocks[0].id;
    let at = ui.text_point(dated.id, line, 0);
    assert!(
        at.distance(last) <= 40.0,
        "the date starts at {at:?}, near the last click at {last:?}"
    );
    ui.shot("p5-insert-at-the-last-click");

    // A picture too, once nothing is being typed.
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    assert_eq!(ui.app().canvas.active_text, None);
    let before = ui.note_ids();
    ui.queue_png();
    ui.ribbon_button("Pictures");
    let pictured = ui
        .items()
        .into_iter()
        .find(|item| !before.contains(&item.id))
        .expect("a new note");
    assert!(
        pictured.doc().is_some_and(|doc| doc
            .blocks
            .iter()
            .any(|block| matches!(block.kind, crate::model::BlockKind::Image { .. }))),
        "{pictured:?} holds the picture"
    );
}

/// Insert › Table opens the desktop notebook's grid: hovering a cell names the size, and a click inserts
/// a table of that size in a note, without a header row, with the caret in its first cell
/// and its contextual Table tab.
#[test]
fn the_table_grid_inserts_the_size_picked() {
    let mut ui = Ui::start();
    ui.new_page("Grid");
    ui.ribbon_tab("Insert");
    ui.ribbon_button("Table");
    let cell = ui.h.get_by_label("4 × 2").rect().center();
    ui.h.event(Event::PointerMoved(cell));
    ui.steps(2);
    assert!(
        ui.h.query_all_by_label_contains("4 × 2 Table")
            .next()
            .is_some(),
        "the grid names the size hovered"
    );
    ui.shot("p5-table-grid");
    ui.click(cell);

    // Nothing was being typed in: one new note holds the table; no page table any more.
    let items = ui.items();
    assert_eq!(items.len(), 1, "{items:?}");
    assert!(items[0].doc().is_some(), "{:?} is a note", items[0].kind);
    let (note, table) = ui.table();
    assert_eq!(note, items[0].id);
    let rows = ui.cells();
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert!(
        rows.iter()
            .all(|row| row.len() == 4 && row.iter().all(String::is_empty)),
        "{rows:?}"
    );
    assert!(
        !table.header,
        "the desktop notebook's tables have no header row"
    );
    assert_eq!(ui.app().canvas.active_text, Some(note));
    assert_eq!(
        ui.app().editor.active_cell,
        Some((0, 0)),
        "the caret is in A1"
    );
    assert!(
        ui.h.query_by_role_and_label(egui::Role::Tab, "Table")
            .is_some(),
        "the Table tab comes with the table"
    );
}

/// Insert › Insert Space, as in the desktop notebook: a drag down from a line pushes what is below it
/// down, as one undo step, and the tool goes back to Select; a drag up closes the space
/// only until what is below reaches the line.
#[test]
fn insert_space_pushes_content_down_and_pulls_it_back() {
    use crate::canvas::{PAGE_CONTENT_TOP, PAGE_LEFT};
    let mut ui = Ui::start();
    ui.new_page("Space");
    ui.press(Key::Enter);
    ui.type_text("Upper");
    let upper = ui.items()[0].id;
    ui.click(ui.screen(egui::pos2(PAGE_LEFT, PAGE_CONTENT_TOP + 300.0)));
    ui.type_text("Lower");
    let lower = ui
        .items()
        .into_iter()
        .find(|item| text_of(item) == "Lower")
        .expect("the lower note")
        .id;
    let top = |ui: &Ui, id: u64| ui.item(id).y;
    let (upper_top, lower_top) = (top(&ui, upper), top(&ui, lower));
    assert!(
        lower_top - ui.item(upper).rect().bottom() >= 150.0,
        "{upper_top} and {lower_top} are far apart"
    );
    // Just above the lower note, clear of the upper one.
    let line = lower_top - 20.0;
    let from = ui.screen(egui::pos2(PAGE_LEFT + 100.0, line));

    ui.ribbon_tab("Insert");
    ui.ribbon_button("Insert Space");
    assert_eq!(ui.app().canvas.tool, CanvasTool::InsertSpace);
    ui.drag(from, ui.screen(egui::pos2(PAGE_LEFT + 100.0, line + 80.0)));
    assert_eq!(
        top(&ui, lower),
        lower_top + 80.0,
        "the lower note went down"
    );
    assert_eq!(top(&ui, upper), upper_top, "the upper note stayed");
    assert_eq!(ui.app().canvas.tool, CanvasTool::Select, "one use");
    ui.shot("p5-insert-space");

    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert_eq!(top(&ui, lower), lower_top, "one undo puts it back");

    // Dragging up takes space out only until the note reaches the line.
    ui.ribbon_button("Insert Space");
    ui.drag(from, ui.screen(egui::pos2(PAGE_LEFT + 100.0, line - 400.0)));
    assert!(
        (top(&ui, lower) - line).abs() <= 1.0,
        "the note stopped at the line: {} for {line}",
        top(&ui, lower)
    );
    assert_eq!(top(&ui, upper), upper_top);
}

/// Insert › Page Templates adds a page laid out from the template, in the app's language.
#[test]
fn page_templates_add_a_page() {
    use crate::model::BlockKind;
    use crate::templates::PageTemplate;
    /// Adds a To-do list page in `lang`, checks it, and returns its note's first line.
    fn add_todo_list(lang: Lang) -> String {
        let mut ui = Ui::start_in(lang);
        let pages = ui.app().pages.len();
        let open = ui.app().page.as_ref().map(|page| page.id.clone());
        ui.ribbon_tab(&t!("ribbon-tab-insert"));
        ui.ribbon_button(&t!("ribbon-page-templates"));
        ui.shot(&format!("p5-page-templates-{}", lang.code()));
        let row = menu_row_label(PageTemplate::ToDoList.glyph(), &t!("ribbon-template-todo"));
        ui.click_label(&row);

        let page = ui.app().page.as_ref().expect("a page is open");
        assert_ne!(Some(&page.id), open.as_ref(), "a new page is open");
        assert_eq!(page.title, "", "its title is left to type");
        assert_eq!(ui.app().pages.len(), pages + 1, "the page list has it");
        let items = ui.items();
        assert_eq!(items.len(), 1, "one note: {items:?}");
        let blocks = &items[0].doc().expect("a note").blocks;
        let todos = blocks
            .iter()
            .filter(|block| matches!(block.kind, BlockKind::Todo { checked: false }))
            .count();
        assert_eq!(todos, 5, "{blocks:?}");
        ui.shot(&format!("p5-todo-list-page-{}", lang.code()));
        blocks[0].text.text.clone()
    }
    assert_eq!(add_todo_list(Lang::En), "To do");
    assert_eq!(add_todo_list(Lang::Fr), "À faire");
}

/// Insert › Symbol puts the symbol picked at the caret, and typing carries on after it.
#[test]
fn symbols_go_in_at_the_caret() {
    let mut ui = Ui::start();
    ui.new_page("Symbols");
    ui.press(Key::Enter);
    ui.type_text("a");
    ui.ribbon_tab("Insert");
    ui.ribbon_button("Symbol");
    ui.shot("p5-symbol-grid");
    let cell =
        ui.h.get_all_by_role_and_label(egui::Role::Button, "±")
            .last()
            .expect("the ± symbol")
            .rect();
    ui.click(cell.center());
    assert_eq!(ui.blocks()[0].text.text, "a±");
    ui.type_text("b");
    assert_eq!(ui.blocks()[0].text.text, "a±b", "the caret stayed after it");
}

/// With the caret in the page title, Insert › Symbol and More Symbols… put the symbol at
/// the title's caret, as the desktop notebook does, not in a note; the title keeps the keyboard.
#[test]
fn symbols_go_into_the_title_at_its_caret() {
    use egui::Role;
    let mut ui = Ui::start();
    ui.new_page("Pi day");
    // The caret after "Pi".
    for _ in 0..4 {
        ui.press(Key::ArrowLeft);
    }
    let title = |ui: &Ui| ui.app().page.as_ref().expect("a page").title.clone();
    pick_symbol(&mut ui, "π");
    assert_eq!(title(&ui), "Piπ day");
    assert!(ui.items().is_empty(), "no note was made for it");
    ui.type_text("e");
    assert_eq!(
        title(&ui),
        "Piπe day",
        "the caret stayed after it, in the title"
    );

    // More Symbols… too, and the title has the keyboard once the dialog is closed.
    ui.steps(30);
    ui.ribbon_button("Symbol");
    ui.click_label_containing(&t!("editor-more-symbols"));
    ui.steps(3);
    ui.type_text("infty");
    let cell =
        ui.h.get_all_by_role_and_label(Role::Button, "∞")
            .last()
            .expect("∞ found by its name")
            .rect();
    ui.click(cell.center());
    ui.steps(30);
    let button = |ui: &Ui, label: &str| {
        ui.h.get_all_by_role_and_label(Role::Button, label)
            .last()
            .unwrap_or_else(|| panic!("no {label:?}"))
            .rect()
    };
    let insert = button(&ui, &t!("editor-insert"));
    ui.click(insert.center());
    assert_eq!(title(&ui), "Piπe∞ day");
    ui.steps(30);
    let close = button(&ui, &t!("common-close"));
    ui.click(close.center());
    assert!(ui.app().dialog.is_none());
    ui.steps(3);
    ui.type_text("!");
    assert_eq!(title(&ui), "Piπe∞! day");
    assert!(ui.items().is_empty(), "still no note");

    // Once a note has the caret, symbols go there again.
    ui.press(Key::Enter);
    ui.type_text("a");
    pick_symbol(&mut ui, "±");
    assert_eq!(ui.blocks()[0].text.text, "a±");
    assert_eq!(title(&ui), "Piπe∞! day");
}

/// With the caret in the page title, Insert › Date, Time and Date & Time go in at the
/// title's caret, as symbols do, not in a note; the title keeps the keyboard.
#[test]
fn date_and_time_go_into_the_title_at_its_caret() {
    let mut ui = Ui::start();
    ui.new_page("Log");
    let title = |ui: &Ui| ui.app().page.as_ref().expect("a page").title.clone();
    ui.type_text(" ");
    ui.ribbon_tab("Insert");
    ui.ribbon_button("Date");
    let today = crate::editor::today();
    assert_eq!(title(&ui), format!("Log {today}"));
    assert!(ui.items().is_empty(), "no note was made for it");
    ui.type_text(" at ");
    assert_eq!(
        title(&ui),
        format!("Log {today} at "),
        "the caret stayed after it, in the title"
    );
    ui.steps(30);
    ui.ribbon_button("Time");
    let stamped = title(&ui);
    let time = stamped
        .strip_prefix(&format!("Log {today} at "))
        .expect("the time after what was typed");
    assert!(time.contains(':'), "{stamped:?}");
    ui.steps(30);
    ui.ribbon_button("Date & Time");
    assert!(
        title(&ui).len() > stamped.len() && title(&ui).starts_with(&stamped),
        "{:?}",
        title(&ui)
    );
    assert!(ui.items().is_empty(), "still no note");
}

/// Picks one symbol from Insert › Symbol.
fn pick_symbol(ui: &mut Ui, symbol: &str) {
    ui.ribbon_tab("Insert");
    ui.ribbon_button("Symbol");
    let cell =
        ui.h.get_all_by_role_and_label(egui::Role::Button, symbol)
            .last()
            .unwrap_or_else(|| panic!("the {symbol} symbol"))
            .rect();
    ui.click(cell.center());
}

/// A symbol picked while an equation is typed (Alt+= at the end of a line) goes into its
/// source at the caret: opening the Symbol menu does not end the equation, and the source
/// field keeps the keyboard. With a file's card the active block (it was clicked), the
/// symbol goes in the paragraph after the card.
#[test]
fn symbols_go_into_the_equation_being_typed_and_after_a_card() {
    let mut ui = Ui::start();
    ui.new_page("Maths");
    ui.press(Key::Enter);
    ui.type_text("Area");
    let note = ui.items()[0].id;
    let outline = |ui: &Ui| outline_of(&ui.item(note).doc().expect("a note").blocks);
    // Alt+= at the end of the line: the equation after it is what is typed in.
    ui.press_with(Modifiers::ALT, Key::Equals);
    ui.type_text("x=");
    let (_, equation) = equation_blocks(&ui)[0].clone();
    assert_eq!(ui.app().editor.active_block, Some(equation.id));
    // The Symbol menu opens without ending it.
    pick_symbol(&mut ui, "±");
    assert_eq!(sources(&ui), ["x=±"], "the symbol went into the source");
    assert_eq!(
        ui.app().editor.equation_block,
        Some(equation.id),
        "the session is still open"
    );
    ui.type_text("1");
    assert_eq!(sources(&ui), ["x=±1"], "the source field kept the keyboard");
    ui.press(Key::Enter);
    assert_eq!(ui.app().editor.equation_block, None);
    assert_eq!(outline(&ui), ["Area", "[equation x=±1]", ""]);

    // A file at the caret, then a click on its card: the symbol goes after the card.
    ui.queue_file("notes.txt", b"0123456789");
    ui.ribbon_button("File Attachment");
    assert_eq!(
        outline(&ui),
        ["Area", "[equation x=±1]", "[file notes.txt 10]", ""]
    );
    let card = ui.item(note).doc().unwrap().blocks[2].id;
    let row = ui.row(note, card).rect;
    ui.click(ui.screen(row.left_center() + egui::vec2(60.0, 0.0)));
    assert_eq!(ui.app().editor.active_block, Some(card));
    pick_symbol(&mut ui, "π");
    assert_eq!(
        outline(&ui),
        ["Area", "[equation x=±1]", "[file notes.txt 10]", "π"]
    );
    ui.type_text("r");
    assert_eq!(
        outline(&ui),
        ["Area", "[equation x=±1]", "[file notes.txt 10]", "πr"],
        "the caret is after it"
    );
}

/// A PDF attached in a note (Insert › File Attachment) becomes a printout from its card's
/// menu, as the desktop notebook's attachment icon offers Insert Printout: the pages go under the note,
/// lined up with its text, with no second icon, and the card then offers no second
/// printout. With Options › Advanced › Printouts on, the pages go in the background.
#[test]
fn a_pdf_attached_in_a_note_becomes_a_background_printout() {
    use crate::model::BlockKind;
    let mut ui = Ui::start();
    ui.new_page("Worksheet");

    // Options › Advanced › Printouts: "Automatically set inserted file printouts in the
    // background", kept as a setting.
    ui.h.state_mut().dialog = Some(super::Dialog::Settings);
    ui.steps(3);
    ui.options_page(&t!("app-options-advanced"));
    ui.click_label(&t!("printout-option-background"));
    assert_eq!(
        ui.app()
            .library
            .setting("printout_background")
            .unwrap()
            .as_deref(),
        Some("true")
    );
    ui.click_label(&t!("common-ok"));
    assert!(ui.app().dialog.is_none());

    ui.ribbon_tab("Insert");
    ui.queue_file(
        "sheet.pdf",
        &crate::pdf::letter_pdf(&["Question one", "Question two"]),
    );
    ui.ribbon_button("File Attachment");
    let note = ui.note_ids()[0];
    let (card, attachment) = ui
        .item(note)
        .doc()
        .unwrap()
        .blocks
        .iter()
        .find_map(|block| match &block.kind {
            BlockKind::File { attachment, .. } => Some((block.id, attachment.clone())),
            _ => None,
        })
        .expect("the PDF's card is in the note");
    let at = ui.screen(ui.row(note, card).rect.left_center() + egui::vec2(60.0, 0.0));
    ui.right_click(at);
    ui.click_label_containing("Insert Printout");
    for _ in 0..1000 {
        ui.h.step();
        if !ui.app().canvas.printout_busy() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    ui.steps(2);

    let items = ui.items();
    let mut pages: Vec<&CanvasItem> = items
        .iter()
        .filter(|item| item.kind.printout().is_some())
        .collect();
    pages.sort_by_key(|item| item.kind.printout().map(|printout| printout.page));
    assert_eq!(pages.len(), 2);
    let frame = ui.item(note);
    for page in &pages {
        let printout = page.kind.printout().unwrap();
        assert_eq!(printout.source, attachment, "the pages know their PDF");
        assert_eq!(page.x, (frame.x + crate::canvas::NOTE_PAD_X).round());
        assert!(page.is_background(), "Options › Advanced › Printouts is on");
    }
    assert!(pages[0].y >= frame.y + frame.height, "under the note");
    assert!(pages[1].y > pages[0].y + pages[0].height, "in page order");
    assert!(
        !items
            .iter()
            .any(|item| matches!(item.kind, CanvasKind::File { .. })),
        "the card in the note is the printout's icon"
    );

    // The card has its printout now: no second one.
    ui.right_click(at);
    assert!(
        ui.h.query_by_label_contains("Save As").is_some(),
        "its menu"
    );
    assert!(ui.h.query_by_label_contains("Insert Printout").is_none());
    ui.press(Key::Escape);

    // A page in the background still copies its text and opens its PDF.
    let on_sheet = ui.screen(pages[0].rect().min + egui::vec2(200.0, 150.0));
    assert!(ui.app().canvas.viewport().contains(on_sheet), "in view");
    ui.right_click(on_sheet);
    assert!(
        ui.h.query_by_label_contains("Set Picture as Background")
            .is_some(),
        "the paper's menu for a background picture"
    );
    assert!(ui.h.query_by_label_contains("Open Original").is_some());
    let outputs = ui.click_label_outputs("Copy Text from All Pages of the Printout");
    assert_eq!(copied(&outputs), ["Question one\n\nQuestion two"]);
}

/// Record Audio while typing puts the recording in the note at the caret, as the desktop notebook puts
/// its icon at the cursor; stopping gives it its length, and its card plays it.
#[test]
fn a_recording_started_while_typing_goes_into_the_note() {
    use crate::model::BlockKind;
    let mut ui = Ui::start();
    ui.new_page("Lecture");
    ui.press(Key::Enter);
    ui.type_text("Lecture");
    let note = ui.items()[0].id;
    // (Tests have no microphone: this records silence.)
    ui.ribbon_tab("Insert");
    ui.ribbon_button("Record Audio");
    let blocks = ui.blocks();
    assert_eq!(outline_of(&blocks)[..2], ["Lecture", "[recording]"]);
    let (block, attachment) = match &blocks[1].kind {
        BlockKind::Audio {
            attachment,
            duration_ms,
        } => {
            assert_eq!(*duration_ms, 0, "it is recording");
            (blocks[1].id, attachment.clone())
        }
        other => panic!("not a recording: {other:?}"),
    };
    assert!(
        ui.h.query_by_role_and_label(egui::Role::Tab, "Audio & Video")
            .is_some(),
        "the Audio & Video tab shows while it records"
    );
    ui.shot("p5-recording-in-a-note");

    ui.ribbon_tab("Audio & Video");
    ui.ribbon_button("Stop");
    assert!(ui.app().canvas.recording_time().is_none(), "it stopped");
    assert!(
        matches!(
            &ui.blocks()[1].kind,
            BlockKind::Audio { attachment: clip, duration_ms } if *clip == attachment && *duration_ms > 0
        ),
        "{:?}",
        ui.blocks()[1]
    );
    ui.steps(2);
    assert_eq!(ui.app().editor.media.recording, None);

    // The play circle at the card's left plays it.
    let card = ui.row(note, block).rect;
    ui.click(ui.screen(egui::pos2(card.left() + 26.0, card.center().y)));
    assert_eq!(
        ui.app().canvas.player_attachment(),
        Some(attachment.as_str())
    );
}

/// A recording in a note keeps its length once stopped: undoing what was typed while it
/// recorded does not take the length away, and one stopped from another page (it carries
/// on while you move, as in the desktop notebook) has its length in its own page too.
#[test]
fn a_recording_in_a_note_keeps_its_length() {
    use crate::model::BlockKind;
    let mut ui = Ui::start();
    ui.new_page("Other");
    save(&mut ui);
    ui.new_page("Lecture");
    ui.press(Key::Enter);
    ui.type_text("Lecture");
    let note = ui.items()[0].id;
    let lecture = ui.app().page.as_ref().unwrap().id.clone();
    let lengths = |blocks: &[crate::model::Block]| -> Vec<u64> {
        blocks
            .iter()
            .filter_map(|block| match &block.kind {
                BlockKind::Audio { duration_ms, .. } => Some(*duration_ms),
                _ => None,
            })
            .collect()
    };

    // Words typed while it records, taken back after it stopped.
    ui.ribbon_tab("Insert");
    ui.ribbon_button("Record Audio");
    // A second later, so the typing is an undo step of its own.
    ui.steps(60);
    ui.type_text("during");
    assert_eq!(
        outline_of(&ui.blocks()),
        ["Lecture", "[recording]", "during"]
    );
    ui.ribbon_tab("Audio & Video");
    ui.ribbon_button("Stop");
    assert!(lengths(&ui.blocks())[0] > 0, "{:?}", ui.blocks());
    let typed = ui.blocks()[2].id;
    ui.click(ui.text_point(note, typed, 6));
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert_eq!(outline_of(&ui.blocks()), ["Lecture", "[recording]", ""]);
    assert!(
        lengths(&ui.blocks())[0] > 0,
        "undo kept the length: {:?}",
        ui.blocks()
    );

    // A second recording, stopped while another page is open.
    ui.ribbon_tab("Insert");
    ui.ribbon_button("Record Audio");
    assert_eq!(lengths(&ui.blocks()).len(), 2);
    let other =
        ui.h.get_by_role_and_label(egui::accesskit::Role::ListItem, "Other")
            .rect()
            .center();
    ui.click(other);
    assert_ne!(ui.app().page.as_ref().unwrap().id, lecture);
    assert!(ui.app().canvas.recording_time().is_some(), "it carries on");
    ui.steps(3);
    ui.ribbon_tab("Audio & Video");
    ui.ribbon_button("Stop");
    assert!(ui.app().canvas.recording_time().is_none(), "it stopped");
    let stored = ui.app().library.get_page(&lecture).unwrap().unwrap();
    let doc = stored.canvas.item(note).and_then(CanvasItem::doc).unwrap();
    assert!(
        lengths(&doc.blocks).iter().all(|length| *length > 0),
        "both clips have their length in the page as saved: {:?}",
        doc.blocks
    );

    // A clip saved without its length (as before) shows the length read from its file, as
    // a recording item does.
    let row =
        ui.h.get_by_role_and_label(egui::accesskit::Role::ListItem, "Lecture")
            .rect()
            .center();
    ui.click(row);
    assert_eq!(ui.app().page.as_ref().unwrap().id, lecture);
    let clip = {
        let app = ui.h.state_mut();
        let doc = app
            .page
            .as_mut()
            .and_then(|page| page.canvas.item_mut(note))
            .and_then(CanvasItem::doc_mut)
            .unwrap();
        let block = doc
            .blocks
            .iter_mut()
            .rev()
            .find(|block| matches!(block.kind, BlockKind::Audio { .. }))
            .unwrap();
        let BlockKind::Audio {
            attachment,
            duration_ms,
        } = &mut block.kind
        else {
            unreachable!()
        };
        *duration_ms = 0;
        attachment.clone()
    };
    ui.steps(2);
    assert!(
        ui.app()
            .editor
            .media
            .lengths
            .get(&clip)
            .is_some_and(|length| *length > 0),
        "{:?}",
        ui.app().editor.media.lengths
    );
}

/// The Insert tab holds the desktop notebook's commands: no Text Box, sticky note or divider any more,
/// and File Printout is ready on an open page.
#[test]
fn insert_matches_notebook_with_file_printout() {
    let mut ui = Ui::start();
    ui.new_page("Insert");
    ui.ribbon_tab("Insert");
    let labels: Vec<String> = ui
        .band_controls()
        .into_iter()
        .map(|(label, _)| label.to_lowercase())
        .collect();
    for gone in ["text box", "sticky", "divider"] {
        assert!(
            !labels.iter().any(|label| label.contains(gone)),
            "no {gone:?} on Insert: {labels:?}"
        );
    }
    assert!(
        !ui.ribbon_button_disabled("File Printout"),
        "File Printout inserts a PDF's pages on an open page"
    );
    for label in [
        "Insert Space",
        "Page Templates",
        "Symbol",
        "Time",
        "Date & Time",
    ] {
        ui.ribbon_button_rect(label);
    }
    ui.shot("p5-insert-tab");
}

/// Draw's gallery holds the desktop notebook's pens and highlighters: picking one takes up its tool,
/// colour and width, and what it draws keeps them.
#[test]
fn pens_draw_with_their_colour_and_width() {
    let mut ui = Ui::start();
    ui.new_page("Pens");
    ui.pick_pen(2);
    let red = Rgb(0xe7, 0x12, 0x24);
    assert_eq!(ui.app().canvas.tool, CanvasTool::Pen);
    assert_eq!(ui.app().canvas.ink_color, red);
    assert_eq!(ui.app().canvas.pen, Some(2), "the red pen is in hand");
    let from = ui.empty_paper_point();
    ui.drag(from, from + egui::vec2(120.0, 30.0));
    let ink = ui.ink();
    assert_eq!(ink.len(), 1, "{ink:?}");
    assert_eq!(
        (ink[0].color, ink[0].width, ink[0].highlighter),
        (red, 2.0, false)
    );
    ui.shot("p5-red-pen");

    // The yellow highlighter draws highlighter ink, in its colour and width.
    ui.pick_pen(4);
    assert_eq!(ui.app().canvas.tool, CanvasTool::Highlighter);
    let from = ui.empty_paper_point();
    ui.drag(from, from + egui::vec2(120.0, 0.0));
    let ink = ui.ink();
    assert_eq!(ink.len(), 2, "{ink:?}");
    assert_eq!(
        (ink[1].color, ink[1].width, ink[1].highlighter),
        (Rgb(0xff, 0xff, 0x00), 3.0, true)
    );
    ui.shot("p5-yellow-highlighter");
}

/// Add Pen puts a new pen or highlighter at the end of the gallery, in hand; Pen settings
/// retunes the pen in hand. The gallery is kept with the library, so both survive a
/// restart, and Remove Pen takes the pen in hand out of it.
#[test]
fn add_pen_and_pen_settings_survive_a_restart() {
    use crate::canvas::PenPreset;
    let mut ui = Ui::start();
    ui.new_page("Gallery");
    ui.ribbon_tab("Draw");
    assert_eq!(
        ui.app().canvas.pens.len(),
        6,
        "the desktop notebook's six to start"
    );
    ui.ribbon_button("Add Pen");
    ui.shot("p5-add-pen-menu");
    // A Pen grid, then a Highlighter grid of the same colours: the second purple is the
    // highlighter's.
    let purples: Vec<egui::Rect> =
        ui.h.get_all_by_role_and_label(egui::Role::Button, "#7719aa")
            .map(|node| node.rect())
            .collect();
    assert_eq!(purples.len(), 2, "{purples:?}");
    assert!(purples[1].top() > purples[0].bottom(), "{purples:?}");
    ui.click(purples[1].center());
    let purple = Rgb(0x77, 0x19, 0xaa);
    let canvas = &ui.app().canvas;
    assert_eq!(canvas.pens.len(), 7, "{:?}", canvas.pens);
    assert_eq!(
        canvas.pens[6],
        PenPreset {
            highlighter: true,
            color: purple,
            width: 3.0
        }
    );
    assert_eq!(canvas.pen, Some(6), "the new one is in hand");
    assert_eq!(canvas.tool, CanvasTool::Highlighter);

    // Pen settings ▾, the widest thickness. A highlighter draws four times as wide as a
    // pen, so it reads four times the millimetres: the pens' "1 mm" is "4 mm" here.
    ui.ribbon_button("Pen settings");
    ui.shot("p5-pen-settings");
    let widest = t!("unit-millimetres", size = "4");
    let dot =
        ui.h.get_by_role_and_label(egui::Role::Button, &widest)
            .rect();
    ui.click(dot.center());
    assert_eq!(ui.app().canvas.pens[6].width, 4.0);
    assert_eq!(
        ui.app().canvas.ink_width,
        4.0,
        "the pen in hand draws wider"
    );

    ui.reopen();
    let pens = &ui.app().canvas.pens;
    assert_eq!(pens.len(), 7, "{pens:?}");
    assert_eq!(
        pens[6],
        PenPreset {
            highlighter: true,
            color: purple,
            width: 4.0
        }
    );

    // Remove Pen, at the bottom of Pen settings, takes away the pen in hand.
    ui.pick_pen(6);
    ui.ribbon_button("Pen settings");
    ui.click_label(&menu_row_label(
        crate::icons::TRASH,
        &t!("ribbon-remove-pen"),
    ));
    assert_eq!(ui.app().canvas.pens.len(), 6);
    assert_eq!(ui.app().canvas.pens, PenPreset::defaults());
    assert_eq!(
        crate::canvas::load_pens(&ui.app().library),
        PenPreset::defaults(),
        "the library keeps the six"
    );
}

/// Lock Drawing Mode keeps the shape tool for shape after shape, as the desktop notebook's does;
/// without it the tool goes back to Type after one shape.
#[test]
fn lock_drawing_mode_keeps_the_shape_tool() {
    let mut ui = Ui::start();
    ui.new_page("Locked");
    ui.ribbon_tab("Draw");
    ui.ribbon_button("Rectangle");
    assert_eq!(ui.app().canvas.tool, CanvasTool::Shape(ShapeKind::Rect));
    ui.ribbon_button("Lock Drawing Mode");
    assert!(ui.app().canvas.lock_drawing);
    let rectangles = |ui: &Ui| {
        ui.items()
            .iter()
            .filter(|item| {
                matches!(
                    item.kind,
                    CanvasKind::Shape {
                        shape: ShapeKind::Rect,
                        ..
                    }
                )
            })
            .count()
    };
    for _ in 0..2 {
        let from = ui.empty_paper_point();
        ui.drag(from, from + egui::vec2(100.0, 60.0));
    }
    assert_eq!(rectangles(&ui), 2);
    assert_eq!(
        ui.app().canvas.tool,
        CanvasTool::Shape(ShapeKind::Rect),
        "the tool stays for the next shape"
    );
    ui.shot("p5-lock-drawing-mode");

    ui.ribbon_button("Lock Drawing Mode");
    assert!(!ui.app().canvas.lock_drawing);
    let from = ui.empty_paper_point();
    ui.drag(from, from + egui::vec2(100.0, 60.0));
    assert_eq!(rectangles(&ui), 3);
    assert_eq!(
        ui.app().canvas.tool,
        CanvasTool::Select,
        "one shape, then back to Type"
    );
}

/// Draw › Edit acts on the selection: Arrange ▾ › Bring Forward lifts it one place in the
/// stacking order, and Rotate ▾ turns or mirrors the selected ink and shapes.
#[test]
fn arrange_and_rotate_act_on_the_selection() {
    use crate::canvas::{PAGE_CONTENT_TOP, PAGE_LEFT};
    use crate::icons;
    let mut ui = Ui::start();
    ui.new_page("Arrange");
    ui.ribbon_tab("Draw");
    // Three rectangles, each overlapping the one before, so each is above the last.
    let mut rectangles = Vec::new();
    for step in 0..3 {
        ui.ribbon_button("Rectangle");
        let corner = egui::pos2(
            PAGE_LEFT + 40.0 * step as f32,
            PAGE_CONTENT_TOP + 60.0 + 30.0 * step as f32,
        );
        ui.drag(
            ui.screen(corner),
            ui.screen(corner + egui::vec2(120.0, 80.0)),
        );
        rectangles.push(
            ui.app()
                .canvas
                .selection
                .primary()
                .expect("the rectangle drawn is selected"),
        );
    }
    let stacking = |ui: &Ui| {
        let mut items = ui.items();
        items.sort_by_key(|item| item.z);
        items.into_iter().map(|item| item.id).collect::<Vec<_>>()
    };
    assert_eq!(stacking(&ui), rectangles);
    // The bottom one, by its corner clear of the other two.
    ui.click(ui.screen(egui::pos2(PAGE_LEFT + 15.0, PAGE_CONTENT_TOP + 75.0)));
    assert_eq!(ui.selection().items, vec![rectangles[0]]);
    ui.ribbon_button("Arrange");
    ui.shot("p5-arrange-menu");
    ui.click_label(&menu_row_label(
        icons::ARROW_UP,
        &t!("ribbon-bring-forward"),
    ));
    assert_eq!(
        stacking(&ui),
        [rectangles[1], rectangles[0], rectangles[2]],
        "one place up: the middle of the three"
    );

    // A horizontal stroke, lassoed and turned a quarter to the right, runs down the page.
    let extent = |ui: &Ui| {
        let points = &ui.ink()[0].points;
        let (xs, ys): (Vec<f32>, Vec<f32>) = points.iter().map(|p| (p[0], p[1])).unzip();
        let span = |values: &[f32]| {
            values.iter().copied().fold(f32::MIN, f32::max)
                - values.iter().copied().fold(f32::MAX, f32::min)
        };
        (span(&xs), span(&ys))
    };
    ui.pick_pen(0);
    let start = egui::pos2(PAGE_LEFT, PAGE_CONTENT_TOP + 360.0);
    ui.drag(ui.screen(start), ui.screen(start + egui::vec2(160.0, 0.0)));
    let (wide, high) = extent(&ui);
    assert!(wide > 100.0 && high < 10.0, "{wide} × {high}");
    ui.ribbon_button("Lasso");
    let centre = ui.screen(start + egui::vec2(80.0, 0.0));
    let path: Vec<Pos2> = (0..=24)
        .map(|i| {
            let angle = i as f32 / 24.0 * std::f32::consts::TAU;
            centre + egui::vec2(angle.cos() * 120.0, angle.sin() * 40.0)
        })
        .collect();
    ui.drag_path(&path);
    assert_eq!(ui.selection().strokes, vec![0]);
    assert!(ui.selection().items.is_empty(), "the stroke alone");
    ui.ribbon_button("Rotate");
    ui.shot("p5-rotate-menu");
    ui.click_label(&menu_row_label(
        icons::ARROW_ROTATE_CLOCKWISE,
        &t!("ribbon-rotate-right"),
    ));
    let (wide, high) = extent(&ui);
    assert!(high > 100.0 && wide < 10.0, "turned: {wide} × {high}");

    // A triangle from the Shapes gallery, flipped upside down.
    ui.ribbon_button("Triangle");
    let corner = egui::pos2(PAGE_LEFT + 320.0, PAGE_CONTENT_TOP + 60.0);
    ui.drag(
        ui.screen(corner),
        ui.screen(corner + egui::vec2(100.0, 80.0)),
    );
    let triangle = ui
        .app()
        .canvas
        .selection
        .primary()
        .expect("the triangle drawn is selected");
    assert!(
        matches!(
            ui.item(triangle).kind,
            CanvasKind::Shape {
                shape: ShapeKind::Triangle,
                flip_y: false,
                ..
            }
        ),
        "{:?}",
        ui.item(triangle)
    );
    ui.ribbon_button("Rotate");
    ui.click_label(&menu_row_label(
        icons::FLIP_VERTICAL,
        &t!("ribbon-flip-vertical"),
    ));
    assert!(
        matches!(
            ui.item(triangle).kind,
            CanvasKind::Shape { flip_y: true, .. }
        ),
        "{:?}",
        ui.item(triangle)
    );
    ui.shot("p5-flipped-triangle");
}

/// P5 fills Home, Insert and Draw, and they still fit in both languages: at the app's
/// narrowest, default and a wide window nothing leaves the card or overlaps; at the
/// default width Paste, Bold, Insert Space and Add Pen can be reached, directly or through
/// their group's button; and a wide window shows all of English Home unfolded.
#[test]
fn the_ribbon_fits_with_p5_in_both_languages() {
    for lang in [Lang::En, Lang::Fr] {
        let mut ui = Ui::start_in(lang);
        ui.new_page("P5");
        ui.press(Key::Escape);
        let fr = lang == Lang::Fr;
        let paste = if fr { "Coller" } else { "Paste" };
        let bold = if fr { "Gras" } else { "Bold" };
        let space = if fr {
            "Insérer un espace"
        } else {
            "Insert Space"
        };
        let add_pen = if fr { "Ajouter un stylet" } else { "Add Pen" };
        let tabs = [
            (RibbonTab::Home, vec![paste, bold]),
            (RibbonTab::Insert, vec![space]),
            (RibbonTab::Draw, vec![space, add_pen]),
        ];
        for width in [980.0, 1320.0, 1620.0] {
            ui.h.set_size(egui::vec2(width, 900.0));
            ui.steps(3);
            for (tab, wanted) in &tabs {
                let tab = *tab;
                ui.ribbon_tab(&tab.label());
                assert_eq!(ui.app().ribbon.tab, tab);
                let controls = band_fits(&ui, lang, width, tab);
                let present = |name: &str| controls.iter().any(|(label, _)| label == name);
                if width == 1320.0 {
                    for name in wanted {
                        let group =
                            ui.ribbon_groups(tab)
                                .into_iter()
                                .find(|group| {
                                    group.controls.iter().any(|control| {
                                        control_names(control).iter().any(|n| n == name)
                                    })
                                })
                                .unwrap_or_else(|| panic!("{lang:?} {tab:?} has no {name:?}"));
                        assert!(
                            present(name) || present(&group.label),
                            "{lang:?} 1320 {tab:?}: {name:?} ({}) cannot be reached: {controls:?}",
                            group.label
                        );
                    }
                }
                if !fr && width == 1620.0 && tab == RibbonTab::Home {
                    for group in ui.ribbon_groups(tab) {
                        assert!(
                            !controls
                                .iter()
                                .any(|(label, rect)| *label == group.label && rect.height() > 40.0),
                            "{} is folded at 1620: {controls:?}",
                            group.label
                        );
                    }
                }
                ui.shot(&format!("p5-ribbon-{}-{width}-{tab:?}", lang.code()));
            }
        }
    }
}

/// Every symbol of More Symbols…'s sets has a glyph in the note font (Carlito with its
/// fallbacks), so none is drawn as a box.
#[test]
fn every_symbol_has_a_glyph() {
    let ui = Ui::start();
    let font = crate::theme::body(17.0);
    let mut missing = Vec::new();
    ui.h.ctx.fonts_mut(|fonts| {
        for (set, symbols) in crate::editor::SYMBOL_SETS {
            for (symbol, name) in symbols {
                if !fonts.has_glyph(&font, *symbol) {
                    missing.push(format!(
                        "{set}: {symbol:?} {name} (U+{:04X})",
                        *symbol as u32
                    ));
                }
            }
        }
    });
    assert!(missing.is_empty(), "no glyph for {missing:#?}");
}

/// What P5 draws as text has its glyphs in the app's fonts, so nothing shows as a box: the
/// list markers and Insert › Symbol's grid in the note font (with its fallbacks), and P5's
/// icons in the Fluent subsets, Regular and Filled.
#[test]
fn p5_markers_symbols_and_icons_have_glyphs() {
    use crate::icons;
    use crate::model::ListMarker;
    let ui = Ui::start();
    let mut missing = Vec::new();
    // Whether `font`'s family, with its fallbacks, draws all of `text`; if not, a line
    // naming `what` and each glyph it lacks.
    let mut check = |font: &egui::FontId, text: &str, what: &str| {
        ui.h.ctx.fonts_mut(|fonts| {
            if !fonts.has_glyphs(font, text) {
                let lacking: Vec<String> = text
                    .chars()
                    .filter(|ch| !fonts.has_glyph(font, *ch))
                    .map(|ch| format!("{ch:?} (U+{:04X})", ch as u32))
                    .collect();
                missing.push(format!(
                    "{what} {text:?} in {:?}: {}",
                    font.family,
                    lacking.join(", ")
                ));
            }
        });
    };
    // The note font P4 writes Normal text in.
    let body_font = crate::theme::body(crate::editor::BODY_SIZE);
    for marker in ListMarker::BULLETS.into_iter().chain(ListMarker::NUMBERS) {
        for number in 1..=12 {
            check(
                &body_font,
                &marker.text(number),
                &format!("list marker {marker:?}"),
            );
        }
    }
    for symbol in crate::editor::SYMBOLS {
        check(&body_font, &symbol.to_string(), "symbol");
    }
    let regular = [
        ("CLIPBOARD_TEXT", icons::CLIPBOARD_TEXT),
        ("SCISSORS", icons::SCISSORS),
        ("PAINT_BRUSH", icons::PAINT_BRUSH),
        ("TEXT_INDENT_DECREASE_LTR", icons::TEXT_INDENT_DECREASE_LTR),
        ("TEXT_INDENT_INCREASE_LTR", icons::TEXT_INDENT_INCREASE_LTR),
        ("TEXT_HEADER_3", icons::TEXT_HEADER_3),
        ("TEXT_HEADER_4", icons::TEXT_HEADER_4),
        ("TEXT_HEADER_5", icons::TEXT_HEADER_5),
        ("TEXT_HEADER_6", icons::TEXT_HEADER_6),
        ("BOOK_OPEN", icons::BOOK_OPEN),
        ("ARROW_AUTOFIT_HEIGHT", icons::ARROW_AUTOFIT_HEIGHT),
        ("FILE_PDF", icons::FILE_PDF),
        ("CALENDAR_CLOCK", icons::CALENDAR_CLOCK),
        (
            "DOCUMENT_ONE_PAGE_MULTIPLE",
            icons::DOCUMENT_ONE_PAGE_MULTIPLE,
        ),
        ("SYMBOLS", icons::SYMBOLS),
        ("PENCIL_SIMPLE", icons::PENCIL_SIMPLE),
        ("HIGHLIGHTER", icons::HIGHLIGHTER),
        ("LINE", icons::LINE),
        ("RECTANGLE_LANDSCAPE", icons::RECTANGLE_LANDSCAPE),
        ("SQUARE", icons::SQUARE),
        ("OVAL", icons::OVAL),
        ("TRIANGLE", icons::TRIANGLE),
        ("DIAMOND", icons::DIAMOND),
        ("ARROW_RIGHT", icons::ARROW_RIGHT),
        ("LOCK_CLOSED", icons::LOCK_CLOSED),
        ("ARROW_ROTATE_CLOCKWISE", icons::ARROW_ROTATE_CLOCKWISE),
        (
            "ARROW_ROTATE_COUNTERCLOCKWISE",
            icons::ARROW_ROTATE_COUNTERCLOCKWISE,
        ),
        ("FLIP_HORIZONTAL", icons::FLIP_HORIZONTAL),
        ("FLIP_VERTICAL", icons::FLIP_VERTICAL),
    ];
    let icon_font = crate::theme::icon(18.0);
    for (name, glyph) in regular {
        check(&icon_font, glyph, &format!("icons::{name}"));
    }
    let filled_font = crate::theme::icon_filled(18.0);
    for (name, glyph) in [
        ("PEN_FILLED", icons::PEN_FILLED),
        ("HIGHLIGHT_FILLED", icons::HIGHLIGHT_FILLED),
    ] {
        check(&filled_font, glyph, &format!("icons::{name}"));
    }
    assert!(missing.is_empty(), "no glyph for:\n{}", missing.join("\n"));
}

/// Clicking the ribbon while typing leaves the caret in the note, as in the desktop notebook: switching
/// tabs, or a click on an empty part of the band, never throws the next keystrokes away.
#[test]
fn clicking_the_ribbon_keeps_typing_in_the_note() {
    let mut ui = Ui::start();
    ui.new_page("Tabs");
    ui.press(Key::Enter);
    ui.type_text("Hello");
    let note = ui.items()[0].id;
    let text = |ui: &Ui| ui.item(note).doc().unwrap().blocks[0].text.text.clone();
    ui.ribbon_tab("Insert");
    ui.type_text(" world");
    assert_eq!(text(&ui), "Hello world");
    ui.ribbon_tab("Home");
    ui.type_text("!");
    assert_eq!(text(&ui), "Hello world!");
    // Right of Home's groups, clear of every control: halfway from the last group to the
    // display-options chevron in the card's corner.
    let card = ui.ribbon_card();
    let options = ui.ribbon_button_rect("Ribbon display options");
    let last = ui
        .band_controls()
        .into_iter()
        .filter(|(label, _)| label != "Ribbon display options")
        .map(|(_, rect)| rect.right())
        .fold(0.0, f32::max);
    assert!(last + 20.0 < options.left(), "room right of Home's groups");
    let spot = egui::pos2((last + options.left()) / 2.0, card.center().y);
    assert!(
        !ui.band_controls()
            .iter()
            .any(|(_, rect)| rect.expand(2.0).contains(spot)),
        "{spot:?} is empty band"
    );
    ui.click(spot);
    ui.type_text("?");
    assert_eq!(text(&ui), "Hello world!?");
    assert_eq!(ui.app().canvas.active_text, Some(note));
    assert!(ui.h.ctx.text_edit_focused(), "the note has the caret");
    // View's page commands leave the text alone, and so its caret: a zoom, Hide Page
    // Title, and a pick in the Page Color and Rule Lines dropdowns.
    {
        use crate::model::{PageColor, RuleLines, RuleSize};
        ui.ribbon_tab("View");
        ui.ribbon_button("Zoom In");
        ui.type_text(" 1");
        ui.ribbon_button("Hide Page Title");
        ui.type_text(" 2");
        ui.ribbon_button("Page Color");
        ui.click_label(&PageColor::Lavender.label());
        ui.type_text(" 3");
        ui.ribbon_button("Rule Lines");
        ui.click_label(&RuleLines::Lines(RuleSize::College).label());
        ui.type_text(" 4");
        assert_eq!(text(&ui), "Hello world!? 1 2 3 4");
        // Tabs Layout ▾ moves the panels round the page, not the caret.
        ui.ribbon_button("Tabs Layout");
        ui.click_label(&t!("ribbon-horizontal-tabs"));
        assert_eq!(ui.app().tabs_layout, crate::ribbon::TabsLayout::Horizontal);
        ui.type_text(" h");
        ui.ribbon_button("Tabs Layout");
        ui.click_label(&t!("ribbon-vertical-tabs"));
        assert_eq!(ui.app().tabs_layout, crate::ribbon::TabsLayout::Vertical);
        ui.type_text("v");
        assert_eq!(text(&ui), "Hello world!? 1 2 3 4 hv");
        let page = ui.app().page.as_ref().unwrap();
        assert_eq!(
            (page.color, page.rule, page.title_hidden),
            (
                Some(PageColor::Lavender),
                RuleLines::Lines(RuleSize::College),
                true
            )
        );
        assert!(ui.app().canvas.zoom() > 1.05);
        ui.ribbon_button("Hide Page Title");
        ui.ribbon_tab("Home");
        assert!(!ui.app().page.as_ref().unwrap().title_hidden);
    }
    // So do Draw's Lock Drawing Mode and Arrange ▾ › Bring Forward (the note typed in is
    // what it would lift).
    ui.ribbon_tab("Draw");
    ui.ribbon_button("Lock Drawing Mode");
    assert!(ui.app().canvas.lock_drawing);
    ui.type_text(" 5");
    ui.ribbon_button("Arrange");
    ui.click_label(&menu_row_label(
        crate::icons::ARROW_UP,
        &t!("ribbon-bring-forward"),
    ));
    ui.type_text(" 6");
    ui.ribbon_button("Lock Drawing Mode");
    assert!(!ui.app().canvas.lock_drawing);
    ui.type_text(" 7");
    assert_eq!(text(&ui), "Hello world!? 1 2 3 4 hv 5 6 7");
    ui.ribbon_tab("Home");
    // With Show tabs only, a tab opens its band over the page, and typing goes on too.
    ui.press_with(Modifiers::COMMAND, Key::F1);
    ui.ribbon_tab("Draw");
    assert!(ui.app().ribbon.overlay);
    ui.type_text(" Yes");
    assert_eq!(text(&ui), "Hello world!? 1 2 3 4 hv 5 6 7 Yes");
}

/// The same in a note's second line, the caret at the end of the first; from the page
/// title, a tab click never moves the caret into the note.
#[test]
fn clicking_a_ribbon_tab_keeps_typing_in_a_note_and_never_from_the_title() {
    let mut ui = Ui::start();
    ui.new_page("Lines");
    ui.press(Key::Enter);
    ui.type_text("alpha");
    ui.press(Key::Enter);
    ui.type_text("beta");
    let note = ui.items()[0].id;
    // The caret at the end of the first line, so a tab click must put it back there and
    // not in the line typed last.
    let first = ui.item(note).doc().unwrap().blocks[0].id;
    ui.h.state_mut().editor.focus_block(first, "alpha".len());
    ui.steps(3);
    let texts = |ui: &Ui| {
        ui.item(note)
            .doc()
            .unwrap()
            .blocks
            .iter()
            .map(|block| block.text.text.clone())
            .collect::<Vec<_>>()
    };
    ui.type_text(" one");
    ui.ribbon_tab("View");
    ui.type_text(" two");
    ui.ribbon_tab("Home");
    ui.type_text(" three");
    assert_eq!(texts(&ui), ["alpha one two three", "beta"]);
    ui.h.state_mut().canvas.focus_title = true;
    ui.steps(3);
    ui.ribbon_tab("Insert");
    ui.type_text("x");
    assert_eq!(texts(&ui), ["alpha one two three", "beta"]);
}

/// The band shown fits the window: it has controls, each one inside the card (8 px in from
/// the window's sides), and no two overlapping by more than 1 px². Returns them, by
/// accessible name.
fn band_fits(ui: &Ui, lang: Lang, width: f32, tab: RibbonTab) -> Vec<(String, egui::Rect)> {
    let controls = ui.band_controls();
    assert!(controls.len() > 1, "{lang:?} {width} {tab:?}: {controls:?}");
    for (label, rect) in &controls {
        assert!(
            rect.left() >= 8.0 && rect.right() <= width - 8.0,
            "{lang:?} {width} {tab:?}: {label:?} {rect:?} leaves the card"
        );
    }
    for (index, (a, first)) in controls.iter().enumerate() {
        for (b, second) in &controls[index + 1..] {
            let across =
                (first.right().min(second.right()) - first.left().max(second.left())).max(0.0);
            let down =
                (first.bottom().min(second.bottom()) - first.top().max(second.top())).max(0.0);
            assert!(
                across * down <= 1.0,
                "{lang:?} {width} {tab:?}: {a:?} {first:?} overlaps {b:?} {second:?}"
            );
        }
    }
    controls
}

/// Every band fits every window the app allows, from its 980 px minimum up: controls stay
/// inside the card and never overlap, and what does not fit folds into its group's button
/// rather than running off the window.
#[test]
fn the_ribbon_fits_at_every_window_width() {
    for lang in [Lang::En, Lang::Fr] {
        the_ribbon_fits_at_every_width_in(lang);
    }
}

fn the_ribbon_fits_at_every_width_in(lang: Lang) {
    let mut ui = Ui::start_in(lang);
    ui.new_page("Widths");
    ui.press(Key::Escape);
    // A table holding the caret, so the contextual Table tab is there too.
    ui.insert_table(3, 3);
    ui.ribbon_tab_rect(&t!("ribbon-tab-table"));
    let tabs = [
        RibbonTab::Home,
        RibbonTab::Insert,
        RibbonTab::Draw,
        RibbonTab::History,
        RibbonTab::Review,
        RibbonTab::View,
        RibbonTab::Help,
        RibbonTab::Table,
    ];
    // 980 to 1620 px in 40 px steps, and the default 1320.
    let widths = (0..=16)
        .map(|step| 980.0 + 40.0 * step as f32)
        .chain([1320.0]);
    for width in widths {
        ui.h.set_size(egui::vec2(width, 900.0));
        ui.steps(3);
        for tab in tabs {
            ui.ribbon_tab(&tab.label());
            assert_eq!(ui.app().ribbon.tab, tab);
            let tab_rect = ui.ribbon_tab_rect(&tab.label());
            assert!(
                tab_rect.right() <= width - 8.0,
                "{lang:?} {width}: the {tab:?} tab {tab_rect:?}"
            );
            let controls = band_fits(&ui, lang, width, tab);
            let present = |name: &str| controls.iter().any(|(label, _)| label == name);
            if tab == RibbonTab::Home {
                // Shown whole at 980 px, the Styles gallery stays whole in every wider
                // window: a wider band never shows less.
                assert!(
                    present(&t!("block-heading-1")),
                    "{lang:?} {width}: the Styles gallery is not whole: {controls:?}"
                );
            }
            if lang == Lang::En && width == 1320.0 && tab == RibbonTab::Home {
                // Room for all of Home: Bold and Find Tags show as themselves, and no group
                // is folded to its one Large button.
                ui.ribbon_button_rect("Bold");
                ui.ribbon_button_rect("Find Tags");
                for group in ui.ribbon_groups(tab) {
                    assert!(
                        !controls
                            .iter()
                            .any(|(label, rect)| *label == group.label && rect.height() > 40.0),
                        "{} is collapsed at 1320: {controls:?}",
                        group.label
                    );
                }
                ui.shot("p2-ribbon-en-1320-home");
            }
            if width == 1320.0 && tab == RibbonTab::Home {
                // Tags keep their names at 1320 in both languages: the French gallery drops
                // to one column (To Do, Important, Question) rather than to three icons.
                assert!(
                    present(&t!("tag-question")) && present(&t!("ribbon-find-tags")),
                    "{lang:?}: Tags lost their names at 1320: {controls:?}"
                );
            }
            if lang == Lang::Fr && width == 980.0 && tab == RibbonTab::Home {
                // The narrowest French Home shrinks, but loses nothing: each command shows,
                // or its group's button does. Tags, a gallery of named items, fold into
                // their one button rather than three bare icons, as the desktop notebook folds them;
                // Styles' gallery shows whole, and no other group folds.
                ui.ribbon_button_rect(&t!("block-heading-1"));
                for group in ui.ribbon_groups(tab) {
                    let folded = controls
                        .iter()
                        .any(|(label, rect)| *label == group.label && rect.height() > 40.0);
                    assert_eq!(
                        folded,
                        group.id == "tags",
                        "{} folded={folded} at 980: {controls:?}",
                        group.label
                    );
                }
                assert!(
                    !present(&t!("tag-question")),
                    "no bare Tags icons at 980: {controls:?}"
                );
                for group in ui.ribbon_groups(tab) {
                    for control in &group.controls {
                        let names = control_names(control);
                        assert!(
                            names.iter().any(|name| present(name)) || present(&group.label),
                            "{names:?} ({}) cannot be reached at 980: {controls:?}",
                            group.label
                        );
                    }
                }
                ui.shot("p2-ribbon-fr-980-home");
            }
            if lang == Lang::Fr && tab == RibbonTab::View && (width == 980.0 || width == 1320.0) {
                // View with Page Setup (P3): at 1320 every French command shows at its
                // designed size, labels and all (Couleur de page, Réglure, Masquer le titre
                // de la page as Large buttons); at the narrowest window each still shows,
                // or its group's button does.
                use crate::ribbon::Size;
                for group in ui.ribbon_groups(tab) {
                    for control in &group.controls {
                        let names = control_names(control);
                        let found = controls.iter().find(|(label, _)| names.contains(label));
                        if width == 1320.0 {
                            let (_, rect) = found.unwrap_or_else(|| {
                                panic!(
                                    "{names:?} ({}) is not in the band: {controls:?}",
                                    group.label
                                )
                            });
                            match control.size {
                                Size::Large => assert!(rect.height() > 40.0, "{names:?} {rect:?}"),
                                Size::Small => assert!(rect.width() > 40.0, "{names:?} {rect:?}"),
                                Size::Icon => {}
                            }
                        } else {
                            assert!(
                                found.is_some() || present(&group.label),
                                "{names:?} ({}) cannot be reached at 980: {controls:?}",
                                group.label
                            );
                        }
                    }
                }
                ui.shot(&format!("p3-ribbon-fr-{width}-view"));
            }
        }
    }
}

/// A window narrower than the app allows (980 px): the only width at which a Home group
/// still folds into one button in P2, now that the groups fit at the minimum.
const FOLDED_WIDTH: f32 = 480.0;

/// A group too wide for the window folds into one button, whose menu holds the group's
/// commands, which run from there as from the band. A group only folds when its button is
/// narrower than its icons, and P2's bands fit the app's narrowest window (980 px) before
/// that: the test narrows the window past it, in French, where Home's Basic Text folds.
#[test]
fn collapsed_groups_open_their_commands() {
    use crate::editor::{Mark, ToolbarCommand};
    use crate::ribbon::Cmd;
    let mut ui = Ui::start_in(Lang::Fr);
    ui.new_page("Chapitres");
    ui.press(Key::Enter);
    ui.type_text("Introduction ");
    let note = ui.items()[0].id;
    ui.h.set_size(egui::vec2(FOLDED_WIDTH, 900.0));
    ui.steps(3);
    let basic = ui
        .ribbon_groups(RibbonTab::Home)
        .into_iter()
        .find(|group| group.id == "basic-text")
        .expect("Home has Basic Text");
    let bold = basic
        .controls
        .iter()
        .find(|control| control.cmd == Some(Cmd::Text(ToolbarCommand::Toggle(Mark::Bold))))
        .expect("Basic Text has Bold");
    // Folded: one Large button named after the group, and none of its commands in the band.
    let button = ui.ribbon_button_rect(&basic.label);
    assert!(button.height() > 40.0, "{button:?}");
    let controls = ui.band_controls();
    for control in &basic.controls {
        assert!(
            !controls.iter().any(|(label, _)| *label == control.label),
            "{} shows in the band: {controls:?}",
            control.label
        );
    }
    ui.shot("p2-fr-folded-basic-text");
    ui.click(button.center());
    // Its menu lists the group's commands.
    let row = menu_row_label(bold.glyph, &bold.label);
    let found = ui.h.get_by_label(&row).rect();
    assert!(found.top() > button.bottom(), "the menu's row: {found:?}");
    ui.shot("p2-fr-folded-basic-text-menu");
    // Picking one runs it on the note being typed in, whose caret stays: what is typed
    // next is bold.
    ui.click(found.center());
    assert!(
        ui.h.query_all_by_label(&row).next().is_none(),
        "the menu closed"
    );
    ui.type_text("gras");
    let block = ui.item(note).doc().unwrap().blocks[0].clone();
    assert_eq!(block.text.text, "Introduction gras");
    assert!(
        block.text.style_at(14).bold && !block.text.style_at(2).bold,
        "{block:#?}"
    );
}

/// The Simplified ribbon is one row of icons; the groups that do not fit go, whole, into
/// "More commands". The layout is remembered.
#[test]
fn the_simplified_ribbon_is_one_row_and_overflows_into_more() {
    use crate::ribbon::RibbonLayout;
    let mut ui = Ui::start();
    ui.new_page("Simple");
    ui.press(Key::Escape);
    ui.ribbon_button("Ribbon display options");
    ui.shot("p2-ribbon-display-options");
    ui.click_label("Simplified Ribbon");
    assert_eq!(ui.app().ribbon.layout, RibbonLayout::Simplified);
    assert_eq!(
        crate::ribbon::panel_height(&ui.app().ribbon),
        30.0 + 44.0 + 8.0
    );
    ui.h.set_size(egui::vec2(980.0, 900.0));
    ui.steps(3);
    ui.ribbon_tab("Draw");
    ui.shot("p2-simplified-980-draw");
    // One row: every control is centred on the same line (the display-options chevron
    // keeps the card's corner).
    let row: Vec<(String, egui::Rect)> = ui
        .band_controls()
        .into_iter()
        .filter(|(label, _)| label != "Ribbon display options")
        .collect();
    let middle = row[0].1.center().y;
    for (label, rect) in &row {
        assert!(
            (rect.center().y - middle).abs() < 1.0,
            "{label:?} {rect:?} is off the row at {middle}"
        );
    }
    // Draw fits the row whole at 980 px, the narrowest window (P2's bands are short); in
    // a narrower one its last groups go into "More commands", and its menu holds every
    // control missing from the row.
    for control in ui
        .ribbon_groups(RibbonTab::Draw)
        .iter()
        .flat_map(|group| &group.controls)
    {
        let name = &control_names(control)[0];
        assert!(
            row.iter().any(|(label, _)| label == name),
            "{name:?} is in the row: {row:?}"
        );
    }
    assert!(
        !row.iter().any(|(label, _)| label == "More commands"),
        "{row:?}"
    );
    ui.h.set_size(egui::vec2(460.0, 900.0));
    ui.steps(3);
    let shown = ui.band_controls();
    let present = |name: &str| shown.iter().any(|(label, _)| label == name);
    let missing: Vec<crate::ribbon::Control> = ui
        .ribbon_groups(RibbonTab::Draw)
        .into_iter()
        .flat_map(|group| group.controls)
        .filter(|control| !present(&control_names(control)[0]))
        .collect();
    assert!(!missing.is_empty(), "{shown:?}");
    ui.ribbon_button("More commands");
    ui.shot("p2-simplified-more-commands");
    for control in &missing {
        let label = menu_row_label(control.glyph, &control_names(control)[0]);
        assert!(
            ui.h.query_all_by_label(&label).next().is_some(),
            "{label:?} is in More commands"
        );
    }
    ui.press(Key::Escape);
    ui.h.set_size(egui::vec2(1320.0, 900.0));
    ui.steps(3);

    ui.reopen();
    assert_eq!(ui.app().ribbon.layout, RibbonLayout::Simplified);
    assert_eq!(
        crate::ribbon::panel_height(&ui.app().ribbon),
        30.0 + 44.0 + 8.0
    );
    ui.ribbon_button("Ribbon display options");
    ui.click_label("Classic Ribbon");
    assert_eq!(ui.app().ribbon.layout, RibbonLayout::Classic);
    assert_eq!(
        crate::ribbon::panel_height(&ui.app().ribbon),
        30.0 + 96.0 + 8.0
    );
    ui.ribbon_button_rect("Bold");
}

/// The Simplified ribbon keeps "Styles" by name, as the desktop notebook's does: a dropdown wide
/// enough for its glyph, its label and its chevron, where Tags is a bare icon. While a
/// note is typed in, it opens the Styles menu.
#[test]
fn the_simplified_ribbon_shows_styles_by_name() {
    let mut ui = Ui::start();
    ui.new_page("Simple");
    ui.press(Key::Escape);
    ui.ribbon_button(&t!("ribbon-display-options"));
    ui.click_label(&t!("ribbon-simplified"));
    ui.steps(2);
    let control = |ui: &Ui, name: &str| {
        ui.band_controls()
            .into_iter()
            .find(|(label, _)| label == name)
            .unwrap_or_else(|| panic!("no {name:?} in the row: {:?}", ui.band_controls()))
            .1
    };
    let label = t!("ribbon-group-styles");
    let styles = control(&ui, &label);
    let words = crate::widgets::text_width(&ui.h.ctx, &label, egui::FontId::proportional(12.0));
    assert!(styles.width() > words + 24.0, "{styles:?} for {words}");
    let tags = control(&ui, &t!("ribbon-group-tags"));
    assert!(
        styles.width() > tags.width() + words,
        "{styles:?} vs {tags:?}"
    );
    // Typing in a note, it opens the Styles menu.
    let at = ui.empty_paper_point();
    ui.click(at);
    ui.type_text("Words");
    ui.click(styles.center());
    assert!(
        ui.h.query_by_label(&crate::model::BlockKind::heading(1).label())
            .is_some(),
        "the Styles menu"
    );
}

/// Ctrl+F1 shows the tabs only, as in the desktop notebook: clicking a tab opens its band over the
/// page until a command runs, and Ctrl+F1 brings the band back. The choice is remembered.
#[test]
fn ctrl_f1_shows_tabs_only_and_a_tab_opens_the_band() {
    let mut ui = Ui::start();
    ui.new_page("Tabs only");
    ui.press(Key::Enter);
    ui.type_text("Plain ");
    let note = ui.items()[0].id;
    let bold = |ui: &Ui| {
        ui.h.query_all_by_role_and_label(egui::Role::Button, "Bold")
            .next()
            .map(|node| node.rect())
    };
    ui.press_with(Modifiers::COMMAND, Key::F1);
    assert!(ui.app().ribbon.collapsed);
    assert_eq!(bold(&ui), None, "the band is hidden");
    ui.ribbon_tab_rect("Home");
    ui.ribbon_tab_rect("Insert");
    assert_eq!(
        crate::ribbon::panel_height(&ui.app().ribbon),
        crate::ribbon::TAB_ROW
    );
    ui.shot("p2-tabs-only");

    // Home opens its band over the page, below the tabs.
    ui.ribbon_tab("Home");
    assert!(ui.app().ribbon.overlay);
    let button = bold(&ui).expect("Bold is in the band over the page");
    assert!(button.top() >= 48.0 + crate::ribbon::TAB_ROW, "{button:?}");
    ui.shot("p2-tabs-only-open");
    ui.click(button.center());
    assert!(!ui.app().ribbon.overlay, "a command closes the band");
    assert_eq!(bold(&ui), None);
    ui.type_text("bold");
    let block = ui.item(note).doc().unwrap().blocks[0].clone();
    assert_eq!(block.text.text, "Plain bold");
    assert!(block.text.style_at(7).bold, "{block:#?}");
    assert!(!block.text.style_at(2).bold, "{block:#?}");

    ui.press_with(Modifiers::COMMAND, Key::F1);
    assert!(!ui.app().ribbon.collapsed);
    ui.ribbon_button_rect("Bold");
    assert_eq!(
        crate::ribbon::panel_height(&ui.app().ribbon),
        30.0 + 96.0 + 8.0
    );

    // Remembered either way.
    ui.press_with(Modifiers::COMMAND, Key::F1);
    ui.reopen();
    assert!(ui.app().ribbon.collapsed);
    assert_eq!(bold(&ui), None);
    ui.press_with(Modifiers::COMMAND, Key::F1);
    ui.reopen();
    assert!(!ui.app().ribbon.collapsed);
    ui.ribbon_button_rect("Bold");
}

/// The Table tab comes with the caret in a table, as the desktop notebook's contextual tabs do, runs
/// its commands on that table with the caret kept in it, and goes when the caret leaves.
#[test]
fn the_table_tab_comes_and_goes_with_the_caret_in_a_table() {
    let mut ui = Ui::start();
    ui.new_page("Table");
    ui.press(Key::Enter);
    let tab = |ui: &Ui| {
        ui.h.query_all_by_role_and_label(egui::Role::Tab, "Table")
            .next()
            .is_some()
    };
    assert!(!tab(&ui), "no table, no Table tab");
    ui.insert_table(3, 3);
    assert!(tab(&ui), "the caret is in the new table");
    // It does not take the selection from Insert.
    assert_eq!(ui.app().ribbon.tab, RibbonTab::Insert);
    let rows = |ui: &Ui| {
        ui.items()
            .iter()
            .filter_map(CanvasItem::doc)
            .flat_map(|doc| &doc.blocks)
            .find_map(|block| match &block.kind {
                crate::model::BlockKind::Table(table) => Some(table.rows.len()),
                _ => None,
            })
            .expect("a table in a note")
    };
    let before = rows(&ui);
    ui.ribbon_tab("Table");
    assert_eq!(ui.app().ribbon.tab, RibbonTab::Table);
    ui.shot("p2-table-tab");
    ui.ribbon_button("Insert Below");
    assert_eq!(rows(&ui), before + 1);
    assert_eq!(
        ui.app().editor.active_cell,
        Some((1, 0)),
        "the caret goes to the new row"
    );

    let spot = ui.empty_paper_point();
    ui.click(spot);
    assert!(!tab(&ui), "the caret left the table");
    assert_eq!(ui.app().ribbon.tab, RibbonTab::Home);
    ui.ribbon_button_rect("Bold");
    ui.shot("p2-table-tab-gone");
}

// The shell: the navigation pane, the page list card and the page card, as the desktop notebook lays
// them out.

/// The rect of the open right-click menu's row named `name`: its glyph, four spaces, then
/// the name (`widgets::menu_item`; a `widgets::menu_row` adds its shortcut, a space when
/// it has none), so a pane row with the same name never matches.
fn menu_item_rect(ui: &Ui, name: &str) -> Option<egui::Rect> {
    use egui_kittest::kittest::NodeT;
    let suffix = format!("    {name}");
    ui.h.query_all_by_label_contains(name)
        .find(|node| {
            node.accesskit_node()
                .label()
                .is_some_and(|label| label.trim_end().ends_with(&suffix))
        })
        .map(|node| node.rect())
}

/// `menu_item_rect` for a row that shows a shortcut `hint` after its name (Cut's
/// "Ctrl+X", Delete's "Del"): a `widgets::menu_row` names the row by both.
fn menu_row_rect(ui: &Ui, name: &str, hint: &str) -> Option<egui::Rect> {
    menu_item_rect(ui, format!("{name} {hint}").trim_end())
}

/// A shortcut with the command key as menu rows show it ("Ctrl+X").
fn ctrl_hint(key: Key) -> String {
    crate::util::menu_shortcut(Modifiers::COMMAND, key)
}

/// A shortcut with the command key and Alt as menu rows show it ("Ctrl+Alt+M").
fn ctrl_alt_hint(key: Key) -> String {
    crate::util::menu_shortcut(Modifiers::COMMAND | Modifiers::ALT, key)
}

/// Clicks the open menu's row named `name` (with its shortcut `hint`, if it shows one).
fn click_menu_row(ui: &mut Ui, name: &str, hint: &str) {
    let row =
        menu_row_rect(ui, name, hint).unwrap_or_else(|| panic!("no menu row {name:?} {hint:?}"));
    ui.click(row.center());
}

/// The page list's rows top to bottom, by title.
fn rows_in_order(ui: &Ui) -> Vec<String> {
    use egui_kittest::kittest::NodeT;
    let mut rows: Vec<(String, f32)> =
        ui.h.query_all_by_role(egui::Role::ListItem)
            .map(|node| {
                let label = node.accesskit_node().label().unwrap_or_default();
                (label, node.rect().top())
            })
            .collect();
    rows.sort_by(|a, b| a.1.total_cmp(&b.1));
    rows.into_iter().map(|(title, _)| title).collect()
}

/// The navigation pane reads like the desktop notebook's: each section is a row named after it, the
/// open one on the grey selection fill; "+ New section" and a double-click (rename) open
/// today's dialogs; ≡ hides the pane and the page list's own ≡ brings it back; Quick notes
/// sits at the bottom; and a notebook's commands are on its right-click menu.
#[test]
fn the_navigation_pane_reads_like_notebook() {
    use egui::Role;
    let mut ui = Ui::start();
    let t = crate::theme::tokens(&ui.h.ctx);
    let row = |ui: &Ui, name: &str| ui.h.get_by_role_and_label(Role::Button, name).rect();
    // Selected: the desktop notebook's grey fill, painted inside the row.
    let selected = |ui: &Ui, row: egui::Rect| {
        painted(ui).iter().any(|shape| {
            matches!(shape, egui::Shape::Rect(fill)
                if fill.fill == t.nav_selected && row.contains_rect(fill.rect))
        })
    };
    for name in [
        "This week",
        "History",
        "Science",
        "English",
        "French",
        "Maths",
        "Last year",
        "Geography",
        "Journal",
        "Recipes",
    ] {
        row(&ui, name);
    }
    // The sample opens on My week, in This week: that row is selected, and no other.
    assert_eq!(
        ui.app().selected_section.as_deref(),
        Some("section-this-week")
    );
    assert!(selected(&ui, row(&ui, "This week")));
    assert!(!selected(&ui, row(&ui, "History")));
    ui.shot("p2-nav-pane");

    // "+ New section" (the first notebook's) asks for a name, then opens the new section.
    let add =
        ui.h.get_all_by_role_and_label(Role::Button, "New section")
            .map(|node| node.rect())
            .min_by(|a, b| a.top().total_cmp(&b.top()))
            .unwrap();
    ui.click(add.center());
    assert!(
        matches!(&ui.app().dialog,
            Some(super::Dialog::NewSection { notebook_id, .. }) if notebook_id == "notebook-school"),
        "the New section dialog, for School"
    );
    ui.type_text("Errands");
    ui.press(Key::Enter);
    ui.steps(2);
    assert!(ui.app().dialog.is_none());
    let errands = ui
        .app()
        .workspace
        .notebooks
        .iter()
        .flat_map(|notebook| notebook.sections.iter())
        .find(|section| section.name == "Errands")
        .expect("the new section")
        .clone();
    assert_eq!(errands.notebook_id, "notebook-school");
    assert_eq!(
        ui.app().selected_section.as_deref(),
        Some(errands.id.as_str())
    );
    assert!(selected(&ui, row(&ui, "Errands")));
    assert!(!selected(&ui, row(&ui, "This week")));

    // the desktop notebook renames a section on double-click. (A moment passes first: the clicks just
    // before would make it a triple-click.)
    ui.steps(40);
    ui.double_click(row(&ui, "History").center());
    assert!(
        matches!(&ui.app().dialog,
            Some(super::Dialog::RenameSection { name, .. }) if name == "History"),
        "the Rename section dialog"
    );
    ui.shot("p2-nav-rename");
    ui.press(Key::Escape);
    assert!(ui.app().dialog.is_none());

    // ≡ hides the pane; the same button, now "Show navigation", waits at the left of the
    // page list card's header, and brings the pane back.
    ui.click_label("Hide navigation");
    assert!(!ui.app().show_library);
    assert!(ui
        .h
        .query_all_by_role_and_label(Role::Button, "History")
        .next()
        .is_none());
    let show =
        ui.h.get_by_role_and_label(Role::Button, "Show navigation")
            .rect();
    let first_row =
        ui.h.get_all_by_role(Role::ListItem)
            .next()
            .expect("a page row")
            .rect();
    let add_page = ui.h.get_by_role_and_label(Role::Button, "Add page").rect();
    assert!(
        show.left() >= first_row.left() - 0.5
            && show.right() <= add_page.left() + 0.5
            && show.bottom() <= first_row.top() + 0.5,
        "Show navigation {show:?} heads the page list (first row {first_row:?}, \
         Add page {add_page:?})"
    );
    ui.shot("p2-nav-hidden");
    ui.click(show.center());
    assert!(ui.app().show_library);
    row(&ui, "History");
    assert!(ui
        .h
        .query_all_by_role_and_label(Role::Button, "Show navigation")
        .next()
        .is_none());

    // Quick notes, pinned under the notebooks, opens the quick section. (The quick section
    // itself, Personal's "Quick notes", has the same name, higher up.)
    let mut quick_rows: Vec<egui::Rect> =
        ui.h.get_all_by_role_and_label(Role::Button, "Quick notes")
            .map(|node| node.rect())
            .collect();
    quick_rows.sort_by(|a, b| a.top().total_cmp(&b.top()));
    let [section_row, quick] = quick_rows[..] else {
        panic!("Personal's Quick notes and the pinned one: {quick_rows:?}")
    };
    assert!(
        section_row.top() >= row(&ui, "Recipes").bottom(),
        "{section_row:?}"
    );
    assert!(quick.top() > section_row.bottom(), "{quick:?}");
    ui.click(quick.center());
    let quick_section = ui.app().workspace.quick_section_id.clone();
    assert!(quick_section.is_some());
    assert_eq!(ui.app().selected_section, quick_section);
    assert!(selected(&ui, quick));
    assert!(!selected(&ui, row(&ui, "History")));

    // A notebook's commands are on its right-click menu, in the desktop notebook's order.
    ui.right_click(row(&ui, "School").center());
    ui.shot("p2-nav-notebook-menu");
    let tops: Vec<f32> = [
        t!("app-menu-new-section"),
        t!("app-menu-new-section-group"),
        t!("app-move-up"),
        t!("app-move-down"),
        t!("app-recycle-bin"),
        t!("app-notebook-properties"),
        t!("common-delete"),
    ]
    .iter()
    .map(|name| {
        menu_item_rect(&ui, name)
            .unwrap_or_else(|| panic!("no {name:?} in the notebook's menu"))
            .top()
    })
    .collect();
    assert!(tops.windows(2).all(|pair| pair[0] < pair[1]), "{tops:?}");
    let properties = menu_item_rect(&ui, &t!("app-notebook-properties")).unwrap();
    ui.click(properties.center());
    assert!(
        matches!(&ui.app().dialog,
            Some(super::Dialog::RenameNotebook { name, .. }) if name == "School"),
        "Properties… opens the notebook's properties"
    );
}

/// The page list sorts as the desktop notebook's sort button does: by title, by date (newest first),
/// each turned around by picking it again, and back to the section's own order, where
/// rows drag again. The choice is remembered.
#[test]
fn pages_sort_by_title_and_date_and_back() {
    use super::PageSort;
    let mut ui = Ui::start();
    // A fresh section, made the way the pane makes one.
    let add =
        ui.h.get_all_by_role_and_label(egui::Role::Button, "New section")
            .next()
            .unwrap()
            .rect();
    ui.click(add.center());
    ui.type_text("Fruit");
    ui.press(Key::Enter);
    ui.steps(2);
    assert!(ui.app().pages.is_empty(), "{:?}", titles(&ui));
    for title in ["Banana", "Apple", "Cherry"] {
        ui.new_page(title);
        save(&mut ui);
        // Each page is created a clear millisecond after the last.
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(rows_in_order(&ui), ["Banana", "Apple", "Cherry"]);
    let sort = |ui: &mut Ui, by: &str| {
        ui.click_label("Sort pages");
        ui.click_label(by);
        ui.steps(2);
    };

    sort(&mut ui, "Alphabetical");
    assert_eq!(ui.app().page_sort, PageSort::Alphabetical);
    assert_eq!(rows_in_order(&ui), ["Apple", "Banana", "Cherry"]);
    // A sorted list is flat and keeps its order: a dragged row lands nowhere.
    let apple =
        ui.h.get_by_role_and_label(egui::Role::ListItem, "Apple")
            .rect();
    let cherry =
        ui.h.get_by_role_and_label(egui::Role::ListItem, "Cherry")
            .rect();
    ui.drag(cherry.center(), apple.center_top() + egui::vec2(0.0, 6.0));
    assert_eq!(rows_in_order(&ui), ["Apple", "Banana", "Cherry"]);
    ui.click_label("Sort pages");
    ui.shot("p2-sort-menu");
    ui.press(Key::Escape);

    // The same order again turns it around.
    sort(&mut ui, "Alphabetical");
    assert!(ui.app().page_sort_desc);
    assert_eq!(rows_in_order(&ui), ["Cherry", "Banana", "Apple"]);

    // Dates start newest first.
    sort(&mut ui, "Date Created");
    assert_eq!(ui.app().page_sort, PageSort::Created);
    assert_eq!(rows_in_order(&ui), ["Cherry", "Apple", "Banana"]);

    // None is the section's own order, where a dragged row moves again.
    sort(&mut ui, "None");
    assert_eq!(ui.app().page_sort, PageSort::None);
    assert_eq!(rows_in_order(&ui), ["Banana", "Apple", "Cherry"]);
    let banana =
        ui.h.get_by_role_and_label(egui::Role::ListItem, "Banana")
            .rect();
    let cherry =
        ui.h.get_by_role_and_label(egui::Role::ListItem, "Cherry")
            .rect();
    ui.drag(cherry.center(), banana.center_top() + egui::vec2(0.0, 6.0));
    assert_eq!(rows_in_order(&ui), ["Cherry", "Banana", "Apple"]);
    let order: Vec<String> = titles(&ui).into_iter().map(|(title, _)| title).collect();
    assert_eq!(order, ["Cherry", "Banana", "Apple"], "the move was saved");

    // Remembered for the next start.
    sort(&mut ui, "Alphabetical");
    ui.reopen();
    assert_eq!(ui.app().page_sort, PageSort::Alphabetical);
    assert!(!ui.app().page_sort_desc);
    assert_eq!(rows_in_order(&ui), ["Apple", "Banana", "Cherry"]);
}

/// A page row is the desktop notebook's: a 30 px row holding the title alone. Its right-click menu
/// holds the page's commands (pinning is gone), and Page Versions opens that page's
/// versions.
#[test]
fn page_rows_show_titles_and_their_menu_holds_the_page_actions() {
    use egui::Role;
    let mut ui = Ui::start();
    let week = ui
        .app()
        .page
        .as_ref()
        .expect("the sample opens a page")
        .clone();
    ui.new_page("Shopping list");
    ui.press(Key::Enter);
    ui.type_text("Eggs, flour and a good deal of butter for the weekend");
    save(&mut ui);
    for title in [week.title.as_str(), "Shopping list"] {
        let row = ui.h.get_by_role_and_label(Role::ListItem, title).rect();
        assert!(
            (row.height() - 30.0).abs() <= 2.0,
            "{title:?}'s row is {} high",
            row.height()
        );
        let texts: Vec<String> = texts_in(&ui, row)
            .into_iter()
            .map(|(text, _)| text)
            .collect();
        assert_eq!(texts, [title], "the row shows its title alone");
    }

    // Right-click: the page's commands, in the desktop notebook's order, and no pinning, Duplicate,
    // Move to ▸ or Move up/down (Copy and Paste, Move or Copy… and dragging do those).
    let row =
        ui.h.get_by_role_and_label(Role::ListItem, &week.title)
            .rect();
    ui.right_click(row.center());
    ui.shot("p2-page-row-menu");
    let move_hint = ctrl_alt_hint(Key::M);
    let tops: Vec<f32> = [
        (t!("common-rename"), ""),
        (t!("common-delete"), ""),
        (t!("common-cut"), ""),
        (t!("common-copy"), ""),
        (t!("common-paste"), ""),
        (t!("app-menu-copy-link-to-page"), ""),
        (t!("app-move-or-copy"), move_hint.as_str()),
        (t!("app-menu-new-page"), ""),
        (t!("app-show-page-versions"), ""),
    ]
    .iter()
    .map(|(name, hint)| {
        menu_row_rect(&ui, name, hint)
            .unwrap_or_else(|| panic!("no {name:?} row"))
            .top()
    })
    .collect();
    assert!(tops.windows(2).all(|pair| pair[0] < pair[1]), "{tops:?}");
    for gone in [
        "Pin page",
        "Unpin page",
        "Copy preview text",
        "Duplicate",
        "Move to",
        "Move up",
        "Move down",
    ] {
        assert!(
            ui.h.query_all_by_label_contains(gone).next().is_none(),
            "{gone:?} is still offered"
        );
    }

    // Show Page Versions opens the page, then its versions in the task pane.
    click_menu_row(&mut ui, &t!("app-show-page-versions"), "");
    assert_eq!(
        ui.app().page.as_ref().map(|page| page.id.as_str()),
        Some(week.id.as_str())
    );
    assert!(
        matches!(&ui.app().task_pane,
            Some(super::TaskPane::Versions(pane)) if pane.page_id == week.id),
        "the versions of {:?}",
        week.title
    );
    ui.shot("p2-page-row-versions");

    // the desktop notebook has no Duplicate: Copy, then Paste on the same row, makes a copy of the page
    // that keeps its title.
    let shopping = |ui: &Ui| {
        titles(ui)
            .iter()
            .filter(|(title, _)| title == "Shopping list")
            .count()
    };
    assert_eq!(shopping(&ui), 1);
    let row =
        ui.h.get_by_role_and_label(Role::ListItem, "Shopping list")
            .rect();
    ui.right_click(row.center());
    click_menu_row(&mut ui, &t!("common-copy"), "");
    ui.right_click(row.center());
    click_menu_row(&mut ui, &t!("common-paste"), "");
    assert_eq!(shopping(&ui), 2, "the copy keeps the title");
}

/// Full Page View (F11, or the ↗↙ button) leaves the page alone under the header and the
/// tabs, as in the desktop notebook; Esc or the button again brings the panels back.
#[test]
fn full_page_view_hides_the_panels_and_comes_back() {
    use egui::Role;
    let mut ui = Ui::start();
    let title = ui.app().page.as_ref().unwrap().title.clone();
    let shows = |ui: &Ui, role: Role, label: &str| {
        ui.h.query_all_by_role_and_label(role, label)
            .next()
            .is_some()
    };
    let panels = |ui: &Ui| {
        [
            shows(ui, Role::Button, "History"),
            shows(ui, Role::ListItem, &title),
            shows(ui, Role::Button, "Bold"),
        ]
    };
    assert_eq!(panels(&ui), [true; 3]);
    assert!(shows(&ui, Role::Button, "Full Page View"));

    ui.press(Key::F11);
    assert!(ui.app().full_page);
    assert_eq!(panels(&ui), [false; 3], "the pane, the list and the band");
    // The header strip and the tabs stay; the corner button now leaves.
    assert!(shows(&ui, Role::Button, "Search"));
    ui.ribbon_tab_rect("Home");
    assert!(shows(&ui, Role::Button, "Normal View"));
    assert!(!shows(&ui, Role::Button, "Full Page View"));
    ui.shot("p2-full-page");

    ui.press(Key::Escape);
    assert!(!ui.app().full_page);
    assert_eq!(panels(&ui), [true; 3]);

    // The ↗↙ button in the page card's corner does the same, both ways.
    let corner =
        ui.h.get_by_role_and_label(Role::Button, "Full Page View")
            .rect();
    ui.click(corner.center());
    assert!(ui.app().full_page);
    assert_eq!(panels(&ui), [false; 3]);
    let corner =
        ui.h.get_by_role_and_label(Role::Button, "Normal View")
            .rect();
    ui.click(corner.center());
    assert!(!ui.app().full_page);
    assert_eq!(panels(&ui), [true; 3]);
}

/// The Recycle Bin is the desktop notebook's: History › Notebook Recycle Bin shows the deleted pages in
/// the page list, a deleted page opens under a banner, and Restore puts it back in its
/// section.
#[test]
fn the_recycle_bin_lives_in_history() {
    use egui::Role;
    let mut ui = Ui::start();
    ui.new_page("Old notes");
    save(&mut ui);
    let page = ui.app().page.as_ref().unwrap().clone();

    // Deleted from its row's menu (the desktop notebook's Delete sends a page to the Recycle Bin).
    let row =
        ui.h.get_by_role_and_label(Role::ListItem, "Old notes")
            .rect();
    ui.right_click(row.center());
    click_menu_row(&mut ui, &t!("common-delete"), "");
    let stored = |ui: &Ui| ui.app().library.get_page(&page.id).unwrap().unwrap();
    assert!(stored(&ui).trashed);
    assert!(ui
        .h
        .query_all_by_role_and_label(Role::ListItem, "Old notes")
        .next()
        .is_none());

    // History › Notebook Recycle Bin: the page list becomes the bin.
    ui.ribbon_tab("History");
    ui.ribbon_button("Notebook Recycle Bin");
    assert_eq!(ui.app().view, super::View::Trash);
    let ribbon_bottom = 48.0 + crate::ribbon::panel_height(&ui.app().ribbon);
    let heading = texts_in(&ui, egui::Rect::EVERYTHING)
        .into_iter()
        .find(|(text, rect)| text == "Notebook Recycle Bin" && rect.top() > ribbon_bottom)
        .expect("the page list's heading");
    let row =
        ui.h.get_by_role_and_label(Role::ListItem, "Old notes")
            .rect();
    assert!(
        heading.1.bottom() <= row.top() && heading.1.left() < row.right(),
        "the heading {heading:?} tops the list ({row:?})"
    );
    assert!(ui
        .h
        .query_all_by_role_and_label(Role::Button, "Add page")
        .next()
        .is_none());

    // The deleted page opens under its banner.
    ui.click(row.center());
    assert_eq!(
        ui.app().page.as_ref().map(|page| page.id.as_str()),
        Some(page.id.as_str())
    );
    assert!(ui.app().page.as_ref().unwrap().trashed);
    let restore = format!(
        "{} {}",
        crate::icons::ARROW_COUNTER_CLOCKWISE,
        t!("common-restore")
    );
    let forever = format!("{} {}", crate::icons::TRASH, t!("common-delete-forever"));
    ui.h.get_by_label(&forever);
    ui.shot("p2-recycle-bin");

    // Restore: back in its section, which the list shows again.
    ui.click_label(&restore);
    assert!(!stored(&ui).trashed);
    assert_eq!(stored(&ui).section_id, page.section_id);
    assert_eq!(ui.app().view, super::View::Section);
    assert_eq!(
        ui.app().selected_section.as_deref(),
        Some(page.section_id.as_str())
    );
    ui.h.get_by_role_and_label(Role::ListItem, "Old notes");
    assert!(ui.h.query_all_by_label(&restore).next().is_none());
}

// the desktop notebook's page (P3): the title, date and time on the page, the page with no fixed size
// and its scrollbars, and View › Page Setup and Zoom.

#[test]
fn the_title_and_date_sit_on_the_page_and_scroll_with_it() {
    use crate::canvas::{PAGE_CONTENT_TOP, PAGE_LEFT, TITLE_TOP};
    let mut ui = Ui::start();
    ui.new_page("Rings of Saturn");
    assert!(ui.app().editor.title_focused);
    // The title is drawn on the page, at the desktop notebook's place; the date and time are under it.
    let date = ui.h.get_by_label("Change the page date").rect();
    let time = ui.h.get_by_label("Change the page time").rect();
    let title_at = ui.screen(egui::pos2(PAGE_LEFT, TITLE_TOP));
    assert!(
        (date.left() - title_at.x).abs() < 4.0 && date.top() > title_at.y + 30.0,
        "the date {date:?} sits under the title at {title_at:?}"
    );
    assert!(
        time.left() > date.right() && (time.top() - date.top()).abs() < 2.0,
        "the time {time:?} follows the date {date:?}"
    );
    ui.shot("p3-header");

    // Enter in the title starts the first note under the date.
    ui.press(Key::Enter);
    ui.type_text("Saturn has rings");
    let note = ui.items()[0].clone();
    assert_eq!(text_of(&note), "Saturn has rings");
    // Its text starts under the title's first letter, its frame just left of it.
    assert_eq!(
        note.rect().min,
        egui::pos2(PAGE_LEFT - crate::canvas::NOTE_PAD_X, PAGE_CONTENT_TOP)
    );
    let page = ui.app().page.as_ref().unwrap().id.clone();
    let row = &ui.app().editor.rows[&format!("{page}:{}", note.id)][0];
    let (_, origin) = row.text.clone().expect("a text paragraph");
    assert!(
        (origin.x - PAGE_LEFT).abs() < 0.5,
        "the text starts at {}, under the title at {PAGE_LEFT}",
        origin.x
    );

    // A click on the header's empty space goes to the title, not to a new note.
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    assert!(!ui.app().editor.title_focused);
    let gap = ui.screen(egui::pos2(
        PAGE_LEFT + 300.0,
        (TITLE_TOP + PAGE_CONTENT_TOP) / 2.0 + 10.0,
    ));
    ui.click(gap);
    ui.steps(2);
    assert_eq!(ui.items().len(), 1);
    assert!(ui.app().editor.title_focused);

    // The wheel scrolls the page, header and all.
    let far = CanvasItem::new(
        CanvasKind::Text {
            doc: crate::model::Document::default(),
        },
        PAGE_LEFT,
        3000.0,
        400.0,
        40.0,
    );
    ui.h.state_mut().page.as_mut().unwrap().canvas.add(far);
    ui.steps(2);
    let before = ui.h.get_by_label("Change the page date").rect().top();
    let spot = ui.empty_paper_point();
    ui.wheel(spot, egui::vec2(0.0, -200.0), Modifiers::NONE);
    let after =
        ui.h.query_by_label("Change the page date")
            .map(|node| node.rect().top());
    assert!(
        after.is_none_or(|top| top < before - 150.0),
        "{before} -> {after:?}"
    );
}

#[test]
fn the_page_scrolls_within_its_content_and_remembers_where_it_was() {
    let mut ui = Ui::start();
    ui.new_page("Long page");
    let view = |ui: &Ui| ui.app().canvas.scene_rect.expect("a view");
    let spot = ui.empty_paper_point();
    // Nothing above the page's top edge.
    ui.wheel(spot, egui::vec2(0.0, 300.0), Modifiers::NONE);
    assert_eq!(view(&ui).min.y, 0.0);
    // A note far down and far right makes room to scroll, with scrollbars.
    let page = ui.h.state_mut().page.as_mut().unwrap();
    page.canvas.add(CanvasItem::new(
        CanvasKind::Text {
            doc: crate::model::Document::default(),
        },
        2600.0,
        5200.0,
        400.0,
        40.0,
    ));
    ui.steps(2);
    ui.shot("p3-scrollbars");
    let thumb = ui.h.get_by_label("Vertical scroll bar").rect().center();
    assert!(ui.h.query_by_label("Horizontal scroll bar").is_some());
    ui.wheel(spot, egui::vec2(0.0, -300.0), Modifiers::NONE);
    let scrolled = view(&ui).min.y;
    assert!(scrolled > 100.0, "the wheel scrolls down: {scrolled}");
    ui.wheel(spot, egui::vec2(0.0, -300.0), Modifiers::SHIFT);
    assert!(view(&ui).min.x > 100.0, "Shift+wheel scrolls sideways");
    ui.drag(thumb, thumb + egui::vec2(0.0, 200.0));
    assert!(
        view(&ui).min.y > scrolled + 300.0,
        "dragging the thumb scrolls far: {scrolled} -> {:?}",
        view(&ui)
    );
    // The page reaches past the old 4000-unit limit: you can type down there.
    ui.wheel(spot, egui::vec2(0.0, -30000.0), Modifiers::NONE);
    let bottom = view(&ui);
    assert!(bottom.max.y > 5200.0, "{bottom:?}");
    let low = ui.empty_paper_point();
    ui.click(low);
    ui.type_text("Way down");
    let typed = ui.items().last().unwrap().clone();
    assert_eq!(text_of(&typed), "Way down");
    assert!(typed.rect().top() > 4000.0, "{typed:?}");
    // Ctrl+wheel zooms.
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    ui.wheel(spot, egui::vec2(0.0, 200.0), Modifiers::COMMAND);
    assert!(
        (ui.app().canvas.zoom() - 1.0).abs() > 0.05,
        "{}",
        ui.app().canvas.zoom()
    );
    // Leaving and coming back keeps the view.
    let kept = view(&ui);
    let long = ui.app().page.as_ref().unwrap().id.clone();
    ui.new_page("Elsewhere");
    assert_eq!(
        view(&ui).min,
        egui::Pos2::ZERO,
        "a new page opens at its top"
    );
    ui.h.state_mut().open_page(&long, false);
    ui.steps(3);
    assert!(
        (view(&ui).min - kept.min).length() < 1.0,
        "{kept:?} vs {:?}",
        view(&ui)
    );
    assert!(
        (view(&ui).width() - kept.width()).abs() < 1.0,
        "the same zoom"
    );
}

#[test]
fn the_view_tab_colors_rules_and_zooms_the_page() {
    use crate::model::{PageColor, RuleLines, RuleSize};
    let mut ui = Ui::start();
    ui.new_page("Styled");
    ui.ribbon_tab("View");
    ui.ribbon_button("Page Color");
    ui.shot("p3-page-color-menu");
    ui.click_label("Lavender");
    assert_eq!(
        ui.app().page.as_ref().unwrap().color,
        Some(PageColor::Lavender)
    );
    ui.ribbon_button("Rule Lines");
    ui.shot("p3-rule-lines-menu");
    ui.click_label("College ruled");
    assert_eq!(
        ui.app().page.as_ref().unwrap().rule,
        RuleLines::Lines(RuleSize::College)
    );
    ui.ribbon_button("Rule Lines");
    ui.click_label("Always Create Pages with Rule Lines");
    assert_eq!(
        ui.app().library.new_page_rule().unwrap(),
        Some(RuleLines::Lines(RuleSize::College))
    );
    ui.ribbon_button("Hide Page Title");
    assert!(ui.app().page.as_ref().unwrap().title_hidden);
    assert!(ui.h.query_by_label("Change the page date").is_none());
    ui.ribbon_button("Hide Page Title");
    assert!(ui.h.query_by_label("Change the page date").is_some());
    // Zoom In, 100%, Page Width.
    ui.ribbon_button("Zoom In");
    assert!(ui.app().canvas.zoom() > 1.05, "{}", ui.app().canvas.zoom());
    ui.ribbon_button("100%");
    assert!((ui.app().canvas.zoom() - 1.0).abs() < 0.01);
    let page = ui.h.state_mut().page.as_mut().unwrap();
    page.canvas.add(CanvasItem::new(
        CanvasKind::Text {
            doc: crate::model::Document::default(),
        },
        48.0,
        200.0,
        2400.0,
        40.0,
    ));
    ui.ribbon_button("Page Width");
    assert!(ui.app().canvas.zoom() < 0.7, "{}", ui.app().canvas.zoom());
    // Saved with the page, and a new page starts with the lines.
    let id = ui.app().page.as_ref().unwrap().id.clone();
    assert!(ui.h.state_mut().flush_save());
    let stored = ui.app().library.get_page(&id).unwrap().unwrap();
    assert_eq!(
        (stored.color, stored.rule),
        (
            Some(PageColor::Lavender),
            RuleLines::Lines(RuleSize::College)
        )
    );
    ui.new_page("Next");
    assert_eq!(
        ui.app().page.as_ref().unwrap().rule,
        RuleLines::Lines(RuleSize::College)
    );
    assert_eq!(ui.app().page.as_ref().unwrap().color, None);
}

/// View › Page Color › More Colors… picks any colour in a dialog; Rule Lines › Rule Line
/// Color (greyed while the page has no lines) recolours the lines; Ctrl+Shift+R turns the
/// lines off and back on as they were; Ctrl+Shift+T goes to the title, selected.
#[test]
fn more_colors_rule_line_color_and_the_page_shortcuts() {
    use crate::model::{PageColor, RuleLines, RuleSize};
    use egui_kittest::kittest::NodeT;
    let mut ui = Ui::start();
    ui.new_page("Colours");
    ui.press(Key::Enter);
    ui.type_text("Notes");
    let page = |ui: &Ui| ui.app().page.as_ref().unwrap().clone();
    ui.ribbon_tab(&t!("ribbon-tab-view"));

    // More Colors…: the dialog starts from the page's colour; OK sets the one picked.
    ui.ribbon_button(&t!("ribbon-page-color"));
    ui.click_label(&format!(
        "{}    {}",
        crate::icons::PALETTE,
        t!("ribbon-more-colors")
    ));
    let Some(super::Dialog::PageColor { color, hex }) = ui.h.state_mut().dialog.as_ref() else {
        panic!("More Colors… opens its dialog");
    };
    assert_eq!((*color, hex.as_str()), (egui::Color32::WHITE, "#ffffff"));
    ui.steps(2);
    ui.shot("p7-more-colors");
    // A colour typed as "#rrggbb".
    let field =
        ui.h.get_all_by_value("#ffffff")
            .last()
            .expect("the colour's field")
            .rect();
    ui.click(field.center());
    ui.press_with(Modifiers::COMMAND, Key::A);
    ui.type_text("#faf0e6");
    ui.click_label(&t!("common-ok"));
    assert!(ui.app().dialog.is_none());
    assert_eq!(
        page(&ui).color,
        Some(PageColor::Custom(Rgb(0xfa, 0xf0, 0xe6)))
    );

    // Rule Line Color: greyed with no lines, then the page's lines in the colour picked.
    let color = t!("ribbon-rule-line-color");
    ui.ribbon_button(&t!("ribbon-rule-lines"));
    assert!(ui.h.get_by_label(&color).accesskit_node().is_disabled());
    ui.click_label(&RuleLines::Lines(RuleSize::Wide).label());
    ui.ribbon_button(&t!("ribbon-rule-lines"));
    ui.click_label(&color);
    ui.shot("p7-rule-line-color");
    let (green, _) = crate::canvas::PEN_COLORS[6];
    ui.click_label(&green.hex());
    assert_eq!(page(&ui).rule_color, Some(green));
    assert!(ui.h.state_mut().flush_save());
    let stored = ui.app().library.get_page(&page(&ui).id).unwrap().unwrap();
    assert_eq!(
        (stored.color, stored.rule_color),
        (Some(PageColor::Custom(Rgb(0xfa, 0xf0, 0xe6))), Some(green))
    );

    // Ctrl+Shift+R: the lines go, then come back as they were.
    ui.press_with(Modifiers::COMMAND | Modifiers::SHIFT, Key::R);
    assert_eq!(page(&ui).rule, RuleLines::None);
    ui.press_with(Modifiers::COMMAND | Modifiers::SHIFT, Key::R);
    assert_eq!(page(&ui).rule, RuleLines::Lines(RuleSize::Wide));
    assert_eq!(
        text_of(&ui.items()[0]),
        "Notes",
        "no shortcut reached the note"
    );

    // Ctrl+Shift+T: the title, its text selected, so typing replaces it.
    ui.press_with(Modifiers::COMMAND | Modifiers::SHIFT, Key::T);
    ui.steps(2);
    ui.type_text("Palette");
    assert_eq!(page(&ui).title, "Palette");
}

#[test]
fn the_page_date_and_time_change_from_their_pickers() {
    use chrono::{Datelike, Local, TimeZone, Timelike};
    let mut ui = Ui::start();
    ui.new_page("Dated");
    let created = |ui: &Ui| {
        Local
            .timestamp_millis_opt(ui.app().page.as_ref().unwrap().created_at)
            .single()
            .unwrap()
    };
    let before = created(&ui);
    // The page list sorted by date created, newest first: this page, then an older one.
    let dated = ui.app().page.as_ref().unwrap().id.clone();
    let older = ui
        .app()
        .pages
        .iter()
        .find(|row| row.id != dated)
        .expect("another page in the section")
        .id
        .clone();
    ui.h.state_mut().set_page_sort(super::PageSort::Created);
    ui.steps(2);
    let row = |ui: &Ui, id: &str| ui.app().pages.iter().position(|row| row.id == id);
    assert!(row(&ui, &dated) < row(&ui, &older), "newest first");
    ui.click_label("Change the page date");
    ui.shot("p3-date-picker");
    ui.click_label("Previous month");
    let target = before
        .date_naive()
        .with_day(1)
        .unwrap()
        .pred_opt()
        .unwrap()
        .with_day(15)
        .unwrap();
    let name = crate::util::long_date(
        Local
            .from_local_datetime(&target.and_time(before.time()))
            .earliest()
            .unwrap(),
    );
    ui.click_label(&name);
    let after = created(&ui);
    assert_eq!(after.date_naive(), target);
    assert_eq!(
        (after.hour(), after.minute()),
        (before.hour(), before.minute())
    );
    // The header shows the new date at once, and once saved the page list has it too,
    // in its new place.
    assert_eq!(ui.app().canvas.header_text().0, name);
    assert!(ui.h.state_mut().flush_save());
    ui.steps(2);
    let summary = ui.app().pages.iter().find(|row| row.id == dated).unwrap();
    assert_eq!(
        summary.created_at,
        ui.app().page.as_ref().unwrap().created_at
    );
    assert!(row(&ui, &dated) > row(&ui, &older), "a month older now");
    ui.click_label("Change the page time");
    ui.shot("p3-time-picker");
    ui.click_label("Now");
    let now = Local::now();
    let changed = created(&ui);
    assert_eq!(
        changed.date_naive(),
        target,
        "the time picker keeps the day"
    );
    assert_eq!(changed.hour(), now.hour());
    assert_eq!(
        ui.app().canvas.header_text().1,
        crate::util::clock_time(changed)
    );
}

/// the desktop notebook puts typed text on the rule lines: the line under the first note's first line
/// of text runs just under its baseline, not through its letters.
#[test]
fn typed_text_sits_on_the_rule_lines() {
    let mut ui = Ui::start();
    ui.new_page("Rings");
    ui.press(Key::Enter);
    ui.type_text("Saturn has rings");
    let note = ui.items()[0].clone();
    let page = ui.app().page.as_ref().unwrap().id.clone();
    let block = note.doc().unwrap().blocks[0].id;
    let id = crate::editor::block_id(&format!("{page}:{}", note.id), block);
    let row = ui.h.ctx.read_response(id).expect("the line is drawn").rect;
    let baseline = row.top() + crate::editor::body_baseline(&ui.h.ctx, None);
    let rule = crate::canvas::FIRST_RULE;
    assert!(
        (0.5..=2.0).contains(&(rule - baseline)),
        "the rule at {rule} sits just under the baseline at {baseline}"
    );
    ui.ribbon_tab("View");
    pick_rule_lines(
        &mut ui,
        crate::model::RuleLines::Lines(crate::model::RuleSize::College),
        None,
    );
    ui.shot("p3-text-on-rule-lines");

    // On the ruled page every line sits on a rule: a wrapped paragraph's second line, the
    // paragraphs after it, and a note started anywhere or moved a little.
    let step = crate::model::RuleLines::Lines(crate::model::RuleSize::College)
        .spacing()
        .unwrap();
    let off_rule = |ui: &Ui, note: u64| -> Vec<f32> {
        let blocks = ui
            .item(note)
            .doc()
            .expect("a note")
            .blocks
            .iter()
            .map(|block| block.id)
            .collect::<Vec<_>>();
        let mut misses = Vec::new();
        for block in blocks {
            let (galley, origin) = ui.row(note, block).text.expect("text");
            for row in &galley.rows {
                let Some(glyph) = row.glyphs.first() else {
                    continue;
                };
                let baseline = origin.y + row.pos.y + glyph.pos.y;
                let under = (rule - baseline).rem_euclid(step);
                if !(0.5..=2.0).contains(&under) {
                    misses.push(baseline);
                }
            }
        }
        misses
    };
    let text = ui.text_point(note.id, block, 16);
    ui.click(text);
    ui.press(Key::End);
    for _ in 0..8 {
        ui.type_text(" and its moons go round and round");
    }
    ui.press(Key::Enter);
    ui.type_text("Jupiter has more");
    let (galley, _) = ui.row(note.id, block).text.expect("text");
    assert!(galley.rows.len() >= 2, "the first paragraph wraps");
    assert_eq!(
        off_rule(&ui, note.id),
        Vec::<f32>::new(),
        "every line on a rule"
    );
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    let spot = ui.empty_paper_point() + egui::vec2(0.0, 7.0);
    ui.click(spot);
    ui.type_text("Uranus");
    let other = ui
        .items()
        .iter()
        .find(|item| text_of(item) == "Uranus")
        .expect("a second note")
        .id;
    assert_eq!(
        off_rule(&ui, other),
        Vec::<f32>::new(),
        "a new note's text too"
    );
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    let bar = ui.bar(other);
    ui.drag(bar, bar + egui::vec2(30.0, 11.0));
    ui.steps(2);
    assert_eq!(
        off_rule(&ui, other),
        Vec::<f32>::new(),
        "and a note moved a little"
    );
    ui.shot("p4-text-on-rule-lines");
}

/// Dragging a scrollbar's thumb or clicking its track while typing leaves the caret in the
/// note, as the desktop notebook does: the next keystrokes still go into it.
#[test]
fn using_the_scrollbars_keeps_typing_in_the_note() {
    let mut ui = Ui::start();
    ui.new_page("Scroll");
    ui.press(Key::Enter);
    ui.type_text("Hello");
    let note = ui.items()[0].id;
    let text = |ui: &Ui| ui.item(note).doc().unwrap().blocks[0].text.text.clone();
    ui.h.state_mut()
        .page
        .as_mut()
        .unwrap()
        .canvas
        .add(CanvasItem::new(
            CanvasKind::Text {
                doc: crate::model::Document::default(),
            },
            crate::canvas::PAGE_LEFT,
            4000.0,
            400.0,
            40.0,
        ));
    ui.steps(2);
    assert!(
        ui.h.query_by_label("Horizontal scroll bar").is_none(),
        "nothing to the right: no bar along the bottom"
    );
    let thumb = ui.h.get_by_label("Vertical scroll bar").rect();
    ui.drag(thumb.center(), thumb.center() + egui::vec2(0.0, 30.0));
    let top = ui.app().canvas.scene_rect.unwrap().min.y;
    assert!(top > 50.0, "the thumb scrolled the page: {top}");
    ui.type_text(" world");
    assert_eq!(text(&ui), "Hello world");
    assert_eq!(ui.app().canvas.active_text, Some(note));
    // A click on the track below the thumb scrolls a screen, and typing goes on.
    let thumb = ui.h.get_by_label("Vertical scroll bar").rect();
    ui.click(egui::pos2(thumb.center().x, thumb.bottom() + 60.0));
    assert!(ui.app().canvas.scene_rect.unwrap().min.y > top + 200.0);
    ui.type_text("!");
    assert_eq!(text(&ui), "Hello world!");
    assert_eq!(ui.app().canvas.active_text, Some(note));
    assert!(ui.h.ctx.text_edit_focused(), "the note has the caret");
}

/// At the page's edges the wheel (and Ctrl+wheel near its corner) never draws a frame of
/// the page pushed past them, which would snap back on the next frame.
#[test]
fn the_wheel_never_draws_the_page_past_its_edges() {
    let mut ui = Ui::start();
    ui.new_page("Edges");
    ui.press(Key::Enter);
    ui.type_text("Top");
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    let turn = |ui: &mut Ui, at: Pos2, delta: egui::Vec2, modifiers: Modifiers| {
        ui.h.event(Event::PointerMoved(at));
        ui.h.step();
        ui.h.event(Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta,
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
            modifiers,
        });
        for _ in 0..40 {
            ui.h.step();
            let shown = ui.app().canvas.visible().expect("the page is drawn");
            assert!(
                shown.min.x >= -0.01 && shown.min.y >= -0.01,
                "a frame drawn past the page's corner: {shown:?}"
            );
        }
    };
    // Up and left at the top-left corner.
    let spot = ui.empty_paper_point();
    turn(&mut ui, spot, egui::vec2(0.0, 300.0), Modifiers::NONE);
    turn(&mut ui, spot, egui::vec2(300.0, 0.0), Modifiers::NONE);
    // Zooming out near the corner.
    let near = ui.screen(egui::pos2(30.0, 30.0));
    turn(&mut ui, near, egui::vec2(0.0, -200.0), Modifiers::COMMAND);
    assert!(ui.app().canvas.zoom() < 0.9, "{}", ui.app().canvas.zoom());
    let corner = ui.app().canvas.scene_rect.unwrap().min;
    assert!(corner.to_vec2().length() < 0.01, "{corner:?}");
}

/// A click on the header puts the caret in the title without moving the view when the
/// title's end is in view, and moves it only as far as needed when it is not.
#[test]
fn a_header_click_moves_the_view_only_to_show_the_caret() {
    use crate::canvas::{PAGE_CONTENT_TOP, PAGE_LEFT, TITLE_TOP};
    let mut ui = Ui::start();
    ui.new_page("Rings of Saturn");
    ui.press(Key::Enter);
    ui.type_text("Saturn has rings");
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    let size = ui.app().canvas.viewport().size() / 2.0;
    let show = |ui: &mut Ui, x: f32| {
        ui.h.state_mut().canvas.scene_rect =
            Some(egui::Rect::from_min_size(egui::pos2(x, 0.0), size));
        ui.steps(3);
        assert!((ui.app().canvas.zoom() - 2.0).abs() < 0.01);
    };
    let header = egui::pos2(
        PAGE_LEFT + 380.0,
        (TITLE_TOP + PAGE_CONTENT_TOP) / 2.0 + 10.0,
    );
    // Zoomed to 200 % and scrolled right, with the title's end still in view.
    show(&mut ui, 100.0);
    ui.click(ui.screen(header));
    assert!(ui.app().editor.title_focused);
    let x = ui.app().canvas.scene_rect.unwrap().min.x;
    assert!((x - 100.0).abs() < 0.5, "no jump: {x}");
    // Scrolled past the title's end: the view comes back just far enough.
    ui.press(Key::Enter);
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    show(&mut ui, 380.0);
    ui.click(ui.screen(header));
    assert!(ui.app().editor.title_focused);
    let x = ui.app().canvas.scene_rect.unwrap().min.x;
    assert!(x > 100.0 && x < 380.0, "{x}");
}

/// French: the Rule Lines menu's "Aucune" tile fits its label inside its frame.
#[test]
fn the_rule_lines_none_tile_fits_in_french() {
    let mut ui = Ui::start_in(Lang::Fr);
    ui.new_page("Réglure");
    ui.ribbon_tab("Affichage");
    ui.ribbon_button("Réglure");
    let label = t!("rule-none");
    assert_eq!(label, "Aucune");
    let tile =
        ui.h.get_all_by_label(label.as_ref())
            .last()
            .expect("the None tile")
            .rect();
    let font = crate::ribbon::tile_label_font(&ui.h.ctx, &label, tile.width());
    let width = crate::widgets::text_width(&ui.h.ctx, &label, font);
    assert!(
        width <= tile.width() - 8.0,
        "{width} in a {} wide tile",
        tile.width()
    );
    ui.shot("fr-p3-rule-none-tile");
}

#[test]
fn a_new_container_widens_as_you_type_then_wraps_at_the_page_edge() {
    let mut ui = Ui::start();
    ui.new_page("Widths");
    ui.click(ui.empty_paper_point());
    let id = ui.items()[0].id;
    let empty = ui.item(id).rect().width();
    assert!(empty <= 120.0, "a new container starts small: {empty}");
    ui.type_text("Short");
    let short = ui.item(id).rect().width();
    ui.type_text(" and then a much longer line that keeps going");
    let long = ui.item(id).rect().width();
    assert!(
        long > short + 150.0,
        "it widened as text came: {short} → {long}"
    );
    let one_line = ui.item(id).rect().height();
    for _ in 0..6 {
        ui.type_text(" and on and on across the page until it has to wrap");
    }
    let view = ui.app().canvas.visible().expect("a view");
    let rect = ui.item(id).rect();
    assert!(
        rect.right() <= view.right(),
        "it stops at the page edge: {rect:?} / {view:?}"
    );
    assert!(rect.height() > one_line + 10.0, "then it wraps");
    ui.shot("p4-auto-width");

    // Dragging its width handle sets the width, which stays as more text comes.
    let handle = ui.width_handle(id);
    ui.drag(handle, handle - egui::vec2(120.0, 0.0));
    let fixed = ui.item(id).rect().width();
    assert!(!ui.item(id).auto_width);
    ui.click(ui.text_point(id, ui.blocks()[0].id, 3));
    ui.type_text(" more");
    assert_eq!(ui.item(id).rect().width(), fixed);
}

#[test]
fn a_containers_frame_has_a_grab_bar_and_no_delete_button() {
    let mut ui = Ui::start();
    ui.new_page("Frame");
    ui.click(ui.empty_paper_point());
    ui.type_text("Framed");
    let id = ui.items()[0].id;
    // The harness may run in either theme: compare with the tokens it paints with.
    let tokens = crate::theme::tokens(&ui.h.ctx);
    let note = ui.item(id).rect();
    let top = ui.screen(note.min).y;
    // While typing, the bar is the desktop notebook's grey too, not the accent of a selected note.
    ui.steps(2);
    let fills: Vec<egui::Color32> = painted(&ui)
        .iter()
        .filter_map(|shape| match shape {
            egui::Shape::Rect(bar) if (bar.rect.bottom() - top).abs() < 1.0 => Some(bar.fill),
            _ => None,
        })
        .collect();
    assert_eq!(fills, [tokens.grab_bar], "a grey bar while typing");
    // Escape selects the note as a whole: the desktop notebook's grey frame with a darker bar, never
    // the accent (pictures, shapes and ink keep it).
    ui.press(Key::Escape);
    ui.steps(2);
    assert_eq!(
        ui.selection().items,
        vec![id],
        "Escape leaves the note selected"
    );
    let shapes = painted(&ui);
    let bars: Vec<egui::Color32> = shapes
        .iter()
        .filter_map(|shape| match shape {
            egui::Shape::Rect(bar) if (bar.rect.bottom() - top).abs() < 1.0 => Some(bar.fill),
            _ => None,
        })
        .collect();
    assert_eq!(
        bars,
        [tokens.line_strong],
        "a darker grey bar when selected"
    );
    let frame = egui::Rect::from_min_max(ui.screen(note.min), ui.screen(note.max))
        .expand2(egui::vec2(8.0, 24.0));
    let frames: Vec<egui::Color32> = shapes
        .iter()
        .filter_map(|shape| match shape {
            egui::Shape::Rect(rect)
                if rect.stroke.width > 0.0 && frame.contains_rect(rect.rect.shrink(1.0)) =>
            {
                Some(rect.stroke.color)
            }
            _ => None,
        })
        .collect();
    assert!(
        frames.contains(&tokens.container_border) && !frames.contains(&tokens.accent),
        "a grey frame, not the accent: {frames:?}"
    );
    // A second Escape leaves the note; then hovering shows its frame.
    ui.press(Key::Escape);
    ui.h.event(Event::PointerMoved(ui.screen(note.center())));
    ui.steps(2);
    let shapes = painted(&ui);
    let bar = shapes.iter().any(|shape| {
        matches!(shape, egui::Shape::Rect(bar)
            if bar.fill == tokens.grab_bar && (bar.rect.bottom() - top).abs() < 1.0)
    });
    assert!(bar, "a grey bar along the top of the frame");
    // The ribbon has its own Delete button (an ×): look only at the note and its bar.
    let frame = egui::Rect::from_min_max(ui.screen(note.min), ui.screen(note.max))
        .expand2(egui::vec2(8.0, 24.0));
    let cross = shapes.iter().any(|shape| {
        matches!(shape, egui::Shape::Text(text)
            if text.galley.text() == crate::icons::X
                && frame.intersects(text.visual_bounding_rect()))
    });
    assert!(!cross, "no delete button on a note");
    // Away from it, the frame goes.
    ui.h.event(Event::PointerMoved(
        ui.empty_paper_point() + egui::vec2(0.0, 200.0),
    ));
    ui.steps(2);
    assert!(!painted(&ui)
        .iter()
        .any(|shape| matches!(shape, egui::Shape::Rect(bar) if bar.fill == tokens.grab_bar)));
    ui.shot("p4-frame");
}

#[test]
fn shift_dragging_a_container_onto_another_merges_them() {
    let mut ui = Ui::start();
    ui.new_page("Merge");
    let first = ui.empty_paper_point();
    ui.click(first);
    ui.type_text("Keep");
    ui.click(first + egui::vec2(0.0, 160.0));
    ui.type_text("Join");
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    let items = ui.items();
    let (keep, join) = (items[0].id, items[1].id);
    let target = ui.screen(ui.item(keep).rect().center() + egui::vec2(0.0, 6.0));
    ui.with_modifiers(Modifiers::SHIFT, |ui| {
        let bar = ui.bar(join);
        ui.drag(bar, target);
    });
    let items = ui.items();
    assert_eq!(items.len(), 1, "{items:#?}");
    assert_eq!(text_of(&items[0]), "Keep\nJoin");
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert_eq!(ui.items().len(), 2, "one undo step brings it back");
}

/// The note last typed in keeps an undo history of its own; Shift+dragging another note
/// into it must not let that history reach back past the merge and wipe what came in.
#[test]
fn merging_into_the_note_last_typed_in_never_undoes_past_the_merge() {
    let mut ui = Ui::start();
    ui.new_page("Merge undo");
    let first = ui.empty_paper_point();
    ui.click(first);
    ui.type_text("Keep");
    ui.click(first + egui::vec2(0.0, 160.0));
    ui.type_text("Join");
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    let items = ui.items();
    let (keep, join) = (items[0].id, items[1].id);
    let target = ui.screen(ui.item(join).rect().center() + egui::vec2(0.0, 6.0));
    ui.with_modifiers(Modifiers::SHIFT, |ui| {
        let bar = ui.bar(keep);
        ui.drag(bar, target);
    });
    assert_eq!(ui.items().len(), 1);
    assert_eq!(text_of(&ui.item(join)), "Join\nKeep");
    let kept = ui.item(join).doc().expect("a note").blocks[1].id;
    ui.click(ui.text_point(join, kept, 4));
    ui.press(Key::End);
    ui.type_text("x");
    assert_eq!(text_of(&ui.item(join)), "Join\nKeepx");
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert_eq!(
        text_of(&ui.item(join)),
        "Join\nKeep",
        "undo takes the typing back"
    );
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert_eq!(
        text_of(&ui.item(join)),
        "Join\nKeep",
        "and stops at the merge"
    );
}

/// Clicking near the page's left edge starts a note with room for its paragraph handles.
#[test]
fn a_note_at_the_page_edge_keeps_its_handles_on_the_page() {
    let mut ui = Ui::start();
    ui.new_page("Edge");
    let view = ui.app().canvas.viewport();
    let spot = egui::pos2(view.left() + 4.0, ui.empty_paper_point().y);
    ui.click(spot);
    ui.type_text("Edge");
    let note = ui.items()[0].clone();
    assert!(
        note.x >= crate::editor::HANDLE_GUTTER,
        "the handles' margin is on the page: {}",
        note.x
    );
    let block = note.doc().expect("a note").blocks[0].id;
    let handle = ui.handle_point(note.id, block);
    assert!(handle.x > view.left(), "{handle:?} / {view:?}");
}

#[test]
fn tags_hang_left_so_tagged_and_plain_lines_start_together() {
    let mut ui = Ui::start();
    ui.new_page("Tags");
    ui.press(Key::Enter);
    ui.type_text("Plain");
    ui.press(Key::Enter);
    ui.type_text("Starred");
    ui.press_with(Modifiers::COMMAND, Key::Num2);
    ui.press(Key::Enter);
    ui.press_with(Modifiers::COMMAND, Key::Num1);
    ui.type_text("Boxed");
    ui.steps(2);
    let (plain, ..) = line_lead(&ui, "Plain");
    let (starred, star_gaps, _) = line_lead(&ui, "Starred");
    let (boxed, ..) = line_lead(&ui, "Boxed");
    assert!(
        (plain - starred).abs() < 0.5 && (plain - boxed).abs() < 0.5,
        "{plain} {starred} {boxed}"
    );
    assert_eq!(star_gaps.len(), 1, "the star hangs left of its line");
    // A ticked To Do keeps its text as it is.
    let blocks = ui.blocks();
    let ticked = blocks
        .iter()
        .find(|block| block.text.text.starts_with("Boxed"))
        .expect("to-do");
    assert!(matches!(ticked.kind, crate::model::BlockKind::Todo { .. }));
    ui.shot("p4-tags");
}

#[test]
fn styles_and_levels_look_like_notebook() {
    use crate::model::BlockKind;
    let mut ui = Ui::start();
    ui.new_page("Styles");
    ui.press(Key::Enter);
    ui.type_text("Cells");
    ui.press_with(Modifiers::COMMAND | Modifiers::ALT, Key::Num1);
    ui.press(Key::Enter);
    ui.press_with(Modifiers::COMMAND, Key::Slash);
    ui.type_text("Nucleus");
    ui.press(Key::Enter);
    ui.press(Key::Tab);
    ui.type_text("DNA");
    let blocks = ui.blocks();
    assert_eq!(blocks[0].kind, BlockKind::heading(1));
    assert_eq!(
        (blocks[2].kind.clone(), blocks[2].indent),
        (BlockKind::Numbered, 1)
    );
    // The level-2 item reads "a.", the one above it "1.".
    let labels: Vec<String> = painted(&ui)
        .iter()
        .filter_map(|shape| match shape {
            egui::Shape::Text(text) if text.galley.text().ends_with('.') => {
                Some(text.galley.text().to_string())
            }
            _ => None,
        })
        .collect();
    assert!(
        labels.contains(&"1.".to_string()) && labels.contains(&"a.".to_string()),
        "{labels:?}"
    );
    // Normal text is 11 pt at 96 DPI on single-spaced lines.
    let row = ui.row(ui.items()[0].id, blocks[1].id);
    assert!(
        (row.head.span() - crate::editor::BODY_LINE).abs() < 1.0,
        "{row:?}"
    );
    ui.shot("p4-styles");
}

/// Types an outline into a new note: A, a1 and a2 under it, then B.
fn outline_note(ui: &mut Ui) -> u64 {
    ui.press(Key::Enter);
    ui.type_text("A");
    ui.press(Key::Enter);
    ui.press(Key::Tab);
    ui.type_text("a1");
    ui.press(Key::Enter);
    ui.type_text("a2");
    ui.press(Key::Enter);
    ui.press_with(Modifiers::SHIFT, Key::Tab);
    ui.type_text("B");
    ui.items()[0].id
}

/// Containers and outlines for the galleries: styles, levels with their markers (bullets
/// and numbers), tags in the margin, a folded paragraph, a handle on hover and a selection
/// across paragraphs, shot as `{prefix}-outline…`. Typed on a page of its own, titled
/// `{prefix} outline`; ends on the page it started from.
fn outline_shots(ui: &mut Ui, prefix: &str) {
    let start = ui.app().page.as_ref().expect("a page is open").id.clone();
    ui.new_page(&format!("{prefix} outline"));
    ui.press(Key::Enter);
    ui.type_text("Cells");
    ui.press_with(Modifiers::COMMAND | Modifiers::ALT, Key::Num1);
    ui.press(Key::Enter);
    ui.type_text("Every living thing is made of cells.");
    ui.press(Key::Enter);
    ui.press_with(Modifiers::COMMAND, Key::Period);
    ui.type_text("Nucleus");
    ui.press(Key::Enter);
    ui.press(Key::Tab);
    ui.type_text("Holds the DNA");
    ui.press(Key::Enter);
    ui.press(Key::Tab);
    ui.type_text("Chromosomes");
    ui.press(Key::Enter);
    ui.press_with(Modifiers::SHIFT, Key::Tab);
    ui.press_with(Modifiers::SHIFT, Key::Tab);
    ui.type_text("Membrane");
    ui.press_with(Modifiers::COMMAND, Key::Num2);
    // Tags carry over to the next paragraph, as in the desktop notebook: Ctrl+0 takes the star off.
    // To Do is a kind of paragraph, like the bullets and numbers, so it gets a line of its
    // own, and the numbers go one level down too (1. then a.).
    ui.press(Key::Enter);
    ui.press_with(Modifiers::COMMAND, Key::Num0);
    ui.press_with(Modifiers::COMMAND, Key::Period);
    ui.press_with(Modifiers::COMMAND, Key::Num1);
    ui.type_text("Revise for Thursday");
    ui.press(Key::Enter);
    ui.press_with(Modifiers::COMMAND, Key::Num1);
    ui.press_with(Modifiers::COMMAND, Key::Slash);
    ui.type_text("Ask about the lab");
    ui.press_with(Modifiers::COMMAND, Key::Num3);
    ui.press(Key::Enter);
    ui.press_with(Modifiers::COMMAND, Key::Num0);
    ui.press(Key::Tab);
    ui.type_text("Bring a lab coat");
    {
        use crate::model::BlockKind;
        let kinds: Vec<(BlockKind, u8, usize)> = ui
            .blocks()
            .iter()
            .map(|block| (block.kind.clone(), block.indent, block.tags.len()))
            .collect();
        assert_eq!(
            kinds[2..],
            [
                (BlockKind::Bullet, 0, 0),
                (BlockKind::Bullet, 1, 0),
                (BlockKind::Bullet, 2, 0),
                (BlockKind::Bullet, 0, 1),
                (BlockKind::Todo { checked: false }, 0, 0),
                (BlockKind::Numbered, 0, 1),
                (BlockKind::Numbered, 1, 0),
            ],
            "the outline the shots show"
        );
    }
    settle(ui);
    ui.shot(&format!("{prefix}-outline"));
    let note = ui.items()[0].id;
    let blocks = ui.blocks();
    ui.h.event(Event::PointerMoved(ui.text_point(note, blocks[2].id, 2)));
    ui.steps(2);
    ui.shot(&format!("{prefix}-outline-handle"));
    ui.double_click(ui.handle_point(note, blocks[2].id));
    assert!(ui.blocks()[2].collapsed, "the double-click folded Nucleus");
    // The pointer off the "+", so it shows whole.
    settle(ui);
    ui.shot(&format!("{prefix}-outline-folded"));
    ui.click(ui.handle_point(note, blocks[2].id));
    assert!(
        !ui.blocks()[2].collapsed,
        "a click on the \"+\" unfolded it"
    );
    ui.drag(
        ui.text_point(note, blocks[1].id, 6),
        ui.text_point(note, blocks[3].id, 4),
    );
    assert!(
        ui.app().editor.range.is_some(),
        "the drag selected across paragraphs"
    );
    ui.shot(&format!("{prefix}-outline-range"));
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    save(ui);

    // Back to the page it started from.
    ui.h.state_mut().open_page(&start, true);
    ui.steps(1);
    settle(ui);
}

fn texts_and_levels(ui: &Ui) -> Vec<(String, u8)> {
    ui.blocks()
        .iter()
        .map(|block| (block.text.text.clone(), block.indent))
        .collect()
}

fn shown(ui: &Ui, text: &str) -> bool {
    painted(ui)
        .iter()
        .any(|shape| matches!(shape, egui::Shape::Text(shape) if shape.galley.text() == text))
}

#[test]
fn tab_and_alt_shift_keys_shape_an_outline() {
    let mut ui = Ui::start();
    ui.new_page("Outline");
    outline_note(&mut ui);
    let pairs = |list: &[(&str, u8)]| -> Vec<(String, u8)> {
        list.iter()
            .map(|(text, level)| (text.to_string(), *level))
            .collect()
    };
    assert_eq!(
        texts_and_levels(&ui),
        pairs(&[("A", 0), ("a1", 1), ("a2", 1), ("B", 0)])
    );

    // Alt+Shift+↑ takes B above A's whole family; Alt+Shift+↓ brings it back.
    ui.press_with(Modifiers::ALT | Modifiers::SHIFT, Key::ArrowUp);
    assert_eq!(
        texts_and_levels(&ui),
        pairs(&[("B", 0), ("A", 0), ("a1", 1), ("a2", 1)])
    );
    ui.press_with(Modifiers::ALT | Modifiers::SHIFT, Key::ArrowDown);
    assert_eq!(
        texts_and_levels(&ui),
        pairs(&[("A", 0), ("a1", 1), ("a2", 1), ("B", 0)])
    );

    // Show level 1 folds a1 and a2 away; Alt+Shift+0 shows them again.
    ui.press_with(Modifiers::ALT | Modifiers::SHIFT, Key::Num1);
    ui.steps(2);
    assert!(!shown(&ui, "a1") && shown(&ui, "A"), "folded");
    ui.press_with(Modifiers::ALT | Modifiers::SHIFT, Key::Num0);
    ui.steps(2);
    assert!(shown(&ui, "a1"), "unfolded");

    // Tab on A takes its subparagraphs one level deeper with it.
    let a = ui.blocks()[0].id;
    let note = ui.items()[0].id;
    ui.click(ui.text_point(note, a, 0));
    ui.press(Key::Tab);
    assert_eq!(
        texts_and_levels(&ui),
        pairs(&[("A", 1), ("a1", 2), ("a2", 2), ("B", 0)])
    );
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert_eq!(
        texts_and_levels(&ui)[0],
        ("A".to_string(), 0),
        "one undo step"
    );
}

#[test]
fn paragraph_handles_select_fold_and_move_a_paragraph() {
    let mut ui = Ui::start();
    ui.new_page("Handles");
    let note = outline_note(&mut ui);
    let a = ui.blocks()[0].id;
    let a2 = ui.blocks()[2].id;

    // A click on A's handle selects A with a1 and a2.
    ui.click(ui.handle_point(note, a));
    let range = ui
        .app()
        .editor
        .range
        .expect("a selection across paragraphs");
    let ordered = if range.0.block == a {
        range
    } else {
        (range.1, range.0)
    };
    assert_eq!((ordered.0.block, ordered.0.char), (a, 0));
    assert_eq!((ordered.1.block, ordered.1.char), (a2, 2));
    ui.shot("p4-handle-select");

    // A double-click folds them under a "+"; a click on it unfolds them. (Each comes long
    // enough after the click before it that egui doesn't count it in.)
    ui.steps(30);
    ui.double_click(ui.handle_point(note, a));
    assert!(ui.blocks()[0].collapsed);
    assert!(!shown(&ui, "a1"));
    ui.shot("p4-handle-folded");
    ui.steps(30);
    ui.click(ui.handle_point(note, a));
    assert!(!ui.blocks()[0].collapsed);
    // The caret stayed in the note (at the end of A, where the fold put it).
    ui.type_text("1");
    assert_eq!(
        ui.blocks()[0].text.text,
        "A1",
        "still typing after the \"+\""
    );

    // Dragging B's handle above A moves it there, and typing goes on where it was.
    let b = ui.blocks()[3].id;
    let to = ui.screen(egui::pos2(
        ui.item(note).rect().left() - 8.0,
        ui.row(note, a).rect.top() + 2.0,
    ));
    ui.drag(ui.handle_point(note, b), to);
    let order = |ui: &Ui| -> Vec<String> {
        ui.blocks()
            .iter()
            .map(|block| block.text.text.clone())
            .collect()
    };
    assert_eq!(order(&ui), ["B", "A1", "a1", "a2"]);
    ui.type_text("2");
    assert_eq!(
        order(&ui),
        ["B", "A12", "a1", "a2"],
        "still typing after the drag"
    );
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert_eq!(ui.blocks()[0].text.text, "A1", "the move undoes");
}

/// A paragraph handle's right-click menu, as the desktop notebook's gripper has it: Select › Level 1
/// shows only the outline's first level and All Levels shows it all again; the note's own
/// menu has the same Select ›; Cut takes the paragraph with its subparagraphs.
#[test]
fn a_handle_menu_shows_levels_and_cuts_a_paragraph() {
    let mut ui = Ui::start();
    ui.new_page("Gripper");
    let note = outline_note(&mut ui);
    let a = ui.blocks()[0].id;
    let select = format!("    {}", t!("editor-select-levels"));
    let pick_level = |ui: &mut Ui, levels: u8| {
        let submenu =
            ui.h.get_all_by_label_contains(&select)
                .last()
                .expect("Select ›")
                .rect();
        ui.click(submenu.center());
        let label = if levels == 0 {
            t!("editor-all-levels").into_owned()
        } else {
            t!("editor-level", level = levels).into_owned()
        };
        let key = [Key::Num0, Key::Num1][usize::from(levels)];
        let hint = crate::util::menu_shortcut(Modifiers::ALT | Modifiers::SHIFT, key);
        let row = ui.h.get_by_label(&format!("{label} {hint}")).rect();
        ui.click(row.center());
        ui.steps(2);
    };
    let folded = |ui: &Ui| ui.blocks()[0].collapsed;

    ui.right_click(ui.handle_point(note, a));
    ui.shot("p7-handle-menu");
    pick_level(&mut ui, 1);
    assert!(folded(&ui), "Level 1 folds a1 and a2 under A");
    assert!(!shown(&ui, "a1"));
    ui.right_click(ui.handle_point(note, a));
    pick_level(&mut ui, 0);
    assert!(!folded(&ui), "All Levels shows them again");

    // The note's grab bar has Select › as well.
    ui.right_click(ui.bar(note));
    pick_level(&mut ui, 1);
    assert!(folded(&ui), "the note's Select › Level 1");
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert!(!folded(&ui), "and it undoes");

    // Cut: A, a1 and a2 go to the clipboard and off the note; B stays.
    ui.steps(30);
    ui.right_click(ui.handle_point(note, a));
    let cut = menu_row_rect(&ui, &t!("common-cut"), &ctrl_hint(Key::X)).expect("Cut");
    let clipboard = copied(&ui.click_outputs(cut.center()));
    assert_eq!(clipboard, ["A\na1\na2"]);
    let texts: Vec<String> = ui
        .blocks()
        .iter()
        .map(|block| block.text.text.clone())
        .collect();
    assert_eq!(texts, ["B"]);
    assert!(
        ui.h.query_all_by_label_contains(&select).next().is_none(),
        "the menu closed"
    );
}

/// The caret never ends up in a paragraph a fold hides: ↓ steps over it, Tab and Backspace
/// unfold it, and Enter on a folded paragraph adds the new one after its subparagraphs,
/// which stay folded.
#[test]
fn keys_beside_a_folded_paragraph_keep_the_caret_in_sight() {
    let texts = |ui: &Ui| -> Vec<(String, u8, bool)> {
        ui.blocks()
            .iter()
            .map(|block| (block.text.text.clone(), block.indent, block.collapsed))
            .collect()
    };
    let row = |text: &str, level: u8, folded: bool| (text.to_string(), level, folded);
    let typing = |ui: &Ui| ui.h.ctx.memory(|memory| memory.focused()).is_some();
    // A folded over a1 and a2, then B; the caret ends where `start` puts it.
    let folded = |start: &dyn Fn(&mut Ui, u64, u64, u64)| -> Ui {
        let mut ui = Ui::start();
        ui.new_page("Folded");
        let note = outline_note(&mut ui);
        let (a, b) = (ui.blocks()[0].id, ui.blocks()[3].id);
        ui.click(ui.text_point(note, a, 1));
        ui.press_with(Modifiers::ALT | Modifiers::SHIFT, Key::Minus);
        assert!(ui.blocks()[0].collapsed);
        start(&mut ui, note, a, b);
        ui
    };
    let at_end_of_a = |ui: &mut Ui, _: u64, _: u64, _: u64| ui.press(Key::End);
    let at_start_of_b = |ui: &mut Ui, note: u64, _: u64, b: u64| {
        ui.click(ui.text_point(note, b, 0));
        ui.press(Key::Home);
    };

    // ↓ at the end of A goes to B, past a1 and a2.
    let mut ui = folded(&at_end_of_a);
    ui.press(Key::ArrowDown);
    ui.type_text("Z");
    assert!(typing(&ui));
    assert_eq!(
        texts(&ui),
        [
            row("A", 0, true),
            row("a1", 1, false),
            row("a2", 1, false),
            row("ZB", 0, false)
        ]
    );
    drop(ui);

    // Tab on B puts it under A, which unfolds to show it.
    let mut ui = folded(&at_start_of_b);
    ui.press(Key::Tab);
    ui.type_text("Z");
    assert!(typing(&ui));
    assert_eq!(texts(&ui)[0], row("A", 0, false));
    assert_eq!(texts(&ui)[3], row("ZB", 1, false));
    drop(ui);

    // Backspace at the start of B unfolds A first; the next one joins B to a2.
    let mut ui = folded(&at_start_of_b);
    ui.press(Key::Backspace);
    assert!(typing(&ui));
    assert_eq!(texts(&ui)[0], row("A", 0, false));
    assert_eq!(texts(&ui)[3], row("B", 0, false));
    ui.press(Key::Backspace);
    ui.type_text("Z");
    assert_eq!(texts(&ui)[2], row("a2ZB", 1, false));
    drop(ui);

    // Enter at the end of A adds a paragraph after a2; A stays folded.
    let mut ui = folded(&at_end_of_a);
    ui.press(Key::Enter);
    ui.type_text("new");
    assert!(typing(&ui));
    assert_eq!(
        texts(&ui),
        [
            row("A", 0, true),
            row("a1", 1, false),
            row("a2", 1, false),
            row("new", 0, false),
            row("B", 0, false)
        ]
    );
    drop(ui);

    // Enter at its start opens a line above it; the caret stays in A.
    let mut ui = folded(&|ui: &mut Ui, _: u64, _: u64, _: u64| ui.press(Key::Home));
    ui.press(Key::Enter);
    ui.type_text("Z");
    assert!(typing(&ui));
    assert_eq!(
        texts(&ui)[..3],
        [row("", 0, false), row("ZA", 0, true), row("a1", 1, false)]
    );
}

/// The ribbon's Delete and the Enter key take a selection across paragraphs whole, each as
/// one undo step.
#[test]
fn the_ribbon_delete_and_enter_take_a_selection_across_paragraphs() {
    let mut ui = Ui::start();
    ui.new_page("Range edits");
    ui.press(Key::Enter);
    ui.type_text("one");
    ui.press(Key::Enter);
    ui.type_text("two");
    ui.press(Key::Enter);
    ui.type_text("three");
    let note = ui.items()[0].id;
    let texts = |ui: &Ui| -> Vec<String> {
        ui.blocks()
            .iter()
            .map(|block| block.text.text.clone())
            .collect()
    };
    let ids: Vec<u64> = ui.blocks().iter().map(|block| block.id).collect();
    ui.drag(
        ui.text_point(note, ids[0], 1),
        ui.text_point(note, ids[2], 2),
    );
    assert!(ui.app().editor.range.is_some());
    ui.ribbon_tab("Draw");
    ui.ribbon_button("Delete");
    assert_eq!(texts(&ui), ["oree"]);
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert_eq!(texts(&ui), ["one", "two", "three"]);

    let ids: Vec<u64> = ui.blocks().iter().map(|block| block.id).collect();
    ui.drag(
        ui.text_point(note, ids[0], 1),
        ui.text_point(note, ids[1], 2),
    );
    ui.press(Key::Enter);
    assert_eq!(texts(&ui), ["o", "o", "three"]);
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert_eq!(texts(&ui), ["one", "two", "three"], "one undo step");
}

#[test]
fn dragging_across_paragraphs_selects_them_for_bold_copy_and_typing() {
    let mut ui = Ui::start();
    ui.new_page("Range");
    ui.press(Key::Enter);
    ui.type_text("first line");
    ui.press(Key::Enter);
    ui.type_text("second");
    ui.press(Key::Enter);
    ui.type_text("third line");
    let note = ui.items()[0].id;
    let ids: Vec<u64> = ui.blocks().iter().map(|block| block.id).collect();
    ui.drag(
        ui.text_point(note, ids[0], 6),
        ui.text_point(note, ids[2], 5),
    );
    assert!(
        ui.app().editor.range.is_some(),
        "the drag crossed paragraphs"
    );
    ui.shot("p4-range");

    ui.press_with(Modifiers::COMMAND, Key::B);
    let bold = |ui: &Ui, index: usize, char: usize| {
        let text = &ui.blocks()[index].text;
        text.style_at(text.char_to_byte(char)).bold
    };
    assert!(!bold(&ui, 0, 0) && bold(&ui, 0, 6) && bold(&ui, 1, 0) && bold(&ui, 2, 0));
    assert!(!bold(&ui, 2, 6), "it ends where the drag did");

    ui.h.event(Event::Copy);
    ui.h.step();
    let copied =
        ui.h.output()
            .platform_output
            .commands
            .iter()
            .find_map(|command| match command {
                egui::OutputCommand::CopyText(text) => Some(text.clone()),
                _ => None,
            })
            .expect("copied");
    assert_eq!(copied, "line\nsecond\nthird");

    ui.type_text("X");
    let texts: Vec<String> = ui
        .blocks()
        .iter()
        .map(|block| block.text.text.clone())
        .collect();
    assert_eq!(texts, ["first X line"]);
    assert!(ui.app().editor.range.is_none());
}

#[test]
fn shift_arrows_and_ctrl_a_select_across_paragraphs_and_escape_drops_it() {
    let mut ui = Ui::start();
    ui.new_page("Keys");
    ui.press(Key::Enter);
    ui.type_text("one");
    ui.press(Key::Enter);
    ui.type_text("two");
    let note = ui.items()[0].id;
    let first = ui.blocks()[0].id;
    // At the end of "one", Shift+↓ carries the selection into "two".
    ui.click(ui.text_point(note, first, 3));
    ui.press(Key::End);
    ui.press_with(Modifiers::SHIFT, Key::ArrowDown);
    assert!(ui.app().editor.range.is_some());
    // Escape drops it and stays in the note.
    ui.press(Key::Escape);
    assert!(ui.app().editor.range.is_none());
    assert!(ui.app().canvas.active_text.is_some(), "still typing");
    // Ctrl+A selects the paragraph, a second Ctrl+A the whole note; Delete empties it.
    ui.press_with(Modifiers::COMMAND, Key::A);
    ui.press_with(Modifiers::COMMAND, Key::A);
    assert!(ui.app().editor.range.is_some(), "the whole note");
    ui.press(Key::Delete);
    assert_eq!(ui.blocks().len(), 1);
    assert_eq!(ui.blocks()[0].text.text, "");
}

// P7: File (Backstage) and Options, and the History, Review, View and Help tabs.

/// Clicks the button whose accessible name, without the trailing space a
/// `widgets::menu_row` with no glyph and no shortcut adds, is `name` (the last one drawn:
/// menus come last).
fn click_named(ui: &mut Ui, name: &str) {
    use egui_kittest::kittest::NodeT;
    let rect =
        ui.h.query_all_by_role(egui::Role::Button)
            .rfind(|node| {
                node.accesskit_node()
                    .label()
                    .is_some_and(|label| label.trim() == name)
            })
            .unwrap_or_else(|| panic!("no button named {name:?}"))
            .rect();
    ui.click(rect.center());
}

/// File opens Backstage over the window, as in Office: its left column, Info with the
/// notebooks, and Export's choices. Esc leaves it and the ribbon comes back; Alt+F opens it
/// again.
#[test]
fn file_opens_backstage_and_esc_leaves_it() {
    use super::{BackstagePage, ExportScope};
    use egui::Role;
    use egui_kittest::kittest::NodeT;
    let mut ui = Ui::start();
    let home = t!("ribbon-tab-home").into_owned();
    let ribbon_shown = |ui: &Ui| {
        ui.h.query_all_by_role_and_label(Role::Tab, &home)
            .next()
            .is_some()
    };
    assert!(ribbon_shown(&ui));
    ui.ribbon_tab(&t!("ribbon-tab-file"));
    let page = |ui: &Ui| ui.app().backstage.as_ref().map(|backstage| backstage.page);
    assert_eq!(page(&ui), Some(BackstagePage::Info));
    assert!(!ribbon_shown(&ui), "Backstage covers the ribbon");
    ui.h.get_by_label(&t!("app-info-notebooks"));
    // The left column: ← Back, the five pages (Info selected), Options and Close.
    for each in BackstagePage::ALL {
        let label = each.label();
        let tab = ui.h.get_by_role_and_label(Role::Tab, &label);
        assert_eq!(
            tab.accesskit_node()
                .toggled()
                .map(|toggled| toggled == egui::accesskit::Toggled::True),
            Some(each == BackstagePage::Info),
            "{each:?}"
        );
    }
    for button in [
        t!("app-backstage-back"),
        t!("app-backstage-options"),
        t!("app-backstage-close"),
    ] {
        ui.h.get_by_role_and_label(Role::Button, &button);
    }
    ui.shot("p7-backstage-info");

    // Export: what to export, then the format.
    let export =
        ui.h.get_by_role_and_label(Role::Tab, &t!("app-backstage-export"))
            .rect();
    ui.click(export.center());
    assert_eq!(page(&ui), Some(BackstagePage::Export));
    assert_eq!(
        ui.app().backstage.as_ref().unwrap().export,
        ExportScope::Page,
        "Export starts on the open page"
    );
    let section =
        ui.h.get_by_role_and_label(Role::RadioButton, &t!("app-export-section"))
            .rect();
    ui.click(section.center());
    assert_eq!(
        ui.app().backstage.as_ref().unwrap().export,
        ExportScope::Section
    );
    ui.h.get_by_label_contains(&t!("app-export-html-format"));

    // Esc goes back to the page, with the ribbon.
    ui.press(Key::Escape);
    assert!(ui.app().backstage.is_none());
    assert!(ribbon_shown(&ui), "the ribbon is back");
    assert!(ui.app().page.is_some());

    // Alt+F opens File again, on Info.
    ui.press_with(Modifiers::ALT, Key::F);
    assert_eq!(page(&ui), Some(BackstagePage::Info));
    ui.click_label(&t!("app-backstage-back"));
    assert!(ui.app().backstage.is_none(), "← Back leaves it too");
}

/// Backstage › New makes a notebook named as typed, in the desktop notebook's first colour, opens it
/// and goes back to the page.
#[test]
fn backstage_new_creates_a_notebook() {
    use egui::Role;
    let mut ui = Ui::start();
    let before = ui.app().workspace.notebooks.len();
    ui.ribbon_tab(&t!("ribbon-tab-file"));
    let new =
        ui.h.get_by_role_and_label(Role::Tab, &t!("app-backstage-new"))
            .rect();
    ui.click(new.center());
    assert_eq!(
        ui.app().backstage.as_ref().map(|backstage| backstage.page),
        Some(super::BackstagePage::New)
    );
    ui.steps(2);
    ui.shot("p7-backstage-new");
    // The name field has the keyboard.
    ui.type_text("Travel");
    assert_eq!(ui.app().backstage.as_ref().unwrap().name, "Travel");
    ui.click_label(&t!("dialog-create-notebook"));

    assert!(ui.app().backstage.is_none(), "Backstage closes");
    let notebooks = &ui.app().workspace.notebooks;
    assert_eq!(notebooks.len(), before + 1);
    let travel = notebooks
        .iter()
        .find(|notebook| notebook.name == "Travel")
        .expect("the new notebook");
    assert!(
        travel
            .color
            .eq_ignore_ascii_case(crate::theme::SECTION_COLORS[0]),
        "{}",
        travel.color
    );
    assert_eq!(
        ui.app().selected_notebook.as_deref(),
        Some(travel.id.as_str())
    );
}

/// Backstage › Open offers "Import the desktop notebook…": clicking it closes Backstage and
/// asks for a file or a folder; Esc then closes only that question.
#[test]
fn backstage_open_imports_from_notebook() {
    use egui::Role;
    use egui_kittest::kittest::NodeT;
    let mut ui = Ui::start();
    ui.ribbon_tab(&t!("ribbon-tab-file"));
    let open =
        ui.h.get_by_role_and_label(Role::Tab, &t!("app-backstage-open"))
            .rect();
    ui.click(open.center());
    let label = t!("app-open-import").into_owned();
    let import = |ui: &Ui| {
        let node =
            ui.h.query_all_by_role(Role::Button)
                .find(|node| {
                    node.accesskit_node()
                        .label()
                        .is_some_and(|text| text.ends_with(&label))
                })
                .expect("the import button");
        (node.rect(), node.accesskit_node().is_disabled())
    };
    let (rect, disabled) = import(&ui);
    assert!(!disabled, "the import is ready");
    ui.shot("p7-backstage-open");
    // Every notebook is listed to open.
    for notebook in ui.app().workspace.notebooks.clone() {
        assert!(
            ui.h.query_all_by_role_and_label(Role::Button, &notebook.name)
                .next()
                .is_some(),
            "{} is not listed",
            notebook.name
        );
    }
    let notebooks = ui.app().workspace.notebooks.len();
    ui.click(rect.center());
    assert!(ui.app().backstage.is_none(), "Backstage closes");
    assert!(matches!(
        ui.app().dialog,
        Some(super::Dialog::ImportNotebook(super::ImportDialog::Choose))
    ));
    ui.h.get_by_label(&t!("import-notebook-choose-file"));
    ui.h.get_by_label(&t!("import-notebook-choose-folder"));
    ui.press(Key::Escape);
    assert!(ui.app().dialog.is_none(), "Esc closes the question");
    assert!(ui.app().backstage.is_none(), "and Backstage stays closed");
    assert_eq!(ui.app().workspace.notebooks.len(), notebooks);
}

fn notebook_fixture(path: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/notebooks")
        .join(path)
}

#[test]
fn a_notebook_imports_opens_and_reports() {
    let mut ui = Ui::start();
    ui.h.state_mut()
        .start_notebook_import(notebook_fixture("mpl/handwriting_recognition.one"));
    ui.wait_for_label(&t!("import-notebook-done-title"));
    // The new notebook is open on its first page behind the report.
    let page = ui.h.state().page.as_ref().expect("a page is open").clone();
    assert_eq!(page.title, "Test Page");
    assert_eq!(page.canvas.ink.len(), 62);
    // What came in besides pages is reported; the equation came in as an equation, so it
    // is not among what was left out.
    let pictures = t!("import-notebook-pictures", count = 1i64);
    assert!(
        ui.h.query_by_label_contains(&pictures).is_some(),
        "the picture is reported"
    );
    let math = t!("import-notebook-skipped-math", count = 1i64);
    assert!(ui.h.query_by_label_contains(&math).is_none());
    let equation = |block: &crate::model::Block| matches!(&block.kind, crate::model::BlockKind::Equation { source, .. } if source == "a=b");
    assert!(
        page.canvas
            .items
            .iter()
            .filter_map(|item| item.doc())
            .any(|doc| doc.blocks.iter().any(equation)),
        "an equation the Equation tab edits"
    );
    ui.h.get_by_label(&t!("common-close")).click();
    ui.steps(3);
    assert!(ui
        .h
        .query_by_label(&t!("import-notebook-done-title"))
        .is_none());
}

#[test]
fn a_password_protected_section_is_explained() {
    let mut ui = Ui::start();
    let before = ui.h.state().workspace.notebooks.len();
    ui.h.state_mut()
        .start_notebook_import(notebook_fixture("mit/Encryption.one"));
    ui.wait_for_label(&t!("import-notebook-failed-title"));
    let text = t!("import-notebook-failed-password", name = "Encryption");
    assert!(
        ui.h.query_by_label_contains(&text).is_some(),
        "the refusal says why: a password"
    );
    ui.h.get_by_label(&t!("common-close")).click();
    ui.steps(3);
    assert_eq!(
        ui.h.state().workspace.notebooks.len(),
        before,
        "nothing was added"
    );
}

/// An open Find Tags pane is listed again once an import is written, as after Move or Copy:
/// the new notebook's tags join it at once.
#[test]
fn an_import_lists_the_find_tags_pane_again() {
    use crate::model::{TagMark, TaggedLine};
    let mut ui = Ui::start();
    ui.h.state_mut().load_tags(super::TagsScope::All);
    ui.steps(2);
    // A line the pane still shows for a page deleted since. Listing the pane again drops it;
    // merging only the page the import opens would keep it.
    let stale = TaggedLine {
        page_id: "gone".into(),
        page_title: "Gone".into(),
        section_id: String::new(),
        section_name: String::new(),
        section_color: String::new(),
        notebook_id: String::new(),
        notebook_name: String::new(),
        item: 1,
        block: 1,
        mark: TagMark::Todo,
        text: "stale".into(),
        checked: false,
        order: 0,
        custom: None,
        updated_at: 0,
    };
    ui.h.state_mut()
        .tags_pane
        .as_mut()
        .expect("the Find Tags pane is open")
        .lines
        .push(stale);
    ui.h.state_mut()
        .start_notebook_import(notebook_fixture("mpl/handwriting_recognition.one"));
    ui.wait_for_label(&t!("import-notebook-done-title"));
    let lines = &ui.app().tags_pane.as_ref().unwrap().lines;
    assert!(
        !lines.iter().any(|line| line.page_id == "gone"),
        "the pane was listed again"
    );
    assert!(lines
        .iter()
        .any(|line| line.notebook_name == "handwriting_recognition" && line.text == "ABCDEFGH"));
}

/// An imported notebook lands at the end of the navigation pane, often below the fold: the
/// pane scrolls its open section into view, as the page list does for a new page.
#[test]
fn an_imported_notebook_is_scrolled_into_view() {
    let mut ui = Ui::start();
    // Two imports, so the second surely sits below the pane's bottom edge.
    ui.h.state_mut()
        .start_notebook_import(notebook_fixture("mpl/handwriting_recognition.one"));
    ui.wait_for_label(&t!("import-notebook-done-title"));
    ui.h.get_by_label(&t!("common-close")).click();
    ui.steps(3);
    ui.h.state_mut()
        .start_notebook_import(notebook_fixture("mpl/notebook"));
    ui.wait_for_label(&t!("import-notebook-done-title"));
    ui.h.get_by_label(&t!("common-close")).click();
    ui.steps(3);
    let section = ui
        .app()
        .selected_section
        .clone()
        .expect("a section is open");
    let name = ui
        .app()
        .workspace
        .notebooks
        .iter()
        .flat_map(|notebook| notebook.sections.iter())
        .find(|s| s.id == section)
        .map(|s| s.name.clone())
        .unwrap();
    // Rows below the pane's bottom edge (above Quick notes) are laid out but not seen.
    let bottom = ui.h.ctx.content_rect().bottom() - 40.0;
    let rects: Vec<egui::Rect> = ui.h.get_all_by_label(&name).map(|row| row.rect()).collect();
    assert!(
        rects
            .iter()
            .any(|rect| rect.top() >= 180.0 && rect.bottom() <= bottom),
        "a row of the open section is in view: {rects:?}"
    );
}

#[test]
fn the_notebook_import_reads_in_french() {
    let mut ui = Ui::start_in(Lang::Fr);
    ui.h.state_mut()
        .start_notebook_import(notebook_fixture("mpl/notebook"));
    ui.wait_for_label(&t!("import-notebook-done-title"));
    assert_eq!(t!("import-notebook-done-title"), "Bloc-notes importé");
    let notebook =
        ui.h.state()
            .workspace
            .notebooks
            .iter()
            .find(|n| n.name == "notebook")
            .cloned()
            .unwrap();
    assert_eq!(notebook.groups.len(), 1);
}

/// Options has the desktop notebook's six pages, and what is chosen there is stored and kept when Folio
/// starts again: Display › Tabs layout and Advanced › Default search scope.
#[test]
fn options_has_notebooks_pages_and_settings_stick() {
    use super::{OptionsPage, ScopeKind};
    use crate::ribbon::TabsLayout;
    use egui::Role;
    use egui_kittest::kittest::NodeT;
    let mut ui = Ui::start();
    ui.h.state_mut().dialog = Some(super::Dialog::Settings);
    ui.steps(3);
    let selected = |ui: &Ui, page: OptionsPage| {
        ui.h.get_by_role_and_label(Role::Tab, &page.label())
            .accesskit_node()
            .toggled()
            .map(|toggled| toggled == egui::accesskit::Toggled::True)
    };
    for page in OptionsPage::ALL {
        assert_eq!(
            selected(&ui, page),
            Some(page == OptionsPage::General),
            "{page:?}"
        );
    }
    ui.h.get_by_label(&t!("app-options-personalize"));
    let setting = |ui: &Ui, key: &str| ui.app().library.setting(key).unwrap();

    // Display › Tabs layout: Horizontal Tabs.
    ui.options_page(&t!("app-options-display"));
    assert_eq!(selected(&ui, OptionsPage::Display), Some(true));
    let combo = ui.h.get_by_value(&t!("ribbon-vertical-tabs")).rect();
    ui.click(combo.center());
    ui.click_label(&t!("ribbon-horizontal-tabs"));
    assert_eq!(ui.app().tabs_layout, TabsLayout::Horizontal);
    assert_eq!(setting(&ui, "tabs_layout").as_deref(), Some("horizontal"));
    assert!(ui.app().dialog.is_some(), "Options stays open");
    ui.shot("p7-options-display");

    // Advanced › Default search scope: This Notebook.
    ui.options_page(&t!("app-options-advanced"));
    let combo = ui.h.get_by_value(&t!("app-scope-all")).rect();
    ui.click(combo.center());
    ui.click_label(&t!("app-scope-notebook"));
    assert_eq!(ui.app().search.default_scope, ScopeKind::Notebook);
    assert_eq!(setting(&ui, "search_scope").as_deref(), Some("notebook"));
    ui.click_label(&t!("common-ok"));
    assert!(ui.app().dialog.is_none(), "OK closes Options");

    // Folio starts again with both.
    ui.reopen();
    assert_eq!(ui.app().tabs_layout, TabsLayout::Horizontal);
    assert_eq!(ui.app().search.default_scope, ScopeKind::Notebook);
}

/// Options › Display: pinning the notebooks shows the navigation pane, even one hidden by
/// hand. Cancel unpins them and hides that pane again, as it was when Options opened.
#[test]
fn options_cancel_hides_the_pane_that_pinning_showed() {
    use crate::ribbon::TabsLayout;
    let mut ui = Ui::start();
    ui.h.state_mut().show_library = false;
    ui.steps(2);
    ui.h.state_mut().dialog = Some(super::Dialog::Settings);
    ui.steps(3);
    ui.options_page(&t!("app-options-display"));
    let combo = ui.h.get_by_value(&t!("ribbon-vertical-tabs")).rect();
    ui.click(combo.center());
    ui.click_label(&t!("ribbon-horizontal-tabs"));
    ui.steps(30);
    ui.click_label(&t!("app-options-pin-notebooks"));
    assert!(ui.app().notebooks_pinned);
    assert!(ui.app().show_library, "pinning shows the pane");

    ui.steps(30);
    ui.click_label(&t!("common-cancel"));
    assert!(ui.app().dialog.is_none());
    assert_eq!(ui.app().tabs_layout, TabsLayout::Vertical);
    assert!(!ui.app().notebooks_pinned);
    assert!(!ui.app().show_library, "Cancel hides the pane again");
}

/// Home › Font Color ▾ › More Colors… opens a dialog titled "Font Color", after the desktop notebook's
/// button and dialog.
#[test]
fn more_colors_for_the_font_is_titled_font_color() {
    use super::{ColorTarget, Dialog};
    let mut ui = Ui::start();
    ui.new_page("Colours");
    ui.press(Key::Enter);
    ui.type_text("Notes");
    ui.ribbon_button(&format!("{} ▾", t!("editor-text-color")));
    ui.click_label(&format!(
        "{}    {}",
        crate::icons::PALETTE,
        t!("ribbon-more-colors")
    ));
    assert!(matches!(
        ui.app().dialog,
        Some(Dialog::MoreColors {
            target: ColorTarget::Font,
            ..
        })
    ));
    ui.h.get_by_label("Font Color");
}

/// Options › General › Show Mini Toolbar on selection. On, a mouse selection brings the
/// Mini Toolbar. Turned off, it stays off on the next page (a page change resets the
/// editor, which keeps it) and after a restart: a mouse selection then brings none.
#[test]
fn the_mini_toolbar_option_stays_off_on_other_pages() {
    let mut ui = Ui::start();
    let select = |ui: &mut Ui, title: &str| {
        ui.new_page(title);
        let at = ui.empty_paper_point();
        ui.click(at);
        ui.type_text("Words to select");
        let note = ui.items()[0].id;
        let block = ui.blocks()[0].id;
        ui.drag(ui.text_point(note, block, 6), ui.text_point(note, block, 8));
        assert_eq!(ui.app().editor.selection, Some(6..8), "a word is selected");
    };
    select(&mut ui, "On");
    assert!(ui.app().editor.mini_bar.is_some(), "on by default");

    ui.h.state_mut().dialog = Some(super::Dialog::Settings);
    ui.steps(3);
    ui.click_label(&t!("app-options-mini-toolbar"));
    assert!(!ui.app().editor.mini_on_selection);
    assert!(
        ui.app().editor.mini_bar.is_none(),
        "turning it off puts it away"
    );
    assert_eq!(
        ui.app()
            .library
            .setting("mini_toolbar_on_selection")
            .unwrap()
            .as_deref(),
        Some("false")
    );
    ui.click_label(&t!("common-ok"));
    assert!(ui.app().dialog.is_none());

    select(&mut ui, "Off");
    assert!(
        !ui.app().editor.mini_on_selection,
        "a new page keeps it off"
    );
    assert!(ui.app().editor.mini_bar.is_none(), "no Mini Toolbar");

    ui.reopen();
    assert!(!ui.app().editor.mini_on_selection, "a restart keeps it off");
}

/// History › Recent Edits › Today lists the pages edited today, newest first, in the
/// results pane; a row opens its page.
#[test]
fn recent_edits_list_pages_edited_today() {
    use super::{ResultsKind, TaskPane};
    use crate::ribbon::EditsRange;
    let mut ui = Ui::start();
    let sample = ui.app().page.as_ref().expect("the sample page").id.clone();
    ui.new_page("Trip plans");
    ui.press(Key::Enter);
    ui.type_text("Pack the tent");
    assert!(ui.h.state_mut().flush_save());
    let trip = ui.app().page.as_ref().unwrap().id.clone();
    ui.h.state_mut().open_page(&sample, true);
    ui.steps(2);

    ui.ribbon_tab("History");
    ui.ribbon_button(&t!("ribbon-recent-edits"));
    ui.shot("p7-recent-edits-menu");
    click_named(&mut ui, &t!("ribbon-recent-today"));
    let Some(TaskPane::Results(pane)) = &ui.app().task_pane else {
        panic!("no results pane");
    };
    assert!(matches!(pane.kind, ResultsKind::Edits(EditsRange::Today)));
    assert_eq!(
        pane.hits.first().map(|hit| hit.page_id.as_str()),
        Some(trip.as_str()),
        "the page edited last comes first"
    );
    ui.h.get_by_label(&t!("app-results-edits", range = t!("ribbon-recent-today")));
    ui.shot("p7-recent-edits-pane");

    // Its row opens it; the pane stays.
    let row =
        ui.h.get_all_by_role_and_label(egui::Role::Button, "Trip plans")
            .last()
            .expect("the page's row")
            .rect();
    ui.click(row.center());
    assert_eq!(ui.app().page.as_ref().unwrap().id, trip);
    assert!(matches!(ui.app().task_pane, Some(TaskPane::Results(_))));
}

/// Review › Spelling (F7) opens the Spelling pane on the page's first misspelling, selected
/// in its note; Change puts the suggestion in, the walk ends, and Ctrl+Z takes it back.
#[test]
fn spelling_pane_walks_and_changes_words() {
    use super::TaskPane;
    let mut ui = Ui::start();
    // Fixed language, whatever the machine's locale.
    ui.h.state_mut().editor.spell.set_language("en_US");
    ui.h.state_mut().editor.spell.wait_until_ready();
    ui.new_page("Pets");
    let spot = ui.empty_spot(-200.0, 0.0);
    ui.click(spot);
    ui.type_text("teh cat");
    let note = ui.items()[0].id;
    let block = ui.item(note).doc().unwrap().blocks[0].id;

    ui.press(Key::F7);
    ui.steps(2);
    let hit = match &ui.app().task_pane {
        Some(TaskPane::Spelling(pane)) => pane.current.clone().expect("a misspelling"),
        _ => panic!("the Spelling pane is not open"),
    };
    assert_eq!(hit.word, "teh");
    assert_eq!(
        (hit.item, hit.block, hit.range.clone()),
        (note, block, 0..3)
    );
    assert_eq!(
        ui.app().canvas.active_text,
        Some(note),
        "the note is active"
    );
    assert_eq!(ui.app().editor.active_block, Some(block));
    assert_eq!(
        ui.app().editor.selection,
        Some(0..3),
        "the word is selected"
    );

    // Its suggestions; one picked, then Change.
    let suggestions =
        ui.h.state_mut()
            .editor
            .spell
            .wait_for_suggestions_in("teh", hit.language);
    ui.steps(3);
    let shown: Vec<String> = suggestions.into_iter().take(8).collect();
    let pick = if shown.iter().any(|word| word == "the") {
        "the".to_string()
    } else {
        shown.first().cloned().expect("a suggestion for \"teh\"")
    };
    ui.shot("p7-spelling-pane");
    let row =
        ui.h.get_all_by_role_and_label(egui::Role::Button, &pick)
            .last()
            .expect("the suggestion's row")
            .rect();
    ui.click(row.center());
    // Wait out the double-click time, so Change is a click of its own.
    ui.steps(30);
    ui.click_label(&t!("app-spell-change"));
    let fixed = format!("{pick} cat");
    assert_eq!(text_of(&ui.item(note)), fixed);
    assert!(matches!(
        &ui.app().task_pane,
        Some(TaskPane::Spelling(pane)) if pane.done && pane.current.is_none()
    ));
    ui.h.get_by_label(&t!("app-spell-done"));

    // Ctrl+Z takes the change back.
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert_eq!(text_of(&ui.item(note)), "teh cat");
}

/// View › Switch Background draws the page in the other theme's colours (here, a light
/// window with a dark page) and is remembered.
#[test]
fn switch_background_draws_the_page_in_the_other_theme() {
    let mut ui = Ui::start();
    let ctx = ui.h.ctx.clone();
    ui.h.state_mut()
        .set_appearance(super::Appearance::Light, &ctx);
    ui.steps(3);
    assert!(!crate::theme::tokens(&ui.h.ctx).dark);
    // A rect filled with the dark page colour under the page's middle.
    let dark_page = |ui: &Ui| {
        let fill = crate::theme::Tokens::for_dark(true).raised;
        let middle = ui.app().canvas.viewport().center();
        painted(ui).iter().any(|shape| {
            matches!(shape, egui::Shape::Rect(rect)
                if rect.fill == fill && rect.rect.contains(middle))
        })
    };
    assert!(!dark_page(&ui));

    ui.ribbon_tab("View");
    ui.ribbon_button(&t!("ribbon-switch-background"));
    ui.steps(2);
    assert!(ui.app().switch_background);
    assert_eq!(
        ui.app()
            .library
            .setting("switch_background")
            .unwrap()
            .as_deref(),
        Some("true")
    );
    assert!(dark_page(&ui), "the page is drawn in the dark theme");
    assert!(
        !crate::theme::tokens(&ui.h.ctx).dark,
        "the window keeps its theme"
    );
    ui.shot("view-switch-background");

    // Again: back to the window's colours.
    ui.ribbon_button(&t!("ribbon-switch-background"));
    ui.steps(2);
    assert!(!ui.app().switch_background);
    assert_eq!(
        ui.app()
            .library
            .setting("switch_background")
            .unwrap()
            .as_deref(),
        Some("false")
    );
    assert!(!dark_page(&ui));
}

/// Ctrl+Shift+M (View › New Quick Note) opens a new page in the Quick Notes section, with
/// the caret in its title.
#[test]
fn new_quick_note_opens_a_page_in_quick_notes() {
    let mut ui = Ui::start();
    let quick = ui.app().quick_section().expect("a Quick Notes section");
    let start = ui.app().page.as_ref().map(|page| page.id.clone());

    ui.press_with(Modifiers::COMMAND | Modifiers::SHIFT, Key::M);
    ui.steps(2);
    let page = ui.app().page.as_ref().expect("a page").clone();
    assert_ne!(Some(page.id.clone()), start, "a new page opened");
    assert_eq!(page.section_id, quick);
    assert_eq!(ui.app().selected_section.as_deref(), Some(quick.as_str()));
    let title = crate::editor::title_id(&page);
    assert!(
        ui.h.ctx.memory(|memory| memory.has_focus(title)),
        "the caret is in the title"
    );
    ui.type_text("Groceries");
    assert_eq!(ui.app().page.as_ref().unwrap().title, "Groceries");

    // The View tab's button does the same.
    ui.ribbon_tab("View");
    ui.ribbon_button(&t!("ribbon-new-quick-note"));
    let second = ui.app().page.as_ref().expect("a page").clone();
    assert_ne!(second.id, page.id);
    assert_eq!(second.section_id, quick);
}

/// Help › About Folio shows Folio's version.
#[test]
fn about_shows_the_version() {
    let mut ui = Ui::start();
    ui.ribbon_tab(&t!("ribbon-tab-help"));
    ui.ribbon_button(&t!("ribbon-about"));
    assert!(matches!(ui.app().dialog, Some(super::Dialog::About)));
    ui.h.get_by_label_contains(env!("CARGO_PKG_VERSION"));
    ui.h.get_by_label(&t!("dialog-about-body"));
    ui.shot("p7-about");
    ui.click_label(&t!("common-ok"));
    assert!(ui.app().dialog.is_none());
}

/// Help › Keyboard Shortcuts lists P7's keys, each beside its row's name, and the dialog
/// fits a 1320 × 900 window; its list scrolls to the last rows.
#[test]
fn shortcuts_dialog_lists_p7_keys_and_fits() {
    use crate::util;
    let mut ui = Ui::start();
    ui.h.set_size(egui::vec2(1320.0, 900.0));
    ui.steps(2);
    ui.ribbon_tab(&t!("ribbon-tab-help"));
    ui.ribbon_button(&t!("common-keyboard-shortcuts"));
    assert!(matches!(ui.app().dialog, Some(super::Dialog::Shortcuts)));
    ui.steps(2);
    let window = egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(1320.0, 900.0));
    // The dialogs' modal, as egui last placed it.
    let dialog_rect = |ui: &Ui| {
        ui.h.ctx
            .memory(|memory| memory.area_rect(egui::Id::unique("folio-dialog")))
            .expect("the dialog was drawn")
    };
    let dialog = dialog_rect(&ui);
    assert!(
        window.contains_rect(dialog) && dialog.width() < window.width(),
        "the dialog {dialog:?} fits the window"
    );

    let alt = if cfg!(target_os = "macos") {
        "⌥".to_string()
    } else {
        crate::i18n::modifier_names().alt.to_string()
    };
    let rows = [
        (t!("app-shortcut-file-tab").into_owned(), format!("{alt} F")),
        (t!("app-shortcut-spelling").into_owned(), "F7".to_string()),
        (t!("common-new-section").into_owned(), util::shortcut("T")),
        (
            t!("ribbon-new-quick-note").into_owned(),
            util::shift_shortcut("M"),
        ),
        (t!("app-shortcut-print").into_owned(), util::shortcut("P")),
        (
            t!("app-shortcut-move-page").into_owned(),
            util::alt_shortcut("M"),
        ),
    ];
    // A row is there when its name and its keys are painted on one line, in the dialog.
    let found = |ui: &Ui| -> Vec<String> {
        let texts = texts_in(ui, dialog);
        rows.iter()
            .filter(|(label, keys)| {
                texts.iter().any(|(text, at)| {
                    text == label
                        && texts.iter().any(|(key, rect)| {
                            key == keys
                                && (rect.center().y - at.center().y).abs() < 4.0
                                && rect.left() > at.right()
                                && dialog.contains_rect(*rect)
                        })
                })
            })
            .map(|(label, _)| label.clone())
            .collect()
    };
    let mut seen = found(&ui);
    ui.shot("p7-shortcuts");
    // Scroll the list to its end for the rows further down.
    let over = egui::pos2(dialog.center().x, dialog.center().y);
    ui.h.event(Event::PointerMoved(over));
    ui.h.event(Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -4000.0),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Unknown,
        modifiers: Modifiers::NONE,
    });
    ui.steps(30);
    seen.extend(found(&ui));
    for (label, keys) in &rows {
        assert!(seen.contains(label), "no row {label:?} with {keys:?}");
    }
    let dialog_after = dialog_rect(&ui);
    assert!(window.contains_rect(dialog_after), "{dialog_after:?}");
}

// P7: the right-click menus, the Move or Copy dialog, the search box and Horizontal Tabs.

/// A menu row's name, when `text` is painted as menu rows are: one icon glyph (a
/// private-use character), four spaces, then the name (`widgets::menu_row`, egui's
/// submenu buttons).
fn menu_row_name(text: &str) -> Option<&str> {
    let (glyph, name) = text.split_once("    ")?;
    let mut chars = glyph.chars();
    let first = chars.next()?;
    let icon = ('\u{E000}'..='\u{F8FF}').contains(&first) || u32::from(first) >= 0xF_0000;
    (icon && chars.next().is_none()).then(|| name.trim_end())
}

/// The names of the open menu's rows, top to bottom, as painted in the column of its first
/// row (`first`) and below it. Only menu rows start with a glyph and four spaces, so the
/// navigation pane and the page list painted behind the menu are left out.
fn menu_names(ui: &Ui, first: egui::Rect) -> Vec<String> {
    let column = egui::Rect::from_min_max(
        egui::pos2(first.left(), first.top() - 1.0),
        egui::pos2(first.right(), 10_000.0),
    );
    let mut rows: Vec<(String, f32)> = texts_in(ui, column)
        .into_iter()
        .filter_map(|(text, rect)| menu_row_name(&text).map(|name| (name.to_owned(), rect.top())))
        .collect();
    rows.sort_by(|a, b| a.1.total_cmp(&b.1));
    rows.into_iter().map(|(name, _)| name).collect()
}

/// Whether the open menu's row named `name` is greyed out.
fn menu_row_disabled(ui: &Ui, name: &str) -> bool {
    use egui_kittest::kittest::NodeT;
    let suffix = format!("    {name}");
    ui.h.query_all_by_label_contains(name)
        .find(|node| {
            node.accesskit_node()
                .label()
                .is_some_and(|label| label.trim_end().ends_with(&suffix))
        })
        .unwrap_or_else(|| panic!("no menu row {name:?}"))
        .accesskit_node()
        .is_disabled()
}

/// The open dialog's rect, as egui last placed it.
fn open_dialog_rect(ui: &Ui) -> egui::Rect {
    ui.h.ctx
        .memory(|memory| memory.area_rect(egui::Id::unique("folio-dialog")))
        .expect("a dialog is open")
}

/// The widget named `name` inside the open dialog (the panes behind it have rows with the
/// same names): its rect and whether it is greyed out.
fn in_dialog(ui: &Ui, name: &str) -> Option<(egui::Rect, bool)> {
    use egui_kittest::kittest::NodeT;
    let dialog = open_dialog_rect(ui);
    ui.h.query_all_by_label(name)
        .find(|node| dialog.contains_rect(node.rect()))
        .map(|node| (node.rect(), node.accesskit_node().is_disabled()))
}

/// Clicks the widget named `name` inside the open dialog.
fn click_in_dialog(ui: &mut Ui, name: &str) {
    let (rect, disabled) =
        in_dialog(ui, name).unwrap_or_else(|| panic!("no {name:?} in the dialog"));
    assert!(!disabled, "{name:?} is greyed out");
    ui.click(rect.center());
}

/// The pages of a section, in its order, as the library lists them.
fn section_pages(ui: &Ui, section: &str) -> Vec<String> {
    ui.app()
        .library
        .page_list(crate::db::PageFilter::Section(section), "", 500)
        .unwrap()
        .into_iter()
        .map(|page| page.id)
        .collect()
}

/// A section of the workspace, by id.
fn section_of(ui: &Ui, id: &str) -> Option<crate::model::Section> {
    ui.app()
        .workspace
        .notebooks
        .iter()
        .flat_map(|notebook| notebook.sections.iter())
        .find(|section| section.id == id)
        .cloned()
}

/// A section's right-click menu has the desktop notebook's rows in the desktop notebook's order (reference §5), and
/// Section Color › gives the section one of the desktop notebook's colours, kept in the library.
#[test]
fn section_menu_follows_notebook_and_colours_the_section() {
    use crate::theme::SECTION_COLORS;
    use egui::Role;
    let mut ui = Ui::start();
    let row = ui.h.get_by_role_and_label(Role::Button, "History").rect();
    ui.right_click(row.center());
    let rename = menu_item_rect(&ui, &t!("common-rename")).expect("the section's menu");
    ui.shot("p7-section-menu");
    let expected: Vec<String> = [
        t!("common-rename"),
        t!("app-export-ellipsis"),
        t!("common-delete"),
        t!("app-move-or-copy"),
        t!("app-merge-section"),
        t!("app-copy-link-section"),
        t!("app-menu-new-section"),
        t!("app-menu-new-section-group"),
        t!("app-section-color"),
    ]
    .iter()
    .map(|name| name.to_string())
    .collect();
    assert_eq!(menu_names(&ui, rename), expected);
    // What P2's menu had and the desktop notebook's has not.
    assert!(
        ui.h.query_all_by_label_contains("Move to group")
            .next()
            .is_none(),
        "Move or Copy… replaces Move to group"
    );

    // Section Color › Green: the desktop notebook's third colour.
    let color = |ui: &Ui| section_of(ui, "section-classes").expect("History").color;
    assert!(!color(&ui).eq_ignore_ascii_case(SECTION_COLORS[2]));
    ui.click_label_containing(&format!("    {}", t!("app-section-color")));
    ui.shot("p7-section-color-menu");
    let green = crate::theme::color_name(SECTION_COLORS[2]);
    assert_eq!(green, t!("app-section-color-green"));
    // All sixteen colours and None are offered.
    for hex in SECTION_COLORS {
        let name = crate::theme::color_name(hex);
        assert!(
            ui.h.query_all_by_role_and_label(Role::Button, &name)
                .next()
                .is_some(),
            "no {name:?} row"
        );
    }
    ui.h.get_by_role_and_label(Role::Button, &t!("app-color-none"));
    let swatch =
        ui.h.get_all_by_role_and_label(Role::Button, &green)
            .last()
            .expect("the Green row")
            .rect();
    ui.click(swatch.center());
    assert!(
        color(&ui).eq_ignore_ascii_case(SECTION_COLORS[2]),
        "{}",
        color(&ui)
    );
    assert!(!egui::Popup::is_any_open(&ui.h.ctx), "the menu closed");
    let stored = ui
        .app()
        .library
        .workspace()
        .unwrap()
        .notebooks
        .into_iter()
        .flat_map(|notebook| notebook.sections)
        .find(|section| section.id == "section-classes")
        .unwrap();
    assert!(stored.color.eq_ignore_ascii_case(SECTION_COLORS[2]));
}

/// A notebook's Move Up and Move Down (greyed for the first and the last) change its place
/// among the notebooks, in the pane and in the library.
#[test]
fn notebooks_move_up_and_down_from_their_menu() {
    use egui::Role;
    let mut ui = Ui::start();
    let order = |ui: &Ui| -> Vec<String> {
        ui.app()
            .workspace
            .notebooks
            .iter()
            .map(|notebook| notebook.name.clone())
            .collect()
    };
    assert_eq!(order(&ui), ["School", "Personal"]);
    // A third notebook, so one can move between two others.
    {
        let app = ui.h.state_mut();
        app.library.create_notebook("Clubs", "#8764b8").unwrap();
        app.refresh_workspace();
    }
    ui.steps(2);
    assert_eq!(order(&ui), ["School", "Personal", "Clubs"]);
    let row = |ui: &Ui, name: &str| ui.h.get_by_role_and_label(Role::Button, name).rect();
    let (up, down) = (t!("app-move-up"), t!("app-move-down"));

    // The last notebook can't go further down; Move Up puts it above Personal.
    ui.right_click(row(&ui, "Clubs").center());
    assert!(menu_row_disabled(&ui, &down));
    assert!(!menu_row_disabled(&ui, &up));
    click_menu_row(&mut ui, &up, "");
    assert_eq!(order(&ui), ["School", "Clubs", "Personal"]);

    // The first can't go further up; Move Down puts it below Clubs.
    ui.right_click(row(&ui, "School").center());
    assert!(menu_row_disabled(&ui, &up));
    assert!(!menu_row_disabled(&ui, &down));
    click_menu_row(&mut ui, &down, "");
    let moved = ["Clubs", "School", "Personal"];
    assert_eq!(order(&ui), moved);
    ui.shot("p7-notebooks-moved");

    // The pane lists them so, and Folio started again too.
    let tops: Vec<f32> = ["Clubs", "School", "Personal"]
        .iter()
        .map(|name| row(&ui, name).top())
        .collect();
    assert!(tops.windows(2).all(|pair| pair[0] < pair[1]), "{tops:?}");
    ui.reopen();
    assert_eq!(order(&ui), moved);
}

/// A page row's Copy, then Paste on another row, puts a copy of the page right after that
/// row; Cut, then Paste in another section's list (its empty part), moves the page there.
#[test]
fn pages_cut_copy_and_paste_in_the_page_list() {
    use egui::Role;
    let mut ui = Ui::start();
    for title in ["Alpha", "Beta", "Gamma"] {
        ui.new_page(title);
        save(&mut ui);
    }
    let names =
        |ui: &Ui| -> Vec<String> { titles(ui).into_iter().map(|(title, _)| title).collect() };
    let row = |ui: &Ui, title: &str| ui.h.get_by_role_and_label(Role::ListItem, title).rect();
    let alpha = page_id(&ui, "Alpha");

    // Copy Alpha; Paste on Beta: a copy of Alpha comes right after Beta.
    ui.right_click(row(&ui, "Alpha").center());
    click_menu_row(&mut ui, &t!("common-copy"), "");
    ui.right_click(row(&ui, "Beta").center());
    ui.shot("p7-page-paste-menu");
    click_menu_row(&mut ui, &t!("common-paste"), "");
    let list = names(&ui);
    let beta = list.iter().position(|title| title == "Beta").unwrap();
    assert_eq!(list[beta + 1], "Alpha", "{list:?}");
    assert_eq!(list[beta + 2], "Gamma", "{list:?}");
    assert_eq!(list.iter().filter(|title| *title == "Alpha").count(), 2);
    let copy = ui.app().pages[beta + 1].id.clone();
    assert_ne!(copy, alpha, "a new page");
    assert!(
        matches!(&ui.app().page_clip, Some(clip) if clip.id == alpha && !clip.cut),
        "a copy can be pasted again"
    );

    // Cut Gamma; it stays listed until it is pasted.
    let gamma = page_id(&ui, "Gamma");
    ui.right_click(row(&ui, "Gamma").center());
    click_menu_row(&mut ui, &t!("common-cut"), "");
    assert!(shows_row(&ui, "Gamma"));

    // Another section's list: Paste in its empty part puts the page at its end.
    let history = ui.h.get_by_role_and_label(Role::Button, "History").rect();
    ui.click(history.center());
    assert_eq!(
        ui.app().selected_section.as_deref(),
        Some("section-classes")
    );
    assert!(!shows_row(&ui, "Gamma"));
    let last =
        ui.h.get_all_by_role(Role::ListItem)
            .map(|node| node.rect())
            .max_by(|a, b| a.bottom().total_cmp(&b.bottom()))
            .expect("History has a page");
    ui.right_click(last.center_bottom() + egui::vec2(0.0, 80.0));
    ui.shot("p7-page-area-menu");
    assert!(menu_item_rect(&ui, &t!("app-menu-new-page")).is_some());
    click_menu_row(&mut ui, &t!("common-paste"), "");
    assert_eq!(names(&ui).last().map(String::as_str), Some("Gamma"));
    assert_eq!(
        ui.app().library.page_section_id(&gamma).unwrap().as_deref(),
        Some("section-classes")
    );
    assert!(ui.app().page_clip.is_none(), "a cut page pastes once");
    assert!(!section_pages(&ui, "section-this-week").contains(&gamma));
}

/// The Move or Copy dialog: its tree narrows to the names typed in its search field; Move
/// puts a page in the section picked; Copy puts a copy of a section, with its pages, in
/// another notebook.
#[test]
fn move_or_copy_moves_a_page_and_copies_a_section() {
    use super::{Dialog, MoveWhat};
    use egui::Role;
    let mut ui = Ui::start();
    ui.new_page("Trip");
    save(&mut ui);
    let trip = page_id(&ui, "Trip");
    ui.h.state_mut().open_page(&trip, false);
    ui.steps(2);

    // Ctrl+Alt+M: Move or Copy the open page.
    ui.press_with(Modifiers::COMMAND | Modifiers::ALT, Key::M);
    assert!(
        matches!(&ui.app().dialog,
            Some(Dialog::MoveOrCopy { what: MoveWhat::Page(id), .. }) if *id == trip),
        "the dialog, for Trip"
    );
    ui.steps(2);
    let (science, _) = in_dialog(&ui, "Science").expect("School's sections");
    // Personal's sections are further down the tree.
    ui.wheel(science.center(), egui::vec2(0.0, -1000.0), Modifiers::NONE);
    assert!(in_dialog(&ui, "Journal").is_some());
    assert!(in_dialog(&ui, "Recipes").is_some());
    let (_, greyed) = in_dialog(&ui, &t!("dialog-move")).expect("Move");
    assert!(greyed, "Move waits for a section");
    // The search field has the keyboard: the tree keeps the names that match.
    ui.type_text("recip");
    ui.steps(2);
    assert!(
        matches!(&ui.app().dialog, Some(Dialog::MoveOrCopy { filter, .. }) if filter == "recip")
    );
    // (Journal sits beside Recipes, in view however the tree was scrolled.)
    assert!(
        in_dialog(&ui, "Journal").is_none(),
        "Journal is filtered out"
    );
    assert!(
        in_dialog(&ui, "Science").is_none(),
        "Science is filtered out"
    );
    assert!(in_dialog(&ui, "School").is_none());
    assert!(in_dialog(&ui, "Personal").is_some(), "Recipes' notebook");
    ui.shot("p7-move-or-copy-page");
    click_in_dialog(&mut ui, "Recipes");
    click_in_dialog(&mut ui, &t!("dialog-move"));
    assert!(ui.app().dialog.is_none());
    assert_eq!(
        ui.app().library.page_section_id(&trip).unwrap().as_deref(),
        Some("section-kitchen")
    );

    // A section's Move or Copy…: Copy into another notebook.
    let history = ui.h.get_by_role_and_label(Role::Button, "History").rect();
    ui.right_click(history.center());
    click_menu_row(&mut ui, &t!("app-move-or-copy"), "");
    assert!(
        matches!(&ui.app().dialog,
            Some(Dialog::MoveOrCopy { what: MoveWhat::Section(id), .. }) if id == "section-classes"),
        "the dialog, for History"
    );
    ui.steps(2);
    // Its own notebook's top level is where it already is.
    let (school, greyed) = in_dialog(&ui, "School").expect("School");
    assert!(greyed);
    // Personal is further down the tree.
    ui.wheel(school.center(), egui::vec2(0.0, -1000.0), Modifiers::NONE);
    click_in_dialog(&mut ui, "Personal");
    ui.shot("p7-move-or-copy-section");
    click_in_dialog(&mut ui, &t!("dialog-copy"));
    assert!(ui.app().dialog.is_none());
    let personal = ui
        .app()
        .workspace
        .notebooks
        .iter()
        .find(|notebook| notebook.id == "notebook-personal")
        .unwrap()
        .clone();
    let copy = personal
        .sections
        .iter()
        .find(|section| section.name == "History")
        .expect("a copy of History in Personal");
    assert_ne!(copy.id, "section-classes");
    assert_eq!(
        section_of(&ui, "section-classes")
            .expect("History stays")
            .notebook_id,
        "notebook-school"
    );
    let titles_in = |ui: &Ui, section: &str| -> Vec<String> {
        ui.app()
            .library
            .page_list(crate::db::PageFilter::Section(section), "", 500)
            .unwrap()
            .into_iter()
            .map(|page| page.title)
            .collect()
    };
    assert_eq!(
        titles_in(&ui, &copy.id),
        titles_in(&ui, "section-classes"),
        "the copy has the section's pages"
    );
    assert!(!section_pages(&ui, &copy.id).is_empty());
}

/// Merge into Another Section…: after the dialog asks once more, the section is gone and
/// the section picked holds its pages, and opens.
#[test]
fn merging_a_section_moves_its_pages() {
    use super::{Dialog, MoveWhat};
    use egui::Role;
    let mut ui = Ui::start();
    let maths = section_pages(&ui, "section-maths");
    let science = section_pages(&ui, "section-science");
    assert!(!maths.is_empty() && !science.is_empty());
    let row = ui.h.get_by_role_and_label(Role::Button, "Maths").rect();
    ui.right_click(row.center());
    click_menu_row(&mut ui, &t!("app-merge-section"), "");
    assert!(
        matches!(&ui.app().dialog,
            Some(Dialog::MoveOrCopy { what: MoveWhat::Merge(id), confirm: false, .. })
                if id == "section-maths"),
        "the Merge dialog, for Maths"
    );
    ui.steps(2);
    // A section never merges into itself.
    let (_, greyed) = in_dialog(&ui, "Maths").expect("Maths");
    assert!(greyed);
    click_in_dialog(&mut ui, "Science");
    click_in_dialog(&mut ui, &t!("dialog-merge"));

    // Merge asks once more, naming both sections; nothing has moved yet.
    assert!(matches!(
        &ui.app().dialog,
        Some(Dialog::MoveOrCopy { confirm: true, .. })
    ));
    let warning = t!(
        "dialog-merge-confirm",
        name = "Maths".to_string(),
        target = "Science".to_string()
    );
    assert!(in_dialog(&ui, &warning).is_some(), "the warning");
    assert_eq!(section_pages(&ui, "section-maths"), maths);
    ui.shot("p7-merge-confirm");
    click_in_dialog(&mut ui, &t!("dialog-merge"));

    assert!(ui.app().dialog.is_none());
    assert!(section_of(&ui, "section-maths").is_none(), "Maths is gone");
    let merged = section_pages(&ui, "section-science");
    assert_eq!(merged.len(), science.len() + maths.len());
    for id in science.iter().chain(&maths) {
        assert!(merged.contains(id), "{id} is not in Science");
    }
    assert_eq!(
        ui.app().selected_section.as_deref(),
        Some("section-science")
    );
}

/// Copy Link to Section copies a folio://section/ link, which opens the section; a link to
/// a section that is gone says so.
#[test]
fn copy_link_to_section_opens_it() {
    use egui::Role;
    let mut ui = Ui::start();
    assert_eq!(
        ui.app().selected_section.as_deref(),
        Some("section-this-week")
    );
    let row = ui.h.get_by_role_and_label(Role::Button, "History").rect();
    ui.right_click(row.center());
    let copy = menu_row_rect(&ui, &t!("app-copy-link-section"), "").expect("Copy Link to Section");
    let copied = copied(&ui.click_outputs(copy.center()));
    let [link] = copied.as_slice() else {
        panic!("one link copied: {copied:?}");
    };
    assert!(link.starts_with("folio://section/"), "{link}");
    assert!(link.starts_with(crate::model::SECTION_LINK_PREFIX));
    assert_eq!(*link, crate::model::section_link("section-classes"));
    assert_eq!(
        ui.app().selected_section.as_deref(),
        Some("section-this-week"),
        "copying the link opens nothing"
    );

    ui.h.state_mut().follow_link(link);
    ui.steps(2);
    assert_eq!(
        ui.app().selected_section.as_deref(),
        Some("section-classes")
    );
    assert_eq!(
        ui.app().page.as_ref().map(|page| page.section_id.as_str()),
        Some("section-classes"),
        "its first page opens"
    );

    ui.h.state_mut()
        .follow_link(&crate::model::section_link("section-gone"));
    ui.steps(1);
    assert_eq!(ui.app().toast.message, t!("error-section-gone"));
    assert!(ui.app().toast.error);
    assert_eq!(
        ui.app().selected_section.as_deref(),
        Some("section-classes")
    );
}

/// A note's menu (on its grab bar) has the desktop notebook's Cut, Copy, Paste and Delete, then Order ›,
/// and no Duplicate; Order › Bring Forward raises the lower of two overlapping shapes
/// above the other.
#[test]
fn item_menus_follow_notebook_and_order_works() {
    use crate::canvas::{PAGE_CONTENT_TOP, PAGE_LEFT};
    let mut ui = Ui::start();
    ui.new_page("Order");
    let spot = ui.empty_paper_point();
    ui.click(spot);
    ui.type_text("Order me");
    let note = ui.items()[0].id;
    ui.right_click(ui.bar(note));
    ui.shot("p7-note-menu");
    let tops: Vec<f32> = [
        (t!("common-cut"), ctrl_hint(Key::X)),
        (t!("common-copy"), ctrl_hint(Key::C)),
        (t!("common-paste"), ctrl_hint(Key::V)),
        (t!("common-delete"), t!("key-del").into_owned()),
    ]
    .iter()
    .map(|(name, hint)| {
        menu_row_rect(&ui, name, hint)
            .unwrap_or_else(|| panic!("no {name:?} row"))
            .top()
    })
    .collect();
    assert!(tops.windows(2).all(|pair| pair[0] < pair[1]), "{tops:?}");
    let order =
        ui.h.get_all_by_label_contains(&format!("    {}", t!("canvas-menu-order")))
            .last()
            .expect("Order ›")
            .rect();
    assert!(order.top() > tops[3], "Order › comes after Delete");
    assert!(
        ui.h.query_all_by_label_contains("Duplicate")
            .next()
            .is_none(),
        "the desktop notebook has Copy and Paste, not Duplicate"
    );
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    assert!(!egui::Popup::is_any_open(&ui.h.ctx));

    // Two rectangles, the second over the first.
    ui.ribbon_tab("Draw");
    let mut shapes = Vec::new();
    for step in 0..2 {
        ui.ribbon_button("Rectangle");
        let corner = egui::pos2(
            PAGE_LEFT + 40.0 * step as f32,
            PAGE_CONTENT_TOP + 60.0 + 30.0 * step as f32,
        );
        ui.drag(
            ui.screen(corner),
            ui.screen(corner + egui::vec2(120.0, 80.0)),
        );
        shapes.push(
            ui.app()
                .canvas
                .selection
                .primary()
                .expect("the rectangle drawn is selected"),
        );
    }
    let z = |ui: &Ui, id: u64| ui.item(id).z;
    assert!(z(&ui, shapes[0]) < z(&ui, shapes[1]));
    // The lower one, by its corner clear of the other: Order › Bring Forward.
    let corner = ui.screen(egui::pos2(PAGE_LEFT + 15.0, PAGE_CONTENT_TOP + 75.0));
    ui.click(corner);
    assert_eq!(ui.selection().items, vec![shapes[0]]);
    ui.right_click(corner);
    ui.click_label_containing(&format!("    {}", t!("canvas-menu-order")));
    ui.shot("p7-order-menu");
    click_menu_row(&mut ui, &t!("ribbon-bring-forward"), "");
    assert!(
        z(&ui, shapes[0]) > z(&ui, shapes[1]),
        "Bring Forward raised it above the other"
    );
    assert_eq!(ui.selection().items, vec![shapes[0]]);
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert!(z(&ui, shapes[0]) < z(&ui, shapes[1]), "one undo step");
}

/// The text menu opens with the desktop notebook's mini toolbar above Cut and Copy; its Bold bolds the
/// selection as the ribbon's does, and the caret stays in the note.
#[test]
fn the_text_menu_has_the_mini_toolbar() {
    use egui::Role;
    let mut ui = Ui::start();
    ui.new_page("Mini");
    ui.press(Key::Enter);
    ui.type_text("Hello world");
    ui.press_with(Modifiers::COMMAND, Key::A);
    let note = ui.note_with("Hello world").id;
    let spot = ui.text_spot(&ui.item(note));
    ui.right_click(spot);
    ui.shot("p7-text-menu");
    // The toolbar's controls are named as their ribbon twins are: the ones under the
    // ribbon are the toolbar's.
    let ribbon_bottom = 48.0 + crate::ribbon::panel_height(&ui.app().ribbon) + 4.0;
    let control = |ui: &Ui, name: &str| {
        ui.h.get_all_by_role_and_label(Role::Button, name)
            .map(|node| node.rect())
            .find(|rect| rect.top() > ribbon_bottom)
            .unwrap_or_else(|| panic!("no {name:?} in the mini toolbar"))
    };
    for name in [
        t!("editor-font"),
        t!("editor-font-size"),
        t!("editor-italic"),
        t!("editor-underline"),
        t!("editor-highlight"),
        t!("editor-text-color"),
        t!("ribbon-bullets"),
        t!("ribbon-numbering"),
        t!("ribbon-group-styles"),
        t!("editor-tag"),
    ] {
        control(&ui, &name);
    }
    let bold = control(&ui, &t!("editor-bold"));
    let row = |ui: &Ui, name: &str| {
        ui.h.get_all_by_label_contains(&format!("    {name}"))
            .last()
            .unwrap_or_else(|| panic!("no {name:?} row"))
            .rect()
    };
    let (cut, copy) = (row(&ui, &t!("common-cut")), row(&ui, &t!("common-copy")));
    assert!(
        bold.bottom() <= cut.top() && cut.top() < copy.top(),
        "Bold {bold:?} sits above Cut {cut:?} and Copy {copy:?}"
    );
    // Select All, and the Bold row, are gone: the toolbar has Bold, Ctrl+A selects.
    assert!(ui
        .h
        .query_all_by_label_contains(&format!("    {}", t!("editor-bold")))
        .next()
        .is_none());

    ui.click(bold.center());
    let text = ui.item(note).doc().unwrap().blocks[0].text.clone();
    assert!(
        text.style_at(0).bold && text.style_at(10).bold,
        "the whole selection is bold"
    );
    assert!(!egui::Popup::is_any_open(&ui.h.ctx), "the menu closed");
    assert_eq!(
        ui.app().canvas.active_text,
        Some(note),
        "the note is still being typed in"
    );
    ui.press(Key::End);
    ui.type_text("!");
    assert_eq!(text_of(&ui.item(note)), "Hello world!");
}

/// The hits listed in the search dropdown, among `titles`.
fn dropdown_rows(ui: &Ui, titles: &[&str]) -> Vec<String> {
    let dropdown = ui.app().search.dropdown_rect.expect("the dropdown");
    titles
        .iter()
        .filter(|title| {
            ui.h.query_all_by_role_and_label(egui::Role::Button, title)
                .any(|node| dropdown.contains_rect(node.rect()))
        })
        .map(|title| title.to_string())
        .collect()
}

/// The header search looks in All Notebooks; its scope menu narrows it (This Notebook),
/// the results open in the docked pane, and Set This Scope as Default keeps the scope for
/// the searches to come.
#[test]
fn search_scopes_narrow_the_results() {
    use super::{ResultsKind, ScopeKind, TaskPane};
    let mut ui = Ui::start();
    // "comet" in a page of Personal and one of School, where the search starts.
    ui.h.state_mut().select_section("section-journal", true);
    ui.steps(2);
    ui.new_page("Comet sightings");
    ui.press(Key::Enter);
    ui.type_text("A comet over the field");
    save(&mut ui);
    ui.h.state_mut().select_section("section-this-week", true);
    ui.steps(2);
    ui.new_page("Comet project");
    ui.press(Key::Enter);
    ui.type_text("Draw the comet's tail");
    save(&mut ui);
    let school_page = ui.app().page.as_ref().unwrap().id.clone();
    assert_eq!(
        ui.app().selected_notebook.as_deref(),
        Some("notebook-school")
    );
    let both = ["Comet sightings", "Comet project"];

    ui.press_with(Modifiers::COMMAND, Key::E);
    assert!(ui.app().search.open);
    assert_eq!(ui.app().search.scope, ScopeKind::All);
    ui.type_text("comet");
    ui.steps(2);
    assert_eq!(ui.app().search.results.len(), 2);
    assert_eq!(dropdown_rows(&ui, &both), both);

    // Scope › This Notebook: School's page alone.
    ui.click_label(&t!("app-search-scope"));
    ui.shot("p7-search-scope-menu");
    click_named(&mut ui, &t!("app-scope-notebook"));
    ui.steps(2);
    assert!(ui.app().search.open, "the search stays open");
    assert_eq!(ui.app().search.scope, ScopeKind::Notebook);
    assert_eq!(ui.app().search.results.len(), 1);
    assert_eq!(dropdown_rows(&ui, &both), ["Comet project"]);
    ui.shot("p7-search-this-notebook");

    // Open in the search results pane: the same hit, docked beside the page.
    ui.click_label_containing(&t!("app-search-open-pane"));
    assert!(!ui.app().search.open);
    match &ui.app().task_pane {
        Some(TaskPane::Results(pane)) => {
            assert!(
                matches!(&pane.kind, ResultsKind::Search { query, scope: ScopeKind::Notebook }
                    if query == "comet"),
                "a search for \"comet\" in This Notebook"
            );
            let hits: Vec<&str> = pane.hits.iter().map(|hit| hit.page_id.as_str()).collect();
            assert_eq!(hits, [school_page.as_str()]);
        }
        _ => panic!("no results pane"),
    }

    // The next search starts in All Notebooks again, until a scope is set as default.
    ui.press_with(Modifiers::COMMAND, Key::E);
    assert_eq!(ui.app().search.scope, ScopeKind::All);
    ui.type_text("comet");
    ui.steps(2);
    assert_eq!(ui.app().search.results.len(), 2);
    ui.click_label(&t!("app-search-scope"));
    click_named(&mut ui, &t!("app-scope-notebook"));
    ui.click_label(&t!("app-search-scope"));
    click_named(&mut ui, &t!("app-scope-set-default"));
    assert_eq!(ui.app().search.default_scope, ScopeKind::Notebook);
    assert_eq!(
        ui.app().library.setting("search_scope").unwrap().as_deref(),
        Some("notebook")
    );
    ui.press(Key::Escape);
    assert!(!ui.app().search.open);
    ui.press_with(Modifiers::COMMAND, Key::E);
    assert_eq!(ui.app().search.scope, ScopeKind::Notebook);
}

/// Scope › This Page closes the search and finds the words typed on the open page, as
/// Ctrl+F does.
#[test]
fn this_page_scope_opens_find() {
    let mut ui = Ui::start();
    ui.new_page("Zoo");
    ui.press(Key::Enter);
    ui.type_text("The zebra sleeps");
    save(&mut ui);
    ui.press_with(Modifiers::COMMAND, Key::E);
    assert!(ui.app().search.open);
    ui.type_text("zebra");
    ui.steps(2);
    ui.click_label(&t!("app-search-scope"));
    click_named(&mut ui, &t!("app-scope-page"));
    assert!(!ui.app().search.open, "the search closed");
    let find = ui.app().canvas.find.as_ref().expect("find on page");
    assert_eq!(find.query, "zebra");
    assert_eq!(find.matches.len(), 1);
    assert_eq!(ui.app().page.as_ref().unwrap().title, "Zoo");
}

/// View › Tabs Layout › Horizontal Tabs.
fn horizontal_tabs(ui: &mut Ui) {
    ui.ribbon_tab(&t!("ribbon-tab-view"));
    ui.ribbon_button(&t!("ribbon-tabs-layout"));
    click_named(ui, &t!("ribbon-horizontal-tabs"));
    assert_eq!(ui.app().tabs_layout, crate::ribbon::TabsLayout::Horizontal);
    ui.steps(2);
}

/// The section tabs of Horizontal Tabs: every `Role::Tab` under the ribbon, by name.
fn section_tab_rects(ui: &Ui) -> Vec<(String, egui::Rect)> {
    use egui_kittest::kittest::NodeT;
    let ribbon_bottom = 48.0 + crate::ribbon::panel_height(&ui.app().ribbon) + 4.0;
    ui.h.query_all_by_role(egui::Role::Tab)
        .filter(|node| node.rect().top() > ribbon_bottom)
        .map(|node| {
            (
                node.accesskit_node().label().unwrap_or_default(),
                node.rect(),
            )
        })
        .collect()
}

/// The tabs Horizontal Tabs shows at `notebook`'s top level, by name: its sections, then
/// its section groups.
fn top_level_tabs(notebook: &crate::model::Notebook) -> Vec<String> {
    let sections = notebook
        .sections
        .iter()
        .filter(|section| section.group_id.is_none())
        .map(|section| section.name.clone());
    let groups = notebook
        .groups
        .iter()
        .filter(|group| group.parent_id.is_none())
        .map(|group| group.name.clone());
    sections.chain(groups).collect()
}

/// Horizontal Tabs: the open notebook's sections run as tabs across the top of the page
/// and a click opens one; the page list sits right of the page; the notebooks are a pane
/// only once pinned from the notebook picker.
#[test]
fn horizontal_tabs_show_sections_across_the_top() {
    use egui::Role;
    use egui_kittest::kittest::NodeT;
    let mut ui = Ui::start();
    horizontal_tabs(&mut ui);
    ui.shot("p7-horizontal");
    let school = ui
        .app()
        .workspace
        .notebooks
        .iter()
        .find(|notebook| notebook.id == "notebook-school")
        .unwrap()
        .clone();
    let tabs = section_tab_rects(&ui);
    let names: Vec<&str> = tabs.iter().map(|(name, _)| name.as_str()).collect();
    // Its sections and its section group ("Last year"), not the sections inside the group.
    let expected = top_level_tabs(&school);
    assert!(expected.contains(&"Last year".to_string()), "{expected:?}");
    for name in &expected {
        assert!(
            names.contains(&name.as_str()),
            "no tab for {name:?}: {names:?}"
        );
    }
    assert!(!names.contains(&"Geography"), "{names:?}");
    let selected = |ui: &Ui, name: &str| {
        let ribbon_bottom = 48.0 + crate::ribbon::panel_height(&ui.app().ribbon) + 4.0;
        ui.h.get_all_by_role_and_label(Role::Tab, name)
            .find(|node| node.rect().top() > ribbon_bottom)
            .unwrap_or_else(|| panic!("no tab {name:?}"))
            .accesskit_node()
            .toggled()
            .map(|toggled| toggled == egui::accesskit::Toggled::True)
    };
    assert_eq!(selected(&ui, "This week"), Some(true));
    assert_eq!(selected(&ui, "History"), Some(false));
    let history = tabs.iter().find(|(name, _)| name == "History").unwrap().1;
    ui.click(history.center());
    assert_eq!(
        ui.app().selected_section.as_deref(),
        Some("section-classes")
    );
    assert_eq!(selected(&ui, "History"), Some(true));
    assert_eq!(selected(&ui, "This week"), Some(false));

    // The page list is on the right of the page.
    let page = ui.app().canvas.viewport();
    let rows: Vec<egui::Rect> =
        ui.h.query_all_by_role(Role::ListItem)
            .map(|node| node.rect())
            .collect();
    assert!(!rows.is_empty());
    let add =
        ui.h.get_by_role_and_label(Role::Button, &t!("app-add-page"))
            .rect();
    for rect in rows.iter().chain([&add]) {
        assert!(
            rect.left() >= page.right(),
            "{rect:?} is not right of the page {page:?}"
        );
    }

    // No notebook pane; the picker names the notebook.
    assert_eq!(ui.app().nav_width, 0.0, "no folio-nav panel");
    assert!(ui
        .h
        .query_all_by_role_and_label(Role::Button, "Personal")
        .next()
        .is_none());
    let picker =
        ui.h.get_by_role_and_label(Role::Button, &t!("app-notebook-picker"))
            .rect();
    assert!(
        texts_in(&ui, picker)
            .iter()
            .any(|(text, _)| text == "School"),
        "the picker shows the notebook"
    );
    // Its list, then its pin: the notebooks become a pane on the left.
    ui.click(picker.center());
    ui.shot("p7-horizontal-picker");
    assert!(menu_item_rect(&ui, "Personal").is_some());
    ui.click_label(&t!("app-pin-notebooks"));
    assert!(ui.app().notebooks_pinned);
    assert!(ui.app().nav_width > 0.0, "the folio-nav panel shows");
    ui.h.get_by_role_and_label(Role::Button, "Personal");
    ui.h.get_by_role_and_label(Role::Button, &t!("app-unpin-notebooks"));
    assert!(
        ui.h.query_all_by_role_and_label(Role::Button, &t!("app-notebook-picker"))
            .next()
            .is_none(),
        "the pane stands in for the picker"
    );
    assert_eq!(
        ui.app()
            .library
            .setting("notebook_pane_pinned")
            .unwrap()
            .as_deref(),
        Some("true")
    );
    ui.shot("p7-horizontal-pinned");
    ui.click_label(&t!("app-unpin-notebooks"));
    assert!(!ui.app().notebooks_pinned);
    assert_eq!(ui.app().nav_width, 0.0);
}

/// Horizontal Tabs follow the open section when it moves into a section group: its tab
/// shows among the group's, not gone from the old level while its pages still show.
#[test]
fn horizontal_tabs_follow_the_open_section_into_a_group() {
    let mut ui = Ui::start();
    horizontal_tabs(&mut ui);
    let section = ui.app().selected_section.clone().expect("an open section");
    let name = ui.app().find_section(&section).expect("its name").1;
    let notebook = ui
        .app()
        .selected_notebook
        .clone()
        .expect("an open notebook");
    let group =
        ui.h.state_mut()
            .library
            .create_section_group(&notebook, None, "Term", "#745cf5")
            .unwrap()
            .id;
    ui.h.state_mut().refresh_workspace();
    ui.steps(2);
    let tabs = |ui: &Ui| -> Vec<String> {
        section_tab_rects(ui)
            .into_iter()
            .map(|(name, _)| name)
            .collect()
    };
    assert!(tabs(&ui).contains(&name), "{:?}", tabs(&ui));
    ui.h.state_mut()
        .handle_library_action(super::LibraryAction::MoveSection {
            id: section.clone(),
            group_id: Some(group.clone()),
            before: None,
        });
    ui.steps(2);
    assert_eq!(ui.app().selected_section.as_deref(), Some(section.as_str()));
    assert_eq!(ui.app().tab_group.as_deref(), Some(group.as_str()));
    assert!(
        tabs(&ui).contains(&name),
        "the open section's tab shows at its new level: {:?}",
        tabs(&ui)
    );
}

/// Horizontal Tabs in French, at the default window and the smallest: the tabs row's texts
/// end inside the window and never overlap, the tabs that don't fit go into "⌄", and the
/// picker names the notebook.
#[test]
fn horizontal_tabs_fit_in_french() {
    use egui::Role;
    use egui_kittest::kittest::NodeT;
    let mut ui = Ui::start_in(Lang::Fr);
    // Two more French names after the sample's seven tabs, so the tabs overflow the
    // narrow window but still fit the default one beside the search box at the row's end.
    {
        let app = ui.h.state_mut();
        for (index, name) in ["Arts plastiques", "Éducation civique"]
            .into_iter()
            .enumerate()
        {
            app.library
                .create_section(
                    "notebook-school",
                    name,
                    crate::theme::SECTION_COLORS[index + 3],
                )
                .unwrap();
        }
        app.refresh_workspace();
    }
    horizontal_tabs(&mut ui);
    let school = ui
        .app()
        .workspace
        .notebooks
        .iter()
        .find(|notebook| notebook.id == "notebook-school")
        .unwrap()
        .clone();
    assert_eq!(school.name, "École");
    // The sample's six sections and its group "L’an dernier", then the two added.
    let names = top_level_tabs(&school);
    let total = names.len();
    assert_eq!(total, 9, "{names:?}");
    let more_label = t!("app-more-sections").into_owned();
    for width in [1320.0, 980.0] {
        ui.h.set_size(egui::vec2(width, 900.0));
        ui.steps(4);
        ui.shot(&format!("p7-fr-horizontal-{width}"));
        let window = egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(width, 900.0));
        let tabs = section_tab_rects(&ui);
        let picker =
            ui.h.get_by_role_and_label(Role::Button, &t!("app-notebook-picker"))
                .rect();
        let more =
            ui.h.query_all_by_role_and_label(Role::Button, &more_label)
                .next()
                .map(|node| node.rect());
        // The tabs that don't fit go into "⌄", and only then is it there.
        assert_eq!(
            more.is_some(),
            tabs.len() < total,
            "{width}: {} of {total} tabs, ⌄ {more:?}",
            tabs.len()
        );
        if width < 1000.0 {
            assert!(tabs.len() < total, "the tabs overflow at {width} px");
        } else {
            assert_eq!(tabs.len(), total, "every tab fits at {width} px");
        }
        // The open section's tab always shows.
        assert!(tabs.iter().any(|(name, _)| name == "Cette semaine"));

        // The row: the picker, the tabs, "+" and "⌄".
        let row = tabs
            .iter()
            .map(|(_, rect)| *rect)
            .chain([picker])
            .chain(more)
            .fold(picker, |row, rect| row.union(rect));
        let row = egui::Rect::from_x_y_ranges(0.0..=width, row.y_range());
        assert!(window.contains_rect(row), "{width}: {row:?}");
        let texts = texts_in(&ui, row);
        assert!(
            texts
                .iter()
                .any(|(text, rect)| text == "École" && picker.contains_rect(*rect)),
            "{width}: the picker names the notebook: {texts:?}"
        );
        for (text, rect) in &texts {
            assert!(
                rect.left() >= 0.0 && rect.right() <= width,
                "{width}: {text:?} at {rect:?} runs past the window"
            );
        }
        for (index, (a, at)) in texts.iter().enumerate() {
            for (b, bt) in &texts[index + 1..] {
                let overlap = at.intersect(*bt);
                assert!(
                    overlap.width() <= 0.5 || overlap.height() <= 0.5,
                    "{width}: {a:?} {at:?} overlaps {b:?} {bt:?}"
                );
            }
        }
        // The tabs left out are listed under "⌄", inside the window. (A group's row there
        // is named by its folder glyph and its name.)
        if let Some(more) = more {
            ui.click(more.center());
            ui.shot(&format!("p7-fr-horizontal-{width}-more"));
            for name in &names {
                if tabs.iter().any(|(tab, _)| tab == name) {
                    continue;
                }
                let group_row = format!("    {name}");
                let rect =
                    ui.h.query_all_by_role(Role::Button)
                        .rfind(|node| {
                            let label = node.accesskit_node().label().unwrap_or_default();
                            label == *name || label.ends_with(&group_row)
                        })
                        .unwrap_or_else(|| panic!("{name:?} is not under ⌄"))
                        .rect();
                assert!(window.contains_rect(rect), "{name:?} at {rect:?}");
            }
            ui.press(Key::Escape);
        }
    }
}

/// the desktop notebook hangs tags in the margin: Ctrl+1 puts a To Do box left of the text, which stays
/// where it was, and the note's frame reaches further left. Ctrl+Z takes both back.
#[test]
fn tags_hang_left_and_the_text_stays_put() {
    let mut ui = Ui::start();
    ui.new_page("Tags");
    let at = ui.empty_paper_point();
    ui.click(at);
    ui.type_text("Pack the bag");
    let note = ui.items()[0].clone();
    let block = ui.blocks()[0].id;
    let before = ui.text_point(note.id, block, 0);
    ui.press_with(Modifiers::COMMAND, Key::Num1);
    ui.steps(3);
    let after = ui.text_point(note.id, block, 0);
    assert!((after.x - before.x).abs() < 0.5, "{before:?} → {after:?}");
    let moved = ui.item(note.id);
    assert!(
        (note.x - moved.x - crate::editor::TAG_SLOT).abs() < 0.5,
        "{note:?} → {moved:?}"
    );
    ui.press_with(Modifiers::COMMAND, Key::Z);
    ui.steps(3);
    assert!(
        (ui.item(note.id).x - note.x).abs() < 0.5,
        "undo puts the frame back"
    );
    assert!((ui.text_point(note.id, block, 0).x - before.x).abs() < 0.5);
}

/// The first note of the sample's "My week" has To Do boxes and tags, yet its heading
/// starts under the page title, as an untagged note's does.
#[test]
fn a_tagged_sample_note_starts_under_the_title() {
    let mut ui = Ui::start();
    ui.h.state_mut().open_page("page-school-week", true);
    ui.steps(4);
    let note = ui
        .items()
        .into_iter()
        .find(|item| {
            item.doc().is_some_and(|doc| {
                doc.blocks.iter().any(|block| {
                    !block.tags.is_empty()
                        || matches!(block.kind, crate::model::BlockKind::Todo { .. })
                })
            })
        })
        .expect("a tagged note");
    let first = note.doc().unwrap().blocks[0].id;
    let text = ui.text_point(note.id, first, 0);
    let title_left = ui
        .screen(egui::pos2(
            crate::canvas::PAGE_LEFT,
            crate::canvas::TITLE_TOP,
        ))
        .x;
    assert!(
        (text.x - title_left).abs() < 2.0,
        "{text:?} vs {title_left}"
    );
}

/// The page list shows the title as it is typed, before any save, and a new page brings
/// no toast. An untitled page is "Untitled page".
#[test]
fn the_page_list_follows_the_title_as_it_is_typed() {
    let mut ui = Ui::start();
    ui.h.state_mut().toast.message.clear();
    ui.press_with(Modifiers::COMMAND, Key::N);
    ui.steps(3);
    assert_eq!(ui.app().toast.message, "", "no toast for a new page");
    let open = ui.app().page.as_ref().unwrap().id.clone();
    let row = |ui: &Ui| {
        ui.app()
            .pages
            .iter()
            .find(|page| page.id == open)
            .unwrap()
            .title
            .clone()
    };
    assert_eq!(row(&ui), "Untitled page");
    ui.type_text("Rivers");
    ui.steps(1);
    assert_eq!(row(&ui), "Rivers");
    assert!(ui.h.query_by_label("Rivers").is_some());
}

/// Right-click on note text: the Mini Toolbar is its own card above the pointer, the menu
/// its own card below it.
#[test]
fn the_mini_toolbar_floats_above_the_text_menu() {
    let mut ui = Ui::start();
    ui.new_page("Menu");
    let at = ui.empty_paper_point();
    ui.click(at);
    ui.type_text("Words to format");
    let note = ui.items()[0].clone();
    let point = ui.text_point(note.id, ui.blocks()[0].id, 3);
    ui.right_click(point);
    ui.steps(2);
    let ribbon = ui.ribbon_button_rect(&t!("editor-bold"));
    let toolbar_bold =
        ui.h.get_all_by_role_and_label(egui::Role::Button, &t!("editor-bold"))
            .map(|node| node.rect())
            .find(|rect| *rect != ribbon)
            .expect("the Mini Toolbar's Bold");
    // The menu's Cut, not the ribbon's: its label starts with the glyph column.
    let cut =
        ui.h.get_by_label_contains(&format!("    {}", t!("common-cut")))
            .rect();
    assert!(
        toolbar_bold.bottom() <= point.y + 1.0,
        "the toolbar is above the pointer: {toolbar_bold:?} vs {point:?}"
    );
    assert!(
        cut.top() >= toolbar_bold.bottom() + 6.0,
        "the menu is a card of its own below it: {cut:?} vs {toolbar_bold:?}"
    );
}

/// Selecting text with the mouse (a drag, a double click) brings the Mini Toolbar, faded,
/// over the selection; its Bold makes the selection bold and keeps it, and the keyboard in
/// the note; typing sends it away. Options can turn it off.
#[test]
fn a_mouse_selection_brings_the_mini_toolbar() {
    let mut ui = Ui::start();
    ui.new_page("Selection");
    let at = ui.empty_paper_point();
    ui.click(at);
    ui.type_text("Make this bold");
    let note = ui.items()[0].clone();
    let block = ui.blocks()[0].id;
    let bolds = |ui: &Ui| {
        ui.h.get_all_by_role_and_label(egui::Role::Button, &t!("editor-bold"))
            .count()
    };
    let ribbon_only = bolds(&ui);
    assert!(ui.app().editor.mini_bar.is_none(), "no selection, no bar");
    ui.drag(
        ui.text_point(note.id, block, 5),
        ui.text_point(note.id, block, 9),
    );
    ui.steps(2);
    assert_eq!(
        ui.app().editor.selection,
        Some(5..9),
        "\"this\" is selected"
    );
    assert_eq!(bolds(&ui), ribbon_only + 1, "the Mini Toolbar shows");
    assert!(ui.app().editor.mini_bar.is_some());
    // Its Bold: the one that is not the ribbon's. It sits above the words it formats.
    let ribbon = ui.ribbon_button_rect(&t!("editor-bold"));
    let bar_bold =
        ui.h.get_all_by_role_and_label(egui::Role::Button, &t!("editor-bold"))
            .map(|node| node.rect())
            .find(|rect| *rect != ribbon)
            .unwrap();
    assert!(
        bar_bold.bottom() < ui.text_point(note.id, block, 5).y,
        "the bar leaves the selection in view: {bar_bold:?}"
    );
    ui.click(bar_bold.center());
    ui.steps(3);
    let runs = ui.blocks()[0].text.runs.clone();
    assert!(runs.iter().any(|run| run.style.bold), "{runs:?}");
    assert_eq!(
        ui.app().editor.selection,
        Some(5..9),
        "the words stay selected"
    );
    ui.type_text("x");
    ui.steps(2);
    assert!(ui.app().editor.mini_bar.is_none(), "typing sends it away");
    // The press on the bar kept the keyboard in the note, on the words: they are typed
    // over.
    assert_eq!(ui.blocks()[0].text.text, "Make x bold");
    // A double click selects a word with the mouse too, and brings it back (after a
    // pause, so the click on the bar does not make it a triple click).
    ui.steps(30);
    ui.double_click(ui.text_point(note.id, block, 1));
    ui.steps(2);
    assert_eq!(
        ui.app().editor.selection,
        Some(0..4),
        "\"Make\" is selected"
    );
    assert!(
        ui.app().editor.mini_bar.is_some(),
        "a double click brings it"
    );
    // Turned off in Options, it no longer shows.
    ui.h.state_mut().editor.mini_on_selection = false;
    ui.drag(
        ui.text_point(note.id, block, 2),
        ui.text_point(note.id, block, 4),
    );
    ui.steps(2);
    assert!(
        ui.app().editor.selection.is_some(),
        "the drag still selects"
    );
    assert!(ui.app().editor.mini_bar.is_none());
    assert_eq!(bolds(&ui), ribbon_only);
}

/// Horizontal Tabs: the search box sits at the right end of the section-tab row, above
/// the page list, and Ctrl+E types in it there.
#[test]
fn horizontal_tabs_put_search_above_the_page_list() {
    let mut ui = Ui::start();
    horizontal_tabs(&mut ui);
    ui.steps(2);
    let search = ui.app().search.box_rect.expect("a search box");
    let ribbon_bottom =
        ui.ribbon_tab_rect(&t!("ribbon-tab-home")).bottom() + crate::ribbon::CLASSIC_HEIGHT;
    assert!(
        search.top() > ribbon_bottom - 1.0,
        "under the ribbon, in the tab row: {search:?}"
    );
    assert!(
        search.right() > ui.h.ctx.content_rect().right() - 20.0,
        "at the row's right end: {search:?}"
    );
    ui.press_with(Modifiers::COMMAND, Key::E);
    ui.type_text("castle");
    ui.steps(2);
    assert_eq!(ui.app().search.query, "castle");
}

/// The pen in hand opens its menu when clicked again, as the desktop notebook's does; a click on
/// another pen only takes it up.
#[test]
fn clicking_the_pen_in_hand_opens_its_menu() {
    let mut ui = Ui::start();
    ui.new_page("Pens");
    ui.pick_pen(1);
    assert!(
        !egui::Popup::is_any_open(&ui.h.ctx),
        "taking up a pen opens nothing"
    );
    let label = crate::canvas::pen_label(&ui.app().canvas.pens[1]);
    ui.ribbon_button(&label);
    ui.steps(2);
    assert!(egui::Popup::is_any_open(&ui.h.ctx), "Pen settings opened");
    assert!(ui
        .h
        .query_by_label_contains(&t!("ribbon-remove-pen"))
        .is_some());
    ui.press(Key::Escape);
    let other = crate::canvas::pen_label(&ui.app().canvas.pens[2]);
    ui.ribbon_button(&other);
    assert!(
        !egui::Popup::is_any_open(&ui.h.ctx),
        "another pen opens nothing"
    );
}

/// A note near the page's left edge has no room there for its tags to hang: its frame
/// stops at the edge, as no move takes a note past it, and its text makes the rest of the
/// room. Its tags and To Do box stay on the page, and the box ticks.
#[test]
fn tags_never_hang_past_the_pages_left_edge() {
    let mut ui = Ui::start();
    ui.new_page("Edge");
    let edge = ui.screen(egui::pos2(0.0, 0.0)).x;
    ui.click(ui.screen(egui::pos2(20.0, crate::canvas::PAGE_CONTENT_TOP + 200.0)));
    ui.type_text("Pack the bag");
    let note = ui.items()[0].clone();
    let block = ui.blocks()[0].id;
    // A To Do box and a tag: two slots, more than the room left of the note.
    assert!(note.x < 2.0 * crate::editor::TAG_SLOT, "{note:?}");
    ui.press_with(Modifiers::COMMAND, Key::Num1);
    ui.press_with(Modifiers::COMMAND, Key::Num2);
    ui.steps(3);
    let tagged = &ui.blocks()[0];
    assert!(
        matches!(tagged.kind, crate::model::BlockKind::Todo { .. }) && !tagged.tags.is_empty(),
        "{tagged:?}"
    );
    assert_eq!(ui.item(note.id).x, 0.0, "the frame stops at the edge");
    let text = ui.text_point(note.id, block, 0);
    assert!(
        text.x - 2.0 * crate::editor::TAG_SLOT >= edge,
        "the tags are on the page: {text:?} from {edge}"
    );
    // The box, right before the text, ticks.
    ui.click(text - egui::vec2(crate::editor::TAG_SLOT / 2.0, 0.0));
    assert!(ui.blocks()[0].checked(), "the box was ticked");
}

/// A click on the Mini Toolbar over a selection that picks nothing (between its buttons)
/// leaves the keyboard in the note, on the words selected: typing then replaces them.
#[test]
fn a_click_on_the_mini_toolbar_keeps_the_keyboard_in_the_note() {
    let mut ui = Ui::start();
    ui.new_page("Selection");
    let at = ui.empty_paper_point();
    ui.click(at);
    ui.type_text("Make this bold");
    let note = ui.items()[0].clone();
    let block = ui.blocks()[0].id;
    ui.drag(
        ui.text_point(note.id, block, 5),
        ui.text_point(note.id, block, 9),
    );
    ui.steps(2);
    let ribbon = ui.ribbon_button_rect(&t!("editor-bold"));
    let bar_bold =
        ui.h.get_all_by_role_and_label(egui::Role::Button, &t!("editor-bold"))
            .map(|node| node.rect())
            .find(|rect| *rect != ribbon)
            .expect("the Mini Toolbar");
    // The gap under Bold, between the bar's two rows.
    ui.click(egui::pos2(bar_bold.center().x, bar_bold.bottom() + 1.0));
    ui.steps(2);
    assert!(ui.app().editor.mini_bar.is_some(), "the bar stays");
    ui.type_text("x");
    ui.steps(2);
    assert_eq!(ui.blocks()[0].text.text, "Make x bold");
}

// S2: writing, keys and navigation.

#[test]
fn shift_enter_breaks_the_line_inside_the_paragraph() {
    use crate::model::BlockKind;
    let mut ui = Ui::start();
    ui.new_page("Letters");
    ui.press(Key::Enter);
    ui.type_text("Dear Sam,");
    ui.press_with(Modifiers::SHIFT, Key::Enter);
    ui.type_text("thank you");
    let blocks = ui.blocks();
    assert_eq!(blocks.len(), 1, "one paragraph: {blocks:#?}");
    assert_eq!(blocks[0].text.text, "Dear Sam,\nthank you");
    // In a list item too: the line breaks, the item stays one.
    ui.press(Key::Enter);
    ui.press_with(Modifiers::COMMAND, Key::Period);
    ui.type_text("milk");
    ui.press_with(Modifiers::SHIFT, Key::Enter);
    ui.type_text("2 litres");
    let blocks = ui.blocks();
    assert_eq!(blocks.len(), 2);
    assert_eq!(
        (blocks[1].kind.clone(), blocks[1].text.text.as_str()),
        (BlockKind::Bullet, "milk\n2 litres")
    );
    // Plain Enter still starts a new one.
    ui.press(Key::Enter);
    assert_eq!(ui.blocks().len(), 3);
}

#[test]
fn typed_letters_and_roman_numbers_start_numbered_lists() {
    use crate::model::{BlockKind, ListMarker};
    let mut ui = Ui::start();
    ui.new_page("Lists");
    ui.press(Key::Enter);
    let cases = [
        ("a. ", "apples", ListMarker::LowerAlpha),
        ("A. ", "Brest", ListMarker::UpperAlpha),
        ("i. ", "intro", ListMarker::LowerRoman),
        ("I. ", "History", ListMarker::UpperRoman),
        ("1) ", "first", ListMarker::DecimalParen),
    ];
    for (prefix, text, _) in cases {
        ui.type_text(prefix);
        ui.type_text(text);
        // A new item, then Enter on it empty: back to plain text for the next case.
        ui.press(Key::Enter);
        ui.press(Key::Enter);
    }
    let blocks = ui.blocks();
    for (_, text, marker) in cases {
        let block = blocks
            .iter()
            .find(|block| block.text.text == text)
            .unwrap_or_else(|| panic!("{text} in {blocks:#?}"));
        assert_eq!(
            (block.kind.clone(), block.marker),
            (BlockKind::Numbered, Some(marker)),
            "{text}"
        );
    }
}

/// A list typed in a new numbering style right after another starts its own count, as in
/// the desktop notebook: "A." after an "a." list is A, not C, and "i." after it is i.
#[test]
fn a_new_numbering_style_typed_after_a_list_starts_its_own_count() {
    let mut ui = Ui::start();
    ui.new_page("Fruit");
    ui.press(Key::Enter);
    ui.type_text("a. ");
    ui.type_text("apple");
    ui.press(Key::Enter);
    ui.type_text("banana");
    // A new item, then Enter on it empty: back to plain text.
    ui.press(Key::Enter);
    ui.press(Key::Enter);
    ui.type_text("A. ");
    ui.type_text("Alpha");
    ui.press(Key::Enter);
    ui.press(Key::Enter);
    ui.type_text("i. ");
    ui.type_text("one");
    // What each item shows, as the note draws it.
    let blocks = ui.blocks();
    let shown: Vec<(String, String)> = blocks
        .iter()
        .zip(crate::model::list_numbers(&blocks))
        .filter_map(|(block, number)| {
            let marker = block.list_marker()?;
            Some((block.text.text.clone(), marker.text(number)))
        })
        .collect();
    let shown: Vec<(&str, &str)> = shown
        .iter()
        .map(|(text, marker)| (text.as_str(), marker.as_str()))
        .collect();
    assert_eq!(
        shown,
        [
            ("apple", "a."),
            ("banana", "b."),
            ("Alpha", "A."),
            ("one", "i.")
        ]
    );
}

#[test]
fn ctrl_1_boxes_ticks_then_clears_a_to_do() {
    use crate::model::BlockKind;
    let mut ui = Ui::start();
    ui.new_page("Chores");
    ui.press(Key::Enter);
    ui.type_text("Feed the cat");
    let kind = |ui: &Ui| ui.blocks()[0].kind.clone();
    ui.press_with(Modifiers::COMMAND, Key::Num1);
    assert_eq!(kind(&ui), BlockKind::Todo { checked: false });
    ui.press_with(Modifiers::COMMAND, Key::Num1);
    assert_eq!(kind(&ui), BlockKind::Todo { checked: true });
    ui.press_with(Modifiers::COMMAND, Key::Num1);
    assert_eq!(kind(&ui), BlockKind::Paragraph);
    assert_eq!(ui.blocks()[0].text.text, "Feed the cat");
}

#[test]
fn typed_sums_get_their_answer_after_the_equals_sign() {
    let mut ui = Ui::start();
    ui.new_page("Maths");
    ui.press(Key::Enter);
    ui.type_text("95+83+416=");
    ui.type_text(" ");
    assert_eq!(ui.blocks()[0].text.text, "95+83+416=594 ");
    // Enter works it out too, then starts the next line.
    ui.type_text("2*(3+4)=");
    ui.press(Key::Enter);
    let blocks = ui.blocks();
    assert_eq!(blocks[0].text.text, "95+83+416=594 2*(3+4)=14");
    assert_eq!(blocks.len(), 2);
    // Not an expression: nothing is added.
    ui.type_text("x=");
    ui.type_text(" ");
    assert_eq!(ui.blocks()[1].text.text, "x= ");
    // Options › Advanced › Editing turns it off, and the setting is kept.
    ui.h.state_mut().dialog = Some(super::Dialog::Settings);
    ui.steps(2);
    ui.options_page(&t!("app-options-advanced"));
    ui.click_label(&t!("app-options-auto-calc"));
    assert!(!ui.app().editor.auto_calc);
    assert_eq!(
        ui.app().library.setting("no_auto_calc").unwrap().as_deref(),
        Some("true")
    );
}

#[test]
fn a_space_after_an_answered_equals_sign_adds_no_second_answer() {
    let mut ui = Ui::start();
    ui.new_page("Maths");
    ui.press(Key::Enter);
    ui.type_text("2+2=");
    ui.type_text(" ");
    assert_eq!(ui.blocks()[0].text.text, "2+2=4 ");
    // Back to just after the "=": Space types a space and writes no second "4".
    ui.press(Key::ArrowLeft);
    ui.press(Key::ArrowLeft);
    ui.type_text(" ");
    assert_eq!(ui.blocks()[0].text.text, "2+2= 4 ");
    // Spaces between the caret and the answer are skipped, and Enter only splits the line.
    ui.press(Key::ArrowLeft);
    ui.press(Key::Enter);
    let blocks = ui.blocks();
    assert_eq!(blocks[0].text.text, "2+2=");
    assert_eq!(blocks[1].text.text, " 4 ");
}

#[test]
fn typed_sums_use_the_decimal_comma_in_french() {
    let mut ui = Ui::start_in(Lang::Fr);
    ui.new_page("Calculs");
    ui.press(Key::Enter);
    ui.type_text("2,5+1=");
    ui.type_text(" ");
    assert_eq!(ui.blocks()[0].text.text, "2,5+1=3,5 ");
    ui.type_text("RACINE(16)=");
    ui.type_text(" ");
    assert_eq!(ui.blocks()[0].text.text, "2,5+1=3,5 RACINE(16)=4 ");
    // Off: the "=" stays as typed.
    ui.h.state_mut().editor.auto_calc = false;
    ui.type_text("1+1=");
    ui.type_text(" ");
    assert!(ui.blocks()[0].text.text.ends_with("1+1= "));
}

#[test]
fn typing_then_tab_starts_a_table_and_the_table_keys_grow_and_shrink_it() {
    let mut ui = Ui::start();
    ui.new_page("Marks");
    ui.press(Key::Enter);
    ui.type_text("Name");
    ui.press(Key::Tab);
    assert_eq!(
        ui.cells(),
        [["Name", ""]],
        "Tab after the text started a table"
    );
    assert_eq!(ui.app().editor.active_cell, Some((0, 1)));
    ui.type_text("Mark");
    // Tab in the last cell of a one-row table adds a column.
    ui.press(Key::Tab);
    assert_eq!(ui.cells(), [["Name", "Mark", ""]]);
    ui.type_text("Rank");
    // Enter at the end of the last cell adds a row; with two rows, Tab there adds a row.
    ui.press(Key::Enter);
    ui.type_cells(&["Zoé", "12", "1"]);
    assert_eq!(ui.cells().len(), 2);
    ui.press(Key::Tab);
    assert_eq!(ui.cells().len(), 3, "a taller table grows a row");
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert_eq!(ui.cells().len(), 2);
    // Enter at the start of a row that is not the first puts an empty row above it.
    ui.press_with(Modifiers::SHIFT, Key::Tab);
    ui.press_with(Modifiers::SHIFT, Key::Tab);
    ui.press(Key::Home);
    assert_eq!(ui.app().editor.active_cell, Some((1, 0)));
    ui.press(Key::Enter);
    assert_eq!(
        ui.cells(),
        [["Name", "Mark", "Rank"], ["", "", ""], ["Zoé", "12", "1"]]
    );
    assert_eq!(
        ui.app().editor.active_cell,
        Some((1, 0)),
        "into the new row"
    );
    // Delete twice at the start of the empty row removes it; the first only selects it.
    ui.press(Key::Delete);
    assert_eq!(ui.cells().len(), 3, "the first Delete selects the row");
    ui.press(Key::Delete);
    assert_eq!(ui.cells(), [["Name", "Mark", "Rank"], ["Zoé", "12", "1"]]);
}

/// The caret: its paragraph and char, as the focused text box holds it.
fn caret(ui: &Ui) -> (u64, usize) {
    let block = ui
        .app()
        .editor
        .active_block
        .expect("a paragraph has the caret");
    let id = focused_widget(ui).expect("a text box has the keyboard");
    let range = egui::text_edit::TextEditState::load(&ui.h.ctx, id)
        .and_then(|state| state.cursor.char_range())
        .expect("a caret");
    (block, range.primary.index.0)
}

#[test]
fn ctrl_arrows_ctrl_home_end_and_ctrl_a_move_through_the_page() {
    let mut ui = Ui::start();
    ui.new_page("Keys");
    ui.press(Key::Enter);
    ui.type_text("one");
    ui.press(Key::Enter);
    ui.type_text("two");
    ui.press(Key::Enter);
    ui.type_text("three");
    let first_note = ui.items()[0].id;
    let blocks = ui.blocks();
    let (one, two, three) = (blocks[0].id, blocks[1].id, blocks[2].id);
    // Ctrl+↑: the start of this paragraph, then of the one above; Ctrl+↓: the next one's.
    ui.press_with(Modifiers::COMMAND, Key::ArrowUp);
    assert_eq!(caret(&ui), (three, 0));
    ui.press_with(Modifiers::COMMAND, Key::ArrowUp);
    assert_eq!(caret(&ui), (two, 0));
    ui.press_with(Modifiers::COMMAND, Key::ArrowDown);
    assert_eq!(caret(&ui), (three, 0));
    ui.press_with(Modifiers::COMMAND, Key::ArrowDown);
    assert_eq!(caret(&ui), (three, 5), "from the last paragraph: its end");

    // A second note lower on the page. Ctrl+Home: the first note's start; Ctrl+End: the
    // lowest note's end.
    ui.click(ui.empty_spot(-150.0, 300.0));
    ui.type_text("last");
    let last_note = ui.app().canvas.active_text.expect("typing in the new note");
    assert_ne!(last_note, first_note);
    ui.press_with(Modifiers::COMMAND, Key::Home);
    assert_eq!(ui.app().canvas.active_text, Some(first_note));
    assert_eq!(caret(&ui), (one, 0));
    ui.press_with(Modifiers::COMMAND, Key::End);
    assert_eq!(ui.app().canvas.active_text, Some(last_note));
    let last_block = ui.item(last_note).doc().unwrap().blocks[0].id;
    assert_eq!(caret(&ui), (last_block, 4));

    // Ctrl+A, again and again: the paragraph, the note, then everything on the page.
    ui.press_with(Modifiers::COMMAND, Key::Home);
    for _ in 0..3 {
        ui.press_with(Modifiers::COMMAND, Key::A);
    }
    assert_eq!(ui.app().canvas.active_text, None, "no note is typed in");
    assert_eq!(ui.app().canvas.selection.len(), ui.items().len());
}

#[test]
fn page_up_and_page_down_scroll_the_page_and_carry_the_caret() {
    let mut ui = Ui::start();
    // A short window: the view scrolls further than one PgUp, which then never stops at
    // the page's top.
    ui.h.set_size(egui::vec2(1320.0, 600.0));
    ui.steps(2);
    ui.new_page("Long");
    ui.press(Key::Enter);
    for n in 0..60 {
        if n > 0 {
            ui.press(Key::Enter);
        }
        ui.type_text(&format!("line {n}"));
    }
    let top = |ui: &Ui| ui.app().canvas.visible().expect("drawn").top();
    let line = |ui: &Ui| {
        let (block, _) = caret(ui);
        ui.blocks()
            .iter()
            .position(|b| b.id == block)
            .expect("a line")
    };
    let (before, last) = (top(&ui), line(&ui));
    assert_eq!(last, 59);
    ui.press(Key::PageUp);
    assert!(
        top(&ui) < before - 100.0,
        "scrolled up: {before} → {}",
        top(&ui)
    );
    assert!(line(&ui) < 50, "the caret went up too: line {}", line(&ui));
    ui.press(Key::PageDown);
    assert!(
        (top(&ui) - before).abs() < 2.0,
        "back down: {before} → {}",
        top(&ui)
    );
}

#[test]
fn shift_arrows_go_by_line_and_ctrl_shift_arrows_by_word_across_paragraphs() {
    use crate::editor::Pos;
    let mut ui = Ui::start();
    ui.new_page("Lines");
    ui.press(Key::Enter);
    // Two paragraphs of short lines, every line the same text so columns line up.
    ui.type_text("abc");
    ui.press_with(Modifiers::SHIFT, Key::Enter);
    ui.type_text("abc");
    ui.press(Key::Enter);
    for n in 0..3 {
        if n > 0 {
            ui.press_with(Modifiers::SHIFT, Key::Enter);
        }
        ui.type_text("abc");
    }
    let blocks = ui.blocks();
    let (first, second) = (blocks[0].id, blocks[1].id);
    // To the end of the first paragraph: the start of the second, then Up.
    ui.press_with(Modifiers::COMMAND, Key::ArrowUp);
    ui.press(Key::ArrowUp);
    assert_eq!(caret(&ui), (first, 7));
    // Shift+↓ goes one line down: into the second paragraph's first line, then its second.
    ui.press_with(Modifiers::SHIFT, Key::ArrowDown);
    let at = |block, char| Pos { block, char };
    assert_eq!(ui.app().editor.range, Some((at(first, 7), at(second, 3))));
    ui.press_with(Modifiers::SHIFT, Key::ArrowDown);
    assert_eq!(
        ui.app().editor.range,
        Some((at(first, 7), at(second, 7))),
        "by line, not to the paragraph's end"
    );
    ui.press(Key::Escape);

    // Ctrl+Shift+→ from a paragraph's end goes on into the next, then by word.
    ui.new_page("Words");
    ui.press(Key::Enter);
    ui.type_text("alpha beta");
    ui.press(Key::Enter);
    ui.type_text("gamma delta");
    let blocks = ui.blocks();
    let (first, second) = (blocks[0].id, blocks[1].id);
    ui.press_with(Modifiers::COMMAND, Key::ArrowUp);
    ui.press(Key::ArrowUp);
    assert_eq!(caret(&ui), (first, 10));
    let shift_ctrl = Modifiers::COMMAND | Modifiers::SHIFT;
    ui.press_with(shift_ctrl, Key::ArrowRight);
    assert_eq!(ui.app().editor.range, Some((at(first, 10), at(second, 0))));
    ui.press_with(shift_ctrl, Key::ArrowRight);
    assert_eq!(ui.app().editor.range, Some((at(first, 10), at(second, 5))));
    ui.press_with(shift_ctrl, Key::ArrowRight);
    assert_eq!(ui.app().editor.range, Some((at(first, 10), at(second, 11))));
    ui.press_with(shift_ctrl, Key::ArrowLeft);
    assert_eq!(ui.app().editor.range, Some((at(first, 10), at(second, 6))));
}

#[test]
fn keys_step_through_pages_sections_and_the_pages_visited() {
    let mut ui = Ui::start();
    for title in ["Alpha", "Beta", "Gamma"] {
        ui.new_page(title);
        save(&mut ui);
    }
    let open = |ui: &Ui| {
        ui.app()
            .page
            .as_ref()
            .map(|page| page.title.clone())
            .unwrap_or_default()
    };
    let first = titles(&ui)[0].0.clone();
    assert_eq!(open(&ui), "Gamma");
    // Ctrl+PgUp / Ctrl+PgDn: the previous and next page in the list.
    ui.press_with(Modifiers::COMMAND, Key::PageUp);
    assert_eq!(open(&ui), "Beta");
    ui.press_with(Modifiers::COMMAND, Key::PageDown);
    assert_eq!(open(&ui), "Gamma");
    // Alt+Home / Alt+End: the first and the last; Alt+PgUp / Alt+PgDn: the first and last
    // in view (here every row is).
    ui.press_with(Modifiers::ALT, Key::Home);
    assert_eq!(open(&ui), first);
    ui.press_with(Modifiers::ALT, Key::End);
    assert_eq!(open(&ui), "Gamma");
    ui.press_with(Modifiers::ALT, Key::PageUp);
    assert_eq!(open(&ui), first);
    ui.press_with(Modifiers::ALT, Key::PageDown);
    assert_eq!(open(&ui), "Gamma");
    // Alt+← / Alt+→: back and forward through the pages visited.
    if cfg!(not(target_os = "macos")) {
        ui.press_with(Modifiers::ALT, Key::ArrowLeft);
        assert_eq!(open(&ui), first);
        ui.press_with(Modifiers::ALT, Key::ArrowLeft);
        assert_eq!(open(&ui), "Gamma");
        ui.press_with(Modifiers::ALT, Key::ArrowLeft);
        assert_eq!(open(&ui), first);
        ui.press_with(Modifiers::ALT, Key::ArrowRight);
        assert_eq!(open(&ui), "Gamma");
    }
    // Ctrl+Tab / Ctrl+Shift+Tab: the next section, and back.
    let section = |ui: &Ui| {
        let app = ui.app();
        app.workspace
            .notebooks
            .iter()
            .flat_map(|notebook| &notebook.sections)
            .find(|section| Some(&section.id) == app.selected_section.as_ref())
            .map(|section| section.name.clone())
            .unwrap_or_default()
    };
    let start = section(&ui);
    ui.press_with(Modifiers::CTRL, Key::Tab);
    assert_ne!(section(&ui), start);
    ui.press_with(Modifiers::CTRL | Modifiers::SHIFT, Key::Tab);
    assert_eq!(section(&ui), start);
    // Alt+Shift+↑, with no note typed in: Beta moves above Alpha.
    ui.press_with(Modifiers::ALT, Key::End);
    ui.press_with(Modifiers::COMMAND, Key::PageUp);
    assert_eq!(open(&ui), "Beta");
    ui.press_with(Modifiers::ALT | Modifiers::SHIFT, Key::ArrowUp);
    let order: Vec<String> = titles(&ui).into_iter().map(|(title, _)| title).collect();
    let place = |title: &str| order.iter().position(|t| t == title).expect("listed");
    assert_eq!(place("Beta") + 1, place("Alpha"), "{order:?}");
}

#[test]
fn dragging_a_page_sideways_makes_and_unmakes_a_subpage() {
    let mut ui = Ui::start();
    ui.new_page("Alpha");
    save(&mut ui);
    ui.new_page("Beta");
    save(&mut ui);
    let row = |ui: &Ui| {
        ui.h.get_by_role_and_label(egui::Role::ListItem, "Beta")
            .rect()
            .center()
    };
    let from = row(&ui);
    ui.drag(from, from + egui::vec2(40.0, 0.0));
    assert!(
        titles(&ui).contains(&("Beta".into(), 1)),
        "{:?}",
        titles(&ui)
    );
    let from = row(&ui);
    ui.drag(from, from - egui::vec2(40.0, 0.0));
    assert!(
        titles(&ui).contains(&("Beta".into(), 0)),
        "{:?}",
        titles(&ui)
    );
}

#[test]
fn the_zoom_box_and_ctrl_plus_and_minus_zoom_the_page() {
    let mut ui = Ui::start();
    ui.new_page("Zoom");
    ui.ribbon_tab(&t!("ribbon-tab-view"));
    ui.ribbon_button(&t!("ribbon-zoom-box"));
    ui.click_label_containing(&t!("ribbon-zoom-percent", percent = 200));
    ui.steps(2);
    let zoom = |ui: &Ui| ui.app().canvas.zoom();
    assert!((zoom(&ui) - 2.0).abs() < 0.01, "{}", zoom(&ui));
    // With no text focused, Ctrl+- and Ctrl+= zoom out and back in; the app never zooms.
    ui.press(Key::Escape);
    ui.press(Key::Escape);
    assert!(!ui.h.ctx.text_edit_focused());
    ui.press_with(Modifiers::COMMAND, Key::Minus);
    assert!(zoom(&ui) < 1.9, "{}", zoom(&ui));
    ui.press_with(Modifiers::COMMAND, Key::Equals);
    assert!((zoom(&ui) - 2.0).abs() < 0.01, "{}", zoom(&ui));
    assert_eq!(ui.h.ctx.zoom_factor(), 1.0);
}

#[test]
fn search_arrows_preview_each_result_and_words_match_in_any_order() {
    let mut ui = Ui::start();
    ui.new_page("Quokkas");
    ui.press(Key::Enter);
    ui.type_text("Rottnest quokkas smile widely.");
    save(&mut ui);
    ui.new_page("Penguins");
    ui.press(Key::Enter);
    ui.type_text("Penguins smile at quokkas too.");
    save(&mut ui);
    ui.press_with(Modifiers::COMMAND, Key::E);
    ui.type_text("smile quokkas");
    ui.steps(2);
    assert_eq!(
        ui.app().search.results.len(),
        2,
        "both pages hold both words"
    );
    // ↓ shows the next result's page; the search stays open.
    for _ in 0..2 {
        ui.press(Key::ArrowDown);
        let app = ui.app();
        assert!(app.search.open);
        assert_eq!(
            app.page.as_ref().map(|page| page.id.clone()),
            Some(app.search.results[app.search.selected].page_id.clone())
        );
    }
    ui.press(Key::Enter);
    assert!(!ui.app().search.open);
    // A quoted phrase must be there as typed.
    ui.press_with(Modifiers::COMMAND, Key::E);
    ui.type_text("\"quokkas smile\"");
    ui.steps(2);
    let found: Vec<String> = ui
        .app()
        .search
        .results
        .iter()
        .map(|hit| hit.title.clone())
        .collect();
    assert_eq!(found, ["Quokkas"]);
}

/// The search's ↓ / ↑ previews are no visits: after ↓ ↓ ↓ and Enter, Back goes to the
/// page open before the search, not through every page previewed.
#[test]
fn search_previews_leave_one_step_for_back() {
    let mut ui = Ui::start();
    for title in ["Apples one", "Apples two", "Apples three", "Home"] {
        ui.new_page(title);
        save(&mut ui);
    }
    let open = |ui: &Ui| {
        ui.app()
            .page
            .as_ref()
            .map(|page| page.title.clone())
            .unwrap_or_default()
    };
    assert_eq!(open(&ui), "Home");
    ui.press_with(Modifiers::COMMAND, Key::E);
    ui.type_text("apples");
    ui.steps(2);
    assert_eq!(ui.app().search.results.len(), 3);
    let mut previewed = Vec::new();
    for _ in 0..3 {
        ui.press(Key::ArrowDown);
        previewed.push(open(&ui));
    }
    previewed.sort();
    previewed.dedup();
    assert_eq!(previewed.len(), 3, "each result showed: {previewed:?}");
    ui.press(Key::Enter);
    assert!(!ui.app().search.open);
    let kept = open(&ui);
    assert!(kept.starts_with("Apples"), "{kept}");
    if cfg!(not(target_os = "macos")) {
        ui.press_with(Modifiers::ALT, Key::ArrowLeft);
        assert_eq!(open(&ui), "Home", "Back skips the pages previewed");
        ui.press_with(Modifiers::ALT, Key::ArrowLeft);
        assert_eq!(
            open(&ui),
            "Apples three",
            "then the visits before the search"
        );
        ui.press_with(Modifiers::ALT, Key::ArrowRight);
        ui.press_with(Modifiers::ALT, Key::ArrowRight);
        assert_eq!(open(&ui), kept, "Forward comes back to the result kept");
    }
}

/// ↓ and ↑ in the search box pick results without moving the box's caret: a letter typed
/// after them goes at the end of the words.
#[test]
fn search_arrows_leave_the_caret_at_the_end_of_the_words() {
    let mut ui = Ui::start();
    ui.new_page("Quokkas");
    save(&mut ui);
    ui.new_page("Quokka facts");
    save(&mut ui);
    ui.press_with(Modifiers::COMMAND, Key::E);
    ui.type_text("quokka");
    ui.steps(2);
    assert_eq!(ui.app().search.results.len(), 2);
    ui.press(Key::ArrowDown);
    assert_eq!(ui.app().search.selected, 1);
    ui.press(Key::ArrowUp);
    assert_eq!(ui.app().search.selected, 0);
    assert!(ui.app().search.open);
    ui.type_text("s");
    assert_eq!(ui.app().search.query, "quokkas");
}

/// Shift+↑ / ↓ by screen line still go on past a table between paragraphs (the line next
/// to the caret is the table's, not text), and Shift+↑ on the note's first line goes to
/// its start, as before the screen-line steps.
#[test]
fn shift_arrows_step_past_a_table_and_the_note_s_first_line() {
    use crate::editor::Pos;
    let mut ui = Ui::start();
    ui.new_page("Past the table");
    ui.press(Key::Enter);
    ui.type_text("one");
    ui.press(Key::Enter);
    ui.insert_table(2, 1);
    ui.type_cells(&["a", "b"]);
    // Out of the last cell, onto the line Insert › Table leaves under it.
    ui.press(Key::ArrowDown);
    ui.type_text("two");
    let blocks = ui.blocks();
    assert_eq!(blocks.len(), 3, "{blocks:#?}");
    let (one, two) = (blocks[0].id, blocks[2].id);
    ui.press(Key::Home);
    assert_eq!(caret(&ui), (two, 0));
    let at = |block, char| Pos { block, char };
    ui.press_with(Modifiers::SHIFT, Key::ArrowUp);
    assert_eq!(ui.app().editor.range, Some((at(two, 0), at(one, 0))));
    ui.press_with(Modifiers::SHIFT, Key::ArrowRight);
    assert_eq!(ui.app().editor.range, Some((at(two, 0), at(one, 1))));
    ui.press_with(Modifiers::SHIFT, Key::ArrowUp);
    assert_eq!(
        ui.app().editor.range,
        Some((at(two, 0), at(one, 0))),
        "from the note's first line: its start"
    );
    ui.press(Key::Escape);
    ui.press_with(Modifiers::COMMAND, Key::Home);
    ui.press(Key::End);
    assert_eq!(caret(&ui), (one, 3));
    ui.press_with(Modifiers::SHIFT, Key::ArrowDown);
    assert_eq!(ui.app().editor.range, Some((at(one, 3), at(two, 3))));
}

/// The keys that change a note take one Ctrl+Z each, and the keyboard stays in the note.
#[test]
fn tab_into_a_table_and_inline_maths_undo_in_one_step() {
    let mut ui = Ui::start();
    ui.new_page("Undo");
    ui.press(Key::Enter);
    ui.type_text("Name");
    ui.press(Key::Tab);
    assert_eq!(ui.cells(), [["Name", ""]]);
    ui.press_with(Modifiers::COMMAND, Key::Z);
    let blocks = ui.blocks();
    assert_eq!(blocks[0].kind, crate::model::BlockKind::Paragraph);
    assert_eq!(blocks[0].text.text, "Name");
    assert!(
        focused_widget(&ui).is_some(),
        "the keyboard stays in the note"
    );
    ui.press(Key::End);
    ui.type_text(" 2+2=");
    ui.type_text(" ");
    assert_eq!(ui.blocks()[0].text.text, "Name 2+2=4 ");
    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert_eq!(ui.blocks()[0].text.text, "Name 2+2=");
    assert!(
        focused_widget(&ui).is_some(),
        "the keyboard stays in the note"
    );
}

/// the desktop notebook's 29 tags are all in the Tag menu. A checkbox tag such as Call back ticks with
/// a click on its box, Ctrl+0 takes it off, and Ctrl+1 … Ctrl+9 apply the gallery's first
/// nine: Ctrl+1 still runs To Do's cycle.
#[test]
fn all_29_tags_tick_and_take_the_first_nine_digits() {
    use crate::model::{BlockKind, LineTag, TagMark};
    use crate::tags::TagKey;
    let mut ui = Ui::start();
    ui.new_page("Week");
    ui.press(Key::Enter);
    ui.type_text("Ring the dentist");
    let note = ui.items()[0].id;
    let block = ui.blocks()[0].id;

    // Call back is the menu's 25th row: past the rows it shows before it scrolls.
    let call_back = t!("tag-call-back").into_owned();
    ui.open_tags_menu();
    ui.h.get_all_by_label(&call_back)
        .last()
        .expect("a Call back row")
        .scroll_to_me();
    ui.steps(4);
    let row = ui.h.get_all_by_label(&call_back).last().unwrap().rect();
    ui.click(row.center());
    assert_eq!(ui.blocks()[0].tags, [LineTag::CallBack]);
    assert!(ui.blocks()[0].checked_tags.is_empty(), "applied unticked");

    // Its box hangs right before the text, as To Do's does; a click ticks it.
    ui.steps(30);
    let text = ui.text_point(note, block, 0);
    ui.click(text - egui::vec2(crate::editor::TAG_SLOT / 2.0, 0.0));
    assert_eq!(
        ui.blocks()[0].checked_tags,
        [LineTag::CallBack],
        "the click ticked Call back"
    );
    // The pointer off the box, so the shot shows its tick.
    ui.h.event(Event::PointerMoved(text + egui::vec2(300.0, 120.0)));
    ui.shot("s3-ticked-tag");

    ui.steps(30);
    ui.click(ui.text_point(note, block, 4));
    ui.press_with(Modifiers::COMMAND, Key::Num0);
    let plain = ui.blocks()[0].clone();
    assert!(
        plain.tags.is_empty() && plain.checked_tags.is_empty(),
        "Ctrl+0 removes the tag and its tick: {plain:?}"
    );

    // Ctrl+1 is the gallery's first tag, To Do: box, tick, then plain again.
    let gallery = ui.app().editor.tags.clone();
    assert_eq!(gallery.at_digit(1), Some(TagKey::Todo));
    let kind = |ui: &Ui| ui.blocks()[0].kind.clone();
    ui.press_with(Modifiers::COMMAND, Key::Num1);
    assert_eq!(kind(&ui), BlockKind::Todo { checked: false });
    ui.press_with(Modifiers::COMMAND, Key::Num1);
    assert_eq!(kind(&ui), BlockKind::Todo { checked: true });
    ui.press_with(Modifiers::COMMAND, Key::Num1);
    assert_eq!(kind(&ui), BlockKind::Paragraph);
    // Ctrl+9 is the ninth, and takes it off again.
    let Some(TagKey::Line(ninth)) = gallery.at_digit(9) else {
        panic!("the ninth tag is a built-in: {:?}", gallery.at_digit(9));
    };
    ui.press_with(Modifiers::COMMAND, Key::Num9);
    assert_eq!(ui.blocks()[0].tags, [ninth]);
    ui.press_with(Modifiers::COMMAND, Key::Num9);
    assert!(ui.blocks()[0].tags.is_empty());

    // The menu has a row for each of the 29 (the gallery shows a few of them inline too).
    let labels: Vec<String> = std::iter::once(TagMark::Todo.label().into_owned())
        .chain(LineTag::ALL.iter().map(|tag| tag.label().into_owned()))
        .collect();
    assert_eq!(labels.len(), 29);
    let rows = |ui: &Ui| -> usize {
        labels
            .iter()
            .map(|label| {
                ui.h.query_all_by_role_and_label(egui::Role::Button, label)
                    .count()
            })
            .sum()
    };
    let inline = rows(&ui);
    ui.open_tags_menu();
    ui.steps(2);
    assert_eq!(rows(&ui) - inline, 29, "one menu row per tag");
    // The menu was left scrolled down to Call back; it opens at the top again, To Do's
    // row in view above Remove tags.
    let todo =
        ui.h.get_all_by_label(&TagMark::Todo.label())
            .last()
            .expect("a To Do row")
            .rect();
    let remove = ui.h.get_by_label(&t!("editor-remove-tags")).rect();
    assert!(
        todo.bottom() <= remove.top() && todo.top() >= remove.top() - 460.0,
        "To Do {todo:?} is the first row in view over {remove:?}"
    );
    ui.press(Key::Escape);
}

/// Customize Tags makes a custom tag and moves it first, so Ctrl+1 applies it. Changing
/// the tag afterwards leaves the paragraphs it already tags as they were, as in the desktop notebook.
#[test]
fn customize_tags_makes_a_tag_moves_it_first_and_keeps_old_notes() {
    use crate::model::{CustomTag, Rgb, TagSymbol};
    use crate::tags::{TagGallery, TagKey};
    let mut ui = Ui::start();
    ui.new_page("School");
    ui.press(Key::Enter);
    ui.type_text("Read chapter 3");
    let open = |ui: &mut Ui| {
        ui.open_tags_menu();
        ui.click_label_containing(&t!("editor-customize-tags"));
        ui.steps(3);
    };
    let gallery = |ui: &Ui| -> TagGallery {
        match &ui.app().dialog {
            Some(super::Dialog::CustomizeTags(state)) => state.gallery.clone(),
            _ => panic!("Customize Tags is not open"),
        }
    };
    open(&mut ui);
    click_in_dialog(&mut ui, &t!("app-new-tag"));
    ui.steps(3);
    ui.type_text("Homework");
    // The box: the first symbol after No symbol.
    let none = in_dialog(&ui, &t!("app-tag-no-symbol"))
        .expect("No symbol")
        .0;
    let (todo, _) = in_dialog(&ui, &t!("tag-todo")).expect("the box symbol");
    assert!(
        todo.left() > none.left() && (todo.center().y - none.center().y).abs() < 1.0,
        "the box follows No symbol: {todo:?} after {none:?}"
    );
    ui.click(todo.center());
    // The second highlighter.
    let pink = Rgb::HIGHLIGHTERS[1];
    click_in_dialog(
        &mut ui,
        &format!("#{:02x}{:02x}{:02x}", pink.0, pink.1, pink.2),
    );
    click_in_dialog(&mut ui, &t!("common-ok"));
    assert_eq!(
        gallery(&ui).custom,
        [CustomTag {
            name: "Homework".into(),
            symbol: TagSymbol::Checkbox,
            font_color: None,
            highlight: Some(pink),
            checked: false,
        }]
    );
    // ↑ until it is first.
    for _ in 0..40 {
        if gallery(&ui).order[0] == TagKey::Custom(0) {
            break;
        }
        click_in_dialog(&mut ui, &t!("app-move-tag-up"));
    }
    assert_eq!(gallery(&ui).order[0], TagKey::Custom(0));
    click_in_dialog(&mut ui, &t!("common-ok"));
    assert!(ui.app().dialog.is_none(), "OK closes Customize Tags");
    assert_eq!(ui.app().editor.tags.order[0], TagKey::Custom(0));
    let stored = ui
        .app()
        .library
        .setting("custom_tags")
        .unwrap()
        .expect("the custom tags are stored");
    assert!(stored.contains("Homework"), "{stored}");

    // Ctrl+1 now applies Homework, and ticks it on a second press.
    ui.press_with(Modifiers::COMMAND, Key::Num1);
    let tags = ui.blocks()[0].custom_tags.clone();
    assert_eq!(tags.len(), 1, "{tags:?}");
    assert_eq!(tags[0].name, "Homework");
    assert!(!tags[0].checked);
    ui.press_with(Modifiers::COMMAND, Key::Num1);
    assert!(
        ui.blocks()[0].custom_tags[0].checked,
        "a second Ctrl+1 ticks it"
    );

    // Renamed "Devoirs": the paragraph tagged before keeps "Homework".
    open(&mut ui);
    click_in_dialog(&mut ui, &t!("app-modify-tag"));
    ui.steps(3);
    ui.press(Key::End);
    for _ in 0.."Homework".len() {
        ui.press(Key::Backspace);
    }
    ui.type_text("Devoirs");
    click_in_dialog(&mut ui, &t!("common-ok"));
    assert_eq!(gallery(&ui).custom[0].name, "Devoirs");
    ui.shot("s3-customize-tags");
    click_in_dialog(&mut ui, &t!("common-ok"));
    assert_eq!(ui.app().editor.tags.custom[0].name, "Devoirs");
    let kept = &ui.blocks()[0].custom_tags;
    assert_eq!(kept[0].name, "Homework", "tagged notes keep their tag");
    assert!(kept[0].checked);

    // Escape leaves the New Tag form for the list, which keeps its changes; Cancel then
    // drops every change made in the dialog.
    open(&mut ui);
    click_in_dialog(&mut ui, &t!("app-new-tag"));
    ui.steps(3);
    ui.type_text("Chores");
    ui.press(Key::Escape);
    ui.steps(2);
    match &ui.app().dialog {
        Some(super::Dialog::CustomizeTags(state)) => {
            assert!(state.editing.is_none(), "back to the list");
            assert_eq!(state.gallery.custom.len(), 1, "no tag made");
        }
        _ => panic!("Escape closed Customize Tags"),
    }
    click_in_dialog(&mut ui, "Devoirs");
    click_in_dialog(&mut ui, &t!("common-delete"));
    assert!(gallery(&ui).custom.is_empty());
    click_in_dialog(&mut ui, &t!("common-cancel"));
    assert!(ui.app().dialog.is_none(), "Cancel closes Customize Tags");
    assert_eq!(
        ui.app().editor.tags.custom[0].name,
        "Devoirs",
        "Cancel kept the tag"
    );
    let stored = ui.app().library.setting("custom_tags").unwrap().unwrap();
    assert!(stored.contains("Devoirs"), "{stored}");
}

/// Find Tags groups by page title, Show only unchecked items hides the ticked To Do, and
/// Create Summary Page writes a page listing what the pane shows, under a heading per
/// group, each line linking back to its page.
#[test]
fn tags_summary_groups_filters_and_writes_a_summary_page() {
    use crate::model::{BlockKind, LineTag};
    use egui::Role;
    let mut ui = Ui::start();
    // A section of its own, so the sample library's tags stay out of the summary.
    {
        let app = ui.h.state_mut();
        let section = app
            .library
            .create_section("notebook-school", "Science", "#745cf5")
            .unwrap();
        app.refresh_workspace();
        app.select_section(&section.id, false);
    }
    ui.steps(3);
    ui.new_page("Biology");
    let biology = ui.app().page.as_ref().unwrap().id.clone();
    ui.press(Key::Enter);
    // Bold, so the summary page shows it copies the paragraph from its page.
    ui.press_with(Modifiers::COMMAND, Key::B);
    ui.type_text("Cells");
    ui.press_with(Modifiers::COMMAND, Key::Num2);
    ui.press(Key::Enter);
    ui.type_text("Read chapter 2");
    // Enter carried Important: off with it, then a ticked To Do.
    ui.press_with(Modifiers::COMMAND, Key::Num0);
    ui.press_with(Modifiers::COMMAND, Key::Num1);
    ui.press_with(Modifiers::COMMAND, Key::Num1);
    let blocks = ui.blocks();
    assert_eq!(blocks[0].tags, [LineTag::Important]);
    assert!(blocks[1].tags.is_empty());
    assert_eq!(blocks[1].kind, BlockKind::Todo { checked: true });
    ui.new_page("Maths");
    ui.press(Key::Enter);
    ui.type_text("Pythagoras");
    ui.press_with(Modifiers::COMMAND, Key::Num3);
    assert_eq!(ui.blocks()[0].tags, [LineTag::Question]);

    ui.ribbon_button("Find Tags");
    ui.steps(2);
    let combo = ui.h.get_by_role(Role::ComboBox).rect();
    ui.click(combo.center());
    ui.steps(2);
    ui.click_label_containing(&t!("app-tags-group-title"));
    ui.steps(2);
    let pane = ui.app().tags_pane.as_ref().expect("the Find Tags pane");
    assert_eq!(pane.group_by, super::TagsGroupBy::Title);
    let texts: Vec<&str> = pane.lines.iter().map(|line| line.text.as_str()).collect();
    assert_eq!(texts, ["Cells", "Read chapter 2", "Pythagoras"]);
    // Two headings, in alphabetical order, over the pane's rows.
    let row = ui.h.get_by_label_contains("Cells").rect();
    let heading = |ui: &Ui, title: &str| -> Vec<egui::Rect> {
        ui.h.query_all_by_role_and_label(Role::Label, title)
            .map(|node| node.rect())
            .filter(|rect| rect.left() >= row.left() - 1.0 && rect.right() <= row.right() + 1.0)
            .collect()
    };
    let (first, second) = (heading(&ui, "Biology"), heading(&ui, "Maths"));
    assert!(
        first.len() == 1 && second.len() == 1,
        "{first:?} {second:?}"
    );
    assert!(first[0].top() < row.top() && row.top() < second[0].top());

    // Show only unchecked items hides the ticked To Do.
    let unchecked = t!("app-tags-unchecked-only").into_owned();
    let check =
        ui.h.get_by_role_and_label(Role::CheckBox, &unchecked)
            .rect();
    ui.click(check.center());
    ui.steps(2);
    assert!(ui.app().tags_pane.as_ref().unwrap().unchecked_only);
    assert!(
        ui.h.query_all_by_label_contains("Read chapter 2")
            .next()
            .is_none(),
        "the ticked To Do is hidden"
    );

    // Create Summary Page: a new page listing what the pane shows.
    ui.click_label_containing(&t!("app-tags-create-summary"));
    ui.steps(3);
    assert_eq!(
        ui.app().page.as_ref().unwrap().title,
        t!("app-tags-summary-title")
    );
    let blocks = ui.blocks();
    let texts: Vec<&str> = blocks
        .iter()
        .map(|block| block.text.text.as_str())
        .collect();
    assert_eq!(
        texts,
        ["Biology", "Cells (Biology)", "Maths", "Pythagoras (Maths)"]
    );
    assert_eq!(blocks[0].kind, BlockKind::heading(2));
    assert_eq!(blocks[2].kind, BlockKind::heading(2));
    let cells = &blocks[1];
    assert_eq!(cells.tags, [LineTag::Important], "the line keeps its tag");
    assert!(
        cells.text.style_at(0).bold,
        "the paragraph's own formatting"
    );
    let last = cells.text.text.len() - 1;
    assert_eq!(
        cells.text.style_at(last).link.as_deref(),
        Some(crate::model::page_link(&biology).as_str()),
        "its page's title links back to it"
    );
    ui.shot("s3-tags-summary");
}

/// Customize Tags and its New Tag form in French: every button and its whole label sit
/// inside the dialog, and the longest tag names fit their rows.
#[test]
fn customize_tags_fits_in_french() {
    use egui::Role;
    let mut ui = Ui::start_in(Lang::Fr);
    ui.new_page("Indicateurs");
    ui.press(Key::Enter);
    ui.type_text("Bonjour");
    ui.open_tags_menu();
    ui.click_label_containing(&t!("editor-customize-tags"));
    ui.steps(3);
    let window = egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(1320.0, 900.0));
    // Every text painted: as shown (a text cut to fit ends in "…"), as written, how many
    // lines it took, and where it went.
    let texts = |ui: &Ui| -> Vec<(String, String, usize, egui::Rect)> {
        painted(ui)
            .iter()
            .filter_map(|shape| match shape {
                egui::Shape::Text(text) => Some((
                    text.galley
                        .rows
                        .iter()
                        .map(|row| row.text())
                        .collect::<String>(),
                    text.galley.text().to_string(),
                    text.galley.rows.len(),
                    text.galley.rect.translate(text.pos.to_vec2()),
                )),
                _ => None,
            })
            .collect()
    };
    // A text button: inside the dialog, its label whole on one line inside it.
    let whole = |ui: &Ui, label: &str| {
        let dialog = open_dialog_rect(ui);
        let (button, _) = in_dialog(ui, label).unwrap_or_else(|| panic!("no {label:?}"));
        assert!(
            window.contains_rect(dialog),
            "the dialog {dialog:?} fits the window"
        );
        let (shown, _, lines, text) = texts(ui)
            .into_iter()
            .find(|(_, full, _, rect)| full == label && button.intersects(*rect))
            .unwrap_or_else(|| panic!("{label:?} is not painted on its button"));
        assert_eq!((shown.as_str(), lines), (label, 1), "{label:?} is cut");
        assert!(
            button.expand(0.5).contains_rect(text),
            "{label:?}: {text:?} leaves its button {button:?}"
        );
    };
    // Painted text: inside the dialog.
    let inside = |ui: &Ui, label: &str| {
        let dialog = open_dialog_rect(ui);
        let (_, _, _, text) = texts(ui)
            .into_iter()
            .find(|(_, full, _, _)| full == label)
            .unwrap_or_else(|| panic!("{label:?} is not painted"));
        assert!(
            dialog.contains_rect(text),
            "{label:?}: {text:?} leaves the dialog {dialog:?}"
        );
    };

    for label in [
        t!("app-new-tag"),
        t!("app-modify-tag"),
        t!("common-delete"),
        t!("common-ok"),
        t!("common-cancel"),
    ] {
        whole(&ui, &label);
    }
    for label in [t!("app-move-tag-up"), t!("app-move-tag-down")] {
        assert!(
            in_dialog(&ui, &label).is_some(),
            "no {label:?} in the dialog"
        );
    }
    inside(&ui, &t!("app-customize-tags-title"));
    inside(&ui, &t!("app-customize-tags-note"));
    // Each row has room for its name and its Ctrl shortcut ("À discuter avec le
    // responsable" is the longest), as the row lays them out.
    let gallery = ui.app().editor.tags.clone();
    let first = gallery.label(gallery.order[0]).into_owned();
    let (row, _) = in_dialog(&ui, &first).expect("the first row");
    let width = |text: &str, size: f32| {
        ui.h.ctx.fonts_mut(|fonts| {
            fonts
                .layout_no_wrap(
                    text.to_owned(),
                    crate::theme::ui(size),
                    egui::Color32::BLACK,
                )
                .size()
                .x
        })
    };
    for key in &gallery.order {
        let label = gallery.label(*key);
        let hint = gallery
            .digit(*key)
            .map(crate::editor::tag_hint)
            .unwrap_or_default();
        let mut needed = 32.0 + width(&label, 12.5) + 10.0;
        if !hint.is_empty() {
            needed += width(&hint, 11.0) + 12.0;
        }
        assert!(
            needed <= row.width(),
            "{label} {hint}: needs {needed}, the row is {}",
            row.width()
        );
    }

    // The New Tag form.
    click_in_dialog(&mut ui, &t!("app-new-tag"));
    ui.steps(3);
    for label in [
        t!("app-tag-automatic"),
        t!("app-tag-no-color"),
        t!("common-ok"),
        t!("common-cancel"),
    ] {
        whole(&ui, &label);
    }
    for label in [
        t!("app-tag-name"),
        t!("app-tag-symbol"),
        t!("app-tag-font-color"),
        t!("app-tag-highlight-color"),
        t!("app-tag-preview"),
    ] {
        inside(&ui, &label);
    }
    // Every symbol is a button inside the dialog.
    let symbols = crate::model::TagSymbol::choices();
    for symbol in &symbols {
        let label = match symbol {
            crate::model::TagSymbol::None => t!("app-tag-no-symbol"),
            crate::model::TagSymbol::Checkbox => t!("tag-todo"),
            crate::model::TagSymbol::Tag(tag) => tag.label(),
        };
        assert!(
            ui.h.query_all_by_role_and_label(Role::Button, &label)
                .any(|node| open_dialog_rect(&ui).contains_rect(node.rect())),
            "the symbol {label:?} is not inside the dialog"
        );
    }
    ui.shot("s3-fr-customize-tags");
}

/// Home › Font Color ▾ picks from Office's theme grid, and More Colors… any colour typed as
/// "#rrggbb", both for the words selected; Text Highlight Color ▾ names Office's 15.
#[test]
fn font_color_theme_grid_and_more_colors_colour_the_selection() {
    use super::{ColorTarget, Dialog};
    let mut ui = Ui::start();
    ui.new_page("Organelles");
    ui.press(Key::Enter);
    ui.type_text("Mitochondria");
    ui.press_with(Modifiers::SHIFT, Key::Home);
    let style = |ui: &Ui| ui.blocks()[0].text.style_at(4);
    let font_color = format!("{} ▾", t!("editor-text-color"));

    // Theme Colors: Blue, Accent 1, on top of its column.
    ui.ribbon_button(&font_color);
    ui.click_label("#4472c4");
    assert_eq!(style(&ui).color, Some(Rgb(0x44, 0x72, 0xC4)));
    assert_eq!(ui.app().editor.selection, Some(0..12), "still selected");

    // More Colors… starts from the text's colour; OK colours the words.
    ui.steps(30);
    ui.ribbon_button(&font_color);
    ui.click_label(&format!(
        "{}    {}",
        crate::icons::PALETTE,
        t!("ribbon-more-colors")
    ));
    let Some(Dialog::MoreColors {
        target: ColorTarget::Font,
        hex,
        ..
    }) = ui.app().dialog.as_ref()
    else {
        panic!("More Colors… opens its dialog");
    };
    assert_eq!(hex, "#4472c4");
    let field =
        ui.h.get_all_by_value("#4472c4")
            .last()
            .expect("the colour's field")
            .rect();
    ui.click(field.center());
    ui.press_with(Modifiers::COMMAND, Key::A);
    ui.type_text("#123456");
    ui.steps(30);
    ui.click_label(&t!("common-ok"));
    assert!(ui.app().dialog.is_none(), "OK closes it");
    assert_eq!(style(&ui).color, Some(Rgb(0x12, 0x34, 0x56)));

    // Text Highlight Color: Dark Blue, named as Office names it.
    ui.steps(30);
    ui.ribbon_button(&format!("{} ▾", t!("editor-highlight")));
    ui.click_label(&t!("color-dark-blue"));
    assert_eq!(style(&ui).highlight, Some(Rgb(0, 0, 0x80)));
    ui.steps(30);
    ui.ribbon_button(&font_color);
    ui.shot("s4-font-colours");
    close_menu(&mut ui);
}

/// Options › Cancel, its Esc and its OK: Cancel and Esc put back every setting changed
/// since the dialog opened, and store them again; OK keeps them.
#[test]
fn options_cancel_puts_settings_back() {
    use super::Appearance;
    use crate::ribbon::TabsLayout;
    let mut ui = Ui::start();
    let original = ui.app().appearance;
    assert_ne!(original, Appearance::Dark);
    let open = |ui: &mut Ui| {
        ui.h.state_mut().dialog = Some(super::Dialog::Settings);
        ui.steps(3);
    };
    let horizontal_tabs = |ui: &mut Ui| {
        ui.options_page(&t!("app-options-display"));
        let combo = ui.h.get_by_value(&t!("ribbon-vertical-tabs")).rect();
        ui.click(combo.center());
        ui.click_label(&t!("ribbon-horizontal-tabs"));
        assert_eq!(ui.app().tabs_layout, TabsLayout::Horizontal);
        ui.steps(30);
    };
    let stored = |ui: &Ui| ui.app().library.setting("tabs_layout").unwrap();

    // Horizontal Tabs and the dark theme, then Cancel.
    open(&mut ui);
    horizontal_tabs(&mut ui);
    ui.options_page(&t!("app-options-general"));
    let combo = ui.h.get_by_value(&original.label()).rect();
    ui.click(combo.center());
    ui.click_label(&Appearance::Dark.label());
    assert_eq!(ui.app().appearance, Appearance::Dark, "it shows at once");
    ui.steps(30);
    ui.click_label(&t!("common-cancel"));
    assert!(ui.app().dialog.is_none(), "Cancel closes Options");
    assert_eq!(ui.app().tabs_layout, TabsLayout::Vertical);
    assert_eq!(ui.app().appearance, original);
    assert_eq!(stored(&ui).as_deref(), Some("vertical"), "stored back too");

    // Esc puts them back too.
    ui.steps(30);
    open(&mut ui);
    horizontal_tabs(&mut ui);
    ui.press(Key::Escape);
    assert!(ui.app().dialog.is_none(), "Esc closes Options");
    assert_eq!(ui.app().tabs_layout, TabsLayout::Vertical);

    // OK keeps them.
    ui.steps(30);
    open(&mut ui);
    horizontal_tabs(&mut ui);
    ui.click_label(&t!("common-ok"));
    assert!(ui.app().dialog.is_none());
    assert_eq!(ui.app().tabs_layout, TabsLayout::Horizontal);
    assert_eq!(stored(&ui).as_deref(), Some("horizontal"));
}

/// Picture › Crop on a page picture: its handles keep part of it, Enter keeps the crop,
/// and Esc during another crop leaves the picture as it was.
#[test]
fn cropping_a_picture_keeps_the_part_chosen() {
    use egui_kittest::kittest::NodeT;
    let mut ui = Ui::start();
    ui.new_page("Halves");
    ui.press(Key::Escape);
    // Red on the left, blue on the right.
    let pixels: Vec<u8> = (0..100)
        .flat_map(|_| {
            (0..200).flat_map(|x| {
                if x < 100 {
                    [255, 0, 0, 255]
                } else {
                    [0, 0, 255, 255]
                }
            })
        })
        .collect();
    let image = egui::ColorImage::from_rgba_unmultiplied([200, 100], &pixels);
    ui.h.event(Event::PasteImage(std::sync::Arc::new(image)));
    ui.steps(4);
    let id = ui
        .items()
        .iter()
        .rev()
        .find(|item| matches!(item.kind, CanvasKind::Image { .. }))
        .expect("a picture")
        .id;
    let before = ui.item(id).rect();
    assert_eq!(before.size(), egui::vec2(200.0, 100.0));
    ui.click(ui.screen(before.center()));
    ui.ribbon_tab("Picture");
    let crop_toggled = |ui: &Ui| {
        let bottom = 48.0 + crate::ribbon::panel_height(&ui.app().ribbon) + 4.0;
        ui.h.get_all_by_label("Crop")
            .find(|node| node.rect().top() >= 48.0 && node.rect().bottom() <= bottom)
            .expect("Picture › Crop")
            .accesskit_node()
            .toggled()
            == Some(egui::accesskit::Toggled::True)
    };
    assert!(!crop_toggled(&ui));
    ui.ribbon_button("Crop");
    assert!(ui.app().canvas.cropping.is_some(), "cropping");
    assert!(crop_toggled(&ui), "Crop shows pressed while cropping");

    // The left handle, 100 px to the right: the right half is kept, where it was.
    let left = egui::pos2(before.left(), before.center().y);
    ui.drag(ui.screen(left), ui.screen(left + egui::vec2(100.0, 0.0)));
    ui.shot("s4-crop");
    ui.press(Key::Enter);
    assert!(ui.app().canvas.cropping.is_none(), "Enter keeps the crop");
    let kept = ui.item(id).rect();
    assert!((kept.width() - 100.0).abs() < 0.5, "{kept:?}");
    assert!(
        (kept.left() - (before.left() + 100.0)).abs() < 0.5,
        "{kept:?}"
    );
    let crop = picture_meta(&ui, id).crop.expect("a crop");
    for (got, want) in crop.iter().zip([0.5, 0.0, 1.0, 1.0]) {
        assert!((got - want).abs() < 0.01, "{crop:?}");
    }
    assert!(!crop_toggled(&ui));

    // Esc during a second crop changes nothing.
    ui.steps(30);
    ui.ribbon_button("Crop");
    assert!(ui.app().canvas.cropping.is_some());
    let top = egui::pos2(kept.center().x, kept.top());
    ui.drag(ui.screen(top), ui.screen(top + egui::vec2(0.0, 40.0)));
    ui.press(Key::Escape);
    assert!(ui.app().canvas.cropping.is_none(), "Esc ends cropping");
    assert_eq!(ui.item(id).rect(), kept);
    assert_eq!(picture_meta(&ui, id).crop, Some(crop));
}

/// Draw › Rotate › Rotate Right 45° turns a stroke an eighth about the selection's centre;
/// with a picture selected, the 45° rows are greyed out.
#[test]
fn rotate_45_turns_ink_and_is_greyed_for_pictures() {
    use crate::canvas::{PAGE_CONTENT_TOP, PAGE_LEFT};
    use crate::icons;
    use egui_kittest::kittest::NodeT;
    let mut ui = Ui::start();
    ui.new_page("Eighths");
    ui.ribbon_tab("Draw");
    ui.pick_pen(0);
    let start = egui::pos2(PAGE_LEFT, PAGE_CONTENT_TOP + 360.0);
    ui.drag(ui.screen(start), ui.screen(start + egui::vec2(160.0, 0.0)));
    let ends = |ui: &Ui| {
        let points = &ui.ink()[0].points;
        let first = points.first().unwrap();
        let last = points.last().unwrap();
        (egui::pos2(first[0], first[1]), egui::pos2(last[0], last[1]))
    };
    let (from, to) = ends(&ui);
    assert!((to.y - from.y).abs() < 2.0, "level: {from:?} → {to:?}");
    ui.ribbon_button("Lasso");
    let centre = ui.screen(start + egui::vec2(80.0, 0.0));
    let path: Vec<Pos2> = (0..=24)
        .map(|i| {
            let angle = i as f32 / 24.0 * std::f32::consts::TAU;
            centre + egui::vec2(angle.cos() * 120.0, angle.sin() * 40.0)
        })
        .collect();
    ui.drag_path(&path);
    assert_eq!(ui.selection().strokes, vec![0]);
    ui.ribbon_button("Rotate");
    let right_45 = menu_row_label(icons::ARROW_ROTATE_CLOCKWISE, &t!("ribbon-rotate-right-45"));
    let left_45 = menu_row_label(
        icons::ARROW_ROTATE_COUNTERCLOCKWISE,
        &t!("ribbon-rotate-left-45"),
    );
    let greyed = |ui: &Ui, label: &str| {
        ui.h.get_all_by_label(label)
            .last()
            .unwrap_or_else(|| panic!("no {label:?} row"))
            .accesskit_node()
            .is_disabled()
    };
    assert!(!greyed(&ui, &right_45) && !greyed(&ui, &left_45));
    ui.shot("s4-rotate-menu");
    ui.click_label(&right_45);
    // y grows downwards: a right turn takes the right end down, as far down as across.
    let (from, to) = ends(&ui);
    let span = to - from;
    assert!(
        span.x > 80.0 && span.y > 80.0,
        "diagonal: {from:?} → {to:?}"
    );
    assert!((span.x - span.y).abs() < 6.0, "an eighth: {span:?}");
    assert!(
        (from.lerp(to, 0.5) - (start + egui::vec2(80.0, 0.0))).length() < 3.0,
        "about its centre: {from:?} → {to:?}"
    );

    // A picture selected: no 45° turns.
    ui.h.state_mut().canvas.tool = CanvasTool::Select;
    ui.steps(30);
    let picture = paste_picture(&mut ui);
    let rect = ui.item(picture).rect();
    ui.click(ui.screen(rect.center()));
    assert_eq!(ui.selection().items, vec![picture]);
    // (Picture › Rotate lists only Office's quarter turns and flips.)
    ui.ribbon_tab("Draw");
    ui.ribbon_button("Rotate");
    assert!(greyed(&ui, &right_45), "Rotate Right 45° is greyed");
    assert!(greyed(&ui, &left_45), "Rotate Left 45° is greyed");
    assert!(!greyed(
        &ui,
        &menu_row_label(icons::ARROW_ROTATE_CLOCKWISE, &t!("ribbon-rotate-right"))
    ));
    close_menu(&mut ui);
}

/// Insert › Equation ▾ puts one of Office's ready-made equations in the note; More
/// Symbols… finds € by name and puts it at the caret, staying open for the next one; the
/// Symbol menu then shows it among the recent symbols.
#[test]
fn insert_a_ready_made_equation_and_a_symbol_from_more_symbols() {
    use egui::Role;
    let mut ui = Ui::start();
    ui.new_page("Roots");
    ui.press(Key::Enter);
    ui.type_text("Roots");
    ui.ribbon_tab("Insert");
    ui.ribbon_button("Equation ▾");
    assert!(egui::Popup::is_any_open(&ui.h.ctx), "the gallery");
    ui.shot("s4-equation-gallery");
    ui.click_label("Quadratic Formula");
    assert_eq!(sources(&ui), [crate::math::READY_EQUATIONS[5].1]);
    let note = ui.items()[0].id;
    assert_eq!(equation_blocks(&ui)[0].0, note, "in the note typed in");
    assert!(
        ui.app().editor.equation_block.is_some(),
        "it is selected for editing"
    );
    // Enter ends it, on a new line after it.
    ui.press(Key::Enter);
    assert_eq!(ui.app().editor.equation_block, None);

    ui.steps(30);
    ui.ribbon_button("Symbol");
    ui.click_label_containing(&t!("editor-more-symbols"));
    assert!(matches!(
        ui.app().dialog,
        Some(super::Dialog::Symbols { .. })
    ));
    // The search field takes the keyboard by itself.
    ui.steps(3);
    ui.type_text("euro");
    // Insert: the dialog's button, not the ribbon's tab of the same name.
    let insert = |ui: &Ui| {
        use egui_kittest::kittest::NodeT;
        let label = t!("editor-insert");
        let node =
            ui.h.get_all_by_role_and_label(Role::Button, &label)
                .last()
                .expect("Insert");
        (node.rect(), node.accesskit_node().is_disabled())
    };
    assert!(insert(&ui).1, "greyed until a symbol is picked");
    let cell =
        ui.h.get_all_by_role_and_label(Role::Button, "€")
            .last()
            .expect("€ found by its name")
            .rect();
    ui.click(cell.center());
    ui.steps(30);
    let (rect, greyed) = insert(&ui);
    assert!(!greyed, "€ picked");
    ui.click(rect.center());
    assert!(ui.app().dialog.is_some(), "the dialog stays open");
    let texts = block_texts(&ui, note);
    assert!(texts.iter().any(|text| text == "€"), "{texts:?}");
    assert_eq!(sources(&ui), [crate::math::READY_EQUATIONS[5].1]);
    ui.shot("s4-more-symbols");
    ui.steps(30);
    let close =
        ui.h.get_all_by_role_and_label(Role::Button, &t!("common-close"))
            .last()
            .expect("Close")
            .rect();
    ui.click(close.center());
    assert!(ui.app().dialog.is_none());
    assert_eq!(ui.app().recent_symbols.first(), Some(&'€'));

    // Symbol ▾ again: € heads the recent row, above the usual grid.
    ui.steps(30);
    ui.ribbon_button("Symbol");
    let first = |ui: &Ui, symbol: &str| {
        ui.h.get_all_by_role_and_label(Role::Button, symbol)
            .next()
            .unwrap_or_else(|| panic!("no {symbol}"))
            .rect()
    };
    assert!(
        first(&ui, "€").bottom() <= first(&ui, "±").top(),
        "€ is in the recent row"
    );
    ui.h.get_by_label(&t!("editor-recent-symbols"));
    ui.shot("s4-symbol-recent");
    close_menu(&mut ui);
}

/// The text painted reading `label`, whole on one line: its rect. The last one painted
/// wins (menus come last).
fn whole_label(ui: &Ui, label: &str) -> egui::Rect {
    painted(ui)
        .iter()
        .rev()
        .find_map(|shape| match shape {
            egui::Shape::Text(text) if text.galley.text() == label => {
                let shown: String = text.galley.rows.iter().map(|row| row.text()).collect();
                assert_eq!(
                    (shown.as_str(), text.galley.rows.len()),
                    (label, 1),
                    "{label:?} is cut"
                );
                Some(text.galley.rect.translate(text.pos.to_vec2()))
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("{label:?} is not painted"))
}

/// S4's menus in French: Font Color ▾, the pen's menu and Equation ▾. Each row's label is
/// whole inside its row, the rows are the menu's width, and the headings sit inside it.
#[test]
fn s4_menus_fit_in_french() {
    use crate::icons;
    use egui::Role;
    let mut ui = Ui::start_in(Lang::Fr);
    ui.new_page("Couleurs");
    ui.press(Key::Enter);
    ui.type_text("Bonjour");
    // A row by its accessible name, with the label painted in it: the label is whole and
    // inside the row. Returns the row.
    let row_fits = |ui: &Ui, name: &str, label: &str| {
        let row =
            ui.h.get_all_by_role_and_label(Role::Button, name)
                .last()
                .unwrap_or_else(|| panic!("no row {name:?}"))
                .rect();
        let text = whole_label(ui, label);
        assert!(
            row.expand(0.5).contains_rect(text),
            "{label:?}: {text:?} leaves its row {row:?}"
        );
        row
    };
    // Rows (glyph, label), as wide as one another, then headings between their edges.
    let check = |ui: &Ui, rows: &[(&str, String)], headings: &[String]| {
        let rects: Vec<egui::Rect> = rows
            .iter()
            .map(|(glyph, label)| row_fits(ui, &menu_row_label(glyph, label), label))
            .collect();
        for rect in &rects {
            assert!(
                (rect.width() - rects[0].width()).abs() < 1.0,
                "{rect:?} vs {:?}: a row is wider than the menu",
                rects[0]
            );
        }
        let (left, right) = (rects[0].left(), rects[0].right());
        for heading in headings {
            let text = whole_label(ui, heading);
            assert!(
                text.left() >= left - 0.5 && text.right() <= right + 0.5,
                "{heading:?}: {text:?} leaves the menu ({left}–{right})"
            );
        }
        rects[0]
    };

    // Accueil › Couleur de police ▾.
    ui.ribbon_button(&format!("{} ▾", t!("editor-text-color")));
    check(
        &ui,
        &[
            (icons::SQUARE, t!("editor-default-color").into_owned()),
            (icons::PALETTE, t!("ribbon-more-colors").into_owned()),
        ],
        &[
            t!("editor-theme-colors").into_owned(),
            t!("editor-standard-colors").into_owned(),
        ],
    );
    ui.shot("fr-s4-font-color-menu");
    close_menu(&mut ui);

    // Dessin › the pen in hand, clicked again, with a recent colour and the custom
    // thickness open.
    ui.h.state_mut().canvas.recent_colors = vec![Rgb(0x12, 0x34, 0x56)];
    ui.pick_pen(1);
    let pen = crate::canvas::pen_label(&ui.app().canvas.pens[1]);
    ui.ribbon_button(&pen);
    assert!(egui::Popup::is_any_open(&ui.h.ctx), "the pen's menu");
    let custom =
        ui.h.get_all_by_role_and_label(Role::Button, &t!("ribbon-custom-thickness"))
            .last()
            .expect("the custom thickness")
            .rect();
    ui.click(custom.center());
    let menu = check(
        &ui,
        &[
            (icons::PALETTE, t!("ribbon-more-colors").into_owned()),
            (icons::TRASH, t!("ribbon-remove-pen").into_owned()),
        ],
        &[
            t!("ribbon-thickness").into_owned(),
            t!("ribbon-recent-colors").into_owned(),
            t!("ribbon-colors").into_owned(),
        ],
    );
    let slider =
        ui.h.get_all_by_role(Role::Slider)
            .last()
            .expect("the thickness slider")
            .rect();
    assert!(
        slider.left() >= menu.left() - 0.5 && slider.right() <= menu.right() + 0.5,
        "the slider {slider:?} fits the menu {menu:?}"
    );
    ui.shot("fr-s4-pen-menu");
    close_menu(&mut ui);
    ui.h.state_mut().canvas.tool = CanvasTool::Select;

    // Insertion › Équation ▾: each ready-made equation's name inside its row, scrolling
    // down to the ones below the fold.
    ui.steps(30);
    ui.ribbon_tab(&RibbonTab::Insert.label());
    ui.ribbon_button(&format!("{} ▾", t!("ribbon-equation")));
    let new = check(
        &ui,
        &[(
            icons::MATH_OPERATIONS,
            t!("ribbon-insert-new-equation").into_owned(),
        )],
        &[],
    );
    ui.shot("fr-s4-equation-gallery");
    let mut unseen: Vec<String> = crate::math::READY_EQUATIONS
        .iter()
        .map(|(id, _)| crate::i18n::tr(id).into_owned())
        .collect();
    for _ in 0..4 {
        unseen.retain(|name| {
            let painted = painted(&ui).iter().any(
                |shape| matches!(shape, egui::Shape::Text(text) if text.galley.text() == name),
            );
            if painted {
                let row = row_fits(&ui, name, name);
                assert!(row.right() <= new.right() + 0.5, "{name:?}: {row:?}");
            }
            !painted
        });
        if unseen.is_empty() {
            break;
        }
        ui.wheel(
            new.center() - egui::vec2(0.0, 60.0),
            egui::vec2(0.0, -200.0),
            Modifiers::NONE,
        );
    }
    assert!(unseen.is_empty(), "never shown: {unseen:?}");
    close_menu(&mut ui);
}

/// Esc on a ribbon menu only closes it, as in the desktop notebook: the note keeps the caret, so Home's
/// text commands stay on and the next letter lands in the note. The next Esc leaves it.
#[test]
fn escape_on_a_ribbon_menu_keeps_the_caret_in_the_note() {
    let mut ui = Ui::start();
    ui.new_page("Escape");
    ui.press(Key::Enter);
    ui.type_text("Notes");
    for menu in [t!("editor-text-color"), t!("editor-highlight")] {
        let arrow = format!("{menu} ▾");
        ui.steps(30);
        ui.ribbon_button(&arrow);
        assert!(egui::Popup::is_any_open(&ui.h.ctx), "{arrow:?} opened");
        ui.press(Key::Escape);
        assert!(!egui::Popup::is_any_open(&ui.h.ctx), "Esc closed {arrow:?}");
        assert!(
            ui.app().text_state().is_some(),
            "still typing after {arrow:?}"
        );
        ui.type_text("!");
    }
    assert_eq!(ui.blocks()[0].text.text, "Notes!!");
    // With no menu open, Esc leaves the note as before.
    ui.press(Key::Escape);
    assert!(ui.app().text_state().is_none(), "Esc leaves the note");
}

/// More Symbols…'s search finds a name anywhere in it ("arrow" finds → by "rightarrow"),
/// and says so when nothing matches instead of showing an empty grid.
#[test]
fn more_symbols_search_matches_inside_names() {
    use egui::Role;
    let mut ui = Ui::start();
    ui.new_page("Arrows");
    ui.press(Key::Enter);
    ui.type_text("Go");
    ui.ribbon_tab("Insert");
    ui.ribbon_button("Symbol");
    ui.click_label_containing(&t!("editor-more-symbols"));
    ui.steps(3);
    ui.type_text("arrow");
    ui.steps(2);
    assert!(
        ui.h.get_all_by_role_and_label(Role::Button, "→")
            .next()
            .is_some(),
        "→ found by rightarrow"
    );
    assert!(ui.h.query_by_label(&t!("editor-no-symbols")).is_none());
    ui.type_text("zz");
    ui.steps(2);
    ui.h.get_by_label(&t!("editor-no-symbols"));
    assert!(ui
        .h
        .query_all_by_role_and_label(Role::Button, "→")
        .next()
        .is_none());
    ui.h.state_mut().dialog = None;
    ui.steps(2);
}

/// Draw › pen ▾'s custom thickness changes the pen only once it is moved: a width between
/// its 0.05 mm steps, merely shown, stays as it is and the menu stays open.
#[test]
fn custom_thickness_shows_a_width_between_its_steps_unchanged() {
    use egui::Role;
    let mut ui = Ui::start();
    ui.new_page("Widths");
    // 0.275 mm, between the slider's steps.
    ui.h.state_mut().canvas.pens[1].width = 1.1;
    ui.pick_pen(1);
    assert_eq!(ui.app().canvas.ink_width, 1.1);
    let pen = crate::canvas::pen_label(&ui.app().canvas.pens[1]);
    ui.ribbon_button(&pen);
    assert!(egui::Popup::is_any_open(&ui.h.ctx), "the pen's menu");
    if ui.h.query_by_role(Role::Slider).is_none() {
        let custom =
            ui.h.get_all_by_role_and_label(Role::Button, &t!("ribbon-custom-thickness"))
                .last()
                .expect("the custom thickness")
                .rect();
        ui.click(custom.center());
    }
    ui.steps(3);
    ui.h.get_by_role(Role::Slider);
    assert!(egui::Popup::is_any_open(&ui.h.ctx), "the menu stays open");
    assert_eq!(ui.app().canvas.pens[1].width, 1.1);
    assert_eq!(ui.app().canvas.ink_width, 1.1);
    close_menu(&mut ui);
}

/// Options › Cancel puts back every setting the dialog can change, the theme, the ribbon,
/// the language and the navigation pane included, whatever was changed in it.
#[test]
fn options_cancel_puts_back_every_setting() {
    use super::{Appearance, DialogAction, ScopeKind};
    use crate::ribbon::{Cmd, RibbonLayout, TabsLayout};
    let mut ui = Ui::start();
    ui.h.state_mut().show_library = false;
    ui.steps(2);
    let ctx = ui.h.ctx.clone();
    let app = ui.h.state_mut();
    let before = app.options_snapshot();
    let actions = [
        DialogAction::SetAppearance(if before.appearance == Appearance::Dark {
            Appearance::Light
        } else {
            Appearance::Dark
        }),
        DialogAction::Run(Cmd::RibbonLayout(match before.ribbon_layout {
            RibbonLayout::Classic => RibbonLayout::Simplified,
            RibbonLayout::Simplified => RibbonLayout::Classic,
        })),
        DialogAction::Run(Cmd::TabsLayout(match before.tabs_layout {
            TabsLayout::Vertical => TabsLayout::Horizontal,
            TabsLayout::Horizontal => TabsLayout::Vertical,
        })),
        DialogAction::SetPageListLeft(!before.page_list_left),
        DialogAction::SetNotebooksPinned(!before.notebooks_pinned),
        DialogAction::SetScrollbarLeft(!before.scrollbar_left),
        DialogAction::SetSpellLanguage(if before.spell_language == "off" {
            "auto".into()
        } else {
            "off".into()
        }),
        DialogAction::SetUiLanguage(if before.ui_language == Some(Lang::Fr) {
            Some(Lang::En)
        } else {
            Some(Lang::Fr)
        }),
        DialogAction::SetSearchScope(if before.search_scope == ScopeKind::Section {
            ScopeKind::Notebook
        } else {
            ScopeKind::Section
        }),
        DialogAction::SetPrintoutBackground(!before.printout_background),
        DialogAction::SetMiniToolbar(!before.mini_on_selection),
        DialogAction::SetAutoCalc(!before.auto_calc),
    ];
    for action in actions {
        app.apply_dialog_action(action, &ctx);
    }
    let changed = app.options_snapshot();
    let differs = [
        changed.appearance != before.appearance,
        changed.ribbon_layout != before.ribbon_layout,
        changed.tabs_layout != before.tabs_layout,
        changed.page_list_left != before.page_list_left,
        changed.notebooks_pinned != before.notebooks_pinned,
        changed.scrollbar_left != before.scrollbar_left,
        changed.spell_language != before.spell_language,
        changed.ui_language != before.ui_language,
        changed.search_scope != before.search_scope,
        changed.printout_background != before.printout_background,
        changed.mini_on_selection != before.mini_on_selection,
        changed.auto_calc != before.auto_calc,
        changed.show_library != before.show_library,
    ];
    assert!(differs.iter().all(|&differs| differs), "{differs:?}");
    app.restore_options(before.clone(), &ctx);
    assert_eq!(app.options_snapshot(), before);
    // Stored back too: a restart reads them as they were.
    let ribbon = app.library.setting("ribbon_layout").unwrap();
    let language = app.library.setting("ui_language").unwrap();
    assert_eq!(
        ribbon.as_deref().unwrap_or("classic") == "classic",
        before.ribbon_layout == RibbonLayout::Classic,
        "{ribbon:?}"
    );
    assert_eq!(
        language.as_deref().filter(|code| !code.is_empty()),
        before.ui_language.map(Lang::code),
        "{language:?}"
    );
    ui.steps(2);
}

// S5: Restart / Continue / Customize Numbering, Paragraph Spacing Options… and deleted
// sections in the Recycle Bin.

/// Each list item's text and the number it shows, as the note draws it.
fn list_items(ui: &Ui) -> Vec<(String, String)> {
    let blocks = ui.blocks();
    blocks
        .iter()
        .zip(crate::model::list_numbers(&blocks))
        .filter_map(|(block, number)| {
            let marker = block.list_marker()?;
            Some((block.text.text.clone(), marker.text(number)))
        })
        .collect()
}

/// Types `value` into the open dialog's number field named `label`, as a click on a
/// DragValue lets you, then leaves it with Tab.
fn type_in_field(ui: &mut Ui, label: &str, value: &str) {
    let dialog = open_dialog_rect(ui);
    let field =
        ui.h.get_all_by_role_and_label(egui::Role::SpinButton, label)
            .find(|node| dialog.contains_rect(node.rect()))
            .unwrap_or_else(|| panic!("no {label:?} field in the dialog"))
            .rect();
    ui.click(field.center());
    ui.press_with(Modifiers::COMMAND, Key::A);
    ui.type_text(value);
    ui.press(Key::Tab);
}

/// Clicks the open dialog's OK (the last one: the panes behind have none).
fn click_dialog_ok(ui: &mut Ui) {
    let ok =
        ui.h.get_all_by_role_and_label(egui::Role::Button, &t!("common-ok"))
            .last()
            .expect("the dialog's OK")
            .rect();
    ui.click(ok.center());
    ui.steps(3);
    assert!(ui.app().dialog.is_none(), "OK closed the dialog");
}

/// A numbered item's text menu has Restart Numbering and Continue Numbering, and Home ›
/// Numbering ▾ › Customize Numbering… picks a format and where the count starts, for the
/// selected items, as in the desktop notebook.
#[test]
fn restart_continue_and_customize_numbering() {
    let mut ui = Ui::start();
    ui.new_page("Steps");
    ui.press(Key::Enter);
    ui.type_text("1. ");
    ui.type_text("Mix");
    ui.press(Key::Enter);
    ui.type_text("Bake");
    // Enter on an empty item: a plain line between the two lists.
    ui.press(Key::Enter);
    ui.press(Key::Enter);
    ui.type_text("Meanwhile");
    ui.press(Key::Enter);
    ui.type_text("1. ");
    ui.type_text("Cool");
    ui.press(Key::Enter);
    ui.type_text("Serve");
    let item = |text: &str, marker: &str| (text.to_string(), marker.to_string());
    assert_eq!(
        list_items(&ui),
        [
            item("Mix", "1."),
            item("Bake", "2."),
            item("Cool", "1."),
            item("Serve", "2.")
        ]
    );
    let note = ui.items()[0].id;
    let cool = ui.blocks()[3].id;
    let numbers = |ui: &Ui| crate::model::list_numbers(&ui.blocks());

    // Continue Numbering carries on from the list above; Restart Numbering starts again.
    ui.right_click(ui.text_point(note, cool, 2));
    ui.click_label_containing(&t!("editor-continue-numbering"));
    assert_eq!(numbers(&ui), [1, 2, 0, 3, 4]);
    ui.steps(30);
    ui.right_click(ui.text_point(note, cool, 2));
    ui.click_label_containing(&t!("editor-restart-numbering"));
    assert_eq!(numbers(&ui), [1, 2, 0, 1, 2]);
    ui.steps(30);
    ui.right_click(ui.text_point(note, cool, 2));
    ui.click_label_containing(&t!("editor-continue-numbering"));
    assert_eq!(numbers(&ui), [1, 2, 0, 3, 4]);
    // A plain paragraph's menu has neither.
    ui.steps(30);
    let meanwhile = ui.blocks()[2].id;
    ui.right_click(ui.text_point(note, meanwhile, 2));
    assert!(ui
        .h
        .query_all_by_label_contains(&t!("editor-restart-numbering"))
        .next()
        .is_none());
    close_menu(&mut ui);

    // Cool and Serve selected, the caret in Cool: Numbering ▾ › Customize Numbering…
    ui.steps(30);
    ui.click(ui.text_point(note, ui.blocks()[4].id, 5));
    ui.press_with(Modifiers::COMMAND | Modifiers::SHIFT, Key::ArrowUp);
    ui.press_with(Modifiers::COMMAND | Modifiers::SHIFT, Key::ArrowUp);
    assert_eq!(ui.app().editor.active_block, Some(cool));
    ui.ribbon_button(&format!("{} ▾", t!("ribbon-numbering")));
    ui.click_label_containing(&t!("ribbon-customize-numbering"));
    ui.steps(3);
    assert!(
        matches!(
            ui.app().dialog,
            Some(super::Dialog::CustomizeNumbering {
                marker: crate::model::ListMarker::Decimal,
                start: 3
            })
        ),
        "the dialog starts from the item at the caret"
    );
    // Start at 5, then the a. format, whose tile shows the item's number: "e.".
    type_in_field(&mut ui, &t!("ribbon-numbering-start-at"), "5");
    let tile =
        ui.h.get_all_by_role_and_label(egui::Role::Button, "e.")
            .last()
            .expect("the a. format, at 5")
            .rect();
    ui.click(tile.center());
    ui.shot("s5-customize-numbering");
    click_dialog_ok(&mut ui);
    assert_eq!(
        list_items(&ui),
        [
            item("Mix", "1."),
            item("Bake", "2."),
            item("Cool", "e."),
            item("Serve", "f.")
        ]
    );
    assert!(shown(&ui, "e.") && shown(&ui, "f."), "the note draws them");
    ui.shot("s5-numbering");
}

/// Home › Paragraph Alignment ▾ › Paragraph Spacing Options… puts space after the
/// paragraph: the next one moves down, the spacing is saved with the page, and one Ctrl+Z
/// takes it back.
#[test]
fn paragraph_spacing_moves_text_and_sticks() {
    let mut ui = Ui::start();
    ui.new_page("Spaced");
    ui.press(Key::Enter);
    ui.type_text("First");
    ui.press(Key::Enter);
    ui.type_text("Second");
    let note = ui.items()[0].id;
    let (first, second) = (ui.blocks()[0].id, ui.blocks()[1].id);
    let top = |ui: &Ui| ui.row(note, second).rect.top();
    let before = top(&ui);
    // The caret in the first paragraph.
    ui.click(ui.text_point(note, first, 2));
    ui.steps(30);
    ui.ribbon_button(&t!("editor-paragraph-alignment"));
    ui.click_label_containing(&t!("editor-paragraph-spacing"));
    ui.steps(3);
    assert!(matches!(
        ui.app().dialog,
        Some(super::Dialog::ParagraphSpacing {
            before: 0.0,
            after: 0.0,
            line: 0.0
        })
    ));
    type_in_field(&mut ui, &t!("editor-spacing-after"), "12");
    ui.shot("s5-paragraph-spacing");
    click_dialog_ok(&mut ui);
    let spacing = ui.blocks()[0].spacing;
    assert_eq!(spacing.after, 12.0);
    assert_eq!((spacing.before, spacing.line_at_least), (0.0, 0.0));
    assert_eq!(
        ui.blocks()[1].spacing,
        Default::default(),
        "only the caret's"
    );
    // 12 pt is 16 px (4/3 px a point); the page has no rule lines here.
    assert!(
        (top(&ui) - before - 16.0).abs() < 0.5,
        "{} then {}",
        before,
        top(&ui)
    );
    // Saved with the page.
    save(&mut ui);
    let id = ui.app().page.as_ref().unwrap().id.clone();
    let stored = ui.app().library.get_page(&id).unwrap().unwrap();
    let doc = stored.canvas.items[0].doc().unwrap().clone();
    assert_eq!(doc.blocks[0].spacing.after, 12.0);

    ui.press_with(Modifiers::COMMAND, Key::Z);
    assert_eq!(ui.blocks()[0].spacing, Default::default());
    assert!((top(&ui) - before).abs() < 0.5, "back where it was");
}

/// Deleting a section sends it to the Notebook Recycle Bin with its page, as the desktop notebook does:
/// the bin lists it above its pages, and Restore puts it back in the navigation pane.
#[test]
fn a_deleted_section_waits_in_the_recycle_bin() {
    use egui::Role;
    let mut ui = Ui::start();
    let science = "Science";
    let nav_row = |ui: &Ui| {
        ui.h.query_all_by_role_and_label(Role::Button, science)
            .next()
            .map(|node| node.rect())
    };
    ui.right_click(
        nav_row(&ui)
            .expect("Science in the navigation pane")
            .center(),
    );
    click_menu_row(&mut ui, &t!("common-delete"), "");
    ui.steps(3);
    ui.h.get_by_label(&t!("dialog-delete-section-body"));
    let confirm =
        ui.h.get_all_by_role_and_label(Role::Button, &t!("dialog-delete-section"))
            .last()
            .expect("the dialog's Delete")
            .rect();
    ui.click(confirm.center());
    ui.steps(3);
    assert!(nav_row(&ui).is_none(), "gone from the navigation pane");
    assert_eq!(ui.app().toast.message, t!("toast-section-deleted"));

    // History › Notebook Recycle Bin: under "Deleted sections", with its notebook and page
    // count.
    ui.ribbon_tab("History");
    ui.ribbon_button("Notebook Recycle Bin");
    assert_eq!(ui.app().view, super::View::Trash);
    let heading = whole_label(&ui, &t!("app-trash-sections"));
    let row = ui.h.get_by_role_and_label(Role::ListItem, science).rect();
    assert!(heading.bottom() <= row.top(), "{heading:?} tops {row:?}");
    let detail = whole_label(&ui, "School · 1 page");
    assert!(row.contains_rect(detail), "{detail:?} in {row:?}");
    assert_eq!(ui.app().trashed_sections.len(), 1);
    assert_eq!(ui.app().trashed_sections[0].page_count, 1);
    // Its page is not listed loose in the bin.
    assert!(ui
        .h
        .query_all_by_role_and_label(Role::ListItem, "Plant cell")
        .next()
        .is_none());
    ui.shot("s5-section-bin");

    // A click only picks it; its menu's Restore puts it back with its page.
    ui.click(row.center());
    assert!(nav_row(&ui).is_none(), "a click restores nothing");
    ui.steps(30);
    ui.right_click(row.center());
    click_menu_row(&mut ui, &t!("common-restore"), "");
    ui.steps(3);
    assert!(ui.app().trashed_sections.is_empty());
    assert!(nav_row(&ui).is_some(), "back in the navigation pane");
    let pages = ui
        .app()
        .library
        .page_list(crate::db::PageFilter::Section("section-science"), "", 100)
        .unwrap();
    let titles: Vec<&str> = pages.iter().map(|page| page.title.as_str()).collect();
    assert_eq!(titles, ["Plant cell"]);
}

/// A page of a section in the Recycle Bin, open (picked loose in the bin, or the page last
/// open at a start): Ctrl+N puts the new page in a section that is shown, never out of
/// sight in the bin; restoring the section takes the open page out of the bin with it;
/// deleting the section forever closes it.
#[test]
fn an_open_page_follows_its_section_in_the_bin() {
    use egui::Role;
    let mut ui = Ui::start();
    let science = "section-science";
    let plant = ui
        .app()
        .library
        .page_list(crate::db::PageFilter::Section(science), "", 100)
        .unwrap()[0]
        .id
        .clone();
    let old = {
        let app = ui.h.state_mut();
        let old = app
            .library
            .create_canvas_page(science, "Old notes")
            .unwrap()
            .id;
        app.library.trash_page(&old).unwrap();
        app.library.trash_section(science).unwrap();
        app.refresh_workspace();
        app.after_container_deleted();
        old
    };
    ui.steps(3);
    ui.ribbon_tab("History");
    ui.ribbon_button("Notebook Recycle Bin");
    let section_row = |ui: &Ui| ui.h.get_by_role_and_label(Role::ListItem, "Science").rect();

    // "Old notes" was deleted on its own: it is in the bin loose, and opens from there.
    ui.click(
        ui.h.get_by_role_and_label(Role::ListItem, "Old notes")
            .rect()
            .center(),
    );
    ui.steps(3);
    assert_eq!(ui.app().page.as_ref().unwrap().id, old);
    ui.press_with(Modifiers::COMMAND, Key::N);
    ui.steps(3);
    let new = ui.app().page.clone().unwrap();
    assert_ne!(new.id, old, "a new page");
    assert_ne!(new.section_id, science, "not in the section in the bin");
    assert!(ui.app().find_section(&new.section_id).is_some());

    // Plant cell, the page last open, comes back at the next start in the bin, where its
    // section is; Restore takes it out with the section.
    let reopen = |ui: &mut Ui| {
        let ctx = ui.h.ctx.clone();
        let app = ui.h.state_mut();
        app.library.set_setting("last_page_id", &plant).unwrap();
        app.restore(&ctx);
        ui.steps(3);
        assert!(ui
            .app()
            .page
            .as_ref()
            .is_some_and(|page| page.id == plant && page.trashed));
        assert_eq!(ui.app().view, super::View::Trash);
    };
    reopen(&mut ui);
    ui.right_click(section_row(&ui).center());
    click_menu_row(&mut ui, &t!("common-restore"), "");
    ui.steps(3);
    let open = ui.app().page.as_ref().unwrap();
    assert!(open.id == plant && !open.trashed, "out of the bin");
    let stored = ui.app().library.get_page(&old).unwrap().unwrap();
    assert!(stored.trashed, "a page deleted on its own stays in the bin");

    // In the bin again, then deleted forever: the open page goes with it.
    {
        let app = ui.h.state_mut();
        app.library.trash_section(science).unwrap();
        app.refresh_workspace();
    }
    reopen(&mut ui);
    ui.right_click(section_row(&ui).center());
    click_menu_row(&mut ui, &t!("common-delete-forever"), "");
    ui.steps(3);
    let confirm =
        ui.h.get_all_by_role_and_label(Role::Button, &t!("common-delete-forever"))
            .last()
            .expect("the dialog's Delete Forever")
            .rect();
    ui.click(confirm.center());
    ui.steps(3);
    assert!(ui.app().library.get_page(&plant).unwrap().is_none());
    assert!(ui.app().page.is_none(), "the deleted page is not left open");
    assert!(ui.app().trashed_sections.is_empty());
}

/// S5's dialogs and the bin's sections in French: every label, field and button inside
/// its dialog, the labels whole.
#[test]
fn s5_dialogs_fit_in_french() {
    use egui::Role;
    let mut ui = Ui::start_in(Lang::Fr);
    ui.new_page("Liste");
    ui.press(Key::Enter);
    ui.type_text("1. ");
    ui.type_text("Un");
    // Inside the open dialog: each label painted whole, each field and button.
    let fits = |ui: &Ui, labels: &[String], fields: &[String], buttons: &[String]| {
        let dialog = open_dialog_rect(ui);
        for label in labels {
            let text = whole_label(ui, label);
            assert!(
                dialog.contains_rect(text),
                "{label:?}: {text:?} leaves {dialog:?}"
            );
        }
        for field in fields {
            assert!(
                ui.h.query_all_by_role_and_label(Role::SpinButton, field)
                    .any(|node| dialog.contains_rect(node.rect())),
                "no {field:?} field inside {dialog:?}"
            );
        }
        for button in buttons {
            let (rect, _) = in_dialog(ui, button).unwrap_or_else(|| panic!("no {button:?}"));
            let text = whole_label(ui, button);
            assert!(
                rect.expand(0.5).contains_rect(text),
                "{button:?}: {text:?} leaves {rect:?}"
            );
        }
    };
    let buttons = [
        t!("common-ok").into_owned(),
        t!("common-cancel").into_owned(),
    ];

    // Accueil › Numérotation ▾ › Personnaliser la numérotation…
    ui.ribbon_button(&format!("{} ▾", t!("ribbon-numbering")));
    ui.click_label_containing(&t!("ribbon-customize-numbering"));
    ui.steps(3);
    let start = t!("ribbon-numbering-start-at").into_owned();
    fits(
        &ui,
        &[
            t!("ribbon-customize-numbering-title").into_owned(),
            start.clone(),
        ],
        std::slice::from_ref(&start),
        &buttons,
    );
    let dialog = open_dialog_rect(&ui);
    for marker in crate::model::ListMarker::NUMBERS {
        let tile = in_dialog(&ui, &marker.text(1))
            .unwrap_or_else(|| panic!("no {marker:?} tile"))
            .0;
        assert!(dialog.contains_rect(tile), "{marker:?}: {tile:?}");
    }
    ui.shot("fr-s5-customize-numbering");
    let cancel = in_dialog(&ui, &t!("common-cancel")).unwrap().0;
    ui.click(cancel.center());
    ui.steps(3);

    // Accueil › Alignement ▾ › Options d’espacement des paragraphes…
    ui.steps(30);
    ui.ribbon_button(&t!("editor-paragraph-alignment"));
    ui.click_label_containing(&t!("editor-paragraph-spacing"));
    ui.steps(3);
    let rows = [
        t!("editor-spacing-before").into_owned(),
        t!("editor-spacing-after").into_owned(),
        t!("editor-spacing-line").into_owned(),
    ];
    let mut labels = rows.to_vec();
    labels.push(t!("editor-paragraph-spacing-title").into_owned());
    fits(&ui, &labels, &rows, &buttons);
    ui.shot("fr-s5-paragraph-spacing");
    let cancel = in_dialog(&ui, &t!("common-cancel")).unwrap().0;
    ui.click(cancel.center());
    ui.steps(3);

    // A section sent to the bin: Historique › Corbeille du bloc-notes lists it, its name
    // and detail whole in its row.
    let section = "SVT";
    ui.h.state_mut()
        .library
        .trash_section("section-science")
        .unwrap();
    ui.h.state_mut().refresh_workspace();
    ui.ribbon_tab(&RibbonTab::History.label());
    ui.ribbon_button(&t!("app-recycle-bin"));
    assert_eq!(ui.app().view, super::View::Trash);
    let row = ui.h.get_by_role_and_label(Role::ListItem, section).rect();
    whole_label(&ui, &t!("app-trash-sections"));
    let detail = t!("app-trash-section-detail", notebook = "École", count = 1).into_owned();
    for text in [section.to_string(), detail] {
        let rect = whole_label(&ui, &text);
        assert!(row.contains_rect(rect), "{text:?}: {rect:?} leaves {row:?}");
    }
    ui.shot("fr-s5-section-bin");
}
