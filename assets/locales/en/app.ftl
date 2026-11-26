# Folio: the window, the library, the page list and dialogs (English).

## Settings
dialog-settings-language-automatic = Automatic ({ $language })
error-save-language = Could not save the language: { $error }

## L2.1 Top bar and rail
app-search-hint = Search pages, text, and tags
app-quick-notes = Quick notes

## L2.2 Library panel
app-fold-notebook = Fold notebook
app-fold-section-group = Fold section group
app-page-count = { $count ->
    [one] { $count } page
   *[other] { $count } pages
}

## L2.3 Tags pane (line tags)
app-line-tags = Tags Summary
app-refresh-line-tags = Refresh tags
app-tags-scope-section = Section
app-tags-scope-notebook = Notebook
app-tags-scope-all = All notebooks
app-no-tagged-lines = No tagged lines
app-no-tagged-lines-body = Tag a line with { $first } to { $last } and it is listed here.
app-tagged-line-label = { $tag }: { $text }
app-tags-group-by = Group tags by:
app-tags-group-tag-name = Tag name
app-tags-group-section = Section
app-tags-group-title = Title
app-tags-group-date = Date
app-tags-group-note-text = Note text
app-tags-unchecked-only = Show only unchecked items
app-tags-create-summary = Create Summary Page
app-tags-summary-title = Tags Summary

## Customize Tags (Home › Tags ▾)
app-customize-tags-title = Customize Tags
app-customize-tags-note = Changes to tags will not affect notes you have already tagged.
app-new-tag = New Tag…
app-modify-tag = Modify Tag…
app-move-tag-up = Move Up
app-move-tag-down = Move Down
app-tag-name = Display name
app-tag-symbol = Symbol
app-tag-no-symbol = No symbol
app-tag-font-color = Font Color
app-tag-highlight-color = Highlight Color
app-tag-automatic = Automatic
app-tag-no-color = No Color
app-tag-preview = Preview
app-tag-name-taken = A tag with this name already exists.

## L2.4 Page list, page rows and their menus
app-view-pages = Pages
app-empty-trash = Empty Recycle Bin
app-list-empty-trash-title = Trash is empty
app-list-empty-trash-body = Deleted pages will collect here until you empty the trash.
app-trash-sections = Deleted sections
app-trash-section-detail = { $notebook } · { $count ->
    [one] { $count } page
   *[other] { $count } pages
}
app-list-empty-section-title = This section is empty
app-list-empty-section-body = Create a page, or move one here from another section.
app-row-words = { $count }w
app-expand-subpages = Expand Subpages
app-collapse-subpages = Collapse Subpages
app-make-subpage = Make Subpage
app-promote-subpage = Promote Subpage
app-move-up = Move Up
app-move-down = Move Down
toast-link-copied = Link to page copied
toast-section-link-copied = Link to section copied

## L2.5 Editor header, empty editor and status bar
app-page-in-trash = This page is in the trash.
app-editor-empty-trash-title = The trash is empty
app-editor-empty-trash-body = Pages you delete will stay here until you remove them for good.
app-editor-empty-title = A blank page is waiting
app-editor-empty-body = Write something down. You can organize it later.
app-editor-empty-hint = { $shortcut } · Local notebook

## L2.6 Search palette
app-search-recent-empty = Your recent pages will appear here.
app-search-no-matches = No matches. Try a shorter phrase.

## L2.7 Dialogs (new, rename, confirm, link)
dialog-new-notebook-body = A notebook holds sections. A section holds pages.
dialog-notebook-name = Notebook name
dialog-create-notebook = Create notebook
dialog-new-section-body = Sections keep related pages together.
dialog-section-name = Section name
dialog-create-section = Create section
dialog-new-group-body = Section groups keep related sections together, like one per school year.
dialog-group-name = Group name
dialog-create-group = Create group
dialog-rename-section = Rename section
dialog-rename-group = Rename section group
dialog-rename-body = Names are local to this device.
dialog-save-changes = Save changes
dialog-delete-notebook-title = Delete this notebook?
dialog-delete-contents-body = Every section and page inside it will be removed from this device. This cannot be undone.
dialog-delete-notebook = Delete notebook
dialog-delete-section-title = Delete this section?
dialog-delete-section-body = The section and its pages go to the Notebook Recycle Bin, where you can restore them.
dialog-delete-section-forever-body = The section and its pages will be removed from this device. This cannot be undone.
dialog-delete-section = Delete section
dialog-delete-group-title = Delete this section group?
dialog-delete-group-body = Its sections and their pages go to the Notebook Recycle Bin, where you can restore them.
dialog-delete-group = Delete group
dialog-delete-page-title = Delete this page forever?
dialog-delete-page-body = This page will be removed from the device. This cannot be undone.
dialog-empty-trash-title = Empty the trash?
dialog-empty-trash-body = Every section and page in the trash will be deleted from this device. This cannot be undone.
dialog-edit-link = Edit link
dialog-insert-link = Insert link
dialog-link-body = Link to a web page, an email address, a file, or another page in Folio.
dialog-link-text = Text to display
dialog-link-address = Address
dialog-link-address-hint = https://… or pick a page below
dialog-link-page = Or link to a page
dialog-link-search = Search pages
dialog-save-link = Save link
dialog-remove-link = Remove link
dialog-color = Color

## L2.8 Settings and Page versions dialogs
app-spelling = Spelling
app-spell-automatic = Automatic ({ $names })
app-spell-off = Off
app-clear-personal-words = { $count ->
    [one] Clear my { $count } added word
   *[other] Clear my { $count } added words
}
app-your-files = Your files
app-notebook-count = { $count ->
    [one] { $count } notebook
   *[other] { $count } notebooks
}
app-storage-stats = { $pages } · { $notebooks } · { $database } database · { $media } media
app-back-up = Back up
app-open-folder = Open folder
app-settings-footer = Folio { $version } · local-first notebook · MIT / Apache-2.0
app-versions-explain = Folio keeps a copy of this page when you change it after leaving it alone for 10 minutes.
app-versions-empty-title = No earlier versions yet
app-versions-empty-body = Come back to this page later: older copies will appear here.
app-version-no-text = No text on this version. It may hold only drawings or pictures.
app-delete-versions = { $count ->
    [one] Delete { $count } version
   *[other] Delete { $count } versions
}
app-restore-version = Restore Version
app-delete-all-versions = Delete All Versions…
app-delete-version = Delete Version

## L2.9 Keyboard-shortcuts dialog
app-shortcut-save = Save now
app-shortcut-find = Find on page
app-shortcut-format = Bold / italic / underline
app-shortcut-link = Insert link (while typing)
app-shortcut-open-link = Open a link
app-shortcut-text-size = Bigger / smaller text
app-shortcut-scripts = Superscript / subscript
app-shortcut-align = Align left / right
app-shortcut-clear-format = Clear formatting
app-shortcut-new-block = New block
app-shortcut-indent = Indent / outdent
app-shortcut-headings = Headings 1 to 6
app-shortcut-move-paragraph = Move paragraph up / down
app-shortcut-show-levels = Show outline levels
app-shortcut-undo = Undo / redo
app-shortcut-select-all = Select everything on the page
app-shortcut-pan = Move around the page
app-shortcut-equation = Insert equation
app-shortcut-page-below = New page below / subpage
app-shortcut-page-level = Make subpage / promote
app-shortcut-tags = To Do / tags
app-shortcut-remove-tags = Remove tags
app-shortcut-file-tab = Open the File tab
app-shortcut-spelling = Check spelling
app-shortcut-title = Go to the page title
app-shortcut-rule-lines = Show or hide rule lines
app-shortcut-move-page = Move or copy the page
app-shortcut-print = Print the page
app-shortcut-expand-collapse = Expand / collapse the paragraph
app-shortcut-date-time = Insert date / time / both
app-key-click = Click
app-key-drag = drag

## L2.10 Toasts and errors, part 1
error-load-pages = Could not load pages: { $error }
error-page-gone = That page no longer exists.
error-open-page = Could not open the page: { $error }
error-needs-section = Create a notebook and section before adding a page.
error-create-page = Could not create the page: { $error }
error-save = Save failed: { $error }
error-list-line-tags = Could not list tags: { $error }
error-recording-stopped = Recording stopped: { $error }
error-finish-recording = The recording could not be finished: { $error }
error-save-dictionary = Could not save your dictionary: { $error }
error-move-section = Could not move the section: { $error }
toast-trashed = Moved to trash
toast-trashed-with-subpages = Moved to trash with its subpages
error-trash-page = Could not move the page to the trash: { $error }
error-restore-page = Could not restore the page: { $error }
toast-restored = Restored
toast-page-moved = Page moved
toast-page-copied = Page copied to { $section }
error-move-page = Could not move the page: { $error }
error-move-copy = Could not move or copy: { $error }
toast-subpage-needs-page-above = Only a page below another page can become its subpage
error-page-level = Could not change the page level: { $error }
error-fold-page = Could not fold the page: { $error }

## L2.11 Toasts and errors, part 2
toast-notebook-created = Notebook created
error-create-notebook = Could not create notebook: { $error }
toast-section-created = Section created
error-create-section = Could not create section: { $error }
toast-group-created = Section group created
error-create-group = Could not create the section group: { $error }
error-rename-notebook = Could not rename notebook: { $error }
error-move-notebook = Could not move the notebook: { $error }
error-rename-section = Could not rename section: { $error }
error-section-gone = That section no longer exists.
error-rename-group = Could not rename the section group: { $error }
error-save-spell-language = Could not save the spelling language: { $error }
error-clear-dictionary = Could not clear your dictionary: { $error }
toast-versions-deleted = { $count ->
    [one] Deleted { $count } version
   *[other] Deleted { $count } versions
}
toast-version-deleted = Version deleted
error-delete-versions = Could not delete the versions: { $error }
toast-notebook-deleted = Notebook deleted
error-delete-notebook = Could not delete the notebook: { $error }
toast-section-deleted = Section moved to the Recycle Bin
error-delete-section = Could not delete the section: { $error }
error-restore-section = Could not restore the section: { $error }
toast-section-moved = Section moved
toast-section-copied = Section copied
toast-section-merged = { $count ->
    [one] 1 page merged
   *[other] { $count } pages merged
} into { $section }
toast-group-deleted = Section group deleted
error-delete-group = Could not delete the section group: { $error }
error-delete-page = Could not delete the page: { $error }
toast-page-deleted = Page deleted
error-empty-trash = Could not empty the trash: { $error }
toast-trash-emptied = Trash emptied

## L2.12 Toasts and errors, part 3
app-file-filter-database = Folio database
toast-backup-saved = Backup saved to { $path } (pictures in { $media })
error-backup = Backup failed: { $error }
app-file-filter-images = Images
app-file-filter-web-page = Web page
toast-exported = Exported { $path }
error-export = Export failed: { $error }
toast-exported-folder = { $count ->
    [one] 1 page exported
   *[other] { $count } pages exported
} to { $path }
error-export-folder = Could not export: { $error }
toast-print-opened = Print view opened in your browser. Choose Save as PDF to keep a copy.
error-print-view = Could not open print view: { $error }
error-link-kind = Folio only opens web, email and file links.
error-open-link = Could not open the link: { $error }
error-read-versions = Could not read the page versions: { $error }
error-restore-version = Could not restore that version: { $error }
toast-version-restored = Version restored. The page as it was is kept in Page Versions.
toast-page-saved = Page saved
toast-search-scope-saved = Searches now look in: { $scope }
error-close-unsaved = Your latest changes could not be saved. Close again to quit anyway.

## L2.13 Immersive Reader
app-reader-close = Close reader
app-reader-text = Text
app-reader-smaller = Smaller text
app-reader-larger = Larger text
app-reader-line-focus = Line focus
app-reader-line-focus-off = Off
app-reader-read-aloud = Read aloud
app-reader-reading-aloud = Reading aloud
app-reader-no-text = This page has no text to read.

## L2.14 Speech errors and tag chips
error-speech-dispatcher = Speech Dispatcher is not available: { $error }
error-system-voice = Could not start the system voice: { $error }
error-read-aloud-unsupported = Read aloud is not available on this platform.

## P2 Shell
app-header-search = Search
app-hide-navigation = Hide navigation
app-show-navigation = Show navigation
app-add-page = Add page
app-add-page-tip = Add a page at the end of the section
app-sort-pages = Sort pages
app-sort-none = None
app-sort-alphabetical = Alphabetical
app-sort-created = Date Created
app-sort-modified = Date Modified
app-recycle-bin = Notebook Recycle Bin
app-normal-view = Normal View
app-full-page-view = Full Page View
app-shortcut-ribbon = Show or hide the ribbon
error-save-ribbon = Could not save the ribbon layout: { $error }
error-save-page-sort = Could not save the page order: { $error }
error-save-setting = Could not save the setting: { $error }
error-no-quick-section = There is no Quick Notes section.

## P7 Backstage
app-backstage-back = Back
app-backstage-info = Info
app-backstage-new = New
app-backstage-open = Open
app-backstage-print = Print
app-backstage-export = Export
app-backstage-options = Options
app-backstage-close = Close
app-backstage-close-tip = Close Folio
app-info-notebooks = Your notebooks
app-section-count = { $count ->
    [one] 1 section
   *[other] { $count } sections
}
app-notebook-properties = Properties…
app-new-notebook-body = Each new notebook starts with one section.
app-open-import = Import notebook…
app-open-import-body = Bring in a notebook saved as a package (.onepkg) or a section (.one).
app-open-recent = Notebooks
app-print-body = Opens a print-ready copy of this page in your web browser. Use its Print dialog to print it or save it as a PDF.
app-print-button = Print…
app-print-no-page = Open a page to print it.
app-export-current = Export current:
app-export-page = Page
app-export-section = Section
app-export-notebook = Notebook
app-export-format = Select format:
app-export-html-format = Web page (.html)
app-export-button = Export…
app-export-folder-body = A section or notebook is saved as a folder of web pages, one per page, with an index.
dialog-about-title = About Folio
dialog-about-body = A native, local-first notebook. Your notes stay on this computer.
dialog-about-licences = Fonts and libraries from other authors are listed in THIRD_PARTY.md.

## P7 Options
app-options-title = Folio Options
app-options-general = General
app-options-display = Display
app-options-proofing = Proofing
app-options-save = Save & Backup
app-options-language = Language
app-options-advanced = Advanced
app-options-general-caption = General options for working with Folio.
app-options-display-caption = Change how Folio looks.
app-options-proofing-caption = Change how Folio checks your spelling.
app-options-save-caption = See where your notes are kept, and back them up.
app-options-language-caption = Set the language Folio speaks.
app-options-advanced-caption = Advanced options for working with Folio.
app-options-personalize = Personalize Folio
app-options-ui = User interface options
app-options-mini-toolbar = Show Mini Toolbar on selection
app-options-theme = Theme
app-options-ribbon = Ribbon
app-options-navigation = Navigation
app-options-tabs-layout = Tabs layout
app-options-page-list-left = Move page list to the left
app-options-pin-notebooks = Show notebook list on the left
app-options-scrollbar-left = Move vertical scroll bar to the left
app-options-horizontal-only = Only with horizontal tabs.
app-options-display-language = Display language
app-options-display-language-body = The language of menus, buttons and messages. Your notes are not changed.
app-options-search = Search
app-options-default-scope = Default search scope

## P7 Menus
# Menu rows in Title Case; the nav link, buttons and dialogs keep the sentence-case common-* ids.
app-menu-new-section = New Section
app-menu-new-section-group = New Section Group
app-menu-new-page = New Page
app-menu-new-notebook = New Notebook
app-menu-copy-link-to-page = Copy Link to Page
app-section-color-blue = Blue
app-section-color-yellow = Yellow
app-section-color-green = Green
app-section-color-red = Red
app-section-color-purple = Purple
app-section-color-cyan = Cyan
app-section-color-orange = Orange
app-section-color-magenta = Magenta
app-section-color-blue-mist = Blue Mist
app-section-color-purple-mist = Purple Mist
app-section-color-tan = Tan
app-section-color-lemon-lime = Lemon Lime
app-section-color-apple = Apple
app-section-color-teal = Teal
app-section-color-red-chalk = Red Chalk
app-section-color-silver = Silver
app-color-none = None
app-export-ellipsis = Export…
app-move-or-copy = Move or Copy…
app-merge-section = Merge into Another Section…
app-copy-link-section = Copy Link to Section
app-section-color = Section Color
app-show-page-versions = Show Page Versions
dialog-notebook-properties = Notebook Properties
dialog-display-name = Display name
dialog-delete-versions-title = Delete versions?
dialog-delete-versions-page-body = Every earlier version of this page is deleted. The page itself stays.
dialog-delete-versions-section-body = Every earlier version of the pages in this section is deleted. The pages stay.
dialog-delete-versions-notebook-body = Every earlier version of the pages in this notebook is deleted. The pages stay.
dialog-move-copy-pages = Move or Copy Pages
dialog-move-copy-section = Move or Copy Section
dialog-move-copy-pages-body = Move or copy “{ $title }” into:
dialog-move-copy-section-body = Move or copy the section “{ $name }” into:
dialog-move-copy-search = Search by name
dialog-move = Move
dialog-copy = Copy
dialog-merge-section = Merge Section
dialog-merge-body = Merge the pages of “{ $name }” into:
dialog-merge = Merge
dialog-merge-confirm = The pages move into “{ $target }” and the section “{ $name }” is deleted.

## P7 Panes
app-results-search = Search results: { $query }
app-results-edits = Recent edits: { $range }
app-results-count = { $count ->
    [one] 1 page
   *[other] { $count } pages
}
app-results-empty = No pages found.
app-versions-no-page = Open a page to see its versions.
app-spell-pane = Spelling
app-spell-not-in-dictionary = Not in dictionary:
app-spell-suggestions = Suggestions
app-spell-change = Change
app-spell-ignore = Ignore
app-spell-done = The spelling check is complete.
app-spell-start-over = Check again
app-spell-is-off = Spelling is turned off.
app-spell-open-options = Proofing options…
app-spell-no-page = Open a page to check its spelling.
app-spell-language = Language: { $name }
error-search = Could not search: { $error }

## P7 Search
app-search-scope = Search scope
app-scope-page = Find on This Page
app-scope-section = This Section
app-scope-group = This Section Group
app-scope-notebook = This Notebook
app-scope-all = All Notebooks
app-scope-set-default = Set This Scope as Default
app-search-recent = Recent pages
app-search-open-pane = Open in the search results pane

## P7 Horizontal Tabs
app-notebook-picker = Notebooks
app-pin-notebooks = Pin notebook pane to side
app-unpin-notebooks = Unpin notebook pane
app-parent-group = Navigate to parent section group
app-more-sections = More sections

## P9 Import notebook (File, Open). $name is a file or folder name.
import-notebook-title = Import notebook
import-notebook-body = Choose a notebook package (.onepkg), a notebook folder, or one section (.one). Folio copies it into a new notebook; the original files are not changed.
import-notebook-choose-file = Choose a file…
import-notebook-choose-folder = Choose a folder…
import-notebook-file-filter = Notebook files
import-notebook-reading = Reading “{ $name }”…
import-notebook-opening = Opening the notebook…
import-notebook-progress = Page { $done } of { $total }
import-notebook-done-title = Notebook imported
import-notebook-done-body = “{ $name }”: { $sections ->
    [one] { $sections } section
   *[other] { $sections } sections
}, { $pages ->
    [one] { $pages } page
   *[other] { $pages } pages
}.
import-notebook-also = Also imported: { $list }.
import-notebook-pictures = { $count ->
    [one] { $count } picture
   *[other] { $count } pictures
}
import-notebook-files = { $count ->
    [one] { $count } attached file
   *[other] { $count } attached files
}
import-notebook-tables = { $count ->
    [one] { $count } table
   *[other] { $count } tables
}
import-notebook-ink = { $count ->
    [one] { $count } ink stroke
   *[other] { $count } ink strokes
}
import-notebook-skipped = Not imported
import-notebook-skipped-password = { $count ->
    [one] A password-protected section: { $names }.
   *[other] Password-protected sections: { $names }.
} Remove the password in the app that made the notebook, then export again.
import-notebook-skipped-unreadable = Sections Folio could not read: { $names }
import-notebook-skipped-missing = { $count ->
    [one] { $count } file whose contents were not in the notebook
   *[other] { $count } files whose contents were not in the notebook
}
import-notebook-skipped-too-big = Files larger than { $max }: { $names }
import-notebook-skipped-pictures = { $count ->
    [one] { $count } picture in a format Folio cannot show, kept as an attached file
   *[other] { $count } pictures in a format Folio cannot show, kept as attached files
}
import-notebook-skipped-printouts = { $count ->
    [one] { $count } printout page of a word-processor document: the source app keeps no picture of it that Folio can read
   *[other] { $count } printout pages of word-processor documents: the source app keeps no picture of them that Folio can read
}
import-notebook-skipped-inline-ink = { $count ->
    [one] { $count } drawing inside text
   *[other] { $count } drawings inside text
}
import-notebook-skipped-math = { $count ->
    [one] { $count } equation, kept as plain text
   *[other] { $count } equations, kept as plain text
}
import-notebook-skipped-tags = Tags Folio does not have: { $names }
import-notebook-skipped-links = { $count ->
    [one] { $count } link to a page outside this notebook, or that Folio does not open (the text is kept)
   *[other] { $count } links to pages outside this notebook, or that Folio does not open (the text is kept)
}
import-notebook-failed-title = Could not import
import-notebook-failed-password = “{ $name }” cannot be read: it is password-protected or damaged. If it has a password, remove it in the app that made the notebook, then export it again.
import-notebook-failed-nothing = There is no notebook or section in “{ $name }”.
import-notebook-failed-too-large = “{ $name }” is larger than { $max } once unpacked: Folio does not import a notebook that big.
import-notebook-failed = Folio could not read “{ $name }”: { $error }
import-notebook-failed-crashed = Folio stopped reading “{ $name }”. Nothing was added to your notebooks.
import-notebook-cancelled = Import cancelled

## S2 Keys, maths and zoom
app-options-editing = Editing
app-options-auto-calc = Calculate mathematical expressions automatically
app-shortcut-line-break = Line break in a paragraph
app-shortcut-paragraphs = Previous / next paragraph
app-shortcut-page-ends = Top / bottom of the page
app-shortcut-scroll = Scroll the page
app-shortcut-pages = Previous / next page
app-shortcut-first-last-page = First / last page
app-shortcut-pages-in-view = First / last page in view
app-shortcut-move-page-order = Move the page up / down (outside a note)
app-shortcut-sections = Next / previous section
app-shortcut-back-forward = Back / forward through the pages visited
app-shortcut-zoom = Zoom in / out (outside a note)
