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

Chaque manifeste associe un chemin relatif à un dossier ou à un hash et une taille de fichier. Les contenus sont lus en flux avec un tampon de 1 Mio. Les fichiers identiques partagent un objet entre versions, variantes et assets. Les dossiers de travail sont des copies ordinaires, sans hardlinks vers les objets : modifier un `.blend` de travail ne peut donc pas réécrire l’historique.

Les fichiers cachés et sauvegardes Blender font partie de la capture. Aucune exclusion implicite ne supprime les rapports, textures ou captures. Les dossiers vides sont conservés. Le format ne promet pas de reproduire les ACL, flux alternatifs NTFS, permissions Unix ou horodatages d’origine. Les chemins doivent être Unicode et rester relatifs ; liens symboliques, junctions, fichiers spéciaux et échappements de chemin sont refusés.

Depuis 0.2.0, chaque workspace conforme contient un `.blend` source et un GLB dérivé. Les imports exigent un `.blend` unique et au plus un GLB avant génération ; `.blend1` ne compte pas comme second modèle. Le GLB est inclus dans le manifeste d’une version, mais sa seule modification, suppression ou régénération ne suffit pas à autoriser une nouvelle version. Tous les autres fichiers et dossiers restent comparés.

La capture d’une version vérifie ses empreintes de sources et de GLB contre le reçu de synchronisation. Elle refuse de publier un nouveau `.blend` accompagné d’un ancien GLB. Une sauvegarde concurrente impose une nouvelle synchronisation/capture, avec une seule nouvelle tentative. Les anciens manifestes et configurations ne sont pas réécrits. Les dossiers non conformes restent présents et consultables, mais ne peuvent pas publier de nouvelle version avant correction.

L’inventaire est contrôlé avant et après capture. Une modification détectée entraîne une seconde tentative, puis une erreur si l’état reste instable. Cette détection protège les sauvegardes habituelles de Blender, mais ne constitue pas un gel atomique de plusieurs fichiers écrits par d’autres processus. Enregistrer et attendre la fin des écritures avant de versionner.

## Publication et interruptions

- Les nouveaux objets sont écrits dans des fichiers temporaires, synchronisés, puis publiés sous leur hash. Un objet déjà présent est vérifié avant réutilisation.
- La version, ses entrées et la nouvelle tête de variante sont ensuite publiées dans une transaction SQLite. Une interruption avant le commit ne remplace pas la tête existante.
- Un import ou une nouvelle variante écrit un journal `operation.json`, matérialise un nouveau dossier, puis publie ses références en base. Les journaux de publications déjà réussies sont nettoyés au redémarrage.
- Si le dossier a été publié mais pas les métadonnées, les fichiers restent présents et une notice de récupération indique le journal et le dossier concernés. L’application ne les supprime pas automatiquement.
- Une interruption peut laisser des objets sans référence. Ils sont conservés ; le MVP n’exécute pas de ramasse-miettes qui pourrait supprimer des données utiles.

SQLite utilise les clés étrangères, WAL et `synchronous=FULL`. Un verrou exclusif empêche deux instances d’Edge Lock d’écrire dans la même bibliothèque. L’ouverture vérifie le catalogue et les relations essentielles ; la matérialisation vérifie les hashes des objets. Les protections couvrent les interruptions de processus testées, pas une garantie absolue contre une panne matérielle, un stockage défectueux ou la suppression manuelle de fichiers.

Les gros transferts sont annulables. Une annulation survenue après une frontière de publication peut laisser le résultat déjà publié ou un dossier récupérable ; elle ne détruit pas les fichiers précédents. Une capture temporaire pour export ou aperçu ne remplit pas l’historique.

## Sauvegarder et restaurer

1. Enregistrer les fichiers ouverts dans Blender, terminer les tâches, puis fermer Edge Lock et les applications qui écrivent dans la bibliothèque.
2. Copier **tout le dossier de bibliothèque** vers un autre support. Garder les fichiers SQLite auxiliaires s’ils existent ; ne pas copier seulement `library.sqlite3` pendant que la bibliothèque est ouverte.
3. Sauvegarder séparément `settings.json` si les réglages globaux doivent aussi être conservés. Les fichiers externes référencés par un `.blend` ne font pas partie de la bibliothèque.
4. Pour tester une restauration, copier la sauvegarde vers **un nouveau dossier**, ouvrir cette copie et matérialiser une version représentative. Conserver la bibliothèque actuelle jusqu’à validation de la copie restaurée.

Le cache est régénérable, mais copier l’ensemble est le procédé le plus simple. Aperçus GLB, GLB canoniques, vignettes PNG et exports stricts ont des espaces séparés et un budget commun de 2 Gio. Les PNG sont associés à la clé d’artefact et à une version du rendu ; ils ne font pas partie des versions de l’asset. Le `.blend` existant n’est jamais sauvegardé ni modifié par Edge Lock. Le GLB de travail est désormais un dérivé géré, remplacé atomiquement après validation. Avant sa première prise en charge, un GLB non certifié est conservé dans l’historique ou dans `backups/glb/` ; les sorties déjà certifiées ne produisent pas une sauvegarde à chaque conversion. Ces sauvegardes ne sont pas un cache évictable.

Les reçus `cache/sync` contiennent les empreintes du `.blend`, des dépendances de rendu et du GLB, ainsi que l’identité du traitement. Un reçu absent, corrompu ou périmé impose une vérification et éventuellement une régénération. Une panne entre remplacement du GLB et publication du reçu est ainsi récupérable. Les reçus ne sont pas ajoutés au workspace ni à l’historique. Aucun changement de schéma SQLite n’est nécessaire. Ne pas fusionner manuellement deux bibliothèques, ne pas modifier les objets, et ne pas remplacer seulement leur catalogue par celui d’une autre sauvegarde.

Lorsqu’une notice signale une publication interrompue, fermer l’application et sauvegarder l’ensemble avant intervention. Le journal contient `workspace`, relatif à la racine. Si ce dossier existe, en faire une copie hors de la bibliothèque et importer cette copie comme un nouvel asset après vérification. Conserver les temporaires de publication pour diagnostic. Seuls les répertoires d’aperçu `.preview-tmp-*` dotés du marqueur interne attendu sont nettoyés automatiquement ; les données de travail et journaux ne sont pas concernés.

## Migrations et compatibilité

Le format de répertoire est `edge-lock-library-v1` et le catalogue courant utilise `PRAGMA user_version=1`. L’initialisation 0→1 est additive et transactionnelle. Lorsqu’un catalogue **existant** doit migrer, l’API de sauvegarde SQLite produit d’abord `backups/catalog-schema-<version>-<id>.sqlite3`, puis la migration démarre. La sauvegarde reste disponible si la migration échoue. Un schéma plus récent est refusé ; il n’est ni réinitialisé ni rétrogradé.

Le test de migration utilise un catalogue synthétique de version 0 avec des réglages préexistants : il vérifie leur conservation et le contenu de la sauvegarde. Ce test ne représente pas une compatibilité avec une ancienne version commerciale d’Edge Lock. Les futures migrations doivent conserver le contrat de sauvegarde préalable et ajouter leurs propres fixtures représentatives.

Une sauvegarde de catalogue avant migration ne remplace pas une sauvegarde de bibliothèque : elle ne contient pas les objets, dossiers de travail ou ressources externes. Toute récupération doit partir d’une copie isolée et vérifiée.
