//! Fluent UI System Icons (MIT, Microsoft): the icon set Office draws its ribbon with,
//! drawn as text in the `icons` font family (theme::icon). The bundled font is a subset:
//! assets/fonts/fluent/icons.txt lists the icons it keeps and tools/fluent_icons.py
//! rebuilds it. Each constant is the 20 px Regular glyph of the named Fluent icon; the
//! constant names are older than the icon set, so call sites read as they did.
#![allow(dead_code)]

macro_rules! icons {
    ($($name:ident = $glyph:literal, $fluent:literal;)*) => {
        $(pub const $name: &str = $glyph;)*
        /// Every icon and the Fluent icon it is drawn from, for the font test.
        #[cfg(test)]
        pub const ALL: &[(&str, &str)] = &[$(($glyph, $fluent)),*];
    };
}

icons! {
    FOLDER_SIMPLE = "\u{F418}", "folder";
    FOLDER_OPEN = "\u{F42E}", "folder_open";
    NOTEBOOK = "\u{E871}", "notebook";
    NOTE_PENCIL = "\u{F314}", "compose";
    MAGNIFYING_GLASS = "\u{F68F}", "search";
    PUSH_PIN = "\u{F601}", "pin";
    TRASH = "\u{F34C}", "delete";
    TAG = "\u{F77C}", "tag";
    GEAR_SIX = "\u{F6A9}", "settings";
    CARET_RIGHT = "\u{F2B0}", "chevron_right";
    CARET_DOWN = "\u{F2A3}", "chevron_down";
    CARET_UP = "\u{F2B6}", "chevron_up";
    DOTS_THREE = "\u{E824}", "more_horizontal";
    FILE_TEXT = "\u{E557}", "document_text";
    FILE = "\u{F378}", "document";
    FILE_PDF = "\u{F3AC}", "document_pdf";
    FILE_DOC = "\u{E55D}", "document_text_link";
    FILE_XLS = "\u{E54B}", "document_table";
    FILE_CSV = "\u{E4F7}", "document_data";
    FILE_PPT = "\u{EB28}", "slide_text";
    FILE_ZIP = "\u{F435}", "folder_zip";
    FILE_AUDIO = "\u{E854}", "music_note_2";
    FILE_VIDEO = "\u{EDFF}", "video_clip";
    FILE_IMAGE = "\u{F0176}", "document_image";
    MICROPHONE = "\u{E7F9}", "mic";
    PLAY = "\u{F605}", "play";
    PAUSE = "\u{F5A1}", "pause";
    STOP = "\u{F72A}", "stop";
    RECORD = "\u{F661}", "record";
    PLUS = "\u{F109}", "add";
    X = "\u{F369}", "dismiss";
    CHECK = "\u{F294}", "checkmark";
    ARROW_LEFT = "\u{F15B}", "arrow_left";
    SIDEBAR_SIMPLE = "\u{E8B0}", "panel_left";
    TEXT_AA = "\u{F7E4}", "text_font";
    TEXT_B = "\u{F7A4}", "text_bold";
    TEXT_ITALIC = "\u{F7F4}", "text_italic";
    TEXT_UNDERLINE = "\u{F80A}", "text_underline";
    TEXT_STRIKETHROUGH = "\u{ED5F}", "text_strikethrough";
    HIGHLIGHTER = "\u{F47C}", "highlight";
    LIST_BULLETS = "\u{ECD9}", "text_bullet_list_ltr";
    LIST_NUMBERS = "\u{F7F9}", "text_number_list_ltr";
    CHECK_SQUARE = "\u{F28D}", "checkbox_checked";
    QUOTES = "\u{F800}", "text_quote";
    CODE = "\u{F2EF}", "code";
    MINUS = "\u{EBD0}", "subtract";
    IMAGE = "\u{F488}", "image";
    PENCIL_SIMPLE = "\u{E8D7}", "pen";
    TABLE = "\u{F75D}", "table";
    ARROW_COUNTER_CLOCKWISE = "\u{F199}", "arrow_undo";
    ARROW_CLOCKWISE = "\u{F16E}", "arrow_redo";
    LINK = "\u{F4E4}", "link";
    PALETTE = "\u{F2F5}", "color";
    EXPORT = "\u{E0C7}", "arrow_export_ltr";
    PRINTER = "\u{F62A}", "print";
    MOON = "\u{F87D}", "weather_moon";
    SUN = "\u{F8A1}", "weather_sunny";
    MONITOR = "\u{F359}", "desktop";
    CLOCK = "\u{F2DD}", "clock";
    CLOCK_COUNTER_CLOCKWISE = "\u{F47E}", "history";
    STAR = "\u{F70F}", "star";
    HOUSE = "\u{F480}", "home";
    BOOK_OPEN = "\u{E178}", "book_open";
    BOOKMARK_SIMPLE = "\u{F1F5}", "bookmark";
    SQUARES_FOUR = "\u{F133}", "apps";
    ARROW_SQUARE_OUT = "\u{F582}", "open";
    COPY = "\u{F32B}", "copy";
    CLOUD_CHECK = "\u{E399}", "cloud_checkmark";
    HARD_DRIVE = "\u{E6E3}", "hard_drive";
    KEYBOARD = "\u{F4B8}", "keyboard";
    COMMAND = "\u{F4B8}", "keyboard";
    SPARKLE = "\u{EB33}", "sparkle";
    CHECK_CIRCLE = "\u{F298}", "checkmark_circle";
    WARNING_CIRCLE = "\u{F3F1}", "error_circle";
    INFO = "\u{F4A3}", "info";
    SIDEBAR = "\u{E8B9}", "panel_left_expand";
    CARET_LEFT = "\u{F2AA}", "chevron_left";
    FOLDER_PLUS = "\u{F41C}", "folder_add";
    FOLDER_SIMPLE_PLUS = "\u{F41C}", "folder_add";
    TAG_SIMPLE = "\u{F77C}", "tag";
    CALENDAR_BLANK = "\u{E24E}", "calendar_ltr";
    PAPERCLIP = "\u{F1A9}", "attach";
    PEN_NIB = "\u{F4A7}", "inking_tool";
    ERASER = "\u{E5E4}", "eraser";
    LINE_SEGMENTS = "\u{ED83}", "timeline";
    CIRCLE = "\u{F2BB}", "circle";
    SQUARE = "\u{EB75}", "square";
    ARROW_UP = "\u{F19B}", "arrow_up";
    ARROW_DOWN = "\u{F148}", "arrow_down";
    ARROW_RIGHT = "\u{F181}", "arrow_right";
    ARROW_U_UP_LEFT = "\u{F8DE}", "arrow_hook_up_left";
    FLOPPY_DISK = "\u{F67F}", "save";
    DOWNLOAD_SIMPLE = "\u{F150}", "arrow_download";
    UPLOAD_SIMPLE = "\u{F1A4}", "arrow_upload";
    LIST = "\u{F560}", "navigation";
    DOTS_SIX = "\u{E6C4}", "grid_dots";
    GRID_FOUR = "\u{F462}", "grid";
    CURSOR = "\u{E443}", "cursor";
    VOLUME_HIGH = "\u{EB42}", "speaker_2";
    HAND = "\u{E6DB}", "hand_left";
    HAND_GRABBING = "\u{E6D7}", "hand_draw";
    LASSO = "\u{E756}", "lasso";
    SELECTION = "\u{F697}", "select_object";
    TEXT_ALIGN_LEFT = "\u{F79F}", "text_align_left";
    TEXT_ALIGN_CENTER = "\u{F799}", "text_align_center";
    TEXT_ALIGN_RIGHT = "\u{F7A1}", "text_align_right";
    TEXT_SUPERSCRIPT = "\u{F808}", "text_superscript";
    TEXT_SUBSCRIPT = "\u{F806}", "text_subscript";
    SCISSORS = "\u{F33A}", "cut";
    CLIPBOARD_TEXT = "\u{F2D5}", "clipboard_paste";
    LINK_BREAK = "\u{E773}", "link_dismiss";
    COPY_LINK = "\u{F2D2}", "clipboard_link";
    SELECTION_ALL = "\u{EA86}", "select_all_on";
    QUESTION = "\u{F63D}", "question_circle";
    USER = "\u{F5BD}", "person";
    PHONE = "\u{E270}", "call";
    LIGHTBULB = "\u{F4D6}", "lightbulb";
    WARNING = "\u{F869}", "warning";
    BOOK = "\u{F8FD}", "book";
    ARROWS_CLOCKWISE = "\u{F13D}", "arrow_clockwise";
    SHAPES = "\u{F6AD}", "shapes";
    MATH_OPERATIONS = "\u{E7E2}", "math_formula";
    // The ribbon's tabs (P2), named after their Fluent icons.
    TEXT_BULLET_LIST = "\u{F0290}", "text_bullet_list";
    TEXT_CLEAR_FORMATTING = "\u{F7BC}", "text_clear_formatting";
    TEXT_COLOR = "\u{F7BF}", "text_color";
    TEXT_PARAGRAPH = "\u{ED42}", "text_paragraph";
    TEXT_CASE_TITLE = "\u{ECED}", "text_case_title";
    TEXT_HEADER_1 = "\u{F7EF}", "text_header_1";
    TEXT_HEADER_2 = "\u{F7F0}", "text_header_2";
    TEXT_HEADER_3 = "\u{F7F1}", "text_header_3";
    TAG_SEARCH = "\u{EC88}", "tag_search";
    TEXTBOX = "\u{F80F}", "textbox";
    NOTE = "\u{F56B}", "note";
    LINE_HORIZONTAL_1 = "\u{F4E0}", "line_horizontal_1";
    LINE_THICKNESS = "\u{F0061}", "line_thickness";
    DRAW_SHAPE = "\u{E575}", "draw_shape";
    COLOR_FILL = "\u{F2F9}", "color_fill";
    POSITION_TO_FRONT = "\u{E9AE}", "position_to_front";
    POSITION_TO_BACK = "\u{E9AC}", "position_to_back";
    LOCAL_LANGUAGE = "\u{F4F3}", "local_language";
    DOCUMENT_ONE_PAGE = "\u{F399}", "document_one_page";
    FULL_SCREEN_MAXIMIZE = "\u{E684}", "full_screen_maximize";
    FULL_SCREEN_MINIMIZE = "\u{E687}", "full_screen_minimize";
    IMMERSIVE_READER = "\u{F49B}", "immersive_reader";
    ZOOM_OUT = "\u{F8C6}", "zoom_out";
    ZOOM_IN = "\u{F8C4}", "zoom_in";
    RATIO_ONE_TO_ONE = "\u{F70A}", "ratio_one_to_one";
    PAGE_FIT = "\u{F58F}", "page_fit";
    // View › Page Setup (P3).
    PAINT_BUCKET = "\u{F595}", "paint_bucket";
    NOTEPAD = "\u{F576}", "notepad";
    DOCUMENT_HEADER_DISMISS = "\u{E505}", "document_header_dismiss";
    TABLE_INSERT_ROW = "\u{EC2A}", "table_insert_row";
    TABLE_INSERT_COLUMN = "\u{EC26}", "table_insert_column";
    TABLE_DELETE_ROW = "\u{EC10}", "table_delete_row";
    TABLE_DELETE_COLUMN = "\u{EC0C}", "table_delete_column";
    // New notebook, Export and Options: P2's File menu, then Backstage and the menus (P7).
    NOTEBOOK_ADD = "\u{E872}", "notebook_add";
    ARROW_EXPORT = "\u{F0280}", "arrow_export";
    OPTIONS = "\u{F587}", "options";
    // The page list (P2).
    DOCUMENT_ADD = "\u{E4DB}", "document_add";
    ARROW_SORT = "\u{F18A}", "arrow_sort";
    // Home, Insert and Draw (P5). Paste, Cut and File Printout use CLIPBOARD_TEXT,
    // SCISSORS and FILE_PDF; Rounded rectangle and Arrow use SQUARE and ARROW_RIGHT.
    PAINT_BRUSH = "\u{F592}", "paint_brush";
    TEXT_INDENT_DECREASE_LTR = "\u{ED23}", "text_indent_decrease_ltr";
    TEXT_INDENT_INCREASE_LTR = "\u{ED2D}", "text_indent_increase_ltr";
    TEXT_HEADER_4 = "\u{F0956}", "text_header_4";
    TEXT_HEADER_5 = "\u{F0958}", "text_header_5";
    TEXT_HEADER_6 = "\u{F095A}", "text_header_6";
    ARROW_AUTOFIT_HEIGHT = "\u{E077}", "arrow_autofit_height";
    CALENDAR_CLOCK = "\u{F21D}", "calendar_clock";
    DOCUMENT_ONE_PAGE_MULTIPLE = "\u{F017B}", "document_one_page_multiple";
    SYMBOLS = "\u{EBE1}", "symbols";
    LINE = "\u{E765}", "line";
    RECTANGLE_LANDSCAPE = "\u{EA09}", "rectangle_landscape";
    OVAL = "\u{E8A2}", "oval";
    TRIANGLE = "\u{EDA6}", "triangle";
    DIAMOND = "\u{E4BB}", "diamond";
    LOCK_CLOSED = "\u{E78F}", "lock_closed";
    ARROW_ROTATE_CLOCKWISE = "\u{F185}", "arrow_rotate_clockwise";
    ARROW_ROTATE_COUNTERCLOCKWISE = "\u{F187}", "arrow_rotate_counterclockwise";
    FLIP_HORIZONTAL = "\u{E62C}", "flip_horizontal";
    FLIP_VERTICAL = "\u{E632}", "flip_vertical";
    // Tables in notes: the cell menu and the Table tab (P6). Select Table uses TABLE,
    // Delete Columns and Rows TABLE_DELETE_COLUMN and TABLE_DELETE_ROW.
    TABLE_CELL_EDIT = "\u{EC02}", "table_cell_edit";
    TABLE_FREEZE_COLUMN = "\u{EC1B}", "table_freeze_column";
    TABLE_FREEZE_ROW = "\u{EC22}", "table_freeze_row";
    TABLE_DISMISS = "\u{EC14}", "table_dismiss";
    TABLE_MOVE_ABOVE = "\u{EC36}", "table_move_above";
    TABLE_MOVE_BELOW = "\u{EC3A}", "table_move_below";
    TABLE_MOVE_LEFT = "\u{EC3E}", "table_move_left";
    TABLE_MOVE_RIGHT = "\u{EC41}", "table_move_right";
    BORDER_NONE = "\u{E1A8}", "border_none";
    TEXT_SORT_ASCENDING = "\u{F802}", "text_sort_ascending";
    TEXT_SORT_DESCENDING = "\u{F803}", "text_sort_descending";
    // Pictures: the picture menus and the Picture tab (P6), and Picture › Crop (S4).
    IMAGE_ARROW_BACK = "\u{E71C}", "image_arrow_back";
    CROP = "\u{E42B}", "crop";
    ARROW_RESET = "\u{F19F}", "arrow_reset";
    IMAGE_ALT_TEXT = "\u{F48D}", "image_alt_text";
    // Paste Options (P6): Keep Source Formatting, Merge Formatting, Keep Text Only.
    CLIPBOARD_TEXT_EDIT = "\u{E366}", "clipboard_text_edit";
    CLIPBOARD_TEXT_LTR = "\u{E369}", "clipboard_text_ltr";
    TEXT_T = "\u{ED63}", "text_t";
    // Backstage, the History, Review, View and Help tabs, the menus, search and the
    // Horizontal Tabs layout (P7). Back, Info, Export › Page, Open, Back up, Close,
    // Section Color, pin, Move Up/Down, Cut, Paste, Bring to Front / Send to Back, Open
    // (attachments), indent, Recent Edits and the notebook picker use ARROW_LEFT, INFO,
    // FILE, FOLDER_OPEN, FLOPPY_DISK, X, PALETTE, PUSH_PIN, ARROW_UP, ARROW_DOWN,
    // SCISSORS, CLIPBOARD_TEXT, POSITION_TO_FRONT, POSITION_TO_BACK, ARROW_SQUARE_OUT,
    // TEXT_INDENT_*, CLOCK and BOOK.
    ARROW_IMPORT = "\u{F159}", "arrow_import";
    TEXT_PROOFING_TOOLS = "\u{F7FE}", "text_proofing_tools";
    ARROW_SWAP = "\u{F18D}", "arrow_swap";
    MERGE = "\u{E7F7}", "merge";
    PIN_OFF = "\u{F603}", "pin_off";
    ARROW_TURN_UP_LEFT = "\u{F021}", "arrow_turn_up_left";
    TAB_DESKTOP = "\u{F747}", "tab_desktop";
    DARK_THEME = "\u{E452}", "dark_theme";
    NOTE_ADD = "\u{F56E}", "note_add";
    POSITION_FORWARD = "\u{E9AA}", "position_forward";
    POSITION_BACKWARD = "\u{E9A8}", "position_backward";
    RENAME = "\u{F669}", "rename";
    LAYER = "\u{F4CB}", "layer";
    DOCUMENT_SEARCH = "\u{F3AE}", "document_search";
    WINDOW = "\u{F8B5}", "window";
    // History › Notebook Recycle Bin ▾ › Empty Recycle Bin.
    DELETE_DISMISS = "\u{E47E}", "delete_dismiss";
    // the desktop notebook's built-in tags (S3). Schedule meeting, Project A/B and Highlight use
    // CALENDAR_BLANK, SQUARE and HIGHLIGHTER.
    GLOBE = "\u{F45A}", "globe";
    KEY = "\u{F4B6}", "key";
    // Movie to see: a clapperboard. Fluent's filmstrip reads as an empty box at 13 px.
    MOVIES_AND_TV = "\u{E82C}", "movies_and_tv";
    MUSIC_NOTE = "\u{E851}", "music_note_1";
    NEWS = "\u{F565}", "news";
    CHAT = "\u{F286}", "chat";
    PERSON_CHAT = "\u{F6D1}", "person_chat";
    PEOPLE_CHAT = "\u{F03C0}", "people_chat";
    MAIL = "\u{F506}", "mail";
    CALL_FORWARD = "\u{E27D}", "call_forward";
    CHECKBOX_1 = "\u{E2F6}", "checkbox_1";
    CHECKBOX_2 = "\u{E2F8}", "checkbox_2";
    // Client request: a clipboard with "!", as the desktop notebook's box has; clipboard_task has a
    // tick of its own, which would read as ticked before it is.
    CLIPBOARD_ERROR = "\u{E353}", "clipboard_error";
}

/// The Filled style of `notebook` (20 px, codepoints.txt "notebook 20 filled"), drawn in
/// the `icons-filled` family (theme::icon_filled) in each notebook's colour, as the desktop notebook's
/// navigation pane draws its notebook books. Outside the macro: its test checks Regular
/// glyphs only.
pub const NOTEBOOK_FILLED: &str = "\u{E87F}";

/// The Filled style of `pen` (20 px, codepoints.txt "pen 20 filled"): the body of a pen
/// in Draw's pen gallery, drawn in `icons-filled` in the pen's colour (P5).
pub const PEN_FILLED: &str = "\u{E8E9}";

/// The Filled style of `highlight` (20 px, codepoints.txt "highlight 20 filled"): the
/// body of a highlighter in Draw's pen gallery, drawn in `icons-filled` (P5).
pub const HIGHLIGHT_FILLED: &str = "\u{F480}";

#[cfg(test)]
mod tests {
    /// Each icon is the 20 px Regular glyph of its Fluent icon, and that glyph is in the
    /// bundled subset (codepoints.txt is written by tools/fluent_icons.py with the fonts).
    #[test]
    fn every_icon_is_its_fluent_glyph_in_the_bundled_font() {
        let table = include_str!("../assets/fonts/fluent/codepoints.txt");
        for (glyph, fluent) in super::ALL {
            let mut chars = glyph.chars();
            let code = chars.next().expect("one character") as u32;
            assert!(chars.next().is_none(), "{fluent}: one character");
            let line = format!("{fluent}\t20\tregular\tU+{code:04X}");
            assert!(
                table.lines().any(|row| row == line),
                "{line:?} is not in codepoints.txt"
            );
        }
    }

    /// The constants outside the macro are the 20 px Filled glyphs of their Fluent icons.
    #[test]
    fn every_filled_icon_is_its_fluent_glyph_in_the_bundled_font() {
        let table = include_str!("../assets/fonts/fluent/codepoints.txt");
        for (glyph, fluent) in [
            (super::NOTEBOOK_FILLED, "notebook"),
            (super::PEN_FILLED, "pen"),
            (super::HIGHLIGHT_FILLED, "highlight"),
        ] {
            let mut chars = glyph.chars();
            let code = chars.next().expect("one character") as u32;
            assert!(chars.next().is_none(), "{fluent}: one character");
            let line = format!("{fluent}\t20\tfilled\tU+{code:04X}");
            assert!(
                table.lines().any(|row| row == line),
                "{line:?} is not in codepoints.txt"
            );
        }
    }
}
