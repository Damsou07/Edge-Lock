# Validation du MVP

Ce document distingue les contrôles exécutés des procédures restant à exécuter. Toutes les données de test sont synthétiques et isolées des bibliothèques personnelles. Les résultats ci-dessous ont été observés le 1er novembre 2026 sur le poste Windows de développement.

## Contrôles reproductibles

```powershell
npm ci
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
npm test
npm run lint
npm run format:check
npm run build
npm run tauri build
```

Les bibliothèques Rust sont créées avec `tempfile`. Le bac à sable AppContainer du poste refuse certains renommages de dossiers et hardlinks ; les tests de stockage ont donc été exécutés hors de ce bac à sable, toujours sur leurs seules fixtures jetables. Ce problème est distinct du fonctionnement de l’application Windows normale. Les avertissements de copie du cache incrémental Rust ne sont pas des échecs de tests.

## Tests du cœur

La suite `cargo test --workspace` réussit : **33 tests Rust**, dont 19 tests de stockage, 1 arrêt brutal en sous-processus, 1 test de sécurité des chemins, 5 tests GLB, 1 validation Blender sans processus externe et 6 tests de l’adaptateur desktop. Les doctests réussissent également (aucun exemple exécutable). `cargo clippy --workspace --all-targets -- -D warnings` et `cargo fmt --all --check` réussissent sur l’état final, en édition Rust 2024.

Les tests couvrent : copie sans modification de l’origine, fichiers binaires et Unicode, dossiers vides, déduplication, historique, variantes indépendantes, archivage, taxonomie sans cycles, filtrage par descendants, configuration versionnée, annulation, fichiers modifiés pendant capture, fichiers verrouillés Windows, échec de publication d’objet, objet corrompu et refus d’écrasement.

La pagination est testée avec 110 modifications plus la version initiale, puis une variante héritant de cet historique. Les résumés n’embarquent pas les manifestes ; les versions au-delà des 100 premières restent accessibles.

Un test lance deux sous-processus qui quittent brutalement sans exécuter leurs destructeurs : après matérialisation d’un nouveau workspace et après publication d’un objet avant commit du snapshot. Le redémarrage retrouve les versions publiées précédemment, conserve le travail courant et signale le workspace non publié. Le helper ignoré dans la sortie Cargo est invoqué par ce test ; il ne représente pas un scénario omis.

La migration 0→1 est testée sur une base synthétique comportant des réglages préexistants. Le test vérifie la sauvegarde SQLite avant migration, le numéro de schéma et la conservation des réglages. Les schémas futurs et catalogues absents sont refusés sans recréation vide. La reprise d’une initialisation interrompue est également testée.

Les protections de chemins et l’inspecteur GLB ont leurs propres tests : données tronquées, plages hors limites, cycles, ressources externes, extensions non prises en charge, comptes des instances/animations et textures disproportionnées.

## Blender réel

Les **5 tests d’intégration Blender passent sur Blender 5.1.1 et Blender 5.2.3 LTS**. La distribution portable officielle 5.2.3 a été vérifiée par SHA avant utilisation. Les tests créent leurs propres projets et ressources ; ils n’ouvrent pas le projet Blender personnel de l’utilisateur.

```powershell
$env:EDGE_LOCK_TEST_BLENDER = 'C:\chemin\vers\blender.exe'
cargo test -p edge-lock-core --test blender_integration -- --include-ignored
```

Les scénarios comprennent création réelle d’un projet, export d’une collection avec sous-collection, textures incorporées, rig, shape keys et animations, conservation du source, détection d’une texture externe, refus d’export non autonome, neutralisation des scripts embarqués, timeout et annulation. Les tests sont ignorés par la commande Rust ordinaire parce qu’un exécutable Blender doit être choisi explicitement.

## Interface

Les **30 tests frontend passent**, répartis entre quatre fichiers. Ils couvrent notamment import, dépôt de dossier, configuration, navigation, erreurs, événements et validation des ressources de la visionneuse. Le typage TypeScript, ESLint, Prettier et le build Vite réussissent. La visionneuse est également exercée dans la véritable WebView2, comme décrit ci-dessous ; les tests unitaires seuls ne prouvent pas le rendu GPU.

## Mesures de performance

```powershell
cargo test -p edge-lock-core --test performance -- --ignored --nocapture
```

Une bibliothèque temporaire a reçu 1 000 imports d’un petit fichier identique, puis un fichier synthétique de 128 Mio a été versionné. Mesure en compilation de test, sur ce poste seulement :

| Mesure | Résultat observé |
| --- | --- |
| 1 000 imports avec première version | 20,80 s |
| Page de 50 assets, médiane sur 20 pages | 23,86 ms |
| Page de 50 assets, percentile 95 | 28,21 ms |
| Snapshot d’un fichier de 128 Mio | 676,97 ms |
| Pic mémoire du processus avant/après ce snapshot | 13,01 / 13,29 Mo |

La mémoire est mesurée avec `GetProcessMemoryInfo` ; il s’agit du pic du processus, pas d’une attribution parfaite au seul moteur de stockage. Ces résultats confirment le transfert en flux sur ce cas. Ils ne prouvent pas un débit identique pour 100 Go, un disque réseau, un million de petits fichiers ou une scène Blender complexe. Aucun test n’a rempli le disque physique ; les erreurs d’écriture sont injectées avec une obstruction de chemin et des verrous.

## Application desktop et installateur

Le script `scripts/e2e.mjs` pilote une vraie WebView2 via `tauri-driver` et un Microsoft Edge WebDriver compatible. Il utilise `.test-data/`, des réglages isolés via `EDGE_LOCK_TEST_DATA_DIR` en build debug et un dossier WebView temporaire. Les chemins des exécutables peuvent être fournis par `EDGE_LOCK_E2E_EXE`, `TAURI_DRIVER` et `EDGE_DRIVER`.

```powershell
node scripts/e2e.mjs
```

Les rapports et captures sont écrits dans le dossier temporaire annoncé par le script. L’exécution finale a **réussi les sept parcours** :

- Événement Tauri de dépôt : formulaire prérempli, aucune copie avant confirmation. Le test injecte l’événement natif ; il ne simule pas un geste de souris depuis l’Explorateur.
- Import Unicode, conservation du source et des dossiers vides.
- Aperçu WebGL local, rotation, zoom, recentrage et lecture/pause d’animation.
- Détection des modifications, version manuelle et variante indépendante depuis une ancienne version.
- Export historique vérifié, refus d’écrasement sans consentement et travail courant préservé.
- Blender réel via Tauri : sélection automatique du principal, collection mesurée à 12 triangles, aperçu, export, source inchangée et création native d’un projet.
- Redémarrage : classement, variantes et historique conservés.

Le rapport de cette session est `.test-data/desktop-70kkKK/report.json` (`status: passed`). Les dossiers `.test-data` sont locaux et exclus du dépôt. Pour inclure le scénario Blender, fournir `EDGE_LOCK_E2E_BLENDER` (exécutable) et `EDGE_LOCK_E2E_BLEND_SOURCE` (dossier d’une fixture jetable avec `main.blend`, texture, collection et animation). Le script refuse les binaires release afin de garantir l’isolation des réglages.

**La construction Tauri release et la génération NSIS ont réussi.** Les métadonnées des deux fichiers produits indiquent `Edge Lock`, version fichier et produit **0.1.0**. L’exécutable applicatif est bien PE x64 (`0x8664`). Le lanceur NSIS est un bootstrap PE x86 qui embarque cette application x64.

| Fichier | Taille | SHA-256 |
| --- | --- | --- |
| `target/release/edge-lock.exe` | 17 174 528 octets | `B06EC6DCA842CAD4978BCB5E1F8524092581DC1B6A9A8ED7A77470A9DC1A6FE2` |
| `target/release/bundle/nsis/Edge Lock_0.1.0_x64-setup.exe` | 7 278 507 octets | `403AE9A479F4F32C67DA949358F1ACF89C8CD1B53665E010C32A9F16A0EB3565` |

Le rapport local d’inspection est `.test-data/release-verification.json`. L’installateur n’est pas signé avec un certificat de signature de code.

Le script `scripts/test-installer.ps1` est prévu pour Windows Sandbox ou une VM jetable. Il vérifie installation, version de l’exécutable installé, réinstallation et intégrité des hashes d’une copie de bibliothèque. Avec `-PreviousInstallerPath`, il peut aussi vérifier le passage depuis un installateur antérieur fourni.

```powershell
# À exécuter uniquement dans une VM jetable ou Windows Sandbox.
./scripts/test-installer.ps1 -InstallerPath 'C:\fixtures\EdgeLock-setup.exe' `
  -FixtureLibraryPath 'C:\fixtures\library-copy' -DisposableEnvironment
```

**Installation et mise à niveau réelles en environnement isolé : non exécutées sur ce poste.** `WindowsSandbox.exe` n’y est pas disponible. Produire et inspecter un installateur ne valide pas son installation ; le script ne doit pas être exécuté sur l’installation personnelle pour masquer cette limite. Une mise à niveau depuis une version précédente reste aussi conditionnée à la disponibilité de son installateur.

Le workflow `.github/workflows/ci.yml` automatise les vérifications, le parcours desktop, la construction et l’installation/réinstallation sur un runner Windows jetable. Il est fourni mais n’a pas été exécuté sur un service distant pendant cette livraison.
