# Architecture

## Responsabilités

| Composant | Responsabilité |
| --- | --- |
| `src/` | Interface React/TypeScript, tâches, vignettes et lecteur Three.js. Aucun accès arbitraire aux fichiers. |
| `src-tauri/` | IPC, dialogues, réglages, coordination des contrôles de fichiers, surveillance de bibliothèque et ressources contrôlées. |
| `crates/edge-lock-core/` | Domaine, SQLite, versions, stockage, validation des projets, contrôle des changements, inspection GLB et adaptation Blender. Indépendant de Tauri. |
| `blender/edge_lock.py` | Worker isolé : création explicite, analyse, aperçu et export glTF. |

Le cœur expose des opérations synchrones testables avec des dossiers jetables. L’adaptateur les exécute hors du thread d’interface. Les opérations longues utilisent la porte d’opération existante, sans framework supplémentaire.

## Sources et aperçus en cache

Chaque workspace accepte plusieurs `.blend` et `.glb`. L’import exige au moins un `.blend`, récursivement et sans distinction de casse ; `.blend1` reste un fichier utilisateur. Le choix stable du principal conserve l’inférence existante par taille, date, nom et profondeur. `ExportConfig.primaryFile` reste versionné.

Aucun flux automatique desktop ne publie de GLB dans le workspace. Import, création de version et variante capturent les fichiers directement. `generatedGlbFile` reste lisible pour la compatibilité historique, mais ses fichiers sont désormais comparés comme tous les autres GLB. Le module `sync` conserve ses interfaces Rust historiques ; ses opérations de génération ne sont plus appelées automatiquement par l’application.

Le pipeline `preview::prepare_preview` prépare une copie cohérente et convertit les `.blend` uniquement dans `cache/previews`, avec scripts désactivés et références adaptées en mémoire. Les GLB existants sont servis sans conversion. Les exports explicites utilisent leur espace de cache et une destination extérieure à la bibliothèque. Seule la création explicite écrit un nouveau `.blend` ; aucun aperçu ne sauvegarde une source existante.

Les dépendances externes disponibles peuvent alimenter les aperçus, mais ne font pas partie du manifeste si elles restent hors du workspace. L’export explicite conserve sa validation stricte. Les erreurs de conversion sont locales à l’aperçu ou à l’export et ne bloquent pas la capture de version.

Les armatures sont exportées sous forme de skins glTF. Les shape keys sans modificateur sont autorisées. Pour les morphs relatifs associés à une unique Armature active et valide, une extension glTF embarquée dans le worker utilise les données originales du maillage, sans évaluation destructrice des morphs. Les autres objets continuent d’évaluer leurs modificateurs. L’extension existe seulement en mémoire dans les préférences du processus isolé ; elle ne modifie ni l’installation Blender ni le `.blend`.

Les hooks natifs encadrent le réglage `gltf_apply` par une pile pour les hiérarchies récursives et classifient aussi les objets de collections instanciées. Ils contrôlent la présence des skins et des morph targets attendus, selon les exclusions natives des shape keys. Les erreurs des hooks étant interceptées par l’exporteur, elles sont mémorisées puis vérifiées après l’export avant toute publication. Les autres piles avec shape keys sont refusées explicitement, avec les noms de l’objet et des modificateurs. Aucun baking personnalisé n’est ajouté. Les fonctionnalités propres à Blender restent soumises aux limites de glTF.

## Surveillance et publication

1. Le watcher surveille récursivement les workspaces actifs, indépendamment de la fiche.
2. Les événements regroupés pendant 500 ms déclenchent un seul contrôle de manifeste BLAKE3, sans Blender. `Library::check_workspace` partage ce calcul avec la comparaison des changements.
3. Le contrôle retourne les changements et une empreinte stable des fichiers et de la configuration, publiée dans `WorkspaceStatus.artifactKey`. Une empreinte nouvelle invalide les données de galerie ; une vérification inchangée ne change pas la date de l’asset.
4. La fiche prépare ensuite son aperçu si nécessaire. Le cache de rendu évite un nouvel export Blender lorsque seules des données sans effet visuel ont changé. Les résultats obsolètes sont ignorés.
5. Les captures publient directement les fichiers enregistrés, avec les contrôles d’écriture concurrente du stockage. Tous les GLB participent aux changements.

Les anciens caches canoniques, reçus et sauvegardes GLB sont conservés pour compatibilité, sans reprise de la génération automatique dans les workspaces. Les fichiers existants ne sont pas supprimés lors de la mise à jour.

L’ouverture et le retour au premier plan réconcilient la bibliothèque. Les erreurs/débordements du watcher déclenchent un réexamen ; une impossibilité d’installation active une vérification périodique. La fermeture annule Blender et attend les traitements admis. Les générations de bibliothèque empêchent les anciens watchers de publier dans une autre bibliothèque.

## Contrats IPC et interface

Les contrats utilisent `camelCase`. `get_workspace_status(variantId)` et `edge-lock://workspace-status` exposent le même état : `variantId`, `revision`, `state`, `sourceChanged`, `canCreateSnapshot`, `artifactKey` et `error`. Les états sont `checking`, `syncing`, `ready`, `failed` et `nonconforming`. `request_workspace_sync(variantId)` demande une vérification ou reprise.

La fiche s’abonne avant sa lecture initiale, ignore les révisions anciennes et recharge l’aperçu courant pour un nouvel artefact réussi. Elle garde le dernier affichage pendant le traitement et libère les ressources obsolètes. La clé d’aperçu ne dépend pas du rafraîchissement global du catalogue, évitant les boucles. Les événements courants ne déplacent pas une consultation historique.

Les fichiers et changements sont renouvelés après stabilisation. Le bouton de version dépend du contrôle des fichiers, sans dépendre de la réussite de l’aperçu Blender. Les sélecteurs indépendants d’aperçu et d’export proposent les sources présentes. `previewFile` et `sourceFile` acceptent un `.blend` ou `.glb` courant/historique. Les GLB choisis sont restitués exactement ; les `.blend` sont préparés isolément. Un GLB vide valide affiche un état calme.

L’aide permanente de la barre latérale réutilise la modale accessible sans bibliothèque active. Elle décrit les sources multiples, les sélections, le principal stable, la conservation historique et les suppressions confirmées.

Le tableau Fichiers filtre uniquement le manifeste déjà chargé, courant ou historique : extension normalisée du nom de fichier, dossiers/sans extension et bornes de taille inclusives en base 1024. Le filtrage précède la pagination de 100 entrées. La sélection asset/variante/version délimite l’état des filtres ; un rafraîchissement de cette même sélection les conserve. Aucune lecture disque, migration ou modification IPC n’est nécessaire.

La galerie lit les dernières PNG via `get_asset_thumbnails`, puis prépare uniquement les cartes visibles, une à la fois. La première variante active représente l’asset. `prepare_library_preview` choisit un GLB existant de façon déterministe (ancien chemin identifié, principal GLB, même nom que le principal, premier chemin trié), sinon le `.blend` principal. Cette résolution ne consulte ni ne modifie les préférences locales de la fiche.

## Versions et compatibilité

Le stockage reconnaît les objets bruts et `.zst`, identifiés par le BLAKE3 des octets originaux. `files.rs` centralise compression, décompression bornée et vérification. Le schéma 6 ajoute les sélections par variante/contexte et `variants.deleted_name`. La migration additive sauvegarde d’abord le catalogue et conserve tous les manifestes, versions et réglages existants. Les applications limitées aux anciens schémas refusent cette bibliothèque.

`history.rs`, sous-module de la bibliothèque, gère les rôles courant/backup, la conversion en consultation seule, les estimations confirmables, la purge des objets libérés et les journaux de restauration. Les mêmes verrous que les captures sérialisent ces mutations. Les références entre variantes restent valides grâce à des enregistrements de filiation conservés lors des suppressions. Les objets du GLB historique restent hors des caches évictables.

Les jobs IPC `storagePlan`, `storageApply`, `storageDiagnostic` et `restore` réutilisent les tâches longues existantes. `variantId` est facultatif pour la compression et la purge globales ; les anciens appels qui le fournissent restent acceptés. Les actions locales exigent toujours une variante. La confirmation présente le résultat du plan ; le backend le recalcule et rejette un jeton périmé. Les jetons globaux portent sur les cibles ordonnées et leur état, sans dépendre d’une variante arbitraire. Les snapshots exposent leur état de stockage et leurs protections. Les backups actifs et les versions courantes ne passent pas par la suppression ordinaire. Une restauration réconcilie la surveillance et l’aperçu après remplacement du dossier. Les captures de sécurité sans aperçu certifié restent complètes.

La page **Optimisation du stockage**, accessible sous Archives dans la navigation gauche, regroupe diagnostic, compression sans perte et purge pour toute la bibliothèque. L’interface d’historique conserve conversion explicite des anciens historiques, restauration, libération du backup et suppression des aperçus anciens. L’annulation explicite des modifications est locale à la variante et revient à sa dernière version créée ; aucun nettoyage sélectif automatique du workspace n’est ajouté. Tous les fichiers ordinaires restent capturés. Meshopt est reporté.

Le diagnostic utilise la bibliothèque déjà ouverte et le verrou de mutation sous la porte d’opération, sans emprunter les chemins d’écriture ou de récupération. Il lit le catalogue et les métadonnées de fichiers ; il ne recalcule pas les hashes et ne décompresse pas les contenus. Les références sont stabilisées contre les mutations Edge Lock pendant le scan ; les programmes externes restent libres d’écrire. La présence d’un journal n’interdit pas l’inventaire, mais elle interdit de présenter sa purge comme disponible. Le résultat est gardé seulement en mémoire, avec progression, annulation et au plus 20 exemples par catégorie d’anomalies. Une sortie de page ou un changement de bibliothèque annule le scan et empêche une ancienne réponse d’être affichée.

L’import valide et copie le dossier, puis publie workspace et première version sans conversion. Nouveau projet utilise Blender uniquement pour créer sa source initiale. Une variante matérialise directement la version complète choisie.

Le prédicat commun des changements compare tous les fichiers et le principal. Les anciennes options scène/collection ne créent pas de changement artificiel. Une préférence d’aperçu ou d’export est hors de la configuration versionnée et ne modifie ni le badge de changements ni la date de l’asset.

La création backend capture directement le workspace. La porte d’opération protège contre les opérations Edge Lock concurrentes ; les empreintes et inventaires avant/après capture détectent les écritures externes. Une capture instable est réessayée puis refusée.

Les anciens assets ne sont ni supprimés ni réorganisés. Le principal et les fichiers d’un snapshot restent figés ; son libellé peut être renommé sans modifier son identité, sa date ou son contenu. L’allègement conserve tous les GLB valides et autonomes ; sans GLB valide, le snapshot reste complet.

`archives.rs` prépare les suppressions définitives sous jeton avec un inventaire de métadonnées et l’identité des dossiers. Le journal v2 précède le retrait transactionnel du catalogue ; tous les déplacements et nettoyages de workspaces suivent le commit. Le reçu SQL authentifie les octets publiés, en conservant la lecture des journaux v1. Les erreurs d’E/S deviennent un nettoyage différé et indépendant des écritures suivantes. Seuls les dossiers identifiés et inchangés sont déplacés vers le staging puis nettoyés ; les contenus recréés, modifiés ou impossibles à identifier restent préservés. Les objets et caches partagés sont conservés tant qu’un survivant les utilise. Les anciennes sauvegardes communes sans propriétaire sont ignorées silencieusement. Les jobs `archiveDeletionPlan`/`archiveDeletionApply` réutilisent la porte d’opération ; les ressources et états de surveillance des variantes supprimées sont révoqués. `LibraryInfo.physicalBytesComplete` et `ArchiveDeletionPlan.workspaceBytesComplete` distinguent les mesures partielles sans empêcher l’ouverture.

Le job `discardChanges` reçoit `variantId` et `expectedHead`. Le cœur prépare la dernière version complète, remplace le workspace sous journal et restaure sa configuration avec un reçu SQLite dans la même transaction. La tête et le backup restent identiques, sans nouveau snapshot. Avant commit, les contrôles de contenu et de verrouillage permettent un rollback ; après commit, seule la copie précédente est nettoyée, sans toucher aux nouveaux changements du workspace vivant. Les jobs réconcilient la surveillance après réussite ou récupération et renvoient `WorkspaceDiscardReport` avec `cleanupPending` et les notices éventuelles.

La fiche garde le sélecteur d’aperçu dans sa barre inférieure, même avant chargement d’un modèle. Le plein écran utilise l’API native sur la racine persistante de `ModelViewer` ; le `ResizeObserver` redimensionne le canvas sans recréer le renderer, la caméra ou l’animation.

`delete_category` supprime une branche et archive ses assets transactionnellement. `delete_game` archive tous les assets associés et retire uniquement les liens au projet supprimé. L’interface réutilise les modales et styles de confirmation existants.

## Ressources et rendu

GLB et PNG sont servis par des jetons opaques du protocole local `edge-preview`. Le registre vérifie la bibliothèque active, borne les ressources et révoque les jetons au changement de bibliothèque. Le lecteur libère textures, buffers et animations lors d’un remplacement.

Les PNG 512 × 384 sont associés à la variante, à l’artefact et à la version du rendu. `store_thumbnail` les valide avant publication atomique. Leur production utilise un contexte WebGL temporaire libéré après capture. Le rendu commun conserve matériaux, textures, UV et transparences et emploie un environnement PBR local. La CSP autorise les ressources locales, blob et data nécessaires aux textures.

Après décodage du GLB, le chargement commun peut masquer en mémoire un sol horizontal très mince et surdimensionné, contenant l’emprise d’un sujet distinct et situé à sa base. La détection utilise les dimensions mondiales et écarte les candidats animés, déformables ou instanciés ainsi que les cas ambigus reconnus. Un sol seul reste affiché. Cette heuristique ne distingue pas parfaitement une grande plaque portant plusieurs accessoires d’un décor de studio.

La fiche et la capture PNG partagent les mêmes bounds de géométrie visible pour détecter une scène vide et cadrer le modèle ; les objets et descendants masqués en sont exclus. Le recentrage prend en compte les matrices d’instances et la pose courante des skins. Seule la visibilité de la copie décodée change : sources, GLB, hiérarchie, matériaux et statistiques sont conservés.

`RENDERER_VERSION = 2` invalide les anciens index de vignettes et publie les nouvelles images sous un nom versionné. La galerie régénère les PNG visibles depuis l’artefact certifié existant ; les versions des caches GLB et des reçus de synchronisation restent identiques. Une mise à jour du seul rendu n’impose donc aucun réexport Blender ni migration des données.

Aperçus, exports et vignettes partagent le budget de cache de 2 Gio ; les ressources actives sont protégées. Les anciens caches canoniques restent compatibles. Les préférences durables sont stockées dans SQLite et ne dépendent pas de la présence du cache.

L’inspecteur GLB vérifie structure, plages, scène et autonomie des ressources. Les plafonds de taille GLB, géométrie, textures et JSON ont été retirés du cœur, du transport et du lecteur. Les contraintes du format, allocations réelles et capacités WebGL restent applicables. Le comptage des arêtes reste borné et peut être indisponible sans bloquer l’aperçu. Les limites PNG des vignettes et le budget de cache restent distincts. Les shaders procéduraux, simulations et dépendances externes non capturées restent des limites documentées.


## Préférences, renommages et suppression de variante

`get_file_selections` et `set_file_selection` utilisent la clé `(variant_id, contexte courant ou snapshot_id)`. Les mises à jour portent sur un seul champ et valident chemins relatifs, extensions, existence et appartenance à l’historique consulté. L’interface attend le chargement des choix avant l’aperçu automatique et les écritures avant navigation. Les commandes de préférence et de renommage participent au suivi des écritures de configuration : elles terminent avant fermeture et refusent un changement de bibliothèque.

`rename_snapshot` modifie le libellé partagé par tous les historiques ; `rename_variant` conserve le chemin stable et l’unicité du nom par asset. `variantDeletionPlan` et `variantDeletionApply` réutilisent la suppression journalisée des archives, avec un journal v3 compatible avec les versions v1/v2. La dernière variante active est protégée. Les propriétaires supprimés gardent une ligne interne et leur nom d’origine dans `deleted_name`, avec un nom réservé inaccessible à la saisie utilisateur pour libérer l’unicité.

Les variantes actives restantes, leurs versions, têtes, backups et filiations parent/restauration définissent les contenus conservés. Seuls les manifestes exclusifs sont retirés ; leurs lignes de filiation subsistent. Les références globales protègent les objets dédupliqués. Les listes, accès workspace et rôles de protection ignorent les propriétaires supprimés ; l’archivage puis suppression de l’asset nettoie aussi leurs métadonnées internes. Les fichiers verrouillés ou modifiés conservent les protections de reprise existantes.
