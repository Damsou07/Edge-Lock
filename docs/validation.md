# Validation du MVP

Ce document distingue les contrôles exécutés des procédures restant à exécuter. Le dernier bilan du 2 novembre 2026 concerne le script de génération d’installateur de la version 0.2.0. Les bilans précédents sont conservés plus bas comme historique ; leurs mesures, binaires et hashes ne décrivent pas cette génération.

## Génération de l’installateur — 0.2.0 — 2 novembre 2026

Le script `scripts/build-installer.ps1` a construit l’interface, l’exécutable release et l’installateur NSIS x64. L’exécutable et l’installateur indiquent tous deux Edge Lock 0.2.0. L’installateur produit pèse 6 406 082 octets et son SHA-256 est `D411DBE7FB4091B6EEE085BF3C5E1196EA8B1405543BD8DA89E9538D788B995E`. Sa construction a nécessité l’accès aux outils NSIS hors du bac à sable réseau.

La syntaxe PowerShell et le message d’échec en l’absence des dépendances npm ont été vérifiés. Une première tentative a échoué lors de l’accès réseau à NSIS et le script a bien signalé l’échec ; la relance a réussi. L’installation et la mise à niveau dans une VM ou Windows Sandbox n’ont pas été exécutées pour cette génération.

## Ajustements visuels — 0.2.0 — 2 novembre 2026

- Les 102 tests frontend passent, dont deux nouveaux scénarios de sélection des projets et d’archivage au clavier et par leurs libellés. Le lint, la vérification Prettier et le build frontend réussissent avec Node 26.13.0.
- Le build Tauri debug et le parcours WebView2 sur bibliothèque et réglages jetables réussissent (`.test-data/desktop-ue3Dl6/report.json`). Le benchmark de galerie à 50 imports a été explicitement ignoré ; aucune fixture personnelle n’a été utilisée.
- Les captures de la galerie, de la fiche et de la modale ont été examinées à 1440, 1024 et 860 pixels de largeur : logo et texte sans débordement, manette visible et cases cochée et décochée distinctes. La largeur de 860 pixels est émulée, car la fenêtre native impose un minimum de 1024 pixels.
- La navigation au clavier et le changement des cases sont couverts par les tests frontend. Le mode couleurs forcées et l’état désactivé n’ont pas fait l’objet d’une inspection visuelle dédiée.

## Cadrage des aperçus 3D — 0.2.0 — 2 novembre 2026

Le GLB Kitsune lu sans modification contient 127 maillages : son sol de studio mesure 200 × 0,06 × 200, contre 8,59 × 2,82 × 1,67 pour le sujet. L'ancien cadrage global réduit le sujet à environ 8 pixels de large dans une vignette de 512 pixels. Le correctif masque uniquement ce type de support démesuré dans la scène décodée en mémoire et partage les bounds visibles entre fiche et vignette. Les critères et leurs limites sont décrits dans l'architecture ; le GLB, les sources et les statistiques restent conservés.

### TDD et contrôles automatisés

- Cinq régressions frontend ont été constatées en échec avant correction : sol restant visible, transformations parentes, objets invisibles inclus, matrices d'instances périmées et pose de skin périmée au recentrage. Le nouveau test Rust refusant l'ancien cache de rendu a également échoué avant le passage à la version 2.
- **100 tests frontend passent dans onze fichiers**, dont les scénarios de conservation des plans, plaques, sols normaux, scènes ambiguës, animations, morphs et états vides/masqués des deux vues. Trois ratios de fenêtre, dont le portrait et la fiche large, vérifient que le modèle entier reste dans le champ avec la marge de cadrage resserrée.
- **74 tests Rust ordinaires passent**, dont 22 de l'adaptateur desktop et les 7 tests de ressources/cache. Les 19 cas opt-in ou helpers ignorés par cette commande ne sont pas comptés comme exécutés.
- **4 tests Node des fixtures passent avec Blender 5.1.1**, y compris la nouvelle scène de deux volumes texturés et son sol bleu démesuré.
- Typage TypeScript, ESLint, Prettier, build Vite, Clippy du workspace et de toutes ses cibles avec `-D warnings`, et `cargo fmt --all --check` passent.

Les temporaires Rust utilisent `.test-data/preview-fix-temp` : le temporaire système refuse certaines publications atomiques dans l'environnement restreint. Les avertissements de liens physiques du cache incrémental entraînent une copie et n'empêchent pas les vérifications.

### Application Windows

Le build Tauri debug sans bundle réussit. Le binaire de contrôle isolé `.test-data/preview-fix-bin/edge-lock.exe` porte les versions fichier et produit **0.2.0**, mesure **23 656 448 octets** et a pour SHA-256 `5D56F9329202BFDB401AA8C56A1A773683B66A073B680854D0535C7E53C38D2A`.

Le scénario WebView2 synthétique vérifie dans la vignette et la fiche des textures couvrant au moins 25 % de la largeur et au plus 5 pixels bleus du sol. Les captures `12-studio-floor-library.png` et `13-studio-floor-viewer.png` ont été examinées ; le sujet est lisible et le sol absent.

Le premier démarrage du pilote dans le sandbox a échoué avant ouverture de l'application (`connect EACCES` vers localhost). Le contrôle desktop utilise ensuite l'accès local nécessaire, avec un exécutable debug, une bibliothèque, des réglages et un profil WebView jetables.

Le passage final `.test-data/desktop-Wa9Rnm/report.json` est **`passed`**, avec 14 contrôles consignés, dont la navigation, les textures, l'historique et la réutilisation des PNG après redémarrage. Les captures attendent la fin du chargement et de la transition d'opacité de la galerie. La largeur du sujet réel est mesurée sur ses pixels clairs et colorés dans la zone centrale, sans badge ni texte ; le seuil de 25 % reste identique.

| Scène | Largeur en vignette | Largeur dans la fiche | Pixels bleus du sol |
| --- | ---: | ---: | ---: |
| Deux volumes texturés et grand sol | 42,8 % | 30,6 % | 0 dans les deux vues |
| Copie de Sniper Kitsune | 36,6 % | 25,9 % | Sans objet : sol original neutre |

Les captures `14-kitsune-library.png` et `15-kitsune-viewer.png` du passage final ont été examinées. Le sniper est visible avec ses textures et le sol est absent. Les hashes et dates de tous les fichiers source personnels ainsi que le contenu du workspace importé sont conservés. Les statistiques du GLB restent de 127 maillages et 79 774 triangles.

Le benchmark de 50 imports a réussi dans les passages `.test-data/desktop-Hd6TJV` et `.test-data/desktop-IW7I7N`, avant leur arrêt sur les assertions de capture/cadrage Kitsune corrigées ensuite. Le passage final utilise `EDGE_LOCK_E2E_SKIP_GALLERY_BENCHMARK=1` pour omettre uniquement ce benchmark déjà exécuté ; cette omission est explicite dans son rapport. Les dix allers-retours restent contrôlés : un contexte WebGL au maximum, zéro contexte actif dans la galerie et aucune URL blob conservée. Sans cette variable, `npm run test:desktop` inclut le benchmark de 50 imports.

## Synchronisation canonique — 0.2.0 — 2 novembre 2026

### TDD et contrôles automatisés

Les régressions ont été reproduites avant correction : Mirror ignoré à l’export lorsque l’ancienne option était fausse, GLB seul autorisant une version, et cinq comportements manquants de synchronisation dans la fiche. Les tests ont ensuite été étendus aux sauvegardes concurrentes, reçus manquants/corrompus, réponses IPC anciennes et conservation des données.

- **73 tests Rust ordinaires passent**, dont 21 pour l’adaptateur desktop, 12 unités du cœur, 20 de stockage et 12 de validation GLB. Le test d’arrêt brutal et les tests d’aperçus historiques passent également.
- **12 scénarios du worker passent avec Blender 5.1.1**, sur des fixtures synthétiques : modificateurs forcés, scène active complète, textures, rigs/animation, shape keys compatibles ou refus explicite, sources inchangées, scripts désactivés, annulation et délai maximal.
- **Le parcours Blender de synchronisation canonique passe** : import sans GLB, Mirror 24→48 triangles après sauvegarde, GLB historique immuable, snapshot cohérent, refus des versions vides/GLB seuls, sauvegarde d’un GLB non certifié et variante historique. Retirer une texture utilisée refuse la publication en conservant le dernier GLB et le hash du `.blend`.
- **77 tests frontend passent dans onze fichiers**, notamment les états de synchronisation, réponses obsolètes, récupération d’événement perdu, refus des versions sans changement source, conservation du dernier aperçu, historique et scènes vides dans le lecteur et les vignettes.
- Les trois tests Node des fixtures passent avec Blender réel. Le typage, le lint frontend, Prettier et le build Vite passent.
- Les quatre intégrations Blender de cache/aperçu passent également : dépendances externes, corruption du cache, sources historiques immuables et sauvegarde pendant conversion. Avec les douze tests du worker et le parcours canonique, **17 scénarios Blender réels** ont été exécutés avec succès.
- Clippy du workspace et de toutes ses cibles avec `-D warnings`, `cargo fmt --all --check` et la vérification du diff passent. Les 21 tests desktop ont été réexécutés après le dernier ajout du contrôle de budget des GLB canoniques.

Les temporaires Rust sont dirigés vers `target/` dans l’environnement restreint : le temporaire système refuse certaines publications atomiques. Les avertissements de liens physiques du cache incrémental entraînent une copie et ne constituent pas une erreur de compilation.

Commandes de validation Blender, après choix explicite d’un exécutable :

```powershell
$env:EDGE_LOCK_TEST_BLENDER = 'C:\chemin\vers\blender.exe'
cargo test -p edge-lock-core --test blender_integration --test preview --test sync -- --include-ignored --test-threads=2
node --test scripts/e2e-fixtures.test.mjs
```

La CI fournit désormais Blender 5.1.1 et adapte les parcours desktop aux imports `.blend` uniques. Son exécution distante et l’installation/mise à niveau en VM restent des validations distinctes des tests locaux. Aucun test ne modifie la bibliothèque personnelle ; les scénarios utilisent des dossiers et réglages jetables. Les anciennes versions, leurs configurations et les objets immuables sont conservés sans migration de schéma.

### Application Windows et artefacts

Le rapport `.test-data/desktop-Vw9TEL/report.json` porte **`status: passed` pour quatorze groupes de scénarios**, dont un contrôle complémentaire après le parcours complet, avec bibliothèque, réglages et profil WebView2 jetables :

- Import réel d’un `.blend` seul et refus des dossiers non conformes sans modifier leurs sources.
- Sauvegarde réelle d’un Mirror : GLB et statistiques passent automatiquement de 1 à 2 triangles sans actualisation manuelle ; le hash du `.blend` enregistré reste inchangé pendant la synchronisation. La capture `11-mirror-autosync.png` a été examinée.
- Absence du menu source, blocage des versions vides, création de version et variante indépendante, export historique conservant la géométrie d’avant Mirror, et projet vide avec `.blend` et GLB.
- Textures rouge/verte vérifiées par pixels dans le lecteur et les vignettes, animation, navigation 3D et interface à 1440/1024 pixels CSS.
- Dix allers-retours : au maximum un contexte WebGL dans la fiche, zéro dans la galerie et aucune URL blob conservée. Galerie de cinquante imports `.blend`, pagination à 36 cartes, vignettes et cache persistant.
- Redémarrage : classement, variantes, historique et vignettes conservés. Le manifeste du dossier source initial est inchangé à la fin.
- Contrôle complémentaire : rendu historique effectivement terminé et contrôles d’édition verrouillés ; nouveau projet affichant « Votre scène est vide. », avec zéro triangle et zéro objet maillé. Les captures `history-render-ready.png` et `empty-project-ready.png` ont été examinées.

Les cinquante imports synthétiques ont pris environ 232 secondes ; la médiane d’ouverture de fiche à chaud était de 935 ms sur ce passage. Ces mesures ont été recueillies pendant une construction release parallèle et ne constituent pas un benchmark contrôlé ni une garantie sur des scènes complexes. Le scénario optionnel du Sniper Kitsune personnel n’a pas été exécuté, aucun chemin de fixture personnelle n’ayant été fourni.

Le premier lancement WebDriver a été refusé par les restrictions localhost du sandbox ; le parcours isolé a réussi avec l’accès approprié. Le build release et l’empaquetage NSIS ont réussi après autorisation du téléchargement de l’outil officiel NSIS. Aucune installation sur le poste personnel n’a été effectuée.

Les métadonnées des deux artefacts indiquent **Edge Lock, versions fichier et produit 0.2.0**. L’application est PE x64 (`0x8664`) ; le bootstrap NSIS est PE x86 (`0x014C`) embarquant l’application x64.

| Artefact | Taille en octets | SHA-256 |
| --- | --- | --- |
| `target/release/edge-lock.exe` | 16 852 992 | `186170299D9AFF4E5C051280CF9F03501198F2926456FE53C5EF8857D1C47910` |
| `target/release/bundle/nsis/Edge Lock_0.2.0_x64-setup.exe` | 6 402 241 | `EBB6C7CD372B0793E84A45F967B46D1211FCF9BB730CDC142348E088E42E884A` |

**Installation et mise à niveau en VM : non exécutées**, Windows Sandbox étant indisponible. Le script de validation isolée a été mis à jour pour 0.2.0 et sa syntaxe PowerShell vérifiée ; deux anciennes chaînes contenant des apostrophes typographiques ont été corrigées. Produire et inspecter l’installateur ne valide pas son installation. La CI a été adaptée mais n’a pas été exécutée sur un service distant durant cette livraison. Les artefacts et mesures 0.1.0 ci-dessous restent historiques.

## Refonte visuelle et sauvegarde automatique — 2 novembre 2026

### TDD et contrôles automatisés

- **79 tests frontend passent dans huit fichiers**, dont 32 pour la fiche d’asset. Cinq échecs ont été observés avant l’implémentation de la sauvegarde automatique, puis deux nouvelles régressions ont reproduit des réponses `getVariant` périmées avant leur correction. Les cas couvrent sauvegarde immédiate des choix, blur/Entrée, absence d’écriture inutile, configuration canonique, erreur/réessai, navigation, historique et invalidation des résultats obsolètes.
- **60 tests Rust ordinaires passent**, dont quatre nouveaux tests de cycle de vie : fermeture attendant une écriture soumise, refus de nouvelles écritures pendant fermeture, libération de la garde après erreur et contrôle de la bibliothèque cible.
- **12 intégrations Blender 5.1.1 passent** avec `--ignored` : huit scénarios du worker et quatre du cache. Les **deux tests Node** des fixtures de texture passent également.
- ESLint, TypeScript, Prettier, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all --check` et le build frontend passent. Les builds Tauri debug et release finaux et l’empaquetage NSIS passent.

Pour les tests Rust locaux dans cet environnement restreint, `TEMP` et `TMP` ont été dirigés vers `.test-data/native-tests` : le dossier temporaire système refusait les écritures des tests de persistance. Les avertissements de cache incrémental Rust indiquent une copie à la place des liens physiques ; ils n’ont pas empêché les vérifications.

### Interface et parcours desktop réels

Le rapport final `.test-data/desktop-Vz0XNV/report.json` porte **`status: passed` pour douze parcours**, avec bibliothèque, réglages et profil WebView2 jetables. Il a été produit après reconstruction du binaire avec la correction finale des réponses obsolètes.

- Captures examinées en **1440 × 940** et **1024 × 700 pixels CSS** : grille, liste, fiche, tableau et modale. Aucun débordement horizontal ; logo et nom tiennent dans la barre latérale. Windows arrondissant la largeur native minimale à 125 % de DPI, le test utilise au besoin l’émulation de viewport du pilote pour obtenir exactement 1024 pixels CSS, sans modifier les contraintes de la fenêtre de production.
- Space Grotesk est effectivement chargée depuis la ressource locale ; le logo mesure respectivement 56 et 50 pixels. Les points actifs sont roses. Les captures de survol et de clic montrent les mêmes dimensions et le même gradient ; le focus clavier reste visible. Le bouton Blender partage le style bleu des boutons primaires.
- Le bouton de sauvegarde manuelle et son paragraphe ont disparu. Une modification par la case de réglage est persistée, conservée après réouverture puis après redémarrage. Les contrôles des versions historiques et le bouton Blender y restent désactivés.
- Import, versions, variantes, export protégé, textures PNG en buffer et data URI, commandes de la visionneuse et opérations Blender réelles passent. Les fichiers sources des fixtures restent intacts.
- Dix allers-retours conservent au maximum un contexte WebGL dans la fiche et zéro dans la galerie ; aucune URL blob ne subsiste après retour. Les images persistantes sont réutilisées après redémarrage sans réécriture.

Sur le dernier passage synthétique, l’ouverture de fiche à chaud prend une médiane de **951 ms**, le retour galerie **701 ms**, et les vignettes en cache de la page de 36 cartes apparaissent en **529 ms**. Douze cartes sont visibles à cette taille de fenêtre. La préparation froide varie entre **9,6 s et 21,9 s** sur les deux passages de validation ; ces observations ne constituent ni une garantie de débit ni une comparaison contrôlée avec les anciens bilans. Aucun benchmark de modèle réel lourd n’a été ajouté.

### Ressources et compatibilité

`npm run icons` a régénéré les ressources Windows depuis `Edge-Lock-Icon.png`. L’icône 32 × 32 extraite du binaire debug final est identique pixel par pixel au PNG généré ; le PNG public est identique au logo source. La police WOFF2 officielle de 49 256 octets est livrée avec licence et provenance dans `public/fonts/`. Aucun changement de schéma ou de signature IPC n’a été nécessaire.

Les artefacts release reconstruits ont été inspectés : nom de produit **Edge Lock**, versions fichier et produit **0.1.0**, application PE x64 (`0x8664`) et bootstrap NSIS PE x86 (`0x014C`) embarquant l’application x64. L’icône extraite du binaire release est également identique pixel par pixel au PNG généré. Le téléchargement NSIS initialement refusé par l’environnement restreint a réussi après autorisation réseau ; l’empaquetage final est terminé.

| Artefact | Taille en octets | SHA-256 |
| --- | --- | --- |
| `target/automatic-preview/release/edge-lock.exe` | 16 603 648 | `3D0A013F5F2504E39C0EFFAAF009BCD64A2D0E917DC9355532BD21E2B587336A` |
| `target/automatic-preview/release/bundle/nsis/Edge Lock_0.1.0_x64-setup.exe` | 6 345 374 | `2BDE848C96699243B946DF6828288F1FFA7A34488151020496DAD2BCC4798F00` |

Le rapport local d’inspection est `.test-data/visual-refresh-release-verification.json`.

L’installation et la mise à niveau en VM jetable ou Windows Sandbox ne sont pas exécutées sur ce poste. Aucun installateur n’est lancé sur l’installation personnelle.

## Bilan précédent : aperçus et vignettes — 2 novembre 2026

### Vérifications exécutées

- **70 tests frontend passent dans huit fichiers**, avec les régressions d’aperçu automatique, de changement de sélection, de configuration, de priorité des tâches, de vignettes, d’annulation et de nettoyage des ressources.
- **56 tests Rust passent dans la suite ordinaire.** Les intégrations nécessitant Blender restent séparées de cette commande.
- **12 intégrations Blender 5.1.1 passent**, exécutées explicitement avec `--ignored` : huit scénarios du worker et quatre scénarios du cache.
- Les **deux tests Node** des fixtures `scripts/e2e-fixtures.test.mjs` passent : PNG dans un buffer et PNG en data URI, avec UV et matériau relié à l’image.
- ESLint, TypeScript, Prettier, `cargo clippy --workspace --all-targets -- -D warnings` et `cargo fmt --all --check` passent.
- Les builds Vite, Tauri **debug** et **release**, ainsi que la génération de l’installateur NSIS, passent. Les versions et architectures des artefacts produits ont été vérifiées.

### Rendu texturé dans la véritable WebView2

Un défaut de textures a été reproduit : le modèle apparaissait blanc parce que la CSP n’autorisait pas `blob:` dans `connect-src`, utilisé pour charger les images incorporées au GLB. La correction autorise aussi les images embarquées en `data:`, acceptées par l’inspecteur et chargées par le même mécanisme. Le parcours desktop vérifie les couleurs d’une texture PNG rouge et verte dans les captures de la vignette et de l’aperçu automatique, pour les deux modes d’incorporation.

Le rapport final local `.test-data/desktop-aNs9mN/report.json` porte `status: passed` pour **onze parcours**. Il couvre import et dépôt, rendu texturé buffer/data URI, commandes de la visionneuse, dix allers-retours bibliothèque/fiche, galerie de 50 assets, versions et variantes, export protégé, Blender réel et redémarrage. La vignette texturée reste disponible après redémarrage et son cache est réutilisé sans réécriture.

| Capture | Pixels verts détectés | Pixels rouges détectés |
| --- | --- | --- |
| Vignette statique | 2 040 | 1 621 |
| Aperçu automatique dans la fiche | 9 854 | 7 598 |
| Vignette et fiche avec texture en data URI | 2 040 / 9 854 | 1 621 / 7 598 |
| Vignette après redémarrage | 2 040 | 1 623 |

Le contrôle initial de l’ancien `HEAD`, avant implémentation, a également réussi ses **six parcours** : `.test-data/desktop-tnizdz/report.json`. Ce contrôle vérifie le fonctionnement de départ ; il ne constitue pas un benchmark avant/après sur des modèles réels. Ces rapports et leurs captures restent locaux et sont exclus du dépôt.

### Temps et ressources observés

Ces mesures proviennent des fixtures synthétiques du rapport `desktop-aNs9mN`, sur ce poste Windows. Les temps incluent la navigation et le polling WebDriver. Le premier aperçu de la fiche peut réutiliser un GLB déjà préparé par la galerie ; la conversion Blender à froid est mesurée séparément.

| Mesure | Résultat observé |
| --- | --- |
| Première vignette visible | 1,271 s |
| Premier aperçu automatique de la fiche | 1,006 s |
| Ouverture de fiche à chaud, médiane sur dix cycles | 0,952 s |
| Ouverture de fiche à chaud, percentile 95 | 0,965 s |
| Retour galerie à chaud, médiane sur dix cycles | 0,708 s |
| Retour galerie à chaud, percentile 95 | 0,714 s |
| Galerie de 50 assets : page de 36 cartes, liste à froid / chaud | 298 / 297 ms |
| Les huit vignettes visibles prêtes à froid / en cache | 6,503 / 0,520 s |
| Blender : première conversion et vignette depuis la galerie | 4,959 s |
| Blender : job d’aperçu en cache | 0,229 s |

Le benchmark de galerie importe 50 copies du même petit GLB texturé dans une bibliothèque jetable. Seules les huit cartes visibles sur la première page de 36 sont préparées : les 50 images ne sont pas préchargées. Le chargement chaud affiche les PNG avant la fin de la revalidation des sources ; aucun contexte WebGL n’est créé sur ce passage. Les fixtures sont archivées ensuite, sans suppression de leurs fichiers ni de leur historique. Le scénario Blender utilise un projet synthétique distinct et lance la génération depuis la galerie avant l’analyse explicite. Ces mesures ne préjugent pas des performances de modèles lourds.

Sur les dix cycles, le maximum est **un contexte WebGL simultané** dans la fiche ; la galerie conserve **zéro contexte**. Aucune URL `blob:` ne reste active et les dix URL de modèles contrôlées deviennent inaccessibles après libération. Le nombre total d’entrées du registre de ressources n’est pas exposé par l’API ; il n’est pas déduit de ces seuls contrôles d’URL.

Le tas JavaScript utilisé mesuré par `performance.memory` passe de **17,68 Mo à 14,70 Mo**, sans GC forcé. Cette observation ne mesure ni la RAM totale de WebView2 ni la mémoire GPU, et ne suffit pas à conclure à l’absence de toute fuite mémoire sur une longue session.

L’interface a aussi été contrôlée à une largeur CSS de **1 024 px** : aucun débordement horizontal du document ou du panneau principal, aperçu large de 501 px et métriques réparties sur deux colonnes. Les captures de la fiche et de son bas, ainsi que le rapport, sont dans `.test-data/desktop-aNs9mN/compact/`.

### Artefacts reconstruits et inspectés

L’exécutable release préexistant était ouvert sur le poste et Windows refusait son remplacement. La construction a donc été réalisée avec `CARGO_TARGET_DIR=target/automatic-preview`, en réutilisant une copie du cache de compilation et en préservant cette instance. Le nouveau binaire et l’installateur ont pour nom de produit **Edge Lock** et pour versions fichier et produit **0.1.0**. L’application est PE x64 (`0x8664`) ; le bootstrap NSIS est PE x86 (`0x014C`) et embarque cette application x64.

| Artefact | Taille en octets | SHA-256 |
| --- | --- | --- |
| `target/automatic-preview/release/edge-lock.exe` | 16 575 488 | `E8FB4827D0E19945699D677F6D775D61C020DF85A309E1E99331A908FB0B0483` |
| `target/automatic-preview/release/bundle/nsis/Edge Lock_0.1.0_x64-setup.exe` | 6 320 829 | `41E7EA8588579D3E9BE2D698040D37EF57733944A4E66FB94567848308145DC5` |

Le rapport local d’inspection est `.test-data/automatic-preview-release-verification.json`. Aucun installateur n’a été exécuté sur l’installation personnelle.

### Validations restant à compléter pour cette livraison

- Comparaison avant/après sur plusieurs modèles réels représentatifs : non exécutée.
- Installation et mise à niveau en VM jetable ou Windows Sandbox : non exécutées.

## Bilan historique — 1er novembre 2026

Les résultats de cette section ont été observés le 1er novembre 2026 sur le poste Windows de développement. Les fixtures étaient synthétiques, à l’exception d’une copie importée de Sniper Kitsune dans une bibliothèque jetable ; ses 72 fichiers personnels ont été contrôlés par hash et date avant/après, sans modification. Les commandes et procédures sont conservées pour reproductibilité ; les comptes de tests, mesures et artefacts ci-dessous sont historiques.

### Contrôles reproductibles historiques

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

### Tests du cœur historiques

La suite `cargo test --workspace` réussit : **46 tests Rust**, dont 19 tests de stockage, 1 arrêt brutal en sous-processus, 12 tests GLB, 3 tests d’aperçu sans Blender, 2 validations du runner sans processus externe, 3 tests internes du cœur et 6 tests de l’adaptateur desktop. Les doctests réussissent également (aucun exemple exécutable). `cargo clippy --workspace --all-targets -- -D warnings` et `cargo fmt --all --check` réussissent, en édition Rust 2024.

Les tests couvrent : copie sans modification de l’origine, fichiers binaires et Unicode, dossiers vides, déduplication, historique, variantes indépendantes, archivage, taxonomie sans cycles, filtrage par descendants, configuration versionnée, annulation, fichiers modifiés pendant capture, fichiers verrouillés Windows, échec de publication d’objet, objet corrompu et refus d’écrasement.

La pagination est testée avec 110 modifications plus la version initiale, puis une variante héritant de cet historique. Les résumés n’embarquent pas les manifestes ; les versions au-delà des 100 premières restent accessibles.

Un test lance deux sous-processus qui quittent brutalement sans exécuter leurs destructeurs : après matérialisation d’un nouveau workspace et après publication d’un objet avant commit du snapshot. Le redémarrage retrouve les versions publiées précédemment, conserve le travail courant et signale le workspace non publié. Le helper ignoré dans la sortie Cargo est invoqué par ce test ; il ne représente pas un scénario omis.

La migration 0→1 est testée sur une base synthétique comportant des réglages préexistants. Le test vérifie la sauvegarde SQLite avant migration, le numéro de schéma et la conservation des réglages. Les schémas futurs et catalogues absents sont refusés sans recréation vide. La reprise d’une initialisation interrompue est également testée.

Les protections de chemins et l’inspecteur GLB ont leurs propres tests : données tronquées, plages hors limites, cycles, ressources externes, extensions non prises en charge, comptes des instances/animations et textures disproportionnées.

Les nouvelles régressions ont été écrites avant leur implémentation et observées en échec, puis exécutées à nouveau après correction. La revue a notamment ajouté un cas rouge pour le GLB principal supprimé : le repli sur l’unique `.blend` conserve désormais la configuration persistante.

### Blender réel — historique

Les **12 scénarios exigeant Blender réussissent sur Blender 5.1.1** : huit intégrations du worker et quatre intégrations du cache. Ils créent leurs propres projets et ressources. Les anciens scénarios avaient également été validés sur Blender 5.2.3 LTS ; la nouvelle suite complète n’a pas été réexécutée sur cette version.

```powershell
$env:EDGE_LOCK_TEST_BLENDER = 'C:\chemin\vers\blender.exe'
cargo test -p edge-lock-core --test blender_integration --test preview -- --include-ignored
```

Les scénarios comprennent création réelle d’un projet, export d’une collection avec sous-collection, textures incorporées, rig, shape keys et animations, conservation du source, détection d’une texture externe, refus d’export strict non autonome, neutralisation des scripts embarqués, timeout et annulation. Les régressions d’aperçu couvrent police/brosse inutilisée, texture absente et remplacement en mémoire, références externes relatives, bibliothèques imbriquées et arêtes exactes.

Le cache est testé pour la réutilisation sans Blender, les rapports ignorés, les changements de textures externes, la corruption, l’isolation historique, le repli après suppression du GLB principal et une source modifiée pendant la conversion. Ce dernier cas vérifie une nouvelle tentative et la publication d’un seul GLB stable. Une annulation après création du temporaire laisse sources, métadonnées et objets inchangés. Les tests exigeant Blender sont ignorés par la commande Rust ordinaire parce qu’un exécutable doit être choisi explicitement.

### Interface — historique

Les **42 tests frontend passent**, répartis entre quatre fichiers. Ils couvrent notamment import, dépôt de dossier, configuration, navigation, erreurs, événements et validation des ressources de la visionneuse. Les nouveaux cas vérifient les dépendances non bloquantes, la dégradation, les erreurs locales d’aperçu, le choix GLB et sa suppression externe, les arêtes et la provenance, la conservation de l’analyse Blender et les résultats obsolètes après changement de sélection. Le typage TypeScript, ESLint, Prettier et le build Vite réussissent. La visionneuse est également exercée dans la véritable WebView2 ; les tests unitaires seuls ne prouvent pas le rendu GPU.

### Mesures de performance historiques

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

### Application desktop et installateur — historique

Le script `scripts/e2e.mjs` pilote une vraie WebView2 via `tauri-driver` et un Microsoft Edge WebDriver compatible. Il utilise `.test-data/`, des réglages isolés via `EDGE_LOCK_TEST_DATA_DIR` en build debug et un dossier WebView temporaire. Les chemins des exécutables peuvent être fournis par `EDGE_LOCK_E2E_EXE`, `TAURI_DRIVER` et `EDGE_DRIVER`.

```powershell
node scripts/e2e.mjs
```

Les rapports et captures sont écrits dans le dossier temporaire annoncé par le script. L’exécution a **réussi huit parcours**, dont Sniper Kitsune :

- Événement Tauri de dépôt : formulaire prérempli, aucune copie avant confirmation. Le test injecte l’événement natif ; il ne simule pas un geste de souris depuis l’Explorateur.
- Import Unicode, conservation du source et des dossiers vides.
- Aperçu WebGL local, rotation, zoom, recentrage et lecture/pause d’animation.
- Détection des modifications, version manuelle et variante indépendante depuis une ancienne version.
- Export historique vérifié, refus d’écrasement sans consentement et travail courant préservé.
- Sniper Kitsune : GLB original avec exécutable Blender indisponible, conversion réelle depuis une copie sans GLB, police externe et brosse manquante secondaires, cache froid/chaud et deux demandes simultanées, arêtes et rendu WebGL.
- Blender réel via Tauri : sélection automatique du principal, collection mesurée à 12 triangles, aperçu, export, source inchangée et création native d’un projet.
- Redémarrage : classement, variantes et historique conservés.

Le rapport final de cette correction est `.test-data/desktop-x9r4gB/report.json` (`status: passed`), après reconstruction du binaire debug et des assets frontend définitifs. Les dossiers `.test-data` sont locaux et exclus du dépôt. Pour inclure le scénario Blender, fournir `EDGE_LOCK_E2E_BLENDER` (exécutable) et `EDGE_LOCK_E2E_BLEND_SOURCE` (dossier d’une fixture jetable avec `main.blend`, texture, collection et animation). Pour Kitsune, ajouter `EDGE_LOCK_E2E_KITSUNE_SOURCE` (dossier contenant son `.blend` et son GLB original). Le script importe une copie avant tout retrait de GLB et vérifie les hashes de la source personnelle ; il refuse les binaires release afin de garantir l’isolation des réglages.

Sur ce poste, Kitsune a fourni **41 112 sommets, 77 982 arêtes et 37 701 faces** depuis Blender. Mesures finales du parcours IPC, pendant une compilation release parallèle : GLB original 869 ms, conversion 11 120 ms, cache chaud 250 ms. Le premier passage avait mesuré 665 / 7 959 / 247 ms. Ces durées ne sont pas une garantie de performances sur d’autres scènes ou machines. Les captures `06-kitsune-original-glb.png` et `07-kitsune-generated-glb.png` des deux passages ont été examinées.

**La construction Tauri release et la génération NSIS ont réussi.** Les métadonnées des deux fichiers produits indiquent `Edge Lock`, version fichier et produit **0.1.0**. L’exécutable applicatif est bien PE x64 (`0x8664`). Le lanceur NSIS est un bootstrap PE x86 qui embarque cette application x64.

| Fichier | Taille | SHA-256 |
| --- | --- | --- |
| `target/release/edge-lock.exe` | 17 414 144 octets | `2343482292E8FB1E3FD0D3E76B4D8E3BB348773BB960E02581547F27C78603DA` |
| `target/release/bundle/nsis/Edge Lock_0.1.0_x64-setup.exe` | 7 342 480 octets | `F70661A9A9E8B704EADF2A600887F1B7DFE280AA955E697206401C7A5E9F841D` |

Le rapport local d’inspection est `.test-data/dependency-release-verification.json`. Le premier packaging a échoué sur la résolution réseau dans le sandbox ; le build standard relancé hors sandbox a téléchargé l’outil NSIS puis réussi. L’installateur n’est pas signé avec un certificat de signature de code.

Le script `scripts/test-installer.ps1` est prévu pour Windows Sandbox ou une VM jetable. Il vérifie installation, version de l’exécutable installé, réinstallation et intégrité des hashes d’une copie de bibliothèque. Avec `-PreviousInstallerPath`, il peut aussi vérifier le passage depuis un installateur antérieur fourni.

```powershell
# À exécuter uniquement dans une VM jetable ou Windows Sandbox.
./scripts/test-installer.ps1 -InstallerPath 'C:\fixtures\EdgeLock-setup.exe' `
  -FixtureLibraryPath 'C:\fixtures\library-copy' -DisposableEnvironment
```

**Installation et mise à niveau réelles en environnement isolé : non exécutées sur ce poste.** `WindowsSandbox.exe` n’y est pas disponible. Produire et inspecter un installateur ne valide pas son installation ; le script ne doit pas être exécuté sur l’installation personnelle pour masquer cette limite. Une mise à niveau depuis une version précédente reste aussi conditionnée à la disponibilité de son installateur.

Le workflow `.github/workflows/ci.yml` automatise les vérifications, le parcours desktop, la construction et l’installation/réinstallation sur un runner Windows jetable. Il est fourni mais n’a pas été exécuté sur un service distant pendant cette livraison.
