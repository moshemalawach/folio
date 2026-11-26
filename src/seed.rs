use rusqlite::{params, Connection, Transaction};

use crate::canvas::NOTE_PAD_X;
use crate::db::DbResult;
use crate::i18n::Lang;
use crate::model::{
    now_ms, Block, BlockKind, CanvasDoc, CanvasItem, CanvasKind, Document, InkStroke, InlineStyle,
    LineTag, Milestone, PageStyle, PictureMeta, Rgb, RuleLines, ShapeKind, StyledText, TableBlock,
    PAGE_CONTENT_TOP as TOP, PAGE_LEFT as LEFT,
};

const DAY: i64 = 24 * 60 * 60 * 1000;
const MINUTE: i64 = 60 * 1000;

mod fr;

/// A file the sample library needs in the attachments folder: a picture, a printout.
/// `install` returns them and `Library::open_at` writes them next to the database.
pub struct SeedFile {
    pub stored_path: String,
    pub bytes: Vec<u8>,
}

/// The sample library's words in one language. Ids, colours, layout and the shape of
/// every outline are shared (the consts next to each page's builder), so the two
/// libraries differ only in their words; tests and links rely on that.
struct SeedText {
    notebook_school: &'static str,
    notebook_personal: &'static str,
    group_last_year: &'static str,
    section_this_week: &'static str,
    section_history: &'static str,
    section_science: &'static str,
    /// The pupil's own-language class: English, or Français.
    section_language: &'static str,
    /// The foreign-language class: French, or Anglais.
    section_foreign: &'static str,
    section_maths: &'static str,
    section_last_year: &'static str,
    section_journal: &'static str,
    section_recipes: &'static str,
    section_quick: &'static str,
    /// My week.
    week: WeekText,
    /// The Middle Ages.
    ages: AgesText,
    /// Castles.
    castles: CastlesText,
    /// Plant cell.
    cell: CellText,
    /// Figures of speech.
    figures: FiguresText,
    /// Food vocabulary.
    vocab: VocabText,
    /// Pythagoras’ theorem.
    pythagoras: PythagorasText,
    /// Worksheet: Pythagoras.
    worksheet: WorksheetText,
    /// Rivers of the world.
    rivers: RiversText,
    /// A quiet Saturday.
    journal: JournalText,
    /// Pancakes.
    pancakes: PancakesText,
    /// Before Friday.
    quick: QuickText,
}

static EN: SeedText = SeedText {
    notebook_school: "School",
    notebook_personal: "Personal",
    group_last_year: "Last year",
    section_this_week: "This week",
    section_history: "History",
    section_science: "Science",
    section_language: "English",
    section_foreign: "French",
    section_maths: "Maths",
    section_last_year: "Geography",
    section_journal: "Journal",
    section_recipes: "Recipes",
    section_quick: "Quick notes",
    week: WeekText {
        title: "My week",
        plan: [
            "What matters this week",
            "Three things, not thirty. Everything else can wait until Friday.",
            "Finish the **history** outline",
            "Print the science worksheet",
            "Revise for Thursday’s maths test",
            "Ask who does what for the group presentation",
        ],
        reminders: [
            "Bring the blue folder on Thursday.",
            "PE kit on Monday and Wednesday.",
        ],
        term: [
            ["September", "Week 1", "Back to school"],
            ["October", "Week 6", "History test"],
            ["December", "Week 15", "Science fair"],
        ],
    },
    ages: AgesText {
        title: "The Middle Ages",
        notes: [
            "The Middle Ages (476–1492)",
            "About a thousand years, from the fall of Rome to the great voyages of discovery.",
            "**Society:** the three orders",
            "those who pray: the clergy",
            "those who fight: the knights and lords",
            "those who work: the peasants, almost everyone",
            "**Feudalism**",
            "a lord gives land, a fief, to a vassal",
            "the vassal swears loyalty and fights for him",
            "**Fief:** land given in exchange for loyalty.",
            "Why did castles change from wood to stone?",
        ],
        frise: [
            ["476", "Fall of Rome", "The western empire ends"],
            ["800", "Charlemagne", "Crowned emperor"],
            ["1066", "Hastings", "Norman conquest"],
            ["1347", "Black Death", "Plague reaches Europe"],
        ],
    },
    castles: CastlesText {
        title: "Castles",
        notes: [
            "A castle protects the lord, his family and the villagers around it.",
            "the **keep**: the tallest tower and the last refuge",
            "the curtain wall and its battlements",
            "the moat and the drawbridge",
            "Label the drawing for Friday",
        ],
        caption: "My castle, drawn in class",
        picture_name: "castle.png",
    },
    cell: CellText {
        title: "Plant cell",
        intro: [
            "Only plant cells have a wall and chloroplasts.",
            "Chloroplasts use sunlight to make sugar: that is photosynthesis.",
        ],
        cell: "Plant cell",
        parts: ["Nucleus", "Cell wall", "Sunlight", "Chloroplast"],
        table_title: "**Parts of the cell**",
        table: [
            ["Part", "Job"],
            ["Nucleus", "Holds the DNA"],
            ["Cell wall", "Keeps the shape"],
            ["Chloroplast", "Makes food"],
        ],
    },
    figures: FiguresText {
        title: "Figures of speech",
        notes: [
            "Figures of speech",
            "**Simile**: compares two things using “like” or “as”.",
            "“Her smile was as bright as the sun.”",
            "**Metaphor**: says one thing is another.",
            "“The classroom was a zoo.”",
            "**Personification**: gives human feelings to a thing.",
            "“The wind whispered through the trees.”",
            "**Hyperbole**: a huge exaggeration.",
            "“I’ve told you a million times!”",
            "Find two metaphors in chapter 3",
            "Reading: The Hobbit, chapters 1–3",
        ],
    },
    vocab: VocabText {
        title: "Food vocabulary",
        heading: "At the café",
        table: [
            ["French", "English"],
            ["le pain", "bread"],
            ["le fromage", "cheese"],
            ["une pomme", "an apple"],
            ["le petit-déjeuner", "breakfast"],
            ["j’ai faim", "I’m hungry"],
        ],
        after: [
            "Learn the list for Monday",
            "How do I say “I would like…” politely?",
        ],
    },
    pythagoras: PythagorasText {
        title: "Pythagoras’ theorem",
        notes: [
            "In a right-angled triangle, the square of the hypotenuse equals the sum of the squares of the other two sides.",
            "",
            "**Example**",
            "a = 3 cm and b = 4 cm",
            "",
            "so c = 5 cm",
            "Does it work for every triangle? No, only for right-angled ones.",
        ],
    },
    worksheet: WorksheetText {
        title: "Worksheet: Pythagoras",
        file_name: "worksheet.pdf",
        page_name: "worksheet page 1",
        sheet: [
            "Worksheet — Pythagoras’ theorem",
            "Name: ______________________     Class: ________",
            "1. ABC is right-angled at A, with AB = 6 cm and AC = 8 cm. Find BC.",
            "2. Is a triangle with sides 5 cm, 12 cm and 13 cm right-angled?",
            "3. A 5 m ladder leans on a wall, its foot 3 m away. How high does it reach?",
        ],
        notes: ["Hand in on Monday", "Show every step"],
    },
    rivers: RiversText {
        title: "Rivers of the world",
        notes: [
            "The longest rivers",
            "the Nile, in Africa: about 6,650 km",
            "the Amazon, in South America: about 6,400 km",
            "the Yangtze, in China: about 6,300 km",
            "**Source**: where a river begins. **Mouth**: where it meets the sea.",
            "In the UK, the Severn is the longest river (354 km).",
        ],
    },
    journal: JournalText {
        title: "A quiet Saturday",
        notes: [
            "Rain all morning, so we stayed in and baked.",
            "In the afternoon, a long walk to the lake with Grandpa. He knows the name of every bird.",
            "Idea: a bird notebook, with a page for every bird we spot.",
            "Slow days are good days too.",
        ],
    },
    pancakes: PancakesText {
        title: "Pancakes",
        ingredients: [
            "Ingredients (8 pancakes)",
            "250 g flour",
            "4 eggs",
            "half a litre of milk",
            "a pinch of salt",
            "2 tablespoons of melted butter",
        ],
        method: [
            "Method",
            "Whisk the flour, eggs and salt.",
            "Add the milk little by little, then the butter.",
            "Let the batter rest for an hour.",
            "or overnight in the fridge",
            "Cook in a hot pan, about a minute per side.",
            "Ask a grown-up before using the stove!",
        ],
    },
    quick: QuickText {
        title: "Before Friday",
        notes: ["Get the trip form signed", "Library books due on the 12th"],
    },
};

fn text(lang: Lang) -> &'static SeedText {
    match lang {
        Lang::En => &EN,
        Lang::Fr => &fr::TEXT,
    }
}

fn milestones(rows: &[[&str; 3]]) -> Vec<Milestone> {
    rows.iter()
        .map(|[title, date, note]| Milestone {
            title: (*title).into(),
            date: (*date).into(),
            note: (*note).into(),
        })
        .collect()
}

/// How one line of a sample outline is shaped. The words come from each language's
/// text, so both libraries share kinds, levels, tags and folds.
#[derive(Clone, Copy)]
enum Kind {
    Para,
    Heading(u8),
    Bullet,
    Numbered,
    Todo(bool),
    Quote,
    /// An equation block; its source is the same in every language and its words are "".
    Equation(&'static str),
}

#[derive(Clone, Copy)]
struct Line {
    kind: Kind,
    /// The outline level (C5).
    level: u8,
    tags: &'static [LineTag],
    /// The lines under it, one level deeper, are folded away.
    folded: bool,
}

const fn line(kind: Kind, level: u8) -> Line {
    Line {
        kind,
        level,
        tags: &[],
        folded: false,
    }
}

const fn tagged(kind: Kind, level: u8, tags: &'static [LineTag]) -> Line {
    Line {
        kind,
        level,
        tags,
        folded: false,
    }
}

const fn folded(kind: Kind, level: u8) -> Line {
    Line {
        kind,
        level,
        tags: &[],
        folded: true,
    }
}

/// The blocks of an outline: line `i` of `shape` with the words `words[i]`.
fn outline<const N: usize>(shape: &[Line; N], words: &[&str; N]) -> Vec<Block> {
    shape
        .iter()
        .zip(words)
        .map(|(line, words)| {
            let text = |kind| Block::new(kind).with_rich(bold_marks(words));
            let mut block = match line.kind {
                Kind::Para => text(BlockKind::Paragraph),
                Kind::Heading(level) => text(BlockKind::Heading { level }),
                Kind::Bullet => text(BlockKind::Bullet),
                Kind::Numbered => text(BlockKind::Numbered),
                Kind::Todo(checked) => text(BlockKind::Todo { checked }),
                Kind::Quote => text(BlockKind::Quote),
                Kind::Equation(source) => equation(source),
            };
            block.indent = line.level;
            block.tags = line.tags.to_vec();
            block.collapsed = line.folded;
            block
        })
        .collect()
}

/// `text` with every `**…**` span in bold.
fn bold_marks(text: &str) -> StyledText {
    let parts: Vec<(&str, InlineStyle)> = text
        .split("**")
        .enumerate()
        .map(|(i, part)| {
            (
                part,
                if i % 2 == 1 {
                    style().with_bold()
                } else {
                    style()
                },
            )
        })
        .collect();
    rich(&parts)
}

/// An equation block (C6), in the linear format the equation editor reads.
fn equation(source: &str) -> Block {
    // P5's `scale` is the formula's font size in page units, not a multiplier.
    Block::new(BlockKind::Equation {
        source: source.into(),
        scale: crate::model::default_equation_size(),
    })
}

/// A note container at (x, y), in page units from the page's top-left (C4). `x` is where
/// its text frame starts; its tags hang further left.
fn note(blocks: Vec<Block>, x: f32, y: f32, width: f32, height: f32) -> CanvasItem {
    let doc = Document { blocks };
    // Its tags hang left of its text (`editor::tag_lead`): the text starts NOTE_PAD_X right
    // of `x`, under the title like every other note's.
    let lead = crate::editor::tag_lead(&doc);
    // `CanvasItem::new` leaves `auto_width` off (P4), so the note keeps the width given
    // here in both languages until someone types in it.
    CanvasItem::new(CanvasKind::Text { doc }, x - lead, y, width + lead, height)
}

/// A pen stroke through `points`, drawn relative to (x, y).
fn pen(points: &[[f32; 2]], x: f32, y: f32, color: Rgb, width: f32) -> InkStroke {
    InkStroke {
        points: points.iter().map(|[dx, dy]| [x + dx, y + dy]).collect(),
        color,
        width,
        highlighter: false,
    }
}

/// One sample page. `created` and `updated` are how long ago, in ms.
struct SeedPage {
    id: &'static str,
    section: &'static str,
    title: &'static str,
    position: i64,
    /// 0 for a page, 1 for a subpage.
    level: i64,
    rule: RuleLines,
    created: i64,
    updated: i64,
    canvas: CanvasDoc,
}

/// A file a sample page shows, stored as `<id>.<extension>`.
struct SeedAttachment {
    id: &'static str,
    page: &'static str,
    name: &'static str,
    mime: &'static str,
    extension: &'static str,
    bytes: Vec<u8>,
}

/// Writes the sample library inside one transaction and collects the files it needs.
struct Seeder<'a> {
    tx: &'a Transaction<'a>,
    now: i64,
    lang: Lang,
    text: &'static SeedText,
    files: Vec<SeedFile>,
}

pub fn install(conn: &mut Connection, lang: Lang) -> DbResult<Vec<SeedFile>> {
    let tx = conn.transaction()?;
    let mut seed = Seeder {
        tx: &tx,
        now: now_ms(),
        lang,
        text: text(lang),
        files: Vec::new(),
    };
    seed.library()?;
    // The pages, in PAGE_IDS order.
    seed.week()?;
    seed.middle_ages()?;
    seed.castles()?;
    seed.plant_cell()?;
    seed.figures()?;
    seed.vocabulary()?;
    seed.pythagoras()?;
    seed.worksheet()?;
    seed.rivers()?;
    seed.journal()?;
    seed.pancakes()?;
    seed.quick_note()?;
    let files = std::mem::take(&mut seed.files);
    tx.commit()?;
    Ok(files)
}

/// Every sample page, in the order `install` writes them. Tests open each one.
#[cfg(test)]
pub(crate) const PAGE_IDS: &[&str] = &[
    "page-school-week",
    "page-school-frise",
    "page-school-castles",
    "page-school-diagram",
    "page-school-figures",
    "page-school-vocab",
    "page-school-pythagoras",
    "page-school-worksheet",
    "page-last-year-rivers",
    "page-journal",
    "page-pancakes",
    "page-quick-note",
];

impl Seeder<'_> {
    /// The notebooks, the section group, the sections, and the settings that point at them.
    fn library(&self) -> DbResult<()> {
        let (text, now) = (self.text, self.now);
        for (id, name, color, position, age) in [
            ("notebook-school", text.notebook_school, "#0166ac", 0, 40),
            (
                "notebook-personal",
                text.notebook_personal,
                "#be0151",
                1,
                64,
            ),
        ] {
            self.tx.execute(
                "INSERT INTO notebooks (id, name, color, position, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?)",
                params![id, name, color, position, now - age * DAY, now],
            )?;
        }
        self.tx.execute(
            "INSERT INTO section_groups (id, notebook_id, parent_id, name, color, position, created_at, updated_at)
             VALUES ('group-last-year', 'notebook-school', NULL, ?, '#a0a0a0', 0, ?, ?)",
            params![text.group_last_year, now - 300 * DAY, now],
        )?;
        for (id, notebook, group, name, color, position) in [
            (
                "section-this-week",
                "notebook-school",
                None,
                text.section_this_week,
                "#0166ac",
                0,
            ),
            (
                "section-classes",
                "notebook-school",
                None,
                text.section_history,
                "#f2610c",
                1,
            ),
            (
                "section-science",
                "notebook-school",
                None,
                text.section_science,
                "#00b24a",
                2,
            ),
            (
                "section-language",
                "notebook-school",
                None,
                text.section_language,
                "#be0151",
                3,
            ),
            (
                "section-foreign",
                "notebook-school",
                None,
                text.section_foreign,
                "#c39f74",
                4,
            ),
            (
                "section-maths",
                "notebook-school",
                None,
                text.section_maths,
                "#fabe14",
                5,
            ),
            (
                "section-last-year",
                "notebook-school",
                Some("group-last-year"),
                text.section_last_year,
                "#00b7c3",
                0,
            ),
            (
                "section-journal",
                "notebook-personal",
                None,
                text.section_journal,
                "#8764b8",
                0,
            ),
            (
                "section-kitchen",
                "notebook-personal",
                None,
                text.section_recipes,
                "#f2610c",
                1,
            ),
            (
                "section-inbox",
                "notebook-personal",
                None,
                text.section_quick,
                "#fabe14",
                2,
            ),
        ] {
            self.tx.execute(
                "INSERT INTO sections (id, notebook_id, group_id, name, color, position, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                params![id, notebook, group, name, color, position, now - 30 * DAY, now],
            )?;
        }
        for (key, value) in [
            ("quick_section_id", "section-inbox"),
            ("last_page_id", "page-school-week"),
        ] {
            self.tx.execute(
                "INSERT INTO settings (key, value) VALUES (?, ?)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )?;
        }
        Ok(())
    }

    fn page(&self, page: SeedPage) -> DbResult<()> {
        let text = page.canvas.plain_text();
        // P3 keeps the page's look as one code in the `paper` column.
        let style = PageStyle {
            rule: page.rule,
            ..PageStyle::default()
        };
        self.tx.execute(
            "INSERT INTO pages (id, section_id, title, content_json, canvas_json, layout, plain_text, paper, position, level, word_count, created_at, updated_at)
             VALUES (?, ?, ?, '', ?, 'canvas', ?, ?, ?, ?, ?, ?, ?)",
            params![
                page.id,
                page.section,
                page.title,
                page.canvas.to_json(),
                text,
                style.code(),
                page.position,
                page.level,
                text.split_whitespace().count() as i64,
                self.now - page.created,
                self.now - page.updated,
            ],
        )?;
        Ok(())
    }

    fn attach(&mut self, file: SeedAttachment) -> DbResult<()> {
        let stored_path = format!("{}.{}", file.id, file.extension);
        self.tx.execute(
            "INSERT INTO attachments (id, page_id, file_name, stored_path, mime, byte_size, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
            params![file.id, file.page, file.name, stored_path, file.mime, file.bytes.len() as i64, self.now],
        )?;
        self.files.push(SeedFile {
            stored_path,
            bytes: file.bytes,
        });
        Ok(())
    }

    /// School › This week › My week: the page a new library opens on.
    fn week(&self) -> DbResult<()> {
        let words = &self.text.week;
        let mut canvas = CanvasDoc::default();
        // A page's first note sits `NOTE_PAD_X` left of the title, so its text starts
        // under the title's first letter, as on a page you type on (P4).
        canvas.add(note(
            outline(&WEEK_PLAN, &words.plan),
            LEFT - NOTE_PAD_X,
            TOP,
            470.0,
            230.0,
        ));
        canvas.add(note(
            outline(&WEEK_REMINDERS, &words.reminders),
            LEFT + 500.0,
            TOP,
            300.0,
            90.0,
        ));
        // The frise sits a few lines under the plan, as a pupil would place it, not half a
        // screen down.
        canvas.add(CanvasItem::new(
            CanvasKind::Timeline {
                color: Rgb(0x01, 0x66, 0xac),
                milestones: milestones(&words.term),
            },
            LEFT,
            TOP + 190.0,
            800.0,
            220.0,
        ));
        canvas.ink.push(pen(
            &STAR,
            LEFT + 830.0,
            TOP + 30.0,
            Rgb(0xd1, 0x34, 0x38),
            3.0,
        ));
        self.page(SeedPage {
            id: "page-school-week",
            section: "section-this-week",
            title: words.title,
            position: 0,
            level: 0,
            rule: RuleLines::None,
            created: 3 * DAY,
            updated: 20 * MINUTE,
            canvas,
        })
    }

    /// School › History › The Middle Ages.
    fn middle_ages(&self) -> DbResult<()> {
        let words = &self.text.ages;
        let mut canvas = CanvasDoc::default();
        // The first note's text starts under the title's first letter (P4), as in `week`.
        canvas.add(note(
            outline(&AGES_NOTES, &words.notes),
            LEFT - NOTE_PAD_X,
            TOP,
            520.0,
            300.0,
        ));
        canvas.add(CanvasItem::new(
            CanvasKind::Timeline {
                color: Rgb(0xf2, 0x61, 0x0c),
                milestones: milestones(&words.frise),
            },
            LEFT,
            // Under the outline with room for the folded family to open (three lines).
            TOP + 250.0,
            // Narrow enough to fit a window of the default width (1320) without scrolling.
            860.0,
            230.0,
        ));
        self.page(SeedPage {
            id: "page-school-frise",
            section: "section-classes",
            title: words.title,
            position: 0,
            level: 0,
            rule: RuleLines::None,
            created: 12 * DAY,
            updated: 2 * DAY,
            canvas,
        })
    }

    /// School › History › Castles, a subpage of The Middle Ages.
    fn castles(&mut self) -> DbResult<()> {
        let text = self.text;
        let words = &text.castles;
        let mut drawing = vec![picture("seed-castle", words.caption)];
        drawing.extend(outline(&CAPTION, &[words.caption]));
        let mut canvas = CanvasDoc::default();
        // The first note's text starts under the title's first letter (P4), as in `week`.
        canvas.add(note(
            outline(&CASTLE_NOTES, &words.notes),
            LEFT - NOTE_PAD_X,
            TOP,
            400.0,
            200.0,
        ));
        canvas.add(note(drawing, LEFT + 440.0, TOP, 380.0, 300.0));
        self.page(SeedPage {
            id: "page-school-castles",
            section: "section-classes",
            title: words.title,
            position: 1,
            level: 1,
            rule: RuleLines::None,
            created: 11 * DAY,
            updated: 2 * DAY,
            canvas,
        })?;
        // After the page: an attachment row points at its page (a foreign key).
        self.attach(SeedAttachment {
            id: "seed-castle",
            page: "page-school-castles",
            name: words.picture_name,
            mime: "image/png",
            extension: "png",
            bytes: castle_png(),
        })
    }

    /// School › Science › Plant cell.
    fn plant_cell(&self) -> DbResult<()> {
        let words = &self.text.cell;
        let mut canvas = CanvasDoc::default();
        // The first note's text starts under the title's first letter (P4), as in `week`.
        canvas.add(note(
            outline(&CELL_INTRO, &words.intro),
            LEFT - NOTE_PAD_X,
            TOP,
            560.0,
            70.0,
        ));
        // The diagram sits under the intro, as the class drew it.
        let top = TOP + 90.0;
        let shape =
            |shape: ShapeKind, stroke: Rgb, fill: Option<Rgb>, label: &str, flip_y: bool| {
                CanvasKind::Shape {
                    shape,
                    stroke,
                    fill,
                    label: label.into(),
                    flip_x: false,
                    flip_y,
                    thickness: None,
                }
            };
        canvas.add(CanvasItem::new(
            shape(
                ShapeKind::Ellipse,
                Rgb(0x12, 0xa5, 0x8a),
                Some(Rgb(0xbf, 0xe9, 0xd2)),
                words.cell,
                false,
            ),
            300.0,
            top + 160.0,
            240.0,
            120.0,
        ));
        for (&label, (x, y, flip_y)) in words.parts.iter().zip([
            (60.0, 80.0, false),
            (620.0, 80.0, true),
            (300.0, 20.0, false),
            (300.0, 360.0, false),
        ]) {
            canvas.add(CanvasItem::new(
                shape(
                    ShapeKind::RoundRect,
                    Rgb(0x3d, 0x7d, 0xd9),
                    Some(Rgb(0xc9, 0xdd, 0xff)),
                    label,
                    flip_y,
                ),
                x,
                top + y,
                160.0,
                70.0,
            ));
        }
        for (x, y, w, h, flip_y) in [
            (220.0, 140.0, 80.0, 80.0, false),
            (540.0, 140.0, 80.0, 80.0, true),
            (360.0, 90.0, 24.0, 70.0, false),
            (400.0, 280.0, 24.0, 80.0, false),
        ] {
            canvas.add(CanvasItem::new(
                shape(ShapeKind::Arrow, Rgb(0x57, 0x93, 0x3f), None, "", flip_y),
                x,
                top + y,
                w,
                h,
            ));
        }
        // Under the diagram, in the intro's column: a bold title and a table block.
        let mut parts = outline(&TABLE_TITLE, &[words.table_title]);
        parts.push(table(&words.table));
        canvas.add(note(parts, LEFT - NOTE_PAD_X, top + 460.0, 440.0, 190.0));
        self.page(SeedPage {
            id: "page-school-diagram",
            section: "section-science",
            title: words.title,
            position: 0,
            level: 0,
            rule: RuleLines::None,
            created: 8 * DAY,
            updated: 7 * DAY,
            canvas,
        })
    }

    /// School › English (or Français) › Figures of speech.
    fn figures(&self) -> DbResult<()> {
        let words = &self.text.figures;
        let mut canvas = CanvasDoc::default();
        // The first note's text starts under the title's first letter (P4), as in `week`.
        canvas.add(note(
            outline(&FIGURES, &words.notes),
            LEFT - NOTE_PAD_X,
            TOP,
            560.0,
            290.0,
        ));
        self.page(SeedPage {
            id: "page-school-figures",
            section: "section-language",
            title: words.title,
            position: 0,
            level: 0,
            rule: RuleLines::None,
            created: 6 * DAY,
            updated: DAY,
            canvas,
        })
    }

    /// School › French (or Anglais) › Food vocabulary.
    fn vocabulary(&self) -> DbResult<()> {
        let words = &self.text.vocab;
        let mut blocks = outline(&VOCAB_HEADING, &[words.heading]);
        blocks.push(table(&words.table));
        blocks.extend(outline(&VOCAB_AFTER, &words.after));
        let mut canvas = CanvasDoc::default();
        // The first note's text starts under the title's first letter (P4), as in `week`.
        canvas.add(note(blocks, LEFT - NOTE_PAD_X, TOP, 440.0, 300.0));
        self.page(SeedPage {
            id: "page-school-vocab",
            section: "section-foreign",
            title: words.title,
            position: 0,
            level: 0,
            rule: RuleLines::None,
            created: 4 * DAY,
            updated: 4 * DAY,
            canvas,
        })
    }

    /// School › Maths › Pythagoras’ theorem, on small-grid paper.
    fn pythagoras(&self) -> DbResult<()> {
        let words = &self.text.pythagoras;
        let mut canvas = CanvasDoc::default();
        // The first note's text starts under the title's first letter (P4), as in `week`;
        // on grid paper that is also the origin a typed page uses (P3).
        canvas.add(note(
            outline(&PYTHAGORAS, &words.notes),
            LEFT - NOTE_PAD_X,
            TOP,
            500.0,
            260.0,
        ));
        // The triangle, drawn with the pen, and its sides' names in small notes.
        let (x, y) = (LEFT + 600.0, TOP + 40.0);
        let blue = Rgb(0x1f, 0x4e, 0x9a);
        canvas.ink.push(pen(&TRIANGLE, x, y, blue, 3.0));
        canvas.ink.push(pen(&RIGHT_ANGLE, x, y, blue, 2.0));
        for (side, dx, dy) in [("a", -40.0, 66.0), ("b", 96.0, 168.0), ("c", 118.0, 58.0)] {
            canvas.add(note(
                vec![Block::paragraph(side)],
                x + dx,
                y + dy,
                80.0,
                30.0,
            ));
        }
        self.page(SeedPage {
            id: "page-school-pythagoras",
            section: "section-maths",
            title: words.title,
            position: 0,
            level: 0,
            rule: RuleLines::Grid(crate::model::GridSize::Small),
            created: 9 * DAY,
            updated: DAY,
            canvas,
        })
    }

    /// School › Maths › Worksheet: a file printout, with pen marks on it.
    fn worksheet(&mut self) -> DbResult<()> {
        let text = self.text;
        let words = &text.worksheet;
        let Some(sheet) = worksheet_sheet(self.lang, &words.sheet) else {
            // hayro failing on this tiny PDF fails on every PDF; the seed tests show it.
            return Ok(());
        };
        let page_id = "page-school-worksheet";
        let mut canvas = CanvasDoc::default();
        // P8's layout: the PDF's file card (FILE_SIZE 260 × 56) at the printout's
        // origin, and its page PRINTOUT_GAP (12) under it, at its printed size.
        canvas.add(CanvasItem::new(
            CanvasKind::File {
                attachment: "seed-worksheet".into(),
                name: words.file_name.into(),
                size: sheet.pdf.len() as u64,
            },
            LEFT,
            TOP,
            260.0,
            56.0,
        ));
        let (x, y) = (LEFT, TOP + 56.0 + 12.0);
        let mut page = CanvasKind::image("seed-worksheet-1");
        if let CanvasKind::Image { printout, .. } = &mut page {
            *printout = Some(Box::new(crate::model::PrintoutPage {
                source: "seed-worksheet".into(),
                page: 1,
                text: sheet.text.clone(),
                size: Some(sheet.size),
            }));
        }
        canvas.add(CanvasItem::new(page, x, y, sheet.size[0], sheet.size[1]));
        // Pen marks: exercise 1 circled, exercise 2 ticked (PDF points to page units).
        let at = |u: f32, v: f32| {
            (
                x + u * crate::pdf::UNITS_PER_POINT,
                y + (842.0 - v) * crate::pdf::UNITS_PER_POINT,
            )
        };
        let red = Rgb(0xd1, 0x34, 0x38);
        let (cx, cy) = at(60.0, 694.0);
        canvas.ink.push(pen(&CIRCLE, cx, cy, red, 2.5));
        let (kx, ky) = at(548.0, 526.0);
        canvas.ink.push(pen(&TICK, kx, ky, red, 3.0));
        // The reminders beside the file card, above the printout: right of the page they
        // would be cut off by a window of the default width (1320).
        canvas.add(note(
            outline(&WORKSHEET_NOTES, &words.notes),
            LEFT + 260.0 + 24.0,
            TOP + 6.0,
            260.0,
            48.0,
        ));
        self.page(SeedPage {
            id: page_id,
            section: "section-maths",
            title: words.title,
            position: 1,
            level: 0,
            rule: RuleLines::None,
            created: 2 * DAY,
            updated: 2 * DAY,
            canvas,
        })?;
        // After the page: an attachment row points at its page (a foreign key).
        self.attach(SeedAttachment {
            id: "seed-worksheet",
            page: page_id,
            name: words.file_name,
            mime: "application/pdf",
            extension: "pdf",
            bytes: sheet.pdf.clone(),
        })?;
        self.attach(SeedAttachment {
            id: "seed-worksheet-1",
            page: page_id,
            name: words.page_name,
            mime: "image/webp",
            extension: "webp",
            bytes: sheet.webp.clone(),
        })
    }

    /// School › Last year › Geography › Rivers of the world.
    fn rivers(&self) -> DbResult<()> {
        let words = &self.text.rivers;
        let mut canvas = CanvasDoc::default();
        // The first note's text starts under the title's first letter (P4), as in `week`.
        canvas.add(note(
            outline(&RIVERS, &words.notes),
            LEFT - NOTE_PAD_X,
            TOP,
            480.0,
            200.0,
        ));
        self.page(SeedPage {
            id: "page-last-year-rivers",
            section: "section-last-year",
            title: words.title,
            position: 0,
            level: 0,
            rule: RuleLines::None,
            created: 300 * DAY,
            updated: 280 * DAY,
            canvas,
        })
    }

    /// Personal › Journal › A quiet Saturday.
    fn journal(&self) -> DbResult<()> {
        let words = &self.text.journal;
        let mut canvas = CanvasDoc::default();
        // The first note's text starts under the title's first letter (P4), as in `week`.
        canvas.add(note(
            outline(&JOURNAL, &words.notes),
            LEFT - NOTE_PAD_X,
            TOP,
            480.0,
            170.0,
        ));
        self.page(SeedPage {
            id: "page-journal",
            section: "section-journal",
            title: words.title,
            position: 0,
            level: 0,
            rule: RuleLines::None,
            created: 9 * DAY,
            updated: 9 * DAY,
            canvas,
        })
    }

    /// Personal › Recipes › Pancakes.
    fn pancakes(&self) -> DbResult<()> {
        let words = &self.text.pancakes;
        let mut canvas = CanvasDoc::default();
        // The first note's text starts under the title's first letter (P4), as in `week`.
        canvas.add(note(
            outline(&INGREDIENTS, &words.ingredients),
            LEFT - NOTE_PAD_X,
            TOP,
            300.0,
            200.0,
        ));
        canvas.add(note(
            outline(&METHOD, &words.method),
            LEFT + 340.0,
            TOP,
            420.0,
            230.0,
        ));
        self.page(SeedPage {
            id: "page-pancakes",
            section: "section-kitchen",
            title: words.title,
            position: 0,
            level: 0,
            rule: RuleLines::None,
            created: 6 * DAY,
            updated: 6 * DAY,
            canvas,
        })
    }

    /// Personal › Quick notes › Before Friday.
    fn quick_note(&self) -> DbResult<()> {
        let words = &self.text.quick;
        let mut canvas = CanvasDoc::default();
        // The first note's text starts under the title's first letter (P4), as in `week`.
        canvas.add(note(
            outline(&QUICK, &words.notes),
            LEFT - NOTE_PAD_X,
            TOP,
            360.0,
            60.0,
        ));
        self.page(SeedPage {
            id: "page-quick-note",
            section: "section-inbox",
            title: words.title,
            position: 0,
            level: 0,
            rule: RuleLines::None,
            created: DAY,
            updated: 60 * MINUTE,
            canvas,
        })
    }
}

/// My week: what matters this week, two reminders, and the term's timeline.
struct WeekText {
    title: &'static str,
    plan: [&'static str; 6],
    reminders: [&'static str; 2],
    /// Title, date and note of each milestone on the term's timeline.
    term: [[&'static str; 3]; 3],
}

const WEEK_PLAN: [Line; 6] = [
    line(Kind::Heading(2), 0),
    line(Kind::Para, 0),
    line(Kind::Todo(false), 0),
    line(Kind::Todo(true), 0),
    tagged(Kind::Todo(false), 0, &[LineTag::Important]),
    tagged(Kind::Todo(false), 0, &[LineTag::Question]),
];
const WEEK_REMINDERS: [Line; 2] = [tagged(Kind::Para, 0, &[LineTag::RememberForLater]); 2];
/// A hand-drawn star, around (0, 0).
const STAR: [[f32; 2]; 11] = [
    [0.0, -18.0],
    [4.4, -6.1],
    [17.1, -5.6],
    [7.1, 2.3],
    [10.6, 14.6],
    [0.0, 7.5],
    [-10.6, 14.6],
    [-7.1, 2.3],
    [-17.1, -5.6],
    [-4.4, -6.1],
    [0.0, -18.0],
];

/// The Middle Ages: an outline with levels, a folded family and tags, and the frise.
struct AgesText {
    title: &'static str,
    notes: [&'static str; 11],
    /// Title, date and note of each milestone on the history frise.
    frise: [[&'static str; 3]; 4],
}

const AGES_NOTES: [Line; 11] = [
    line(Kind::Heading(2), 0),
    line(Kind::Para, 0),
    line(Kind::Bullet, 0),
    line(Kind::Bullet, 1),
    line(Kind::Bullet, 1),
    line(Kind::Bullet, 1),
    folded(Kind::Bullet, 0),
    line(Kind::Bullet, 1),
    line(Kind::Bullet, 1),
    tagged(Kind::Para, 0, &[LineTag::Definition]),
    tagged(Kind::Para, 0, &[LineTag::Question]),
];

/// Castles, a subpage of The Middle Ages, with a picture drawn in code.
struct CastlesText {
    title: &'static str,
    notes: [&'static str; 5],
    caption: &'static str,
    /// The picture's file name, as Save as… offers it.
    picture_name: &'static str,
}

const CASTLE_NOTES: [Line; 5] = [
    line(Kind::Para, 0),
    line(Kind::Bullet, 0),
    line(Kind::Bullet, 0),
    line(Kind::Bullet, 0),
    line(Kind::Todo(false), 0),
];
const CAPTION: [Line; 1] = [line(Kind::Para, 0)];

/// A picture block (C6) showing attachment `attachment`, as wide as its note, with `alt`
/// as its alt text (the HTML export writes it).
fn picture(attachment: &str, alt: &str) -> Block {
    Block::new(BlockKind::Image {
        attachment: attachment.into(),
        width: 1.0,
        meta: PictureMeta {
            alt: alt.into(),
            ..PictureMeta::default()
        },
    })
}

/// The drawing on the Castles page: a small PNG painted here, so the sample owes nothing
/// to anyone's artwork. Flat colours, 360 × 240, the same bytes every time.
fn castle_png() -> Vec<u8> {
    let image = image::RgbImage::from_fn(360, 240, |x, y| {
        let (x, y) = (x as i32, y as i32);
        let within = |left: i32, top: i32, right: i32, bottom: i32| {
            (left..right).contains(&x) && (top..bottom).contains(&y)
        };
        // Battlements: 12 px merlons every 24 px along the top of a wall.
        let merlons = |left: i32, right: i32, top: i32| {
            within(left, top - 10, right, top) && (x - left) % 24 < 12
        };
        let rgb = |hex: u32| image::Rgb([(hex >> 16) as u8, (hex >> 8) as u8, hex as u8]);
        let flag = x >= 181 && (14..=30).contains(&y) && (x - 181) * 8 <= 24 * (8 - (y - 22).abs());
        let gate = within(165, 160, 195, 205)
            || (y < 160 && (x - 180).pow(2) + (y - 160).pow(2) <= 15 * 15);
        let windows =
            within(68, 100, 80, 118) || within(278, 100, 290, 118) || within(174, 70, 186, 92);
        let keep = within(150, 42, 210, 205) || merlons(150, 210, 42);
        let towers = within(50, 72, 100, 205)
            || merlons(50, 100, 72)
            || within(260, 72, 310, 205)
            || merlons(260, 310, 72);
        let wall = within(70, 112, 290, 205) || merlons(70, 290, 112);
        if flag {
            rgb(0xd13438)
        } else if within(179, 14, 181, 42) || windows {
            rgb(0x3b3b3b)
        } else if gate {
            rgb(0x6b4423)
        } else if keep {
            rgb(0x7a7a7a)
        } else if towers || wall {
            rgb(0x9e9e9e)
        } else if within(30, 205, 330, 217) {
            rgb(0x4a90d9)
        } else if y >= 205 {
            rgb(0x6ab04c)
        } else if (x - 312).pow(2) + (y - 46).pow(2) <= 22 * 22 {
            rgb(0xfabe14)
        } else {
            // The sky lightens towards the horizon.
            let k = y * 255 / 205;
            image::Rgb([
                (0xbf + (0xe8 - 0xbf) * k / 255) as u8,
                (0xe3 + (0xf4 - 0xe3) * k / 255) as u8,
                0xff,
            ])
        }
    });
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgb8(image)
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .expect("a PNG in memory");
    bytes
}

/// Plant cell: an intro, the diagram the class drew, and a table of the cell's parts.
struct CellText {
    title: &'static str,
    intro: [&'static str; 2],
    /// The label of the ellipse in the middle of the diagram.
    cell: &'static str,
    /// The four labelled boxes around the cell: nucleus, cell wall, sunlight, chloroplast.
    parts: [&'static str; 4],
    table_title: &'static str,
    table: [[&'static str; 2]; 4],
}

const CELL_INTRO: [Line; 2] = [
    tagged(Kind::Para, 0, &[LineTag::Important]),
    line(Kind::Para, 0),
];
const TABLE_TITLE: [Line; 1] = [line(Kind::Para, 0)];

/// A table block (C6, P6) whose first row is its header. A width of 0 lets every
/// column fit its text, as a new the desktop notebook table does.
fn table<const R: usize, const C: usize>(rows: &[[&str; C]; R]) -> Block {
    let rows: Vec<Vec<String>> = rows
        .iter()
        .map(|row| row.iter().map(|cell| cell.to_string()).collect())
        .collect();
    Block::new(BlockKind::Table(TableBlock::from_strings(&rows, true, 0.0)))
}

/// Figures of speech (the own-language class): definitions, each with an example.
struct FiguresText {
    title: &'static str,
    notes: [&'static str; 11],
}

const FIGURES: [Line; 11] = [
    line(Kind::Heading(2), 0),
    tagged(Kind::Para, 0, &[LineTag::Definition]),
    line(Kind::Bullet, 1),
    tagged(Kind::Para, 0, &[LineTag::Definition]),
    line(Kind::Bullet, 1),
    tagged(Kind::Para, 0, &[LineTag::Definition]),
    line(Kind::Bullet, 1),
    tagged(Kind::Para, 0, &[LineTag::Definition]),
    line(Kind::Bullet, 1),
    line(Kind::Todo(false), 0),
    tagged(Kind::Para, 0, &[LineTag::Book]),
];

/// Food vocabulary (the foreign-language class): a word list as a table.
struct VocabText {
    title: &'static str,
    heading: &'static str,
    /// The foreign word, then the pupil's own; the first row is the header.
    table: [[&'static str; 2]; 6],
    after: [&'static str; 2],
}

const VOCAB_HEADING: [Line; 1] = [line(Kind::Heading(2), 0)];
const VOCAB_AFTER: [Line; 2] = [
    line(Kind::Todo(false), 0),
    tagged(Kind::Para, 0, &[LineTag::Question]),
];

/// Pythagoras’ theorem: the rule, a worked example with equations, a pen triangle.
struct PythagorasText {
    title: &'static str,
    /// The words of each line; the equation lines are "".
    notes: [&'static str; 7],
}

const PYTHAGORAS: [Line; 7] = [
    line(Kind::Para, 0),
    line(Kind::Equation("a^2 + b^2 = c^2"), 0),
    line(Kind::Para, 0),
    line(Kind::Para, 1),
    line(Kind::Equation("c^2 = 3^2 + 4^2 = 25"), 1),
    line(Kind::Para, 1),
    tagged(Kind::Para, 0, &[LineTag::Question]),
];
/// A right triangle 220 wide and 160 high, right angle at the bottom left, and its
/// right-angle mark.
const TRIANGLE: [[f32; 2]; 4] = [[0.0, 0.0], [0.0, 160.0], [220.0, 160.0], [0.0, 0.0]];
const RIGHT_ANGLE: [[f32; 2]; 3] = [[0.0, 140.0], [20.0, 140.0], [20.0, 160.0]];

/// Worksheet: a printout of a worksheet PDF made here, with pen marks on it.
struct WorksheetText {
    title: &'static str,
    /// The PDF's file name.
    file_name: &'static str,
    /// The stored page picture's name, as P8 names a printout's pages
    /// (`printout-page-file`: the PDF's name without its extension, "page", the number).
    page_name: &'static str,
    /// The worksheet: its title, a name line and three exercises.
    sheet: [&'static str; 5],
    notes: [&'static str; 2],
}

const WORKSHEET_NOTES: [Line; 2] = [
    line(Kind::Todo(false), 0),
    tagged(Kind::Para, 0, &[LineTag::Important]),
];
/// A circle of radius 14 around (0, 0), and a tick, as pen marks.
const CIRCLE: [[f32; 2]; 13] = [
    [14.0, 0.0],
    [12.1, 7.0],
    [7.0, 12.1],
    [0.0, 14.0],
    [-7.0, 12.1],
    [-12.1, 7.0],
    [-14.0, 0.0],
    [-12.1, -7.0],
    [-7.0, -12.1],
    [0.0, -14.0],
    [7.0, -12.1],
    [12.1, -7.0],
    [14.0, 0.0],
];
const TICK: [[f32; 2]; 3] = [[0.0, 0.0], [6.0, 8.0], [18.0, -10.0]];

/// The worksheet, rendered: what the printout stores.
struct Sheet {
    pdf: Vec<u8>,
    webp: Vec<u8>,
    text: String,
    /// The page's size in page units: its printed size, rounded as P8 rounds it.
    size: [f32; 2],
}

/// The rendered worksheet for `lang`, made once per process: rendering takes a moment,
/// and every test library seeds the sample.
fn worksheet_sheet(lang: Lang, lines: &[&str; 5]) -> Option<&'static Sheet> {
    static SHEETS: [std::sync::OnceLock<Option<Sheet>>; 2] =
        [std::sync::OnceLock::new(), std::sync::OnceLock::new()];
    let slot = match lang {
        Lang::En => &SHEETS[0],
        Lang::Fr => &SHEETS[1],
    };
    slot.get_or_init(|| {
        let pdf_bytes = worksheet_pdf(lines);
        let pdf = crate::pdf::open(std::sync::Arc::new(pdf_bytes.clone()), None).ok()?;
        let rendered = crate::pdf::render_page(
            &pdf,
            &hayro::RenderCache::new(),
            0,
            crate::pdf::PRINTOUT_DPI,
        )?;
        let (width, height) = crate::pdf::page_size(&pdf, 0)?;
        Some(Sheet {
            webp: crate::pdf::encode_webp(&rendered)?,
            text: crate::pdf::page_text(&pdf, 0),
            size: [
                (width * crate::pdf::UNITS_PER_POINT).round(),
                (height * crate::pdf::UNITS_PER_POINT).round(),
            ],
            pdf: pdf_bytes,
        })
    })
    .as_ref()
}

/// A one-page A4 worksheet in Helvetica: `lines[0]` is the title, `lines[1]` the name
/// line, `lines[2..]` three exercises, each with ruled space to answer. Exercise 1 has
/// its triangle drawn on the right.
fn worksheet_pdf(lines: &[&str; 5]) -> Vec<u8> {
    /// `text` as a PDF string in WinAnsi, escaped.
    fn pdf_string(text: &str) -> Vec<u8> {
        let mut out = Vec::new();
        for ch in text.chars() {
            if matches!(ch, '(' | ')' | '\\') {
                out.push(b'\\');
            }
            out.push(match ch {
                ' '..='~' => ch as u8,
                '\u{a0}'..='\u{ff}' => ch as u32 as u8,
                '’' => 0x92,
                '‘' => 0x91,
                '“' => 0x93,
                '”' => 0x94,
                '–' => 0x96,
                '—' => 0x97,
                '…' => 0x85,
                'œ' => 0x9c,
                'Œ' => 0x8c,
                _ => b'?',
            });
        }
        out
    }
    fn show(content: &mut Vec<u8>, font: &str, size: u32, x: u32, y: u32, text: &str) {
        content.extend_from_slice(format!("BT /{font} {size} Tf {x} {y} Td (").as_bytes());
        content.extend(pdf_string(text));
        content.extend_from_slice(b") Tj ET\n");
    }
    let mut content = Vec::new();
    show(&mut content, "F2", 18, 56, 780, lines[0]);
    show(&mut content, "F1", 11, 56, 748, lines[1]);
    for (exercise, (y, right)) in lines[2..].iter().zip([(690, 380), (520, 539), (350, 539)]) {
        show(&mut content, "F1", 11, 56, y, exercise);
        for k in 1..=4 {
            let line_y = y - 26 * k;
            content.extend_from_slice(
                format!("0.75 G 0.5 w 56 {line_y} m {right} {line_y} l S\n").as_bytes(),
            );
        }
    }
    // Exercise 1's triangle, right-angled at A.
    content.extend_from_slice(b"0 G 1.2 w 400 586 m 400 660 l 500 586 l h S\n");
    content.extend_from_slice(b"0.8 w 400 596 m 410 596 l 410 586 l S\n");
    show(&mut content, "F1", 11, 388, 574, "A");
    show(&mut content, "F1", 11, 392, 664, "B");
    show(&mut content, "F1", 11, 504, 574, "C");

    let mut stream = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
    stream.extend(content);
    stream.extend_from_slice(b"\nendstream");
    let objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 4 0 R /F2 5 0 R >> >> /Contents 6 0 R >>".to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>".to_vec(),
        stream,
    ];
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    // Every xref row is exactly 20 bytes, as the format requires.
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// Rivers of the world, in last year's Geography section (inside a section group).
struct RiversText {
    title: &'static str,
    notes: [&'static str; 6],
}

const RIVERS: [Line; 6] = [
    line(Kind::Heading(2), 0),
    line(Kind::Numbered, 0),
    line(Kind::Numbered, 0),
    line(Kind::Numbered, 0),
    tagged(Kind::Para, 0, &[LineTag::Definition]),
    tagged(Kind::Para, 0, &[LineTag::RememberForLater]),
];

/// A quiet Saturday: a diary page.
struct JournalText {
    title: &'static str,
    notes: [&'static str; 4],
}

const JOURNAL: [Line; 4] = [
    line(Kind::Para, 0),
    line(Kind::Para, 0),
    tagged(Kind::Para, 0, &[LineTag::Idea]),
    line(Kind::Quote, 0),
];

/// Pancakes: ingredients as a shopping list, and the steps.
struct PancakesText {
    title: &'static str,
    ingredients: [&'static str; 6],
    method: [&'static str; 7],
}

const INGREDIENTS: [Line; 6] = [
    line(Kind::Heading(2), 0),
    line(Kind::Todo(true), 0),
    line(Kind::Todo(true), 0),
    line(Kind::Todo(false), 0),
    line(Kind::Todo(false), 0),
    line(Kind::Todo(false), 0),
];
const METHOD: [Line; 7] = [
    line(Kind::Heading(2), 0),
    line(Kind::Numbered, 0),
    line(Kind::Numbered, 0),
    line(Kind::Numbered, 0),
    line(Kind::Numbered, 1),
    line(Kind::Numbered, 0),
    tagged(Kind::Para, 0, &[LineTag::Important]),
];

/// A quick note, as Quick Notes collects them.
struct QuickText {
    title: &'static str,
    notes: [&'static str; 2],
}

const QUICK: [Line; 2] = [line(Kind::Para, 0), line(Kind::Para, 0)];

trait RichBlock {
    fn with_rich(self, text: StyledText) -> Self;
}

impl RichBlock for Block {
    fn with_rich(mut self, text: StyledText) -> Self {
        self.text = text;
        self
    }
}

fn style() -> InlineStyle {
    InlineStyle::default()
}

fn rich(parts: &[(&str, InlineStyle)]) -> StyledText {
    let mut text = String::new();
    let mut runs = Vec::new();
    let mut byte = 0;
    for (part, style) in parts {
        let start = byte;
        text.push_str(part);
        byte += part.len();
        if !style.is_plain() && start < byte {
            runs.push(crate::model::StyleRun {
                start,
                end: byte,
                style: style.clone(),
            });
        }
    }
    StyledText { text, runs }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_castle_picture_is_a_png_and_never_changes() {
        let png = castle_png();
        let image = image::load_from_memory(&png).expect("a PNG");
        assert_eq!((image.width(), image.height()), (360, 240));
        assert_eq!(png, castle_png(), "the same bytes every time");
    }

    #[test]
    fn the_worksheet_renders_in_both_languages() {
        for lang in [Lang::En, Lang::Fr] {
            let sheet = worksheet_sheet(lang, &text(lang).worksheet.sheet).expect("rendered");
            assert!(sheet.pdf.starts_with(b"%PDF-1.4"));
            assert!(image::load_from_memory(&sheet.webp).is_ok(), "{lang:?}");
            assert!(
                sheet.size[0] > 780.0 && sheet.size[1] > 1100.0,
                "A4 at its printed size"
            );
            assert!(
                sheet.text.contains("Pythagor"),
                "{lang:?}: {:?}",
                sheet.text
            );
        }
    }

    #[test]
    fn the_french_worksheet_follows_french_typography() {
        let problems: Vec<String> = fr::TEXT
            .worksheet
            .sheet
            .iter()
            .chain(&fr::TEXT.worksheet.notes)
            .flat_map(|line| crate::i18n::tests::typography_problems(line))
            .collect();
        assert!(problems.is_empty(), "{problems:#?}");
    }
}
