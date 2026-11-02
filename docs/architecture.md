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

Chaque workspace conforme contient exactement un `.blend` et, après synchronisation, un GLB. La recherche est récursive et insensible à la casse ; `.blend1` reste un fichier utilisateur. Le module `sync` centralise la validation. Les variantes et archives sont indépendantes du décompte.

Le travail courant impose le `.blend` unique, la scène active entière et les modificateurs actifs évalués. Les anciens champs restent désérialisables et les configurations historiques ne sont pas réécrites.

Le worker `generate` ouvre une copie cohérente avec les scripts désactivés, rebase les références en mémoire et applique les modificateurs à l’export. Aucun chemin d’export ou de génération ne sauvegarde le `.blend`. Seule l’opération explicite `create` écrit un nouveau fichier dans un dossier neuf.

Les dépendances externes disponibles sont autorisées pour le GLB canonique ; les dépendances utilisées manquantes sont refusées. Les références secondaires inutilisées n’empêchent pas la conversion. Aucune substitution magenta n’est publiée dans le GLB canonique. L’export explicite conserve sa validation stricte des dépendances figées.

Les armatures sont exportées sous forme de skins glTF. Les shape keys sans modificateur sont autorisées ; une combinaison dont l’évaluation perdrait les morphs est refusée explicitement. Les fonctionnalités propres à Blender restent soumises aux limites de glTF.

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

Les fichiers et changements sont renouvelés après stabilisation. Le bouton de version utilise l’autorisation backend et reste indisponible pendant vérification, conversion, erreur ou non-conformité. Les contrôles de source/export et de GLB courant ont disparu. Un GLB vide valide affiche un état calme. Les erreurs gardent l’accès au dossier et à Blender et proposent une reprise.

La galerie lit les dernières PNG via `get_asset_thumbnails`, puis prépare les seules cartes visibles avec un job `thumbnail` à la fois. La première variante représente l’asset ; sa vignette et son aperçu utilisent le même GLB canonique.

## Versions et compatibilité

L’import copie et valide le dossier dans un staging, génère le GLB, puis publie workspace et première version. L’original reste intact. Nouveau projet et création de variante depuis un historique conforme suivent la même préparation.

Le prédicat commun des changements compare les fichiers et dossiers utilisateur en excluant uniquement les fichiers `.glb`. Les anciennes options d’export ne créent pas de changement artificiel. Le GLB reste inclus dans les manifestes et comptes d’octets.

La création backend synchronise puis capture le workspace. Les sources et le GLB capturés doivent correspondre au reçu : une sauvegarde externe entre conversion et capture entraîne un nouvel essai puis un refus si le projet reste instable. La porte d’opération protège contre les opérations Edge Lock concurrentes ; les empreintes protègent contre Blender.

Les anciens assets non conformes ne sont ni supprimés ni réorganisés. Les historiques restent lisibles ; synchronisation et nouvelles versions sont bloquées avec diagnostic. Les aperçus historiques utilisent les données immuables ; les nouvelles générations GLB appliquent toujours les modificateurs. Le schéma SQLite reste inchangé, les reçus étant régénérables.

## Ressources et rendu

GLB et PNG sont servis par des jetons opaques du protocole local `edge-preview`. Le registre vérifie la bibliothèque active, borne les ressources et révoque les jetons au changement de bibliothèque. Le lecteur libère textures, buffers et animations lors d’un remplacement.

Les PNG 512 × 384 sont associés à la variante, à l’artefact et à la version du rendu. `store_thumbnail` les valide avant publication atomique. Leur production utilise un contexte WebGL temporaire libéré après capture. Le rendu commun conserve matériaux, textures, UV et transparences et emploie un environnement PBR local. La CSP autorise les ressources locales, blob et data nécessaires aux textures.

Après décodage du GLB, le chargement commun peut masquer en mémoire un sol horizontal très mince et surdimensionné, contenant l’emprise d’un sujet distinct et situé à sa base. La détection utilise les dimensions mondiales et écarte les candidats animés, déformables ou instanciés ainsi que les cas ambigus reconnus. Un sol seul reste affiché. Cette heuristique ne distingue pas parfaitement une grande plaque portant plusieurs accessoires d’un décor de studio.

La fiche et la capture PNG partagent les mêmes bounds de géométrie visible pour détecter une scène vide et cadrer le modèle ; les objets et descendants masqués en sont exclus. Le recentrage prend en compte les matrices d’instances et la pose courante des skins. Seule la visibilité de la copie décodée change : sources, GLB, hiérarchie, matériaux et statistiques sont conservés.

`RENDERER_VERSION = 2` invalide les anciens index de vignettes et publie les nouvelles images sous un nom versionné. La galerie régénère les PNG visibles depuis l’artefact certifié existant ; les versions des caches GLB et des reçus de synchronisation restent identiques. Une mise à jour du seul rendu n’impose donc aucun réexport Blender ni migration des données.

Aperçus historiques, exports, GLB canoniques et vignettes partagent le budget de 2 Gio ; les ressources actives sont protégées. Les reçus ne certifient un artefact que si ses empreintes correspondent. Les anciennes préférences de GLB courant ne pilotent plus la synchronisation.

L’inspecteur GLB vérifie structure, plages, scène et autonomie des ressources. Les budgets de lecture 3D ne bloquent pas le versionnage d’un GLB canonique valide. Le comptage des arêtes reste borné et peut être indisponible. Les shaders procéduraux, simulations et dépendances externes non capturées restent des limites documentées.
