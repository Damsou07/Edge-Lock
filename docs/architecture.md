# Architecture du MVP

## Composants

| Composant | Responsabilité |
| --- | --- |
| `src/` | Interface React/TypeScript, navigation, formulaires, suivi des tâches et lecteur Three.js. Aucun accès direct arbitraire aux fichiers. |
| `src-tauri/` | Commandes IPC typées, dialogues natifs, réglages, lancement des tâches, watcher et diffusion contrôlée des aperçus. |
| `crates/edge-lock-core/` | Domaine sérialisable, catalogue SQLite, versions, variantes, stockage de fichiers, inspection GLB et adaptation Blender. Indépendant de Tauri et testable sur dossiers temporaires. |
| `blender/edge_lock.py` | Worker lancé dans un processus Blender isolé : création, analyse des dépendances et de la géométrie, export glTF. |

Le domaine ne dépend ni de React ni de Tauri. `Library` expose des opérations synchrones réutilisables ; l’adaptateur desktop les exécute dans des workers pour ne pas bloquer le thread de l’interface. Les frontières reposent sur des types explicites plutôt que sur un framework d’injection ou des abstractions génériques inutiles au MVP.

## Parcours d’une opération

1. Le frontend envoie une commande ou une demande de tâche avec des identifiants d’assets, variantes ou versions.
2. L’adaptateur valide l’état de la bibliothèque et enregistre une tâche annulable. Les opérations longues sont sérialisées ; la file accepte jusqu’à 32 tâches actives/en attente.
3. Le cœur résout les identifiants, vérifie les chemins et exécute la lecture/copie en flux. Le catalogue est verrouillé pendant les accès SQLite, sans conserver le verrou SQL pendant les gros transferts.
4. La progression est diffusée avec phase, quantité traitée et total lorsqu’il est connu. L’UI peut aussi relire l’état de la tâche après un événement manqué.
5. La publication du résultat arrive après vérification des données. Un échec laisse les sources et les versions précédemment publiées intactes.

Les états de tâche sont `queued`, `running`, `completed`, `failed` et `cancelled`. Le changement de bibliothèque est refusé pendant une tâche ; l’identité de la bibliothèque est également vérifiée avant exécution d’une tâche déjà enregistrée.

La fermeture normale annule les tâches en cours et attend leur terminaison, notamment l’arrêt des processus Blender, avant de quitter. Le watcher utilise une génération de sélection pour empêcher une lecture ancienne de réinstaller la surveillance d’une autre variante.

## Interfaces et modèle

Les contrats IPC utilisent `camelCase`. Un asset rassemble métadonnées et variantes ; une variante associe un dossier de travail, une configuration d’export et une tête d’historique. Une version possède un parent unique, un message, une date, la configuration et un manifeste des fichiers. Créer une variante ajoute une version dont le parent est la version d’origine ; la lecture de son historique inclut donc cette ascendance.

`list_assets` propose recherche, filtre catégorie avec descendants, tag, jeu, archivage, tri et pagination. `list_snapshots` retourne des résumés paginés avec `total`, sans charger les manifestes complets. `get_snapshot` fournit le manifeste de la version choisie. Le détail d’une variante inclut les 100 premiers résumés ; la pagination permet d’atteindre l’ensemble de l’historique.

La configuration versionnée comprend `primaryFile`, `scene`, `collection` et `applyModifiers`. Le fichier principal est relatif au dossier. Sans choix explicite, un unique `.blend` est sélectionné ; lorsqu’aucun `.blend` n’existe, un unique `.glb` est sélectionné. Plusieurs `.blend` restent ambigus même s’il existe un seul GLB. L’utilisateur choisit alors le fichier qui pilote analyse et export. Les détails de variante exposent leurs propres `workingBytes` et `triangles`, distincts des agrégats de l’asset.

La taille de travail est recalculée lors des captures/actualisations, et non à chaque rendu de la galerie. La taille d’historique d’un asset additionne ses objets distincts ; elle ne doit pas être additionnée entre assets pour mesurer le disque, car des objets peuvent être partagés. `LibraryInfo.physicalBytes` mesure la bibliothèque complète, caches inclus.

## Blender, GLB et cache

Blender est invoqué avec un exécutable configuré, des arguments séparés, les réglages d’usine, l’auto-exécution de scripts désactivée et des dossiers utilisateur temporaires. Les requêtes et résultats transitent par JSON. Les sorties de processus sont bornées ; l’annulation et le délai maximal arrêtent le processus. L’application interactive Blender est ouverte séparément sur le fichier de travail demandé.

Analyse et export travaillent sur une matérialisation temporaire. Pour un snapshot, les objets sont vérifiés par hash lors de la copie. Pour le travail courant, une capture temporaire vérifiée ne crée ni version ni objets permanents. Une dépendance Blender absolue vers l’ancien emplacement reste une dépendance externe : elle doit être rendue relative ou incorporée par l’utilisateur, sans réécriture silencieuse du source.

Le module `preview` du cœur résout les GLB existants avant Blender et gère les empreintes et la publication du cache. La demande IPC `preview` accepte `previewFile?`, relatif à l’asset. Une ambiguïté renvoie `preview_ambiguous` avec `details.candidates`. Le résultat conserve `url`, `inspection`, `sourceLabel` et ajoute `sourceFile` et `sourceKind` (`assetGlb` ou `generatedGlb`). Le choix d’aperçu ne modifie pas la configuration versionnée.

La recherche de cache précède la copie et tout lancement de Blender. Pour un GLB existant, seule son empreinte est suivie. Pour une conversion, les entrées sont le `.blend`, les options, le worker, l’identité de l’exécutable Blender et les dépendances utiles au rendu. Le contenu est haché en flux avec BLAKE3 ; l’absence et les membres des motifs UDIM/séquences font partie des empreintes. Les fichiers internes historiques utilisent les hashes du manifeste. Un cache valide peut être réutilisé même si Blender est devenu indisponible. Les modifications documentaires ne provoquent pas de conversion.

Sur défaut de cache, Blender utilise une copie cohérente ; les références externes relatives sont résolues depuis l’emplacement logique de leur `.blend` propriétaire et rebasées en mémoire. Les bibliothèques imbriquées sont rechargées avant l’analyse. Les sources pertinentes sont contrôlées avant/après conversion ; une instabilité entraîne une seconde tentative. Une capture froide peut encore occuper environ deux copies supplémentaires du dossier.

L’opération Blender `preview` autorise les dépendances non autonomes, conserve les scripts désactivés et peut substituer en mémoire une texture manquante. `inspectPreview` découvre les dépendances sans compter la géométrie. Les dépendances exposent `kind`, `renderRelevant` et `sourcePath`, en plus de `path` et `status`. Le graphe des utilisateurs sélectionnés distingue les ressources utiles des références secondaires. Les alertes pertinentes sont présentées séparément dans « Dépendances ». L’export explicite conserve sa vérification stricte.

Les caches `previews` et `exports` sont séparés ; leurs GLB partagent un budget de 2 Gio et les entrées actives sont protégées. La publication GLB/JSON est atomique ; les répertoires temporaires d’aperçu marqués sont nettoyés au démarrage et avant une nouvelle préparation. La porte d’opération existante évite les conversions simultanées ; une demande suivante réutilise le résultat. Les fichiers dérivés ne participent pas à l’historique. Le frontend accède à un aperçu via un jeton opaque du protocole `edge-preview`, jamais par un chemin arbitraire.

L’inspecteur GLB vérifie structure binaire, plages de données, références de scène et ressources autonomes avant affichage. Le lecteur limite la taille du GLB et les budgets de géométrie/textures. Le lecteur libère textures, buffers et animations lorsqu’un aperçu est remplacé.

La géométrie Blender est évaluée dans la scène choisie, instances comprises, avec `len(mesh.edges)` pour les arêtes. Le GLB fournit les comptes de la scène exportée ; ses arêtes sont des paires non orientées de sommets, dédupliquées par maillage/accessor POSITION. Les indices sparse et modes points/lignes/triangles sont pris en charge. Le travail de comptage est borné à 64 Mio ; `edges` est nullable et les anciens JSON restent lisibles. Les validations structurelles ne recomptent pas les arêtes.

L’interface conserve l’analyse source séparément de l’inspection d’aperçu. Un GLB voisin ne remplace ni ses options scène/collection ni ses métriques mémorisées. La carte d’asset utilise la première variante disposant d’un compte de triangles ; ce n’est pas une somme de variantes. Après des modifications externes, relancer l’analyse pour obtenir un compte à jour. Les paramètres d’export ou une nouvelle version invalident le compte mémorisé.

## Choix de périmètre

Le stockage par fichiers complets évite la complexité d’un format de deltas binaires. SQLite reste un catalogue de métadonnées, pas un conteneur de gros blobs. Les dossiers indépendants évitent un checkout qui pourrait remplacer un fichier ouvert dans Blender. Aucune fusion, synchronisation cloud, suppression d’historique, collaboration simultanée ni découpe de blocs n’est incluse.

Les mesures initiales, les commandes de validation et les limites encore non validées sont consignées dans [validation.md](validation.md).
