# Folio: dates, times, numbers and units (English). Key names live in common.ftl.

## Month and weekday names, looked up by number: month-1 is January, weekday-1 is Monday.
month-1 = January
month-2 = February
month-3 = March
month-4 = April
month-5 = May
month-6 = June
month-7 = July
month-8 = August
month-9 = September
month-10 = October
month-11 = November
month-12 = December
month-short-1 = Jan
month-short-2 = Feb
month-short-3 = Mar
month-short-4 = Apr
month-short-5 = May
month-short-6 = Jun
month-short-7 = Jul
month-short-8 = Aug
month-short-9 = Sep
month-short-10 = Oct
month-short-11 = Nov
month-short-12 = Dec
weekday-1 = Monday
weekday-2 = Tuesday
weekday-3 = Wednesday
weekday-4 = Thursday
weekday-5 = Friday
weekday-6 = Saturday
weekday-7 = Sunday

## Weekday initials over the columns of a calendar, Monday first like the names above.
weekday-initial-1 = Mo
weekday-initial-2 = Tu
weekday-initial-3 = We
weekday-initial-4 = Th
weekday-initial-5 = Fr
weekday-initial-6 = Sa
weekday-initial-7 = Su

## Dates. $month and $weekday are names from above, $day and $year are numbers, $time is "11:09".
date-full = { $month } { $day }, { $year } · { $time }
date-long = { $weekday }, { $month } { $day }, { $year }
date-short = { $month } { $day }
date-short-year = { $month } { $day }, { $year }
## A calendar's heading: "October 2026".
date-month-year = { $month } { $year }

## How long ago
time-just-now = Just now
time-minutes-ago = { $count } min ago
time-hours-ago = { $count } h ago
time-yesterday = Yesterday
time-days-ago = { $count ->
    [one] { $count } day ago
   *[other] { $count } days ago
}

## Numbers and sizes. $size and $value come already formatted ("1.5").
number-decimal-separator = .
unit-bytes = { $size } B
unit-kilobytes = { $size } KB
unit-megabytes = { $size } MB
unit-gigabytes = { $size } GB
unit-millimetres = { $size } mm

## Library errors. $error is the system's own text and stays as the system gives it.
data-error-database = database error: { $error }
data-error-file = file error: { $error }
data-error-data = data error: { $error }
data-error-attachment-too-big = “{ $name }” is { $size }; Folio can attach files up to { $max }.
data-error-group-elsewhere = That section group is not in this notebook.
data-error-merge-self = A section can’t be merged into itself.
data-error-page-not-created = page was not created
data-error-page-gone-in-notebook = this page no longer exists in the notebook
data-error-version-gone = that version no longer exists
data-error-file-missing = This file is missing from the notebook.
data-error-section-in-bin = This page’s section is in the Recycle Bin: restore the section first.
data-error-backup-folder = choose a folder outside the live notebook's own folder

## Shown above a page whose stored content was partly damaged.
data-notice-damaged-saved = Part of this page could not be read. The original was saved in { $dir }.
data-notice-damaged = Part of this page could not be read ({ $error }).

## Names stored in the library when the user gives none. They belong to the user afterwards.
data-default-notebook = New notebook
data-default-first-section = First section
data-default-section = New section
data-default-group = New section group
data-fallback-notebook = Notebook
data-fallback-section = Section
data-fallback-group = Section group

## Exported and printed pages.
data-export-audio = Audio recording · { $time }

## P5 Time stamps and page templates
# Alt+Shift+F's stamp: $date is a short date such as 10/29/2026, $time a time such as 18:41.
date-and-time = { $date } { $time }
data-template-topic = Topic
data-template-key-points = Key points
data-template-questions = Questions
data-template-summary = Summary
data-template-to-do = To do
data-template-date = Date: { $date }
data-template-attendees = Attendees
data-template-agenda = Agenda
data-template-notes = Notes
data-template-actions = Action items
