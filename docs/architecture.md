# Architecture

## Responsabilités

| Composant | Responsabilité |
| --- | --- |
| `src/` | Interface React/TypeScript, tâches, vignettes et lecteur Three.js. Aucun accès arbitraire aux fichiers. |
| `src-tauri/` | IPC, dialogues, réglages, coordination de synchronisation, surveillance de bibliothèque et ressources contrôlées. |
| `crates/edge-lock-core/` | Domaine, SQLite, versions, stockage, validation des projets, synchronisation canonique, inspection GLB et adaptation Blender. Indépendant de Tauri. |
| `blender/edge_lock.py` | Worker isolé : création explicite, analyse, génération canonique et export glTF. |

Le cœur expose des opérations synchrones testables avec des dossiers jetables. L’adaptateur les exécute hors du thread d’interface. Les opérations longues utilisent la porte d’opération existante, sans framework supplémentaire.

## Source et dérivé canonique

Chaque workspace accepte plusieurs `.blend` et `.glb`. L’import exige au moins un `.blend` ; la recherche est récursive et insensible à la casse, et `.blend1` reste un fichier utilisateur. `sync::resolve_project` centralise le choix stable du principal et le chemin du dérivé géré. L’inférence avant copie combine taille (40 points), date (30), nom principal ou de l’asset (20) et profondeur (10). Le chemin départage les égalités. Un choix valide mémorisé n’est pas reclassé à chaque sauvegarde.

Le principal pilote la synchronisation automatique de la scène active entière, avec modificateurs évalués. Les autres sources sont préparées à la demande. `ExportConfig.primaryFile` conserve le choix, `generatedGlbFile` optionnel identifie le dérivé géré ; une collision avec un GLB utilisateur donne un nouveau nom `.edge-lock` suffixé si nécessaire. Les anciens champs restent désérialisables et les configurations historiques ne sont pas réécrites.

Le worker `generate` ouvre une copie cohérente avec les scripts désactivés, rebase les références en mémoire et applique les modificateurs à l’export. Aucun chemin d’export ou de génération ne sauvegarde le `.blend`. Seule l’opération explicite `create` écrit un nouveau fichier dans un dossier neuf.

Les dépendances externes disponibles sont autorisées pour le GLB canonique ; les dépendances utilisées manquantes sont refusées. Les références secondaires inutilisées n’empêchent pas la conversion. Aucune substitution magenta n’est publiée dans le GLB canonique. L’export explicite conserve sa validation stricte des dépendances figées.

Les armatures sont exportées sous forme de skins glTF. Les shape keys sans modificateur sont autorisées. Pour les morphs relatifs associés à une unique Armature active et valide, une extension glTF embarquée dans le worker utilise les données originales du maillage, sans évaluation destructrice des morphs. Les autres objets continuent d’évaluer leurs modificateurs. L’extension existe seulement en mémoire dans les préférences du processus isolé ; elle ne modifie ni l’installation Blender ni le `.blend`.

Les hooks natifs encadrent le réglage `gltf_apply` par une pile pour les hiérarchies récursives et classifient aussi les objets de collections instanciées. Ils contrôlent la présence des skins et des morph targets attendus, selon les exclusions natives des shape keys. Les erreurs des hooks étant interceptées par l’exporteur, elles sont mémorisées puis vérifiées après l’export avant toute publication. Les autres piles avec shape keys sont refusées explicitement, avec les noms de l’objet et des modificateurs. Aucun baking personnalisé n’est ajouté. Les fonctionnalités propres à Blender restent soumises aux limites de glTF.

## Synchronisation et publication

1. Le watcher surveille récursivement les workspaces de la bibliothèque active, indépendamment de la fiche.
2. Un événement invalide immédiatement l’autorisation de versionner et repousse le traitement à 500 ms après la dernière écriture. Un seul worker parcourt les variantes en attente ; les conversions restent sérialisées.
3. Le cœur vérifie son reçu avant de lancer Blender. Celui-ci contient l’identité du traitement/exporteur, les empreintes BLAKE3 du `.blend` et des dépendances utiles, le chemin et le hash du GLB.
4. Si nécessaire, une capture temporaire est exportée, puis les empreintes sont revérifiées. Une source instable entraîne une seconde tentative. Les rapports et captures ne déclenchent pas de conversion si les entrées de rendu sont inchangées.
5. Le GLB validé est conservé dans `cache/canonical/<hash>.glb`, remplacé atomiquement dans le workspace, puis certifié dans `cache/sync/`. Il ne fait jamais partie des entrées de conversion. Les événements de publication appartenant au traitement ne créent pas de boucle.
6. Les statistiques proviennent du GLB généré, y compris courbes converties et géométrie après modificateurs. Le catalogue et les vignettes sont invalidés lorsque l’artefact change.

Un reçu absent, corrompu ou incohérent impose une vérification et éventuellement une régénération. Une panne entre GLB et reçu reste détectable au prochain passage. Avant le premier remplacement d’un GLB non certifié, son contenu est conservé dans l’historique ou dans les sauvegardes gérées. Une sortie déjà certifiée ne produit pas une sauvegarde à chaque conversion.

L’ouverture et le retour au premier plan réconcilient la bibliothèque. Les erreurs/débordements du watcher déclenchent un réexamen ; une impossibilité d’installation active une vérification périodique. La fermeture annule Blender et attend les traitements admis. Les générations de bibliothèque empêchent les anciens watchers de publier dans une autre bibliothèque.

## Contrats IPC et interface

Les contrats utilisent `camelCase`. `get_workspace_status(variantId)` et `edge-lock://workspace-status` exposent le même état : `variantId`, `revision`, `state`, `sourceChanged`, `canCreateSnapshot`, `artifactKey` et `error`. Les états sont `checking`, `syncing`, `ready`, `failed` et `nonconforming`. `request_workspace_sync(variantId)` demande une vérification ou reprise.

La fiche s’abonne avant sa lecture initiale, ignore les révisions anciennes et recharge l’aperçu courant pour un nouvel artefact réussi. Elle garde le dernier affichage pendant le traitement et libère les ressources obsolètes. La clé d’aperçu ne dépend pas du rafraîchissement global du catalogue, évitant les boucles. Les événements courants ne déplacent pas une consultation historique.

Les fichiers et changements sont renouvelés après stabilisation. Le bouton de version utilise l’autorisation backend et reste indisponible pendant vérification, conversion, erreur ou non-conformité du principal. Les sélecteurs indépendants d’aperçu et d’export proposent les sources présentes. Le champ `previewFile` accepte un `.blend` ou `.glb` courant/historique ; l’export ajoute `sourceFile` optionnel. Les GLB choisis sont restitués exactement ; les `.blend` choisis sont préparés isolément. Une erreur secondaire ne modifie pas le principal. Un GLB vide valide affiche un état calme.

L’aide permanente de la barre latérale réutilise la modale accessible sans bibliothèque active. Elle décrit les sources multiples, les sélections, le principal stable, la conservation historique et les suppressions confirmées.

Le tableau Fichiers filtre uniquement le manifeste déjà chargé, courant ou historique : extension normalisée du nom de fichier, dossiers/sans extension et bornes de taille inclusives en base 1024. Le filtrage précède la pagination de 100 entrées. La sélection asset/variante/version délimite l’état des filtres ; un rafraîchissement de cette même sélection les conserve. Aucune lecture disque, migration ou modification IPC n’est nécessaire.

La galerie lit les dernières PNG via `get_asset_thumbnails`, puis prépare les seules cartes visibles avec un job `thumbnail` à la fois. La première variante représente l’asset ; sa vignette utilise le GLB canonique. Le choix local d’aperçu dans la fiche reste indépendant.

## Versions et compatibilité

Le stockage reconnaît les objets bruts et `.zst`, identifiés par le BLAKE3 des octets originaux. `files.rs` centralise compression, décompression bornée et vérification. Le schéma 4 ajoute la propriété des sauvegardes GLB et les reçus durables de suppression ; le schéma 5 ajoute uniquement `workspace_discards(id,journal_hash)` pour l’annulation des modifications. La migration sauvegarde d’abord le catalogue, conserve les versions et interdit leur ouverture par une application limitée à un ancien schéma.

`history.rs`, sous-module de la bibliothèque, gère les rôles courant/backup, la conversion en consultation seule, les estimations confirmables, la purge des objets libérés et les journaux de restauration. Les mêmes verrous que les captures sérialisent ces mutations. Les références entre variantes restent valides grâce à des enregistrements de filiation conservés lors des suppressions. Les objets du GLB historique restent hors des caches évictables.

Les jobs IPC `storagePlan`, `storageApply`, `storageDiagnostic` et `restore` réutilisent les tâches longues existantes. `variantId` est facultatif pour la compression et la purge globales ; les anciens appels qui le fournissent restent acceptés. Les actions locales exigent toujours une variante. La confirmation présente le résultat du plan ; le backend le recalcule et rejette un jeton périmé. Les jetons globaux portent sur les cibles ordonnées et leur état, sans dépendre d’une variante arbitraire. Les snapshots exposent leur état de stockage et leurs protections. Les backups actifs et les versions courantes ne passent pas par la suppression ordinaire. Une restauration réconcilie la surveillance et le reçu canonique après remplacement du dossier. Les captures de sécurité sans aperçu certifié restent complètes.

La page **Optimisation du stockage**, accessible sous Archives dans la navigation gauche, regroupe diagnostic, compression sans perte et purge pour toute la bibliothèque. L’interface d’historique conserve conversion explicite des anciens historiques, restauration, libération du backup et suppression des aperçus anciens. L’annulation explicite des modifications est locale à la variante et revient à sa dernière version créée ; aucun nettoyage sélectif automatique du workspace n’est ajouté. Tous les fichiers ordinaires restent capturés. Meshopt est reporté.

Le diagnostic utilise la bibliothèque déjà ouverte et le verrou de mutation sous la porte d’opération, sans emprunter les chemins d’écriture ou de récupération. Il lit le catalogue et les métadonnées de fichiers ; il ne recalcule pas les hashes et ne décompresse pas les contenus. Les références sont stabilisées contre les mutations Edge Lock pendant le scan ; les programmes externes restent libres d’écrire. La présence d’un journal n’interdit pas l’inventaire, mais elle interdit de présenter sa purge comme disponible. Le résultat est gardé seulement en mémoire, avec progression, annulation et au plus 20 exemples par catégorie d’anomalies. Une sortie de page ou un changement de bibliothèque annule le scan et empêche une ancienne réponse d’être affichée.

L’import copie et valide le dossier dans un staging, génère le GLB, puis publie workspace et première version. L’original reste intact. Nouveau projet et création de variante depuis un historique conforme suivent la même préparation.

Le prédicat commun des changements exclut uniquement les chemins des dérivés gérés identifiés par configuration. Les GLB utilisateur et les changements de principal deviennent versionnables. La surveillance n’assimile plus tous les événements GLB à une publication du worker. Les anciennes options scène/collection ne créent pas de changement artificiel.

La création backend synchronise puis capture le workspace. Les sources et le GLB capturés doivent correspondre au reçu : une sauvegarde externe entre conversion et capture entraîne un nouvel essai puis un refus si le projet reste instable. La porte d’opération protège contre les opérations Edge Lock concurrentes ; les empreintes protègent contre Blender.

Les anciens assets ne sont ni supprimés ni réorganisés. Un reçu valide conserve le chemin du dérivé historique. Les anciens dossiers contenant plusieurs modèles sont désormais acceptés. Le principal d’un snapshot reste figé ; ses dates de matérialisation ne servent pas à une nouvelle inférence. L’allègement conserve tous les GLB valides et autonomes ; un échec de validation conserve le snapshot complet.

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

Aperçus historiques, exports, GLB canoniques et vignettes partagent le budget de 2 Gio ; les ressources actives sont protégées. Les reçus ne certifient un artefact que si ses empreintes correspondent. Les anciennes préférences de GLB courant ne pilotent plus la synchronisation.

L’inspecteur GLB vérifie structure, plages, scène et autonomie des ressources. Les plafonds de taille GLB, géométrie, textures et JSON ont été retirés du cœur, du transport et du lecteur. Les contraintes du format, allocations réelles et capacités WebGL restent applicables. Le comptage des arêtes reste borné et peut être indisponible sans bloquer l’aperçu. Les limites PNG des vignettes et le budget de cache restent distincts. Les shaders procéduraux, simulations et dépendances externes non capturées restent des limites documentées.
