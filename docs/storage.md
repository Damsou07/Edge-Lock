# Stockage, intégrité et récupération

## Organisation de la bibliothèque

```text
Bibliothèque/
  .edge-lock-library
  library.lock
  library.sqlite3
  library.sqlite3-wal          # présent lorsque SQLite l’utilise
  library.sqlite3-shm          # présent lorsque SQLite l’utilise
  objects/<préfixe>/<hash>     # fichiers immuables BLAKE3
  objects/<préfixe>/<hash>.zst # contenus compressés sans perte avec Zstd
  workspaces/<asset-id>/<variant-id>/
  staging/                    # captures temporaires et journaux d’opération
  cache/previews/             # GLB d’aperçu, index JSON et temporaires marqués
  cache/thumbnails/           # PNG fixes et index par variante, régénérables
  cache/exports/              # résultats dérivés des exports stricts
  cache/canonical/            # GLB immuables utilisés par aperçu et vignette
  cache/sync/                 # reçus régénérables source → GLB
  backups/                    # sauvegardes de catalogue avant migration
  backups/glb/                # GLB non historisés préservés avant adoption
```

Les dossiers `cache` et `backups` sont créés lorsqu’ils sont nécessaires. Les noms visibles des assets et variantes résident dans SQLite ; leur renommage ne modifie pas les chemins de travail. Les réglages globaux, dont le chemin de bibliothèque et celui de Blender, sont dans le dossier de configuration Tauri de l’utilisateur, sous `settings.json`.

Une bibliothèque est reconnue par son marqueur de format. Edge Lock refuse d’initialiser un dossier contenant des fichiers sans marqueur reconnu. Pendant l’initialisation, `.edge-lock-initializing` permet de reprendre un état partiellement créé ; le marqueur final n’est publié qu’après création et vérification du catalogue. Des fichiers inconnus dans cet état interrompu provoquent un refus, sans suppression.

## Contenu versionné

Chaque manifeste associe un chemin relatif à un dossier ou à un hash et une taille de fichier originale. Les contenus sont lus en flux avec un tampon de 1 Mio. Les fichiers identiques partagent un objet entre versions, variantes et assets. Les dossiers de travail sont des copies ordinaires, sans hardlinks vers les objets : modifier un `.blend` de travail ne peut donc pas réécrire l’historique.

Les nouveaux objets sont compressés avec Zstd niveau 3, checksum activé et fenêtre bornée à 8 Mio. Le suffixe `.zst` désigne explicitement le codec ; les objets sans suffixe restent bruts, même si leurs octets ressemblent à une archive. La compression n’est publiée que si elle est plus petite, et après décompression complète avec vérification de la taille et du BLAKE3 original. Les lectures bornent la sortie à la taille du manifeste. L’optimisation explicite peut recomprimer les anciens objets ; une interruption entre publication compressée et retrait du brut peut laisser les deux représentations, sans perte et avec reprise possible.

## Diagnostic et optimisations globales

Le menu **Optimisation du stockage**, sous **Archives** dans le panneau de gauche, concerne toute la bibliothèque active. Le diagnostic se lance manuellement avec **Lancer le diagnostic**, puis **Actualiser le diagnostic**. Les commandes **Compresser avec Zstd** et **Libérer les objets inutilisés** ont également cette portée globale et restent utilisables sans asset ou variante, y compris dans une bibliothèque vide. Elles présentent leurs effets avant confirmation ; leur estimation porte sur les objets internes, distincts du total de la bibliothèque. Les commandes propres à une variante restent dans son historique.

Le diagnostic lit les références du catalogue et les métadonnées de fichiers, sans enregistrer de rapport, créer de temporaire, compresser, décompresser, régénérer un aperçu ou lancer une récupération. La date de début et de fin, les tailles des fichiers et leurs nombres sont présentés. Il ne mesure pas l’allocation réelle du système de fichiers et ne vérifie pas l’intégrité des contenus.

Les catégories principales **workspaces, objets, cache, backups, staging, catalogue SQLite et auxiliaires, autres fichiers** sont additives et forment le total mesuré. Les détails décrivent les objets référencés, les candidats connus sans référence, les autres objets sans référence, les représentations brut/Zstd coexistantes, les objets référencés absents et les fichiers inconnus. Le cache distingue les GLB/PNG couverts par le budget actuel des métadonnées et autres fichiers ; les backups distinguent catalogues et GLB préservés ; staging distingue les dossiers de récupération et les autres temporaires. Ces détails peuvent se recouper et ne doivent pas être additionnés entre eux.

**Libérable par la purge actuelle** désigne uniquement les candidats connus sans référence, sous réserve de ses protections. **Sans référence, à examiner** ne signifie pas supprimable : un journal ou une opération interrompue peut encore nécessiter ces données. **Récupération nécessaire** signale les journaux et dossiers concernés. Le diagnostic est disponible en présence de journaux, mais indique alors que la purge est bloquée. Il n’élargit pas le ramasse-miettes existant.

Le parcours n’emprunte aucun lien symbolique ou junction et signale les chemins ignorés. Les erreurs d’accès ou fichiers devenus inaccessibles produisent un rapport partiel, jamais un zéro supposé. Les anomalies exposent leur total et au plus 20 exemples par catégorie. L’analyse est annulable ; les applications externes peuvent modifier les workspaces pendant le scan. Après une optimisation, le rapport précédent est marqué à actualiser. Cette évolution ne change pas le schéma SQLite et ne modifie aucune politique de conservation des workspaces.

## Rétention et opérations historiques

Le catalogue distingue `full`, `preview` et `deleted`. Les variantes pointent vers leur version courante et leur backup actif. Les nouvelles captures tournent lors d’une création manuelle : l’ancien courant devient backup et les anciens états non protégés deviennent des versions de consultation. La conversion vérifie tous les GLB enregistrés et conserve leurs objets et métadonnées. Si un GLB n’est pas autonome ou valide, la version reste complète. `preview_path` désigne le principal parmi les GLB conservés. Consultation et export historiques lisent le GLB immuable choisi sans relancer Blender.

Les snapshots migrés sont marqués `legacy` et restent complets jusqu’à une conversion explicitement confirmée. Une capture sans GLB certifié ou valide reste complète et apparaît comme conversion en attente. La taille seule ne bloque plus la conversion. Ces exceptions peuvent dépasser les deux états complets habituels. Une version de consultation propose tous ses GLB conservés mais ne permet plus de matérialiser les sources ni de créer une variante Blender.

Les commandes présentent d’abord une estimation, puis revalident un jeton de plan avant application. `recompress` traite les objets référencés de toute la bibliothèque sans changer leurs contenus ; `compact` traite les versions éligibles de l’historique sélectionné ; `releaseBackup` retire le rôle de backup en conservant son aperçu ; `deleteVersion` retire une version de consultation non protégée ; `purge` reprend la libération des objets historiques inutilisés. Une autre variante peut maintenir une protection ou une référence et réduire le gain à zéro.

La table `garbage_candidates` recense uniquement les objets libérés par une conversion ou suppression connue. La purge vérifie l’intégrité relationnelle et la portée globale des références sous le verrou de mutation. Elle est bloquée par des journaux non résolus. Les fichiers inconnus et les objets orphelins issus de captures interrompues ne sont pas effacés par heuristique. Un snapshot supprimé conserve son identité et sa filiation pour que les historiques de variantes restent parcourables, mais il n’est plus listé. Les tailles logiques, physiques et récupérables sont distinctes.

Le nettoyage du workspace reste manuel : aucune classification de fichiers inutiles, exclusion implicite, corbeille de workspace ou suppression de scripts, références et sauvegardes Blender n’est ajoutée. Meshopt et les conversions de textures sont reportés.

## Restauration du backup actif

L’utilisateur doit enregistrer et fermer les applications qui écrivent dans le projet. Edge Lock capture le dossier réel, indépendamment de la réussite d’une génération GLB, puis reconstruit le backup dans un dossier de préparation. Les manifestes sont comparés avant remplacement et après déplacement du dossier précédent. Sous Windows, une ouverture exclusive préalable détecte les fichiers verrouillés ; ces contrôles ne constituent pas un gel universel des programmes externes.

Le journal `staging/restore-<id>/restore.json` est synchronisé avant les renommages. Il contient les identités et manifestes avant/après. L’ancien dossier reste dans `previous` jusqu’à publication transactionnelle des nouveaux snapshots et vérification du backup stocké. Le chemin de workspace reste identique. Après publication du journal, l’annulation est différée jusqu’à un état cohérent.

Au redémarrage, un journal non commité remet l’ancien dossier en place. Un journal commité termine sa récupération après vérification des objets du backup. Des modifications externes après interruption, une corruption ou un état ambigu préservent les dossiers et produisent une notice ; les mutations et purges sont bloquées jusqu’à résolution. Ne pas supprimer manuellement ces dossiers sans en avoir sauvegardé et vérifié le contenu.

Une capture précédant la restauration dont les fichiers diffèrent de la version courante n’est pas considérée comme disposant d’un aperçu certifié. Elle reste restaurable intégralement, sans consultation trompeuse d’un GLB potentiellement périmé. La restauration crée une nouvelle entrée et le dossier remplacé devient le nouveau backup. Les changements non enregistrés et les dépendances hors du workspace ne font pas partie de cette garantie.

Les fichiers cachés et sauvegardes Blender font partie de la capture. Aucune exclusion implicite ne supprime les rapports, textures ou captures. Les dossiers vides sont conservés. Le format ne promet pas de reproduire les ACL, flux alternatifs NTFS, permissions Unix ou horodatages d’origine. Les chemins doivent être Unicode et rester relatifs ; liens symboliques, junctions, fichiers spéciaux et échappements de chemin sont refusés.

Depuis 0.3.0, chaque workspace accepte plusieurs `.blend` et `.glb`, avec au moins un `.blend` à l’import. `.blend1` reste une sauvegarde ordinaire. `primaryFile` mémorise le principal et `generatedGlbFile` identifie son dérivé ; seuls les changements du dérivé géré sont exclus de la détection de changements. Les GLB utilisateur et le choix du principal sont versionnables. Les configurations historiques restent figées.

La capture d’une version vérifie ses empreintes de sources et de GLB contre le reçu de synchronisation. Elle refuse de publier un nouveau `.blend` accompagné d’un ancien GLB. Une sauvegarde concurrente impose une nouvelle synchronisation/capture, avec une seule nouvelle tentative. Les anciens manifestes et configurations ne sont pas réécrits. Les dossiers non conformes restent présents et consultables, mais ne peuvent pas publier de nouvelle version avant correction.

L’inventaire est contrôlé avant et après capture. Une modification détectée entraîne une seconde tentative, puis une erreur si l’état reste instable. Cette détection protège les sauvegardes habituelles de Blender, mais ne constitue pas un gel atomique de plusieurs fichiers écrits par d’autres processus. Enregistrer et attendre la fin des écritures avant de versionner.

## Publication et interruptions

- Les nouveaux objets sont écrits dans des fichiers temporaires, synchronisés, puis publiés sous leur hash. Un objet déjà présent est vérifié avant réutilisation.
- La version, ses entrées et la nouvelle tête de variante sont ensuite publiées dans une transaction SQLite. Une interruption avant le commit ne remplace pas la tête existante.
- Un import ou une nouvelle variante écrit un journal `operation.json`, matérialise un nouveau dossier, puis publie ses références en base. Les journaux de publications déjà réussies sont nettoyés au redémarrage.
- Si le dossier a été publié mais pas les métadonnées, les fichiers restent présents et une notice de récupération indique le journal et le dossier concernés. L’application ne les supprime pas automatiquement.
- Une interruption peut laisser des objets sans référence. Ils sont conservés lorsqu’ils ne figurent pas parmi les candidats explicitement libérés par une conversion ou suppression historique.

SQLite utilise les clés étrangères, WAL et `synchronous=FULL`. Un verrou exclusif empêche deux instances d’Edge Lock d’écrire dans la même bibliothèque. L’ouverture vérifie le catalogue et les relations essentielles ; la matérialisation vérifie les hashes des objets. Les protections couvrent les interruptions de processus testées, pas une garantie absolue contre une panne matérielle, un stockage défectueux ou la suppression manuelle de fichiers.

Les gros transferts sont annulables. Une annulation survenue après une frontière de publication peut laisser le résultat déjà publié ou un dossier récupérable ; elle ne détruit pas les fichiers précédents. Une capture temporaire pour export ou aperçu ne remplit pas l’historique.

## Sauvegarder et restaurer

1. Enregistrer les fichiers ouverts dans Blender, terminer les tâches, puis fermer Edge Lock et les applications qui écrivent dans la bibliothèque.
2. Copier **tout le dossier de bibliothèque** vers un autre support. Garder les fichiers SQLite auxiliaires s’ils existent ; ne pas copier seulement `library.sqlite3` pendant que la bibliothèque est ouverte.
3. Sauvegarder séparément `settings.json` si les réglages globaux doivent aussi être conservés. Les fichiers externes référencés par un `.blend` ne font pas partie de la bibliothèque.
4. Pour tester une restauration, copier la sauvegarde vers **un nouveau dossier**, ouvrir cette copie et matérialiser une version représentative. Conserver la bibliothèque actuelle jusqu’à validation de la copie restaurée.

Le cache est régénérable, mais copier l’ensemble est le procédé le plus simple. Aperçus GLB, GLB canoniques, vignettes PNG et exports stricts ont des espaces séparés et un budget commun de 2 Gio. Les PNG sont associés à la clé d’artefact et à une version du rendu ; ils ne font pas partie des versions de l’asset. Le worker Blender ne réenregistre pas le `.blend` existant ; une restauration explicite peut remplacer ses octets par ceux du backup. Le GLB de travail est un dérivé géré, remplacé atomiquement après validation. Avant sa première prise en charge, un GLB non certifié est conservé dans l’historique ou dans `backups/glb/` ; les sorties déjà certifiées ne produisent pas une sauvegarde à chaque conversion. Ces sauvegardes ne sont pas un cache évictable.

Les reçus `cache/sync` contiennent les empreintes du `.blend`, des dépendances de rendu et du GLB, ainsi que l’identité du traitement. Un reçu absent, corrompu ou périmé impose une vérification et éventuellement une régénération. Une panne entre remplacement du GLB et publication du reçu est ainsi récupérable. Les reçus restent régénérables, hors du workspace et du catalogue. Ne pas fusionner manuellement deux bibliothèques, ne pas modifier les objets, et ne pas remplacer seulement leur catalogue par celui d’une autre sauvegarde.

Lorsqu’une notice signale une publication interrompue, fermer l’application et sauvegarder l’ensemble avant intervention. Le journal contient `workspace`, relatif à la racine. Si ce dossier existe, en faire une copie hors de la bibliothèque et importer cette copie comme un nouvel asset après vérification. Conserver les temporaires signalés pour diagnostic. Les temporaires d’aperçu identifiés sont nettoyés automatiquement ; les dossiers de restauration ne sont retirés qu’après récupération et vérification des contenus décrites ci-dessus.

## Suppression définitive des archives

Les jobs d’analyse/application figent les IDs archivés et l’état de leurs fichiers dans un jeton de confirmation. Le vidage porte sur toute la bibliothèque, sans filtre ni pagination. Les assets actifs sont refusés. Les dossiers sont déplacés dans `staging/delete-<id>` sous journal avant suppression transactionnelle des variantes, snapshots et métadonnées. Le reçu SQL authentifie le contenu du journal commité.

Avant commit, une interruption remet les dossiers en place ; après commit, le nettoyage reprend à la réouverture. Un verrou ou une erreur d’E/S après commit laisse un journal et un résultat « nettoyage à terminer ». Les suppressions et purges suivantes restent bloquées tant que cet état persiste. Les liens et junctions sont refusés, les chemins sont vérifiés et les nettoyages répétés sont idempotents.

Seuls les objets sans référence survivante sont libérés. Les sauvegardes GLB avec propriétaire sont supprimées au départ du dernier propriétaire ; les anciennes copies sans attribution fiable sont conservées et signalées. Les sources originales importées, exports externes, dépendances externes et sauvegardes globales du catalogue restent préservés. Cette suppression n’est pas une restauration vers la corbeille Windows.

Supprimer une catégorie retire sa branche et archive ses assets en retirant leur catégorie. Supprimer un projet de jeu archive tous ses assets, conserve leurs autres projets et retire uniquement les liens au projet supprimé. Ces opérations gardent les fichiers et versions.

## Migrations et compatibilité

Le marqueur reste `edge-lock-library-v1` et le catalogue courant utilise `PRAGMA user_version=4`. La migration 1→2 réserve la lecture Zstd aux versions compatibles ; 2→3 ajoute les rôles, types de snapshots et candidats de purge ; 3→4 ajoute `glb_backup_owners` et `archive_deletions`. Les anciennes applications refusent ce schéma. Lorsqu’un catalogue **existant** migre, une sauvegarde SQLite `backups/catalog-schema-<version>-<id>.sqlite3` précède la transaction. Aucune version n’est réduite ou réécrite. Un schéma plus récent est refusé sans réinitialisation ni rétrogradation.

Les tests de migration utilisent des catalogues synthétiques de versions 0 et 1 avec réglages, tags, historique et objets bruts préexistants. Ils vérifient leur conservation, la sauvegarde du catalogue et la restitution exacte après recompression explicite. Les futures migrations doivent conserver le contrat de sauvegarde préalable et ajouter leurs propres fixtures représentatives.

Une sauvegarde de catalogue avant migration ne remplace pas une sauvegarde de bibliothèque : elle ne contient pas les objets, dossiers de travail ou ressources externes. Toute récupération doit partir d’une copie isolée et vérifiée.
