# Folio : la recherche, l’orthographe, les équations, les croquis, l’audio et le lecteur (français).

## L4.5 Le menu d’orthographe
spell-looking = Recherche de suggestions…
spell-no-suggestions = Aucune suggestion
spell-add-to-dictionary = Ajouter au dictionnaire
spell-ignore-all = Tout ignorer

## L4.8 Rechercher dans la page
find-no-matches = Aucun résultat
find-position = { $current } sur { $total }

## L4.8 Erreurs d’équation
math-error-too-deep = Cette équation est trop imbriquée
math-error-missing-end = Il manque quelque chose à la fin
math-error-stray-close = Il y a une accolade « {"}"} » sans « {"{"} » correspondante
math-error-nothing-before = Mettez quelque chose avant « { $symbol } »
math-error-missing-close = Il manque « { $symbol } »
math-error-needs-group = { $command } doit être suivi de {"{"}…{"}"}

## L4.9 L’état de l’orthographe
spell-names-two = { $first } et { $second }
spell-status-off = L’orthographe n’est pas vérifiée.
spell-status-loading = { $count ->
    [one] Chargement du dictionnaire { $names }…
   *[other] Chargement des dictionnaires { $names }…
}
spell-status-ready = Vérification de l’orthographe : { $names }.
spell-status-failed = { $count ->
    [one] Impossible de charger le dictionnaire { $names } : { $error }
   *[other] Impossible de charger les dictionnaires { $names } : { $error }
}
spell-error-unreadable = le dictionnaire n’a pas pu être lu

## L4.9 Erreurs audio (elles suivent « … : » dans les notifications, d’où la minuscule)
audio-error-not-playable = ce fichier n’est pas un enregistrement que Folio sait lire
audio-error-no-microphone = aucun microphone n’a été trouvé
audio-error-mic-format = le format d’échantillon du microphone ({ $format }) n’est pas pris en charge
audio-error-already-stopped = l’enregistrement était déjà arrêté
audio-error-stopped-unexpectedly = l’enregistrement s’est arrêté de façon inattendue
audio-error-not-saved = l’enregistrement n’a pas pu être écrit sur le disque
audio-error-unreadable = cet enregistrement n’a pas pu être lu
audio-error-no-speakers = aucun haut-parleur ni casque n’a été trouvé
audio-error-speaker-format = le format d’échantillon des haut-parleurs ({ $format }) n’est pas pris en charge
