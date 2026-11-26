//! The sample library in French: the same ids, colours, layout and outlines as the
//! English one in `seed.rs`, with words written for a French pupil (a French class, an
//! English class, SVT, Histoire-géo) rather than translated line by line. French
//! typography: a no-break space (U+00A0) before « : ; ! ? », inside guillemets and
//! between a number and its unit; ’ for apostrophes.

use super::*;

pub(super) static TEXT: SeedText = SeedText {
    notebook_school: "École",
    notebook_personal: "Personnel",
    group_last_year: "L’an dernier",
    section_this_week: "Cette semaine",
    section_history: "Histoire-géo",
    section_science: "SVT",
    section_language: "Français",
    section_foreign: "Anglais",
    section_maths: "Maths",
    section_last_year: "Géographie",
    section_journal: "Journal",
    section_recipes: "Recettes",
    section_quick: "Notes rapides",
    week: WeekText {
        title: "Ma semaine",
        plan: [
            "L’essentiel de la semaine",
            "Trois choses, pas trente. Le reste peut attendre vendredi.",
            "Finir le plan d’**histoire**",
            "Imprimer la fiche de SVT",
            "Réviser le contrôle de maths de jeudi",
            "Demander qui fait quoi pour l’exposé",
        ],
        reminders: [
            "Apporter le classeur bleu jeudi.",
            "Affaires de sport lundi et mercredi.",
        ],
        term: [
            ["Septembre", "Semaine 1", "La rentrée"],
            ["Octobre", "Semaine 6", "Contrôle d’histoire"],
            ["Décembre", "Semaine 15", "Exposé de SVT"],
        ],
    },
    ages: AgesText {
        title: "Le Moyen Âge",
        notes: [
            "Le Moyen Âge (476–1492)",
            "Environ mille ans, de la chute de Rome aux Grandes Découvertes.",
            "**La société**\u{a0}: les trois ordres",
            "ceux qui prient\u{a0}: le clergé",
            "ceux qui combattent\u{a0}: les seigneurs et les chevaliers",
            "ceux qui travaillent\u{a0}: les paysans, presque tout le monde",
            "**La féodalité**",
            "un seigneur donne une terre, le fief, à un vassal",
            "le vassal lui jure fidélité et combat pour lui",
            "**Fief**\u{a0}: terre donnée en échange de la fidélité.",
            "Pourquoi les châteaux passent-ils du bois à la pierre\u{a0}?",
        ],
        frise: [
            ["476", "Chute de Rome", "Fin de l’Empire d’Occident"],
            ["800", "Charlemagne", "Couronné empereur"],
            ["1066", "Hastings", "Conquête normande"],
            ["1347", "Peste noire", "La peste atteint l’Europe"],
        ],
    },
    castles: CastlesText {
        title: "Les châteaux forts",
        notes: [
            "Le château fort protège le seigneur, sa famille et les villageois des alentours.",
            "le **donjon**\u{a0}: la tour la plus haute et le dernier refuge",
            "la courtine et ses créneaux",
            "les douves et le pont-levis",
            "Légender le dessin pour vendredi",
        ],
        caption: "Mon château, dessiné en classe",
        picture_name: "chateau-fort.png",
    },
    cell: CellText {
        title: "La cellule végétale",
        intro: [
            "Seules les cellules végétales ont une paroi et des chloroplastes.",
            "Les chloroplastes utilisent la lumière pour fabriquer du sucre\u{a0}: c’est la photosynthèse.",
        ],
        cell: "Cellule végétale",
        parts: ["Noyau", "Paroi", "Lumière", "Chloroplaste"],
        table_title: "**Les parties de la cellule**",
        table: [
            ["Partie", "Rôle"],
            ["Noyau", "Contient l’ADN"],
            ["Paroi", "Maintient la forme"],
            ["Chloroplaste", "Fait la photosynthèse"],
        ],
    },
    figures: FiguresText {
        title: "Les figures de style",
        notes: [
            "Les figures de style",
            "**Comparaison**\u{a0}: rapproche deux éléments grâce à un outil («\u{a0}comme\u{a0}», «\u{a0}tel\u{a0}»).",
            "«\u{a0}Il est fort comme un lion.\u{a0}»",
            "**Métaphore**\u{a0}: une comparaison sans outil.",
            "«\u{a0}Cet homme est un lion.\u{a0}»",
            "**Personnification**\u{a0}: prête des sentiments humains à une chose.",
            "«\u{a0}Le vent murmure dans les arbres.\u{a0}»",
            "**Hyperbole**\u{a0}: une exagération.",
            "«\u{a0}Je te l’ai dit mille fois\u{a0}!\u{a0}»",
            "Trouver deux métaphores dans le chapitre 3",
            "Lecture\u{a0}: Le Petit Prince, chapitres 1 à 3",
        ],
    },
    vocab: VocabText {
        title: "Vocabulaire\u{a0}: la nourriture",
        heading: "Au café",
        table: [
            ["Anglais", "Français"],
            ["bread", "le pain"],
            ["cheese", "le fromage"],
            ["an apple", "une pomme"],
            ["breakfast", "le petit-déjeuner"],
            ["I’m hungry", "j’ai faim"],
        ],
        after: [
            "Apprendre la liste pour lundi",
            "Comment dire poliment «\u{a0}je voudrais…\u{a0}» en anglais\u{a0}?",
        ],
    },
    pythagoras: PythagorasText {
        title: "Le théorème de Pythagore",
        notes: [
            "Dans un triangle rectangle, le carré de l’hypoténuse est égal à la somme des carrés des deux autres côtés.",
            "",
            "**Exemple**",
            "a = 3\u{a0}cm et b = 4\u{a0}cm",
            "",
            "donc c = 5\u{a0}cm",
            "Ça marche pour tous les triangles\u{a0}? Non, seulement pour les triangles rectangles.",
        ],
    },
    worksheet: WorksheetText {
        title: "Fiche d’exercices\u{a0}: Pythagore",
        file_name: "fiche-pythagore.pdf",
        page_name: "fiche-pythagore page 1",
        sheet: [
            "Fiche d’exercices — Théorème de Pythagore",
            "Nom\u{a0}: ______________________     Classe\u{a0}: ________",
            "1. ABC est rectangle en A, avec AB = 6\u{a0}cm et AC = 8\u{a0}cm. Calcule BC.",
            "2. Un triangle de côtés 5\u{a0}cm, 12\u{a0}cm et 13\u{a0}cm est-il rectangle\u{a0}?",
            "3. Une échelle de 5\u{a0}m est posée à 3\u{a0}m d’un mur. Jusqu’où monte-t-elle\u{a0}?",
        ],
        notes: ["À rendre lundi", "Détailler chaque étape"],
    },
    rivers: RiversText {
        title: "Les grands fleuves",
        notes: [
            "Les plus longs fleuves",
            "le Nil, en Afrique\u{a0}: environ 6\u{a0}650\u{a0}km",
            "l’Amazone, en Amérique du Sud\u{a0}: environ 6\u{a0}400\u{a0}km",
            "le Yangzi Jiang, en Chine\u{a0}: environ 6\u{a0}300\u{a0}km",
            "**Source**\u{a0}: là où naît le fleuve. **Embouchure**\u{a0}: là où il se jette dans la mer.",
            "En France, le plus long fleuve est la Loire (1\u{a0}006\u{a0}km).",
        ],
    },
    journal: JournalText {
        title: "Un samedi tranquille",
        notes: [
            "Il a plu toute la matinée, alors on est restés à la maison faire des gâteaux.",
            "L’après-midi, longue balade jusqu’au lac avec Papi. Il connaît le nom de tous les oiseaux.",
            "Idée\u{a0}: un carnet d’oiseaux, avec une page pour chaque oiseau aperçu.",
            "Les journées tranquilles ont du bon.",
        ],
    },
    pancakes: PancakesText {
        title: "Les crêpes",
        ingredients: [
            "Ingrédients (8 crêpes)",
            "250\u{a0}g de farine",
            "4 œufs",
            "un demi-litre de lait",
            "une pincée de sel",
            "2 cuillères à soupe de beurre fondu",
        ],
        method: [
            "Préparation",
            "Mélanger la farine, les œufs et le sel.",
            "Ajouter le lait petit à petit, puis le beurre.",
            "Laisser reposer la pâte une heure.",
            "ou toute une nuit au frigo",
            "Cuire dans une poêle bien chaude, environ une minute de chaque côté.",
            "Demander à un adulte avant d’utiliser la plaque\u{a0}!",
        ],
    },
    quick: QuickText {
        title: "Avant vendredi",
        notes: [
            "Faire signer l’autorisation de sortie",
            "Rendre les livres au CDI le 12",
        ],
    },
};
