# Edge Lock

Edge Lock est une application desktop Windows pour centraliser des dossiers d’assets 3D, enregistrer leurs versions et conserver plusieurs variantes indépendantes. Le MVP fonctionne localement, avec Rust, Tauri 2, React et SQLite.

## Installation Windows

L’installateur **0.1.0 x64** de cette construction se trouve dans `target/automatic-preview/release/bundle/nsis/Edge Lock_0.1.0_x64-setup.exe`. Il installe Edge Lock pour l’utilisateur courant ; la bibliothèque est choisie au premier lancement et reste séparée de l’application. WebView2 est requis. Blender est facultatif pour gérer et versionner les dossiers et les GLB, mais requis pour les opérations sur les `.blend`.

Le binaire et l’installateur ont été construits et leur version vérifiée. L’installation et la mise à niveau en VM jetable restent à valider ; voir [le bilan de validation](docs/validation.md).

## Utilisation

1. Choisir un dossier vide pour créer une bibliothèque, ou ouvrir une bibliothèque Edge Lock existante. Utiliser un disque local ; une seule instance peut ouvrir une même bibliothèque.
2. Dans les réglages, sélectionner `blender.exe`. Blender est nécessaire pour créer un projet `.blend`, analyser un `.blend` et le convertir en GLB. Un GLB autonome fonctionne sans Blender.
3. **Importer un dossier** copie son contenu intégral dans la bibliothèque et crée une première version. Utiliser le sélecteur ou déposer un dossier dans la fenêtre, puis confirmer l’import. Le dossier d’origine est conservé. Un unique `.blend` devient automatiquement principal ; à défaut de `.blend`, un unique `.glb` est choisi. En présence de plusieurs `.blend`, choisir explicitement le fichier à utiliser.
4. **Nouveau projet** crée `main.blend` avec Blender et des dossiers pour textures, références, rapports et captures. Le projet est immédiatement géré par Edge Lock.
5. Ouvrir le dossier ou le fichier Blender depuis la fiche d’asset. Copier le chemin du dossier pour le transmettre à un agent IA. Enregistrer les modifications dans Blender, puis utiliser **Créer une version** avec un message.
6. Choisir une ancienne version pour l’inspecter, l’exporter ou **Créer une variante**. Chaque variante possède son propre dossier stable ; créer une variante ne remplace pas le travail courant.

Les catégories peuvent contenir des sous-catégories. Un asset accepte plusieurs tags et plusieurs projets de jeu. Recherche, filtres, tri, archivage et historique paginé permettent de parcourir la bibliothèque. L’archivage conserve les fichiers et les versions.

## Aperçu et export

La bibliothèque affiche des vignettes texturées fixes, sans rotation ni animation. Les dernières images en cache apparaissent immédiatement, puis les cartes visibles sont vérifiées et préparées une à la fois. Sans aperçu existant, Blender effectue la première conversion en arrière-plan ; les passages suivants réutilisent le GLB et la vignette. La première variante, identique à celle ouverte par défaut dans la fiche, représente l’asset. Une source ambiguë ou une erreur reste indiquée sur la carte et se résout dans la fiche.

L’aperçu 3D s’ouvre automatiquement dans la fiche et fournit navigation orbitale et lecture des animations GLB. Les statistiques et dépendances disponibles s’affichent avec le modèle. Il privilégie un GLB déjà présent dans l’asset : choix explicite d’aperçu, GLB principal, GLB portant le nom du `.blend`, puis unique GLB disponible. Si plusieurs candidats restent possibles, un sélecteur permet de choisir sans changer le fichier principal. Sans GLB, Blender produit automatiquement un aperçu en arrière-plan dans le cache local, sans sauvegarder le `.blend` ni écraser un GLB original. L’ouverture d’une fiche ou une opération utilisateur suspend la préparation des vignettes.

Le rendu conserve les textures, UV, transparences et matériaux pris en charge par glTF. Un environnement de studio généré localement éclaire les matériaux PBR, notamment métalliques, sans téléchargement. Les shaders procéduraux propres à Blender ne sont pas automatiquement convertis en textures : préparer des matériaux compatibles glTF reste nécessaire pour ces cas.

Le choix explicite d’un GLB d’aperçu du travail courant est mémorisé dans le cache pour la fiche et sa vignette, sans modifier le fichier principal ni les versions. Un changement de configuration ou la disparition du GLB invalide ce choix. Les aperçus historiques restent indépendants.

L’analyse affiche objets maillés, sommets, arêtes, faces et triangles. Les arêtes Blender correspondent aux maillages évalués ; celles d’un GLB correspondent à sa topologie exportée, généralement triangulée. Un compte trop coûteux reste indisponible (« — arêtes ») sans empêcher l’aperçu. Une analyse Blender existante conserve la priorité sur les statistiques d’un GLB voisin ; leur origine est indiquée. **Analyser la source** actualise explicitement la géométrie, les scènes, les collections et les dépendances de la source enregistrée.

Les réglages de source et d’export sont enregistrés automatiquement : immédiatement pour les listes et cases, à la sortie du champ ou avec Entrée pour les champs texte Scène et Collection. Les actions qui utilisent la source attendent la sauvegarde ; une erreur conserve la saisie et propose **Réessayer**. La sauvegarde des réglages ne sauvegarde pas le fichier Blender et ne crée aucune version. Les anciennes versions restent en lecture seule.

Pour un `.blend`, choisir le fichier principal, éventuellement une scène et une collection racine. Les sous-collections sont incluses. L’application des modificateurs à l’export est désactivée par défaut pour préserver les shape keys et animations compatibles avec glTF. Les limites de conversion de matériaux, simulations et fonctionnalités propres à Blender restent celles de son exporteur glTF.

Les dépendances externes ou manquantes ne bloquent pas l’aperçu. La section **Dépendances** signale les ressources utiles au rendu ; les polices et brosses inutilisées ne déclenchent pas d’alerte. Une texture manquante peut être remplacée uniquement en mémoire pour afficher un modèle dégradé. Un échec de conversion laisse le fichier Blender accessible.

Le cache est vérifié avant la copie de l’asset ou le lancement de Blender. Il suit le contenu des sources utiles, y compris textures externes et bibliothèques liées ; modifier un rapport ou une capture ne régénère pas l’aperçu. Les vignettes PNG de 512 × 384 sont liées à l’artefact GLB et à la version du rendu. Leur production utilise un seul contexte WebGL temporaire, libéré après la capture. Les temporaires d’aperçu identifiés comme appartenant à l’application sont nettoyés après interruption. Les tâches sont sérialisées et le budget commun des caches, vignettes comprises, reste de 2 Gio.

Un export explicite peut utiliser le travail enregistré sur disque ou une version immuable. Il passe par une copie temporaire et ne sauvegarde pas le `.blend` source. Les exports Blender avec des dépendances externes ou absentes restent refusés et n’utilisent jamais un aperçu permissif du cache. Préférer des chemins relatifs à l’intérieur du dossier ou des ressources incorporées dans le `.blend`. Un dossier versionné ne capture pas les fichiers référencés à l’extérieur ; leur état actuel peut donc influencer un aperçu historique.

Le fichier GLB final est écrit hors de la bibliothèque, avec confirmation avant remplacement d’un fichier existant. Un aperçu est refusé au-delà de 256 Mio de GLB ou des budgets mémoire de géométrie/textures ; cela n’interdit pas de conserver et versionner l’asset. Les GLB qui exigent des ressources externes ou des extensions non prises en charge sont refusés par l’inspecteur.

## Stockage et limites du MVP

- Versions immuables avec déduplication des fichiers identiques par BLAKE3. Ce n’est pas un dépôt Git ; il n’existe ni fusion, ni rebase, ni synchronisation distante.
- Un `.blend` modifié est stocké intégralement à nouveau. Les textures inchangées sont partagées dans l’historique. Chaque variante dispose toutefois d’une copie physique de travail.
- Tous les fichiers ordinaires du dossier sont conservés, y compris références, rapports, captures et sauvegardes `.blend1`. Les dossiers vides sont conservés. Les liens symboliques, junctions et fichiers spéciaux sont refusés.
- Les snapshots sont manuels et portent sur les données déjà enregistrées sur disque. Les écritures concurrentes détectées font réessayer puis échouer la capture ; le MVP ne peut pas figer atomiquement un dossier modifié par un programme externe.
- Une bibliothèque active à la fois, Windows comme plateforme validée, aucun serveur ni compte. Bibliothèques réseau et dossiers synchronisés à chaud non pris en charge.
- Aucune suppression définitive ni récupération automatique d’espace historique. L’historique local ne remplace pas une sauvegarde sur un autre support.

Voir [le format de stockage et la sauvegarde](docs/storage.md), [l’architecture](docs/architecture.md) et [les validations réellement exécutées](docs/validation.md).

## Identité visuelle

L’interface utilise des surfaces bleu ardoise, des boutons bleu ciel avec reflets et des indicateurs actifs roses. Le logo source `Edge-Lock-Icon.png` fournit le logo de navigation et les icônes Windows ; `npm run icons` régénère les ressources avant une reconstruction Tauri. La police Space Grotesk est embarquée localement, sous licence SIL Open Font License fournie dans `public/fonts/` ; aucun service de polices distant n’est sollicité. Les chemins et extraits de code conservent une police monospace.

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

`npm run dev` seul lance uniquement l’interface web ; les opérations de bibliothèque nécessitent l’application Tauri. La construction Tauri produit par défaut l’exécutable dans `target/release/` et l’installateur NSIS dans `target/release/bundle/nsis/`. `CARGO_TARGET_DIR` permet une construction isolée ; celle validée pour les aperçus automatiques utilise `target/automatic-preview/`. L’installateur vise l’utilisateur courant. Les données sont stockées dans la bibliothèque choisie, séparément du dossier d’installation.

Les tests Blender, desktop WebView2, performance et installation isolée demandent des prérequis supplémentaires ; leurs commandes et limites sont détaillées dans [docs/validation.md](docs/validation.md).
