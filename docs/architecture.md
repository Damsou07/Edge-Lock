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

Le cache GLB est indexé par contenu du manifeste, configuration d’export, version d’Edge Lock, script worker et version de Blender. Le budget de cache cible 2 Gio ; les entrées actives sont protégées. Les fichiers dérivés ne participent pas à l’historique. Le frontend accède à un aperçu via un jeton opaque du protocole `edge-preview`, jamais en donnant un chemin de fichier arbitraire.

Le MVP prépare encore une copie cohérente du dossier avant la recherche dans le cache. Le cache évite de relancer Blender, mais pas cette préparation disque. Une capture de travail peut temporairement occuper environ deux copies supplémentaires du dossier ; prévoir cet espace libre pour les gros assets.

L’inspecteur GLB vérifie structure binaire, plages de données, références de scène et ressources autonomes avant affichage. Le lecteur limite la taille du GLB et les budgets de géométrie/textures. Le lecteur libère textures, buffers et animations lorsqu’un aperçu est remplacé.

La géométrie Blender est évaluée dans la scène choisie, instances comprises, au moment de l’analyse. Le GLB fournit les comptes de la scène exportée. La carte d’asset utilise la première variante disposant d’un compte de triangles ; ce n’est pas une somme de variantes. Après des modifications externes, relancer l’analyse pour obtenir un compte à jour. Les paramètres d’export ou une nouvelle version invalident le compte mémorisé.

## Choix de périmètre

Le stockage par fichiers complets évite la complexité d’un format de deltas binaires. SQLite reste un catalogue de métadonnées, pas un conteneur de gros blobs. Les dossiers indépendants évitent un checkout qui pourrait remplacer un fichier ouvert dans Blender. Aucune fusion, synchronisation cloud, suppression d’historique, collaboration simultanée ni découpe de blocs n’est incluse.

Les mesures initiales, les commandes de validation et les limites encore non validées sont consignées dans [validation.md](validation.md).
