# Folio : dates, heures, nombres et unités (français). Les noms des touches sont dans common.ftl.

## Noms des mois et des jours, par numéro : month-1 est janvier, weekday-1 est lundi.
month-1 = janvier
month-2 = février
month-3 = mars
month-4 = avril
month-5 = mai
month-6 = juin
month-7 = juillet
month-8 = août
month-9 = septembre
month-10 = octobre
month-11 = novembre
month-12 = décembre
month-short-1 = janv.
month-short-2 = févr.
month-short-3 = mars
month-short-4 = avr.
month-short-5 = mai
month-short-6 = juin
month-short-7 = juil.
month-short-8 = août
month-short-9 = sept.
month-short-10 = oct.
month-short-11 = nov.
month-short-12 = déc.
weekday-1 = lundi
weekday-2 = mardi
weekday-3 = mercredi
weekday-4 = jeudi
weekday-5 = vendredi
weekday-6 = samedi
weekday-7 = dimanche

## Initiales des jours en tête des colonnes d’un calendrier, lundi d’abord comme ci-dessus.
weekday-initial-1 = lu
weekday-initial-2 = ma
weekday-initial-3 = me
weekday-initial-4 = je
weekday-initial-5 = ve
weekday-initial-6 = sa
weekday-initial-7 = di

## Dates. Le 1er du mois s’écrit « 1er ».
date-full = { $day ->
    [1] 1er
   *[other] { $day }
} { $month } { $year } · { $time }
date-long = { $weekday } { $day ->
    [1] 1er
   *[other] { $day }
} { $month } { $year }
date-short = { $day ->
    [1] 1er
   *[other] { $day }
} { $month }
date-short-year = { $day ->
    [1] 1er
   *[other] { $day }
} { $month } { $year }
## En-tête d’un calendrier : « octobre 2026 ».
date-month-year = { $month } { $year }

## Il y a combien de temps
time-just-now = À l’instant
time-minutes-ago = Il y a { $count } min
time-hours-ago = Il y a { $count } h
time-yesterday = Hier
time-days-ago = { $count ->
    [one] Il y a { $count } jour
   *[other] Il y a { $count } jours
}

## Nombres et tailles. $size et $value arrivent déjà formatés (« 1,5 »).
number-decimal-separator = ,
unit-bytes = { $size } o
unit-kilobytes = { $size } Ko
unit-megabytes = { $size } Mo
unit-gigabytes = { $size } Go
unit-millimetres = { $size } mm

## Erreurs de la bibliothèque. $error est le texte du système, laissé tel quel.
data-error-database = erreur de base de données : { $error }
data-error-file = erreur de fichier : { $error }
data-error-data = erreur de données : { $error }
data-error-attachment-too-big = « { $name } » fait { $size } ; Folio accepte les fichiers jusqu’à { $max }.
data-error-group-elsewhere = Ce groupe de sections n’est pas dans ce bloc-notes.
data-error-merge-self = Une section ne peut pas être fusionnée avec elle-même.
data-error-page-not-created = la page n’a pas été créée
data-error-page-gone-in-notebook = cette page n’existe plus dans le bloc-notes
data-error-version-gone = cette version n’existe plus
data-error-file-missing = Ce fichier est introuvable dans le bloc-notes.
data-error-section-in-bin = La section de cette page est dans la corbeille : restaurez d’abord la section.
data-error-backup-folder = choisissez un dossier en dehors de celui du bloc-notes ouvert

## Affiché au-dessus d’une page dont le contenu enregistré est en partie endommagé.
data-notice-damaged-saved = Une partie de cette page n’a pas pu être lue. L’original a été enregistré dans { $dir }.
data-notice-damaged = Une partie de cette page n’a pas pu être lue ({ $error }).

## Noms enregistrés dans la bibliothèque quand l’utilisateur n’en donne pas. Ils lui appartiennent ensuite.
data-default-notebook = Nouveau bloc-notes
data-default-first-section = Première section
data-default-section = Nouvelle section
data-default-group = Nouveau groupe de sections
data-fallback-notebook = Bloc-notes
data-fallback-section = Section
data-fallback-group = Groupe de sections

## Pages exportées et imprimées.
data-export-audio = Enregistrement audio · { $time }

## P5 Horodatages et modèles de page
# L’horodatage d’Alt+Maj+F : $date est une date courte comme 29/10/2026, $time une heure comme 18:41.
date-and-time = { $date } { $time }
data-template-topic = Sujet
data-template-key-points = Points clés
data-template-questions = Questions
data-template-summary = Résumé
data-template-to-do = À faire
data-template-date = Date : { $date }
data-template-attendees = Participants
data-template-agenda = Ordre du jour
data-template-notes = Notes
data-template-actions = Actions à mener
