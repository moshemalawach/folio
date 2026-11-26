# Folio : la fenêtre, la bibliothèque, la liste des pages et les boîtes de dialogue (français).

## Paramètres
dialog-settings-language-automatic = Automatique ({ $language })
error-save-language = Impossible d’enregistrer la langue : { $error }

## L2.1 Barre du haut et barre latérale
app-search-hint = Rechercher dans les pages, le texte et les indicateurs
app-quick-notes = Notes rapides

## L2.2 Panneau de la bibliothèque
app-fold-notebook = Replier le bloc-notes
app-fold-section-group = Replier le groupe de sections
app-page-count = { $count ->
    [one] { $count } page
   *[other] { $count } pages
}

## L2.3 Volet des indicateurs
app-line-tags = Synthèse des indicateurs
app-refresh-line-tags = Actualiser les indicateurs
app-tags-scope-section = Section
app-tags-scope-notebook = Bloc-notes
app-tags-scope-all = Tous les blocs-notes
app-no-tagged-lines = Aucune ligne avec un indicateur
app-no-tagged-lines-body = Ajoutez un indicateur à une ligne avec { $first } à { $last } pour la retrouver ici.
app-tagged-line-label = { $tag } : { $text }
app-tags-group-by = Regrouper les indicateurs par :
app-tags-group-tag-name = Nom de l’indicateur
app-tags-group-section = Section
app-tags-group-title = Titre
app-tags-group-date = Date
app-tags-group-note-text = Texte de la note
app-tags-unchecked-only = Afficher uniquement les éléments désactivés
app-tags-create-summary = Créer une page de synthèse
app-tags-summary-title = Synthèse des indicateurs

## Customize Tags (Home › Tags ▾)
app-customize-tags-title = Personnaliser les indicateurs
app-customize-tags-note = Les modifications des indicateurs n’affectent pas les notes déjà marquées.
app-new-tag = Nouvel indicateur…
app-modify-tag = Modifier l’indicateur…
app-move-tag-up = Monter
app-move-tag-down = Descendre
app-tag-name = Nom d’affichage
app-tag-symbol = Symbole
app-tag-no-symbol = Aucun symbole
app-tag-font-color = Couleur de police
app-tag-highlight-color = Couleur de surbrillance
app-tag-automatic = Automatique
app-tag-no-color = Aucune couleur
app-tag-preview = Aperçu
app-tag-name-taken = Un indicateur porte déjà ce nom.

## L2.4 Liste des pages, lignes de page et leurs menus
app-view-pages = Pages
app-empty-trash = Vider la corbeille
app-list-empty-trash-title = La corbeille est vide
app-list-empty-trash-body = Les pages supprimées s’accumulent ici jusqu’à ce que vous vidiez la corbeille.
app-trash-sections = Sections supprimées
app-trash-section-detail = { $notebook } · { $count ->
    [one] { $count } page
   *[other] { $count } pages
}
app-list-empty-section-title = Cette section est vide
app-list-empty-section-body = Créez une page, ou déplacez-en une ici depuis une autre section.
app-row-words = { $count ->
    [one] { $count } mot
   *[other] { $count } mots
}
app-expand-subpages = Développer les sous-pages
app-collapse-subpages = Réduire les sous-pages
app-make-subpage = Créer une sous-page
app-promote-subpage = Remonter la sous-page
app-move-up = Déplacer vers le haut
app-move-down = Déplacer vers le bas
toast-link-copied = Lien vers la page copié
toast-section-link-copied = Lien vers la section copié

## L2.5 En-tête de l’éditeur, éditeur vide et barre d’état
app-page-in-trash = Cette page est dans la corbeille.
app-editor-empty-trash-title = La corbeille est vide
app-editor-empty-trash-body = Les pages que vous supprimez restent ici jusqu’à leur suppression définitive.
app-editor-empty-title = Une page blanche vous attend
app-editor-empty-body = Notez quelque chose. Vous pourrez l’organiser plus tard.
app-editor-empty-hint = { $shortcut } · Bloc-notes local

## L2.6 Palette de recherche
app-search-recent-empty = Vos pages récentes apparaîtront ici.
app-search-no-matches = Aucun résultat. Essayez une expression plus courte.

## L2.7 Boîtes de dialogue (nouveau, renommer, confirmer, lien)
dialog-new-notebook-body = Un bloc-notes contient des sections. Une section contient des pages.
dialog-notebook-name = Nom du bloc-notes
dialog-create-notebook = Créer le bloc-notes
dialog-new-section-body = Les sections regroupent les pages qui vont ensemble.
dialog-section-name = Nom de la section
dialog-create-section = Créer la section
dialog-new-group-body = Les groupes de sections rassemblent les sections qui vont ensemble, par exemple une par année scolaire.
dialog-group-name = Nom du groupe
dialog-create-group = Créer le groupe
dialog-rename-section = Renommer la section
dialog-rename-group = Renommer le groupe de sections
dialog-rename-body = Les noms restent sur cet appareil.
dialog-save-changes = Enregistrer
dialog-delete-notebook-title = Supprimer ce bloc-notes ?
dialog-delete-contents-body = Toutes les sections et pages qu’il contient seront supprimées de cet appareil. Cette action est irréversible.
dialog-delete-notebook = Supprimer le bloc-notes
dialog-delete-section-title = Supprimer cette section ?
dialog-delete-section-body = La section et ses pages vont dans la corbeille du bloc-notes, d’où vous pourrez les restaurer.
dialog-delete-section-forever-body = La section et ses pages seront supprimées de cet appareil. Cette action est irréversible.
dialog-delete-section = Supprimer la section
dialog-delete-group-title = Supprimer ce groupe de sections ?
dialog-delete-group-body = Ses sections et leurs pages vont dans la corbeille du bloc-notes, d’où vous pourrez les restaurer.
dialog-delete-group = Supprimer le groupe
dialog-delete-page-title = Supprimer définitivement cette page ?
dialog-delete-page-body = Cette page sera supprimée de l’appareil. Cette action est irréversible.
dialog-empty-trash-title = Vider la corbeille ?
dialog-empty-trash-body = Toutes les sections et pages de la corbeille seront supprimées de cet appareil. Cette action est irréversible.
dialog-edit-link = Modifier le lien
dialog-insert-link = Insérer un lien
dialog-link-body = Créez un lien vers une page web, une adresse e-mail, un fichier ou une autre page de Folio.
dialog-link-text = Texte à afficher
dialog-link-address = Adresse
dialog-link-address-hint = https://… ou choisissez une page ci-dessous
dialog-link-page = Ou un lien vers une page
dialog-link-search = Rechercher des pages
dialog-save-link = Enregistrer le lien
dialog-remove-link = Supprimer le lien
dialog-color = Couleur

## L2.8 Paramètres et versions de la page
app-spelling = Orthographe
app-spell-automatic = Automatique ({ $names })
app-spell-off = Désactivée
app-clear-personal-words = { $count ->
    [one] Effacer { $count } mot ajouté
   *[other] Effacer les { $count } mots ajoutés
}
app-your-files = Vos fichiers
app-notebook-count = { $count ->
    [one] { $count } bloc-notes
   *[other] { $count } blocs-notes
}
app-storage-stats = { $pages } · { $notebooks } · { $database } de base de données · { $media } de médias
app-back-up = Sauvegarder
app-open-folder = Ouvrir le dossier
app-settings-footer = Folio { $version } · bloc-notes local · MIT / Apache-2.0
app-versions-explain = Folio garde une copie de cette page quand vous la modifiez après l’avoir laissée de côté pendant 10 minutes.
app-versions-empty-title = Pas encore de version antérieure
app-versions-empty-body = Revenez plus tard sur cette page : les copies plus anciennes apparaîtront ici.
app-version-no-text = Aucun texte dans cette version. Elle ne contient peut-être que des dessins ou des images.
app-delete-versions = { $count ->
    [one] Supprimer { $count } version
   *[other] Supprimer les { $count } versions
}
app-restore-version = Restaurer la version
app-delete-all-versions = Supprimer toutes les versions…
app-delete-version = Supprimer la version

## L2.9 Boîte de dialogue des raccourcis clavier
app-shortcut-save = Enregistrer maintenant
app-shortcut-find = Rechercher dans la page
app-shortcut-format = Gras / italique / souligné
app-shortcut-link = Insérer un lien (pendant la saisie)
app-shortcut-open-link = Ouvrir un lien
app-shortcut-text-size = Agrandir / réduire le texte
app-shortcut-scripts = Exposant / indice
app-shortcut-align = Aligner à gauche / à droite
app-shortcut-clear-format = Effacer la mise en forme
app-shortcut-new-block = Nouveau bloc
app-shortcut-indent = Augmenter / diminuer le retrait
app-shortcut-headings = Titres 1 à 6
app-shortcut-move-paragraph = Monter / descendre le paragraphe
app-shortcut-show-levels = Afficher les niveaux du plan
app-shortcut-undo = Annuler / rétablir
app-shortcut-select-all = Tout sélectionner sur la page
app-shortcut-pan = Se déplacer dans la page
app-shortcut-equation = Insérer une équation
app-shortcut-page-below = Nouvelle page en dessous / sous-page
app-shortcut-page-level = Créer une sous-page / remonter
app-shortcut-tags = À faire / indicateurs
app-shortcut-remove-tags = Supprimer les indicateurs
app-shortcut-file-tab = Ouvrir l’onglet Fichier
app-shortcut-spelling = Vérifier l’orthographe
app-shortcut-title = Aller au titre de la page
app-shortcut-rule-lines = Afficher ou masquer les lignes de règle
app-shortcut-move-page = Déplacer ou copier la page
app-shortcut-print = Imprimer la page
app-shortcut-expand-collapse = Développer / réduire le paragraphe
app-shortcut-date-time = Insérer la date / l’heure / les deux
app-key-click = clic
app-key-drag = glisser

## L2.10 Messages et erreurs, partie 1
error-load-pages = Impossible de charger les pages : { $error }
error-page-gone = Cette page n’existe plus.
error-open-page = Impossible d’ouvrir la page : { $error }
error-needs-section = Créez un bloc-notes et une section avant d’ajouter une page.
error-create-page = Impossible de créer la page : { $error }
error-save = Échec de l’enregistrement : { $error }
error-list-line-tags = Impossible de lister les indicateurs : { $error }
error-recording-stopped = Enregistrement interrompu : { $error }
error-finish-recording = Impossible de terminer l’enregistrement : { $error }
error-save-dictionary = Impossible d’enregistrer votre dictionnaire : { $error }
error-move-section = Impossible de déplacer la section : { $error }
toast-trashed = Page mise à la corbeille
toast-trashed-with-subpages = Page mise à la corbeille avec ses sous-pages
error-trash-page = Impossible de mettre la page à la corbeille : { $error }
error-restore-page = Impossible de restaurer la page : { $error }
toast-restored = Page restaurée
toast-page-moved = Page déplacée
toast-page-copied = Page copiée dans { $section }
error-move-page = Impossible de déplacer la page : { $error }
error-move-copy = Impossible de déplacer ou de copier : { $error }
toast-subpage-needs-page-above = Seule une page placée sous une autre peut devenir sa sous-page
error-page-level = Impossible de changer le niveau de la page : { $error }
error-fold-page = Impossible de replier la page : { $error }

## L2.11 Messages et erreurs, partie 2
toast-notebook-created = Bloc-notes créé
error-create-notebook = Impossible de créer le bloc-notes : { $error }
toast-section-created = Section créée
error-create-section = Impossible de créer la section : { $error }
toast-group-created = Groupe de sections créé
error-create-group = Impossible de créer le groupe de sections : { $error }
error-rename-notebook = Impossible de renommer le bloc-notes : { $error }
error-move-notebook = Impossible de déplacer le bloc-notes : { $error }
error-rename-section = Impossible de renommer la section : { $error }
error-section-gone = Cette section n’existe plus.
error-rename-group = Impossible de renommer le groupe de sections : { $error }
error-save-spell-language = Impossible d’enregistrer la langue de vérification : { $error }
error-clear-dictionary = Impossible d’effacer votre dictionnaire : { $error }
toast-versions-deleted = { $count ->
    [one] { $count } version supprimée
   *[other] { $count } versions supprimées
}
toast-version-deleted = Version supprimée
error-delete-versions = Impossible de supprimer les versions : { $error }
toast-notebook-deleted = Bloc-notes supprimé
error-delete-notebook = Impossible de supprimer le bloc-notes : { $error }
toast-section-deleted = Section placée dans la corbeille
error-delete-section = Impossible de supprimer la section : { $error }
error-restore-section = Impossible de restaurer la section : { $error }
toast-section-moved = Section déplacée
toast-section-copied = Section copiée
toast-section-merged = { $count ->
    [one] { $count } page fusionnée
   *[other] { $count } pages fusionnées
} dans { $section }
toast-group-deleted = Groupe de sections supprimé
error-delete-group = Impossible de supprimer le groupe de sections : { $error }
error-delete-page = Impossible de supprimer la page : { $error }
toast-page-deleted = Page supprimée
error-empty-trash = Impossible de vider la corbeille : { $error }
toast-trash-emptied = Corbeille vidée

## L2.12 Messages et erreurs, partie 3
app-file-filter-database = Base de données Folio
toast-backup-saved = Sauvegarde enregistrée dans { $path } (images dans { $media })
error-backup = Échec de la sauvegarde : { $error }
app-file-filter-images = Images
app-file-filter-web-page = Page web
toast-exported = Page exportée dans { $path }
error-export = Échec de l’exportation : { $error }
toast-exported-folder = { $count ->
    [one] { $count } page exportée
   *[other] { $count } pages exportées
} dans { $path }
error-export-folder = Impossible d’exporter : { $error }
toast-print-opened = Aperçu avant impression ouvert dans votre navigateur. Choisissez Enregistrer au format PDF pour en garder une copie.
error-print-view = Impossible d’ouvrir l’aperçu avant impression : { $error }
error-link-kind = Folio n’ouvre que les liens web, e-mail et de fichiers.
error-open-link = Impossible d’ouvrir le lien : { $error }
error-read-versions = Impossible de lire les versions de la page : { $error }
error-restore-version = Impossible de restaurer cette version : { $error }
toast-version-restored = Version restaurée. La page telle qu’elle était est conservée dans Versions de page.
toast-page-saved = Page enregistrée
toast-search-scope-saved = Les recherches portent désormais sur : { $scope }
error-close-unsaved = Vos dernières modifications n’ont pas pu être enregistrées. Fermez à nouveau pour quitter quand même.

## L2.13 Lecteur immersif
app-reader-close = Fermer le lecteur
app-reader-text = Texte
app-reader-smaller = Texte plus petit
app-reader-larger = Texte plus grand
app-reader-line-focus = Focus de ligne
app-reader-line-focus-off = Aucun
app-reader-read-aloud = Lire à voix haute
app-reader-reading-aloud = Lecture à voix haute
app-reader-no-text = Cette page ne contient aucun texte à lire.

## L2.14 Erreurs de lecture vocale et étiquettes
error-speech-dispatcher = Speech Dispatcher n’est pas disponible : { $error }
error-system-voice = Impossible de démarrer la voix du système : { $error }
error-read-aloud-unsupported = La lecture à voix haute n’est pas disponible sur cette plateforme.

## P2 Fenêtre principale
app-header-search = Rechercher
app-hide-navigation = Masquer la navigation
app-show-navigation = Afficher la navigation
app-add-page = Ajouter une page
app-add-page-tip = Ajouter une page à la fin de la section
app-sort-pages = Trier les pages
app-sort-none = Aucun
app-sort-alphabetical = Alphabétique
app-sort-created = Date de création
app-sort-modified = Date de modification
app-recycle-bin = Corbeille du bloc-notes
app-normal-view = Affichage normal
app-full-page-view = Affichage page entière
app-shortcut-ribbon = Afficher ou masquer le ruban
error-save-ribbon = Impossible d’enregistrer la disposition du ruban : { $error }
error-save-page-sort = Impossible d’enregistrer l’ordre des pages : { $error }
error-save-setting = Impossible d’enregistrer le réglage : { $error }
error-no-quick-section = Il n’y a pas de section Notes rapides.

## P7 Backstage
app-backstage-back = Retour
app-backstage-info = Informations
app-backstage-new = Nouveau
app-backstage-open = Ouvrir
app-backstage-print = Imprimer
app-backstage-export = Exporter
app-backstage-options = Options
app-backstage-close = Fermer
app-backstage-close-tip = Fermer Folio
app-info-notebooks = Vos blocs-notes
app-section-count = { $count ->
    [one] { $count } section
   *[other] { $count } sections
}
app-notebook-properties = Propriétés…
app-new-notebook-body = Chaque nouveau bloc-notes commence par une section.
app-open-import = Importer un bloc-notes…
app-open-import-body = Importer un bloc-notes enregistré sous forme de package (.onepkg) ou de section (.one).
app-open-recent = Blocs-notes
app-print-body = Ouvre une copie de cette page prête à imprimer dans votre navigateur. Utilisez sa boîte de dialogue Imprimer pour l’imprimer ou l’enregistrer en PDF.
app-print-button = Imprimer…
app-print-no-page = Ouvrez une page pour l’imprimer.
app-export-current = Exporter l’élément actif :
app-export-page = Page
app-export-section = Section
app-export-notebook = Bloc-notes
app-export-format = Sélectionner le format :
app-export-html-format = Page web (.html)
app-export-button = Exporter…
app-export-folder-body = Une section ou un bloc-notes est enregistré dans un dossier de pages web, une par page, avec un index.
dialog-about-title = À propos de Folio
dialog-about-body = Un bloc-notes natif et local. Vos notes restent sur cet ordinateur.
dialog-about-licences = Les polices et bibliothèques d’autres auteurs sont répertoriées dans THIRD_PARTY.md.

## P7 Options
app-options-title = Options de Folio
app-options-general = Général
app-options-display = Affichage
app-options-proofing = Vérification
app-options-save = Enregistrement et sauvegarde
app-options-language = Langue
app-options-advanced = Options avancées
app-options-general-caption = Options générales d’utilisation de Folio.
app-options-display-caption = Modifier l’apparence de Folio.
app-options-proofing-caption = Modifier la façon dont Folio vérifie l’orthographe.
app-options-save-caption = Voir où vos notes sont conservées et les sauvegarder.
app-options-language-caption = Choisir la langue de Folio.
app-options-advanced-caption = Options avancées d’utilisation de Folio.
app-options-personalize = Personnaliser Folio
app-options-ui = Options d’interface utilisateur
app-options-mini-toolbar = Afficher la mini barre d’outils lors de la sélection
app-options-theme = Thème
app-options-ribbon = Ruban
app-options-navigation = Navigation
app-options-tabs-layout = Disposition des onglets
app-options-page-list-left = Déplacer la liste des pages à gauche
app-options-pin-notebooks = Afficher la liste des blocs-notes à gauche
app-options-scrollbar-left = Déplacer la barre de défilement verticale à gauche
app-options-horizontal-only = Uniquement avec les onglets horizontaux.
app-options-display-language = Langue d’affichage
app-options-display-language-body = La langue des menus, des boutons et des messages. Vos notes ne sont pas modifiées.
app-options-search = Recherche
app-options-default-scope = Étendue de recherche par défaut

## P7 Menus
# Les lignes de menu ; en français elles reprennent les libellés common-*.
app-menu-new-section = Nouvelle section
app-menu-new-section-group = Nouveau groupe de sections
app-menu-new-page = Nouvelle page
app-menu-new-notebook = Nouveau bloc-notes
app-menu-copy-link-to-page = Copier le lien vers la page
app-section-color-blue = Bleu
app-section-color-yellow = Jaune
app-section-color-green = Vert
app-section-color-red = Rouge
app-section-color-purple = Violet
app-section-color-cyan = Cyan
app-section-color-orange = Orange
app-section-color-magenta = Magenta
app-section-color-blue-mist = Bleu brume
app-section-color-purple-mist = Mauve
app-section-color-tan = Beige
app-section-color-lemon-lime = Citron vert
app-section-color-apple = Vert pomme
app-section-color-teal = Bleu-vert
app-section-color-red-chalk = Craie rouge
app-section-color-silver = Argent
app-color-none = Aucune couleur
app-export-ellipsis = Exporter…
app-move-or-copy = Déplacer ou copier…
app-merge-section = Fusionner dans une autre section…
app-copy-link-section = Copier le lien vers la section
app-section-color = Couleur de la section
app-show-page-versions = Afficher les versions de page
dialog-notebook-properties = Propriétés du bloc-notes
dialog-display-name = Nom d’affichage
dialog-delete-versions-title = Supprimer les versions ?
dialog-delete-versions-page-body = Toutes les versions précédentes de cette page sont supprimées. La page elle-même est conservée.
dialog-delete-versions-section-body = Toutes les versions précédentes des pages de cette section sont supprimées. Les pages sont conservées.
dialog-delete-versions-notebook-body = Toutes les versions précédentes des pages de ce bloc-notes sont supprimées. Les pages sont conservées.
dialog-move-copy-pages = Déplacer ou copier des pages
dialog-move-copy-section = Déplacer ou copier une section
dialog-move-copy-pages-body = Déplacer ou copier « { $title } » dans :
dialog-move-copy-section-body = Déplacer ou copier la section « { $name } » dans :
dialog-move-copy-search = Rechercher par nom
dialog-move = Déplacer
dialog-copy = Copier
dialog-merge-section = Fusionner la section
dialog-merge-body = Fusionner les pages de « { $name } » dans :
dialog-merge = Fusionner
dialog-merge-confirm = Les pages vont dans « { $target } » et la section « { $name } » est supprimée.

## P7 Volets
app-results-search = Résultats de la recherche : { $query }
app-results-edits = Modifications récentes : { $range }
app-results-count = { $count ->
    [one] { $count } page
   *[other] { $count } pages
}
app-results-empty = Aucune page trouvée.
app-versions-no-page = Ouvrez une page pour voir ses versions.
app-spell-pane = Orthographe
app-spell-not-in-dictionary = Absent du dictionnaire :
app-spell-suggestions = Suggestions
app-spell-change = Modifier
app-spell-ignore = Ignorer
app-spell-done = La vérification orthographique est terminée.
app-spell-start-over = Vérifier à nouveau
app-spell-is-off = La vérification orthographique est désactivée.
app-spell-open-options = Options de vérification…
app-spell-no-page = Ouvrez une page pour en vérifier l’orthographe.
app-spell-language = Langue : { $name }
error-search = Impossible d’effectuer la recherche : { $error }

## P7 Recherche
app-search-scope = Étendue de la recherche
app-scope-page = Page active
app-scope-section = Cette section
app-scope-group = Ce groupe de sections
app-scope-notebook = Ce bloc-notes
app-scope-all = Tous les blocs-notes
app-scope-set-default = Définir cette étendue par défaut
app-search-recent = Pages récentes
app-search-open-pane = Ouvrir dans le volet des résultats

## P7 Onglets horizontaux
app-notebook-picker = Blocs-notes
app-pin-notebooks = Épingler le volet des blocs-notes sur le côté
app-unpin-notebooks = Détacher le volet des blocs-notes
app-parent-group = Accéder au groupe de sections parent
app-more-sections = Autres sections

## P9 Importer un bloc-notes (Fichier, Ouvrir). $name est un nom de fichier ou de dossier.
import-notebook-title = Importer un bloc-notes
import-notebook-body = Choisissez un package de bloc-notes (.onepkg), un dossier de bloc-notes ou une section (.one). Folio le copie dans un nouveau bloc-notes ; les fichiers d’origine ne sont pas modifiés.
import-notebook-choose-file = Choisir un fichier…
import-notebook-choose-folder = Choisir un dossier…
import-notebook-file-filter = Fichiers de bloc-notes
import-notebook-reading = Lecture de « { $name } »…
import-notebook-opening = Ouverture du bloc-notes…
import-notebook-progress = Page { $done } sur { $total }
import-notebook-done-title = Bloc-notes importé
import-notebook-done-body = « { $name } » : { $sections ->
    [one] { $sections } section
   *[other] { $sections } sections
}, { $pages ->
    [one] { $pages } page
   *[other] { $pages } pages
}.
import-notebook-also = Également importés : { $list }.
import-notebook-pictures = { $count ->
    [one] { $count } image
   *[other] { $count } images
}
import-notebook-files = { $count ->
    [one] { $count } fichier joint
   *[other] { $count } fichiers joints
}
import-notebook-tables = { $count ->
    [one] { $count } tableau
   *[other] { $count } tableaux
}
import-notebook-ink = { $count ->
    [one] { $count } trait d’encre
   *[other] { $count } traits d’encre
}
import-notebook-skipped = Non importé
import-notebook-skipped-password = { $count ->
    [one] Section protégée par mot de passe : { $names }.
   *[other] Sections protégées par mot de passe : { $names }.
} Retirez le mot de passe dans l’application qui a créé le bloc-notes, puis exportez à nouveau.
import-notebook-skipped-unreadable = Sections que Folio n’a pas pu lire : { $names }
import-notebook-skipped-missing = { $count ->
    [one] { $count } fichier dont le contenu manquait dans le bloc-notes
   *[other] { $count } fichiers dont le contenu manquait dans le bloc-notes
}
import-notebook-skipped-too-big = Fichiers de plus de { $max } : { $names }
import-notebook-skipped-pictures = { $count ->
    [one] { $count } image dans un format que Folio ne sait pas afficher, gardée en fichier joint
   *[other] { $count } images dans un format que Folio ne sait pas afficher, gardées en fichiers joints
}
import-notebook-skipped-printouts = { $count ->
    [one] { $count } page imprimée d’un document de traitement de texte : l’application source n’en garde pas d’image que Folio sache lire
   *[other] { $count } pages imprimées de documents de traitement de texte : l’application source n’en garde pas d’image que Folio sache lire
}
import-notebook-skipped-inline-ink = { $count ->
    [one] { $count } dessin dans le texte
   *[other] { $count } dessins dans le texte
}
import-notebook-skipped-math = { $count ->
    [one] { $count } équation, gardée en texte simple
   *[other] { $count } équations, gardées en texte simple
}
import-notebook-skipped-tags = Indicateurs que Folio n’a pas : { $names }
import-notebook-skipped-links = { $count ->
    [one] { $count } lien vers une page hors de ce bloc-notes, ou que Folio n’ouvre pas (le texte est gardé)
   *[other] { $count } liens vers des pages hors de ce bloc-notes, ou que Folio n’ouvre pas (le texte est gardé)
}
import-notebook-failed-title = Importation impossible
import-notebook-failed-password = « { $name } » est illisible : la section est protégée par mot de passe ou endommagée. Si elle a un mot de passe, retirez-le dans l’application qui a créé le bloc-notes, puis exportez-la à nouveau.
import-notebook-failed-nothing = Aucun bloc-notes ni aucune section dans « { $name } ».
import-notebook-failed-too-large = « { $name } » fait plus de { $max } une fois décompressé : Folio n’importe pas un bloc-notes aussi gros.
import-notebook-failed = Folio n’a pas pu lire « { $name } » : { $error }
import-notebook-failed-crashed = Folio a arrêté de lire « { $name } ». Rien n’a été ajouté à vos blocs-notes.
import-notebook-cancelled = Importation annulée

## S2 Touches, calculs et zoom
app-options-editing = Modification
app-options-auto-calc = Calculer automatiquement les expressions mathématiques
app-shortcut-line-break = Saut de ligne dans un paragraphe
app-shortcut-paragraphs = Paragraphe précédent / suivant
app-shortcut-page-ends = Haut / bas de la page
app-shortcut-scroll = Faire défiler la page
app-shortcut-pages = Page précédente / suivante
app-shortcut-first-last-page = Première / dernière page
app-shortcut-pages-in-view = Première / dernière page visible
app-shortcut-move-page-order = Monter / descendre la page (hors d’une note)
app-shortcut-sections = Section suivante / précédente
app-shortcut-back-forward = Revenir / avancer dans les pages visitées
app-shortcut-zoom = Zoom avant / arrière (hors d’une note)
