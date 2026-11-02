# Edge Lock

Edge Lock est une application desktop Windows pour centraliser des dossiers d’assets 3D, enregistrer leurs versions et conserver plusieurs variantes indépendantes. Le MVP fonctionne localement, avec Rust, Tauri 2, React et SQLite.

## Installation Windows

L’installateur **0.2.0 x64** est produit dans `target/release/bundle/nsis/Edge Lock_0.2.0_x64-setup.exe`. Il installe Edge Lock pour l’utilisateur courant ; la bibliothèque reste séparée de l’application. WebView2 est requis. Blender est nécessaire pour importer, créer et synchroniser les projets ; un aperçu déjà synchronisé peut être relu sans lancer Blender.

Pour générer un nouvel installateur à partir des fichiers locaux actuels, y compris les modifications non commitées, exécuter depuis PowerShell à la racine du projet :

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build-installer.ps1
```

Le script nécessite les prérequis de développement ci-dessous et les dépendances installées par `npm ci`. Il reconstruit l’application et l’installateur NSIS x64, vérifie leur version et affiche le chemin absolu de l’installateur ainsi que son SHA-256. Il utilise `CARGO_TARGET_DIR` si cette variable est définie. La génération ne lance pas l’installation : ouvrir l’installateur produit pour installer ou mettre à jour Edge Lock.

Les contrôles réellement exécutés et leurs limites sont détaillés dans [le bilan de validation](docs/validation.md). L’installation sur le poste personnel ne fait pas partie de ces contrôles.

## Utilisation

1. Choisir un dossier vide pour créer une bibliothèque, ou ouvrir une bibliothèque Edge Lock existante. Utiliser un disque local ; une seule instance peut ouvrir une même bibliothèque.
2. Dans les réglages, sélectionner `blender.exe`.
3. **Importer un dossier** accepte exactement un `.blend` et au plus un `.glb`, sous-dossiers compris. Utiliser le sélecteur ou déposer un dossier, puis confirmer l’import. Edge Lock copie les fichiers, génère le GLB avec les modificateurs et publie la première version après réussite. Un GLB fourni est régénéré dans la copie ; le dossier d’origine reste intact. Les imports GLB seuls ne sont plus acceptés.
4. **Nouveau projet** crée `main.blend`, son GLB et des dossiers pour textures, références, rapports et captures. Une scène vide affiche un état adapté.
5. Ouvrir le dossier ou le fichier Blender depuis la fiche. Enregistrer dans Blender : le GLB et l’aperçu se mettent à jour automatiquement. **Créer une version** devient disponible après détection d’un changement source et synchronisation réussie.
6. Choisir une ancienne version pour l’inspecter, l’exporter ou **Créer une variante**. Chaque variante possède son propre dossier stable ; créer une variante ne remplace pas le travail courant.

Les catégories peuvent contenir des sous-catégories. Un asset accepte plusieurs tags et plusieurs projets de jeu. Recherche, filtres, tri, archivage et historique paginé permettent de parcourir la bibliothèque. L’archivage conserve les fichiers et les versions.

## Aperçu et export

Chaque dossier de travail de variante contient un `.blend` source et un GLB dérivé. La règle s’applique récursivement ; `.blend1` reste une sauvegarde ordinaire. Les variantes, historiques, caches et sauvegardes sont indépendants de ce décompte. Les anciens dossiers non conformes sont conservés avec leur historique ; leur synchronisation et les nouvelles versions restent bloquées jusqu’à correction manuelle.

Le `.blend` est l’autorité. La scène active enregistrée est exportée entière, avec ses modificateurs actifs évalués. Edge Lock ne sauvegarde jamais un `.blend` existant et ne lui applique aucun modificateur. Seule la création explicite d’un nouveau projet écrit un nouveau `.blend`. Les sélecteurs de fichier principal, scène, collection et application des modificateurs ont disparu.

La surveillance porte sur les workspaces de la bibliothèque active, même lorsque la fiche est fermée. Les événements sont regroupés pendant 500 ms après la dernière écriture, puis les contenus sont comparés par BLAKE3. Une sauvegarde pendant une conversion impose une nouvelle vérification. L’ouverture et le retour au premier plan réconcilient également l’état ; une défaillance de surveillance active une vérification périodique. Edge Lock doit être ouvert pour synchroniser.

Le GLB unique existant conserve son chemin ; s’il manque, il est créé à côté du `.blend` avec le même nom. La génération vérifie une copie temporaire avant remplacement atomique. Le dernier aperçu reste affiché pendant le traitement ou en cas d’erreur. L’aperçu 3D et la vignette utilisent le même artefact immuable ; les résultats anciens sont ignorés. Une consultation historique ne change pas lorsque le travail courant est sauvegardé.

Les vignettes texturées sont fixes, sans rotation ni animation, et préparées une à la fois pour les cartes visibles. La première variante représente l’asset. L’aperçu 3D fournit navigation orbitale et lecture des animations GLB.

Le rendu conserve les textures, UV, transparences et matériaux pris en charge par glTF. Un environnement de studio généré localement éclaire les matériaux PBR, notamment métalliques, sans téléchargement. Les shaders procéduraux propres à Blender ne sont pas automatiquement convertis en textures : préparer des matériaux compatibles glTF reste nécessaire pour ces cas.

La fiche et les vignettes cadrent uniquement la géométrie visible. Un sol de studio horizontal démesuré peut être masqué dans la copie chargée en mémoire, selon des critères géométriques conservateurs, pour rendre le sujet lisible. Le GLB, ses sources et les statistiques restent inchangés. Un sol seul et les cas ambigus reconnus sont conservés ; une grande plaque portant plusieurs accessoires peut toutefois ressembler géométriquement à un sol.

Les armatures restent représentées par les skins glTF et les animations compatibles sont conservées. Les shape keys sans modificateur restent exportables ; une combinaison shape keys/modificateurs qui ferait perdre les morphs bloque la génération avec un message explicite et conserve le dernier GLB. Aucun baking personnalisé des morphs n’est effectué.

Les statistiques affichent la géométrie du GLB effectivement généré : objets maillés, sommets, arêtes, faces et triangles. Elles incluent les effets des modificateurs. La triangulation et les séparations de sommets aux coutures UV peuvent différer des comptes de Blender. Un compte d’arêtes trop coûteux reste indisponible sans empêcher l’aperçu.

Les versions restent manuelles. Une modification du seul GLB dérivé, sa suppression ou sa régénération ne permettent pas de créer une version. Les autres fichiers utilisateur, notamment le `.blend`, les textures, rapports et sauvegardes, restent comparés. La capture d’une version vérifie que ses sources correspondent au GLB synchronisé ; une sauvegarde concurrente entraîne une nouvelle tentative puis un refus si le projet reste instable.

Les limites de conversion de matériaux, simulations et fonctionnalités propres à Blender restent celles de son exporteur glTF.

Les dépendances externes disponibles peuvent alimenter le GLB automatique. Les références manquantes inutilisées, notamment certaines brosses Blender, ne bloquent pas la conversion. Une ressource utilisée manquante bloque la publication : aucune texture de remplacement n’est enregistrée dans le GLB canonique. Un échec laisse le `.blend` accessible.

Le cache est vérifié avant la copie de l’asset ou le lancement de Blender. Il suit le contenu des sources utiles, y compris textures externes et bibliothèques liées ; modifier un rapport ou une capture ne régénère pas l’aperçu. Les vignettes PNG de 512 × 384 sont liées à l’artefact GLB et à la version du rendu. Leur production utilise un seul contexte WebGL temporaire, libéré après la capture. Les temporaires d’aperçu identifiés comme appartenant à l’application sont nettoyés après interruption. Les tâches sont sérialisées et le budget commun des caches, vignettes comprises, reste de 2 Gio.

La version 2 du rendu régénère automatiquement les anciennes vignettes visibles avec ce cadrage. Elle n’invalide pas le cache GLB et ne déclenche pas de réexport Blender lorsque les sources et le GLB certifié sont inchangés.

Un export explicite peut utiliser le travail enregistré sur disque ou une version immuable. Il passe par une copie temporaire et ne sauvegarde pas le `.blend` source. Les exports Blender avec des dépendances externes ou absentes restent refusés et n’utilisent jamais un aperçu permissif du cache. Préférer des chemins relatifs à l’intérieur du dossier ou des ressources incorporées dans le `.blend`. Un dossier versionné ne capture pas les fichiers référencés à l’extérieur ; leur état actuel peut donc influencer un aperçu historique.

L’export explicite est écrit hors de la bibliothèque, avec confirmation avant remplacement d’un fichier existant. Un aperçu est refusé au-delà de 256 Mio de GLB ou des budgets mémoire de géométrie/textures ; cela n’interdit pas de conserver et versionner un projet dont le GLB canonique est valide. Les GLB exigeant des ressources externes ou des extensions non prises en charge restent refusés.

## Stockage et limites du MVP

- Versions immuables avec déduplication des fichiers identiques par BLAKE3. Ce n’est pas un dépôt Git ; il n’existe ni fusion, ni rebase, ni synchronisation distante.
- Un `.blend` modifié est stocké intégralement à nouveau. Les textures inchangées sont partagées dans l’historique. Chaque variante dispose toutefois d’une copie physique de travail.
- Tous les fichiers ordinaires du dossier sont conservés, y compris références, rapports, captures et sauvegardes `.blend1`. Les dossiers vides sont conservés. Les liens symboliques, junctions et fichiers spéciaux sont refusés.
- Les snapshots sont manuels et portent sur les données déjà enregistrées sur disque. Les écritures concurrentes détectées font réessayer puis échouer la capture ; le MVP ne peut pas figer atomiquement un dossier modifié par un programme externe.
- Une bibliothèque active à la fois, Windows comme plateforme validée, aucun serveur ni compte. Bibliothèques réseau et dossiers synchronisés à chaud non pris en charge.
- Aucune suppression définitive ni récupération automatique d’espace historique. L’historique local ne remplace pas une sauvegarde sur un autre support.

Voir [le format de stockage et la sauvegarde](docs/storage.md), [l’architecture](docs/architecture.md) et [les validations réellement exécutées](docs/validation.md).

## Identité visuelle

L’interface utilise des surfaces bleu ardoise, des boutons bleu ciel avec reflets et des accents roses sur les indicateurs, les cases à cocher et certains libellés. Le logo de navigation est adapté à la largeur de la barre latérale et l’icône de manette identifie les projets de jeu. Le logo source `Edge-Lock-Icon.png` fournit le logo de navigation et les icônes Windows ; `npm run icons` régénère les ressources avant une reconstruction Tauri. La police Space Grotesk est embarquée localement, sous licence SIL Open Font License fournie dans `public/fonts/` ; aucun service de polices distant n’est sollicité. Les chemins et extraits de code conservent une police monospace.

## Développement

Prérequis : Windows, outils de compilation C++ MSVC et SDK Windows, WebView2, Rust via rustup, Node.js et npm. Les versions de référence sont Rust **1.99.1** (`rust-toolchain.toml`) et Node **26.13.0** (`.node-version`). `Cargo.lock` et `package-lock.json` fixent les dépendances résolues. Blender est installé séparément.

```powershell
npm ci
npm run tauri dev
```

Si la distribution Node portable préparée pour ce dépôt est présente, on peut la sélectionner uniquement pour le terminal courant :

```powershell
$env:Path = "$PWD\.tools\node-v26.13.0-win-x64;$env:Path"
```

Vérifications et production de l’application :

```powershell
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
npm test
npm run lint
npm run format:check
npm run build
npm run tauri build
```

`npm run dev` seul lance uniquement l’interface web ; les opérations de bibliothèque nécessitent Tauri. La construction produit par défaut l’exécutable dans `target/release/` et l’installateur NSIS dans `target/release/bundle/nsis/`. `CARGO_TARGET_DIR` permet une construction isolée. L’installateur vise l’utilisateur courant ; les données restent dans la bibliothèque choisie.

Les tests Blender, desktop WebView2, performance et installation isolée demandent des prérequis supplémentaires ; leurs commandes et limites sont détaillées dans [docs/validation.md](docs/validation.md).
