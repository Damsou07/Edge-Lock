# Validation d’Edge Lock

Ce document distingue les contrôles exécutés des procédures restant à exécuter. Le bilan le plus récent couvre l’aperçu des morphs avec modificateurs et la reprise des annulations Windows au schéma 7. Les bilans historiques identifient leurs propres binaires et empreintes, même lorsque la version applicative est également 0.3.0.

## Aperçu évalué et annulation différée — 6 novembre 2026

Les régressions TDD ont d’abord reproduit le refus d’aperçu `SOLIDIFY` + `ARMATURE`, le refus Windows `os error 32` avec un lecteur ouvert et l’échec de lecture d’un cache valide à cause d’un temporaire verrouillé. Blender vérifie la forme évaluée à l’image enregistrée, l’épaisseur, le skin, les animations du rig et les morphs du maillage compatible. Les fixtures avec objets directs et instanciés vérifient l’absence de canaux `weights` orphelins et la conservation des sources et du dernier GLB en cas de refus strict.

Les tests Windows utilisent de vrais handles. Un lecteur peut autoriser la suppression du fichier alors que Windows refuse encore le déplacement du dossier parent. La demande est alors différée. Les régressions couvrent la reprise après déverrouillage et redémarrage, les écritures conservant taille et date, le remplacement du dossier, le contenu initial illisible, la suppression de variante, la migration 6→7 et l’interruption autour du commit. Les tests du cache vérifient l’intégrité du dernier aperçu, l’isolation de sa configuration et l’accès à un aperçu historique lorsque le dossier de travail est indisponible.

La suite Rust compte **166 tests réussis** et 31 cas ignorés en mode ordinaire. Les **175 tests frontend** et les **5 tests de fixtures Node** passent. Prettier, ESLint, TypeScript, `cargo fmt --all --check` et Clippy avec `-D warnings` passent. Les tests Rust utilisent des temporaires sous `.test-data` pour les opérations de renommage.

Les **23 intégrations Blender 5.1.1** passent avec les fixtures jetables, dont 17 pour le worker, 4 pour le cache d’aperçu et 2 pour la synchronisation historique.

Les builds Vite et Tauri debug réussissent. Le binaire `target/debug/edge-lock.exe` est un PE Windows x64, produit **Edge Lock 0.3.0**, de **25 976 320 octets**. Son SHA-256 est `D569D5A413F5953F81719226FD6898E3545B95E85AC16FF768418B010288840A`.

Les **20 groupes de vérifications WebView2** passent sur ce binaire, avec la bibliothèque et le profil jetables du rapport `.test-data/desktop-o5i7jf/report.json`. Le parcours maintient un modèle chargé pendant le verrou, ferme la modale en état d’attente et vérifie la restauration automatique des fichiers et réglages après libération. La capture `discard-waiting.png` montre le modèle et le statut d’attente. Le parcours utilise la détection du runtime installé et le pilote local hors sandbox. Le benchmark séparé de 50 imports et les fixtures personnelles optionnelles sont omis. Le build validé est le binaire debug sans bundle.

## GLB manuels, variantes et sélections persistantes — 4 novembre 2026

### TDD et compatibilité

Les régressions ont été observées en échec avant correction : import et versions exigeant Blender, vignette dépendant d’un GLB généré dans le workspace, modifications des anciens GLB gérés ignorées, renommages et suppression absents, sélections perdues après fermeture et choix du principal avant chargement de sa configuration. La revue et le parcours desktop ont ajouté les cas d’édition hors application, de principal supprimé, d’ancêtre partagé appartenant à une variante supprimée, de préférences de cache anciennes et de normalisation des réglages d’import.

`cargo test --workspace --locked` réussit : **156 tests Rust**, aucun échec, 31 cas ignorés par la commande ordinaire. Les helpers d’arrêt brutal sont exécutés par leurs tests parents ; les scénarios Blender et le benchmark restent opt-in. Les tests de variantes couvrent les ancêtres partagés, les noms réutilisables, la dernière variante, les jetons périmés, les objets communs entre assets, les verrous Windows, l’annulation, trois frontières d’interruption et les journaux antérieurs. Les compteurs publics des archives excluent les propriétaires internes supprimés et les versions déjà supprimées, tout en conservant leurs cibles de nettoyage. Les migrations représentatives 4→6 et 5→6 vérifient la sauvegarde du catalogue et la conservation des configurations, manifestes et fichiers.

Les **169 tests frontend** passent dans 19 fichiers. Ils couvrent notamment les dialogues, le renommage d’une version consultée, la bascule après suppression, la mémorisation par variante/version, les écritures avant navigation, les sources disparues et les réponses obsolètes. Les **cinq tests Node des fixtures** passent, dont une création et sauvegarde réelles avec Blender 5.1.1.

Les **24 intégrations Blender 5.1.1** passent : 17 scénarios du worker, quatre du cache d’aperçu, deux des interfaces Rust historiques de synchronisation et un scénario desktop de création puis vignette depuis le `.blend`. Ce dernier vérifie l’absence de GLB dans le workspace, le hash de la source et la réutilisation du cache. Les interfaces historiques restent testées pour compatibilité ; leurs générations ne sont plus appelées par les flux automatiques de l’application. Les journaux sont `.test-data/optimizations-blender-tests.log` et `.test-data/optimizations-blender-app-test.log`.

ESLint, TypeScript, Prettier, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings` et `git diff --check` passent. Les avertissements de hardlinks du cache incrémental Windows utilisent une copie de repli et n’affectent pas les résultats. Les commandes Rust sont exécutées sur des fixtures jetables sous `.test-data/optimizations-temp`, hors des restrictions du sandbox empêchant certains renommages.

### Construction vérifiée

Les builds Vite et Tauri debug sans bundle réussissent. Le binaire inspecté `target/debug/edge-lock.exe` est PE **x64 (`0x8664`)**, produit **Edge Lock**, version fichier et produit **0.3.0**, **25 903 104 octets**, SHA-256 **`99E3ED304D2AADDCFF559E1FA59D48E3E4301E27CAB02A652CEDA140CA887118`**. Son inspection est enregistrée dans `.test-data/optimizations-binary.json`.

Les journaux locaux sont `.test-data/optimizations-rust-tests.log`, `.test-data/optimizations-clippy.log` et `.test-data/optimizations-build.log`. Aucun installateur n’est produit ; installation et mise à niveau en VM ne sont pas exécutées. Aucune bibliothèque personnelle ni installation existante n’est utilisée comme fixture.

### Parcours Windows sur le binaire final

Le rapport `.test-data/desktop-RS7Y6k/report.json` est **`passed` pour 20 groupes de vérifications**, et son empreinte correspond au binaire inspecté ci-dessus. Le parcours utilise la véritable WebView2, Blender 5.1.1, une bibliothèque, des réglages et un profil navigateur jetables. Les GLB des fixtures sont fournis explicitement par le harness ; une sauvegarde réelle du `.blend` actualise l’aperçu en cache sans modifier le GLB utilisateur.

Le nouveau scénario vérifie un asset contenant seulement un `.blend` : vignette de bibliothèque, aperçu, trois versions et une variante, sans GLB écrit dans les workspaces. Les sources historiques restent complètes. Il renomme une version partagée et une variante, vérifie les choix distincts courant/historique après fermeture réelle puis redémarrage, annule une suppression puis la confirme et vérifie la protection de la dernière variante. Les parcours existants de textures, animation, navigation, export exact, restauration, optimisation du stockage et suppression des archives passent également.

Les captures `manual-glb-management.png` et `storage-history.png` ont été inspectées ; le contrôle automatique ne détecte pas de débordement à 1440, 1024 et 860 px. Le sélecteur d’aperçu reste à 30 px de haut. Les dix cycles de navigation conservent au maximum un contexte WebGL dans la fiche, zéro dans la galerie et aucune URL blob active.

Le harness attend maintenant les boutons activés de la modale ouverte et la disponibilité de la commande d’animation après changement de source. Ces corrections évitent de cliquer sur les actions d’arrière-plan ou avant le retour du job. Les passages intermédiaires en échec sont conservés localement ; seul le rapport final ci-dessus porte sur le résultat complet.

Le benchmark séparé de 50 imports est explicitement omis avec `EDGE_LOCK_E2E_SKIP_GALLERY_BENCHMARK=1`. Les fixtures personnelles optionnelles, le stress GPU de plus de 512 Mio et le test opt-in de junction Windows ne sont pas activés. Aucun comparatif de performance avant/après sur des modèles réels n’est revendiqué.

## Suppression non bloquante, annulation et aperçu agrandi — 4 novembre 2026

### TDD et conservation des données

Les régressions ont été constatées en échec avant correction : fichier verrouillé avant l’analyse des archives, volume libéré perdu lorsqu’un autre nettoyage reste différé, abandon des modifications absent, confirmations et état partiel dans l’interface, placement du sélecteur et plein écran. La revue indépendante a ajouté une régression d’écriture tardive dans un fichier restant à nettoyer : il est désormais revérifié juste avant sa suppression et préservé s’il a changé.

`cargo test --workspace --locked` réussit : **135 tests Rust**, aucun échec, 29 cas ignorés par la commande ordinaire. Ces derniers comprennent les intégrations Blender, le benchmark, le test opt-in de junction Windows et les helpers de sous-processus ; les tests parents exécutent effectivement les helpers d’arrêt brutal. Le nouveau test IPC d’annulation fonctionne sans Blender configuré. Les journaux de commandes locaux sont `.test-data/rust-workspace-tests.log` et `.test-data/rust-clippy.log`.

La suppression couvre les verrous de fichiers et de dossiers, l’inventaire inaccessible, les mesures partielles, les écritures suivantes, les références partagées, les modifications après suppression, les dossiers recréés, l’authentification v1/v2 et les journaux commités endommagés. L’annulation couvre les fichiers ajoutés/modifiés/supprimés, dossiers vides, fichiers cachés et secondaires, configuration et principal, isolement des variantes, absence de nouvelle version/backup, corruption, tête périmée, concurrence et reprise après interruption à cinq frontières, y compris un nettoyage partiel.

La migration 4→5 sauvegarde d’abord SQLite et ajoute seulement la table des reçus d’annulation. Les tests vérifient les têtes, backups, configurations et versions préexistantes. Toutes les bibliothèques de test et leurs temporaires sont isolés sous `.test-data` ; aucune bibliothèque personnelle n’est utilisée.

Les **cinq tests Node des fixtures passent**, y compris la création et la sauvegarde réelle d’un Mirror avec Blender 5.1.1. Les intégrations Blender opt-in complètes, le benchmark et l’installation en VM ne sont pas réexécutés pour cette évolution. Aucun installateur n’est produit et aucune installation personnelle n’est modifiée.

### Interface et construction

**156 tests frontend passent dans 18 fichiers**, avec `npm test -- --maxWorkers=2`. Ils couvrent notamment l’annulation confirmée ou abandonnée, les erreurs et nouvelles tentatives, la disponibilité malgré un échec de synchronisation, le nettoyage différé, les mesures partielles, les sources ambiguës et le plein écran. La régression Échap a été observée en échec puis corrigée par un listener limité à la visionneuse qui possède le plein écran ; les huit tests de `ModelViewer` passent sans recréation du renderer.

ESLint, TypeScript, Prettier, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all -- --check`, la syntaxe du harness et `git diff --check` passent. Les builds Vite et Tauri debug sans bundle avec dépendances verrouillées réussissent. Les avertissements de hardlinks du cache incrémental Rust utilisent une copie de repli et n’affectent pas le résultat.

Le binaire final inspecté est `target/debug/edge-lock.exe`, PE **x64 (`0x8664`)**, version fichier et produit **0.3.0**, **25 700 352 octets**, SHA-256 **`B1C2AD8158A60D6E5CA4FA436816EC2B85A08F64B9CA0528651C5696F6F894A0`**. Le harness enregistre désormais le chemin et l’empreinte de l’exécutable effectivement testé dans son rapport.

### Parcours Windows et captures

Le rapport `.test-data/desktop-OPEGeE/report.json` est **`passed` pour 19 groupes de vérifications**, sur le binaire final identifié ci-dessus, avec bibliothèque, réglages et profil WebView2 jetables. Il conserve les parcours d’import, synchronisation Blender réelle, textures, animation, navigation, versions/variantes, exports, historique Zstd, redémarrage et suppression des archives. L’annulation de la confirmation conserve les modifications ; sa validation rétablit les fichiers et réglages avec tête, backup et nombre de versions inchangés.

Le sélecteur d’aperçu mesure **30 px** de haut et se trouve entre provenance et actualisation, sans débordement. La visionneuse mesure **545,2 px** à 1440 × 940 et **420 px** à 1024 × 700. Le plein écran est vérifié à la fois par le DOM et par l’état natif de la fenêtre Tauri. Entrée, sortie par icône et Échap conservent le même canvas, contexte WebGL et temps d’animation. Les dix cycles bibliothèque/fiche gardent au maximum un contexte, aucun dans la galerie et aucune URL blob active.

Les captures `preview-toolbar-1440.png`, `preview-toolbar-1024.png`, `preview-fullscreen-1440.png`, `discard-confirmation-1024.png` et `discard-restored.png` ont été inspectées. Le premier lancement du pilote local dans le sandbox n’a pas pu joindre localhost ; l’exécution autorisée hors sandbox a ensuite révélé l’absence de sortie Échap native dans WebView2. Après le correctif et la reconstruction, le parcours complet final réussit, y compris les confirmations à 1440 et 1024 px.

Le benchmark séparé des 50 imports est explicitement omis avec `EDGE_LOCK_E2E_SKIP_GALLERY_BENCHMARK=1`. Les scénarios personnels Kitsune/personnage et la fixture GPU de plus de 512 Mio ne sont pas activés pour ce passage. Les sources utilisées restent exclusivement synthétiques ; leurs contenus originaux sont vérifiés inchangés par le harness.

## Sources multiples et suppression des archives — 4 novembre 2026

### TDD et données

Les nouveaux comportements ont été constatés en échec avant correction : choix d’aperçu/export/principal dans l’interface, suppression confirmée des archives et des catégories, plafonds GLB/JSON/textures, principal et propriété du dérivé, versionnage des fichiers secondaires, conservation des GLB historiques, annulation des ressources et perte du contexte graphique. La revue a ajouté des régressions pour la redétection du même principal après perte du reçu, les modèles utilisateur préfixés `.edge-lock-`, les préférences d’aperçu par snapshot et les reçus anciens incohérents.

`cargo test --workspace --locked` réussit : **121 tests Rust ordinaires**, dont les contrats Tauri, migration 3→4, récupération après interruption, références partagées, sauvegardes GLB à plusieurs propriétaires et refus d’un journal modifié. Les tests de suppression couvrent un fichier verrouillé après commit, la reprise répétée et un workspace recréé à son ancien chemin. Les temporaires sont isolés sous `.test-data/validation-temp` ; les avertissements de liens physiques du cache incrémental entraînent une copie sans affecter les résultats.

Les **23 intégrations Blender 5.1.1 passent** : 17 tests du worker, quatre du cache et deux de synchronisation. Elles ont été exécutées directement depuis les exécutables de tests compilés, avec `--ignored --test-threads=1` et `EDGE_LOCK_TEST_BLENDER` configuré. Les **cinq tests Node des fixtures passent**, y compris création/sauvegarde réelle du Mirror et vérification des PNG du modèle de stress. Le benchmark de stockage de 1 000 assets et le test opt-in de junction Windows ne sont pas exécutés pour cette évolution ; les helpers ignorés d’arrêt brutal sont invoqués par leurs tests parents.

La migration sauvegarde le catalogue existant avant transaction, conserve les objets bruts/Zstd, tags, variantes et historiques, et ajoute le suivi de propriété des backups et les reçus de suppression. Les anciennes sauvegardes communes non attribuables restent conservées et signalées. Aucune bibliothèque personnelle n’est utilisée comme fixture.

### Application Windows et aperçu volumineux

Le rapport final `.test-data/desktop-Yfyzop/report.json` est **`passed` pour 17 groupes de vérifications**, en environ 163 secondes, sur le binaire debug final indiqué ci-dessous, avec bibliothèque, réglages et profil WebView jetables. Il couvre les parcours existants et les nouveaux choix `.blend`/`.glb`, exports exacts, changement/redétection du principal, version multimodèle, suppression de branche, projet partagé, suppression définitive individuelle et vidage global malgré une recherche sans résultat. Les annulations des confirmations conservent les assets. Le benchmark indépendant de 50 imports est explicitement omis par `EDGE_LOCK_E2E_SKIP_GALLERY_BENCHMARK=1`. Un premier passage complet avait également réussi dans `.test-data/desktop-iw0kqB` avant les derniers garde-fous.

Un GLB synthétique de 3 152 548 octets contient **33 textures PNG distinctes de 2048 × 2048**, toutes utilisées par des triangles visibles, soit **553 648 128 octets / 528 Mio RGBA décompressés**. L’interface le charge et l’affiche en **3 693 ms** lors du passage final, avec `EDGE_LOCK_E2E_STRESS_TEXTURES=1`. La capture `preview-textures-over-512mib-1440.png` a été inspectée. Cette mesure reproduit et dépasse l’ancien plafond de 512 Mio sans décodage massif dans les tests unitaires.

La somme des working sets de l’application et de ses six processus WebView passe de **624 463 872** à **1 802 387 456 octets** pendant ce rendu. Cette mesure exclut la mémoire GPU et peut compter plusieurs fois des pages partagées ; elle ne constitue pas une exigence minimale ni une garantie de performance. Les capacités réelles RAM/GPU restent déterminantes.

Les captures d’aide, fiche, sources et confirmations à 1440 et 1024 pixels ont été inspectées. Les contrôles restent lisibles dans le thème existant. Le premier lancement du pilote localhost a été bloqué par le sandbox ; le parcours réussi a utilisé son autorisation locale hors sandbox. Une première exécution avait ensuite révélé une confusion du harness entre erreur IPC `not_found` attendue et erreur de protocole WebDriver ; le wrapper a été corrigé avant le passage final réussi.

### Livraison

**141 tests frontend passent dans 17 fichiers** avec `npm test -- --maxWorkers=2`. Les contrôles ESLint, TypeScript, Prettier global, `cargo fmt --all --check`, Clippy de toutes les cibles avec `-D warnings` et `git diff --check` passent. Les builds Vite et Tauri debug/release réussissent. La réconciliation conserve le modèle GPU déjà chargé lorsque la clé d’artefact est identique ; le jeton reçu en doublon est libéré. Une lecture de miniatures est sérialisée avec les suppressions pour empêcher la recréation tardive d’un jeton supprimé.

Une exécution frontend sous charge avait révélé une lecture de catalogue supplémentaire provoquée par le debounce d’une recherche inchangée. La mise à jour d’état est désormais évitée dans ce cas et la suite complète repasse. Les premiers contrôles Clippy ont aussi conduit à simplifier les paramètres de génération et à retirer des clones/unwrap inutiles. Un lancement concurrent d’un exécutable de test Blender avait empêché son réédition de liens sous Windows ; la suite Rust finale a été relancée après sa fermeture et réussit intégralement.

L’empaquetage NSIS a nécessité l’accès au composant officiel, initialement bloqué par le réseau restreint puis réussi après autorisation. L’installation et la mise à niveau sur une VM ou Windows Sandbox restent à vérifier : Windows Sandbox n’est pas disponible sur ce poste. La migration des données est vérifiée sur une bibliothèque synthétique ; aucun installateur n’est exécuté sur l’installation personnelle.

Les versions fichier et produit des trois artefacts indiquent **Edge Lock 0.3.0**. Les applications sont PE x64 (`0x8664`) ; le bootstrap NSIS est PE x86 (`0x014C`) et cible l’application x64. Les octets du worker Blender courant sont présents dans les deux exécutables. Le rapport local est `.test-data/release-verification-030.json`.

| Artefact | Octets | SHA-256 |
| --- | ---: | --- |
| `target/debug/edge-lock.exe` | 25 447 424 | `74C5CB3F17714D7A40F7590675DE77E48AA6DDC4396BA3396243420AF64AA1B5` |
| `target/release/edge-lock.exe` | 18 001 408 | `424EBB0DE469D70D30817AAACE6F7E34314E2F5F145119F5435396941300D406` |
| `target/release/bundle/nsis/Edge Lock_0.3.0_x64-setup.exe` | 6 709 643 | `4B95B06647D5A9B74AAA04B042F92ECBD59AE195520F6A7DAFF17C2CFD272B1C` |

## Export des personnages, aide et filtres — 3 novembre 2026

L’inspection en lecture seule du personnage a identifié 42 maillages à morphs relatifs, chacun associé à une unique Armature. Le refus venait du garde-fou Edge Lock appliqué à toute combinaison shape keys/modificateurs. Le worker utilise désormais les données originales pour cette combinaison compatible, via une extension glTF uniquement en mémoire ; les autres objets gardent leurs modificateurs évalués. Les piles hors de ce cas restent refusées explicitement, sans perte du dernier GLB. Aucun changement de schéma, de protocole ou de dépendance n’est ajouté.

### TDD et contrôles automatisés

- Deux tests Blender ont échoué sur le refus initial avant correction ; l’accès à l’aide a également été constaté en échec avant son implémentation, en test frontend puis sur l’ancien binaire Windows.
- **126 tests frontend passent dans 16 fichiers** avec `npm test -- --maxWorkers=2`. Ils couvrent aide, règles, historique, filtres combinés, bornes inclusives, tailles nulles/négatives/décimales, absence d’extension, pagination, rafraîchissement et changements de sélection. Une relance non limitée pendant les compilations parallèles avait rencontré des dépassements de délai ; la limite de workers permet l’exécution complète sans modifier les délais des tests.
- **94 tests Rust ordinaires passent** avec `cargo test --workspace --locked`.
- **22 tests Blender réels passent avec Blender 5.1.1** : 17 tests du worker, 4 du cache/aperçu et le parcours de synchronisation canonique. Les six régressions consacrées aux morphs vérifient les trois chemins export/génération/aperçu, les drivers réellement variables, skins et poids, textures/UV, scènes mixtes avec Mirror, hiérarchies, données partagées et instances. Elles couvrent aussi clés muettes/autoréférentes, maillages vides, callbacks absents, cibles perdues et conservation du dernier GLB et des sources après refus.
- **4 tests Node des fixtures passent**, y compris la création et la sauvegarde d’un Mirror avec Blender réel.
- TypeScript, ESLint, Prettier global, `cargo fmt --all --check`, Clippy de toutes les cibles avec `-D warnings`, build Vite et build Tauri debug sans bundle réussissent. Un lint Clippy introduit dans le test des poids a été corrigé avant le passage final.

Les temporaires Rust sont isolés sous `.test-data/morph-tests-temp`. Les avertissements du cache incrémental Rust indiquent une copie à la place des liens physiques et n’empêchent pas la construction.

### Application Windows et personnage réel

Le parcours WebView2 final `.test-data/desktop-zBxJ6e/report.json` est **`passed` pour 17 groupes de contrôles**, avec bibliothèque, réglages et profil WebView jetables. Il vérifie l’aide avant ouverture d’une bibliothèque et depuis la fiche, ouverture par Entrée, navigation Tab, fermeture Échap et retour du focus, filtres type/taille combinés, bornes exactes, plage invalide et réinitialisation. Les captures de l’aide et des filtres à 1440 et 1024 pixels ont été examinées. Le premier passage réussi `.test-data/desktop-FUufgT` avait révélé une troncature du libellé d’aide à 1024 pixels : le retour à la ligne est corrigé, reconstruit puis contrôlé visuellement et automatiquement dans le passage final.

Le personnage est importé dans la bibliothèque jetable depuis sa source personnelle lue seule : **42 maillages à morphs, un skin, une animation et les huit expressions de `EXP_Iris.R` sont conservés**. Les échantillons de poids de son animation varient réellement dans le GLB ; l’aperçu et la vignette sont affichés. La capture `17-character-viewer.png` a été examinée. Les hashes, tailles et dates de modification de tous les fichiers de la source personnelle et le contenu du workspace importé restent identiques après consultation.

Les parcours de non-régression conservent import strict, synchronisation Mirror, versions/variantes, exports historiques, textures, stockage et redémarrage. Dix allers-retours maintiennent un contexte WebGL au maximum dans la fiche, aucun dans la galerie et aucune URL blob conservée. Le benchmark distinct de 50 imports est explicitement omis avec `EDGE_LOCK_E2E_SKIP_GALLERY_BENCHMARK=1` ; le scénario optionnel Kitsune n’est pas demandé dans ce passage.

Le binaire final `target/debug/edge-lock.exe` indique **Edge Lock 0.2.0**, mesure **24 882 176 octets** et a pour SHA-256 `6B3A42C402F8930D9FAF36D319B1AD51001F9F94939693D8A6F0E8CB639BE5E9`. Ses octets contiennent le worker courant vérifié. Aucun installateur n’est produit et aucune installation ou mise à niveau n’est effectuée sur le poste personnel.

Le test facultatif du personnage se lance en renseignant `EDGE_LOCK_E2E_CHARACTER_SOURCE` avec un dossier contenant le `.blend` unique avant `npm run test:desktop`. Il importe une copie isolée, vérifie les données du personnage connu (`EXP_Iris.R`) et compare la source avant/après ; les tests automatisés ordinaires restent exclusivement synthétiques.

## Diagnostic global et menu d’optimisation — 3 novembre 2026

La page **Optimisation du stockage** regroupe l’inventaire manuel en lecture seule et les deux actions de portée bibliothèque, compression Zstd et purge des objets connus sans référence. Les actions locales restent dans la fiche d’asset. Le diagnostic parcourt les métadonnées et les références, sans vérifier les contenus ni élargir la purge aux orphelins signalés. Aucune migration et aucun nettoyage du workspace ne sont ajoutés.

**94 tests Rust ordinaires passent**, dont 23 de l’adaptateur Tauri, 12 de l’historique et 5 du diagnostic. Les six tests dédiés au diagnostic passent également avec le test opt-in de jonction Windows réellement exécuté hors sandbox. Ils couvrent les références partagées, brut/Zstd simultanés, candidats encore référencés, orphelins hors candidats, objets absents, fichiers inconnus, journaux, catégories additives, exemples plafonnés, annulation et disparition de dossier donnant un rapport partiel. Les fichiers de test restent sous `.test-data/storage-menu-temp`.

Le contrôle de non-écriture compare les contenus persistants par hash, les tailles et dates de modification ainsi que le contenu SQL logique avant/après. Le contenu de `library.sqlite3-shm` est exclu de cette comparaison : les lectures SQLite WAL peuvent modifier ses read-marks volatils, sans modifier les données du catalogue. Les fichiers verrouillés par Windows sont contrôlés par métadonnées lorsqu’une lecture brute est interdite. Le diagnostic n’effectue aucune écriture applicative, aucun hachage ou décompression des contenus, aucune migration ni récupération.

**115 tests frontend passent dans 14 fichiers**. Les nouveaux scénarios vérifient le lancement manuel, la navigation sans bibliothèque, les rapports partiels, la purge bloquée, l’annulation, les réponses tardives, le changement de bibliothèque, la confirmation sans variante et le rapport à actualiser après optimisation. Le typage TypeScript, ESLint, Clippy du workspace et de toutes ses cibles avec `-D warnings`, le format Rust et le contrôle du diff passent. Prettier passe sur les fichiers frontend concernés hors `App.test.tsx`, dont les écarts antérieurs ne sont pas reformattés dans cette évolution.

Le build Tauri debug sans bundle réussit. Le parcours Windows `.test-data/desktop-WtRKBi/report.json` est **`passed` avec 15 groupes de contrôles**. Sur bibliothèque vide avant tout import, il vérifie navigation, lancement manuel, diagnostic IPC sans variante, catégories additives et comparaison des fichiers gérés hors catalogue SQLite et verrou Windows. Les commandes de compression et purge fonctionnent sans variante. Le parcours historique vérifie le déplacement des boutons, le diagnostic après les opérations locales, la portée affichée dans les confirmations globales et le statut à actualiser après optimisation. Le benchmark de galerie de 50 imports est explicitement omis ; les dix allers-retours et le rendu 3D restent vérifiés. Bibliothèque, réglages et profil WebView sont jetables ; aucune bibliothèque personnelle n’est utilisée.

Les captures `storage-empty-diagnostic.png`, `storage-library-diagnostic.png`, `storage-compression-plan.png`, `storage-diagnostic-stale.png` et `storage-purge-plan.png` ont été examinées : menu lisible sur deux lignes, commandes accessibles, séparation des volumes et confirmation de portée globale. L’exécution du pilote localhost et du test de jonction demande l’accès local hors sandbox. Les tests Blender opt-in complets, le benchmark de stockage et l’installation en VM n’ont pas été réexécutés pour cette évolution ; les résultats de la section suivante restent ceux du chantier précédent.

Le binaire final `target/storage-plan/debug/edge-lock.exe` indique les versions fichier et produit **0.2.0**, mesure **24 854 528 octets** et a pour SHA-256 `26DC885FFA141D97C3888AFD963D1E19E6B9DC1D8934A383E6DB3E97AC8C1530`. Aucun installateur n’est produit pour ce contrôle.

## Zstd, historique et restauration — 3 novembre 2026

Le stockage utilise des objets Zstd sans perte lorsque leur compression réduit la taille, tout en lisant les anciens objets bruts. Le schéma 3 conserve les anciennes versions complètes jusqu’à une conversion explicitement confirmée. Les nouveaux historiques conservent le courant et le backup complets, puis les GLB autonomes des versions antérieures. Une version sans aperçu vérifiable reste complète. Aucun nettoyage des fichiers du workspace ni traitement Meshopt n’est ajouté.

### Contrôles exécutés

- **86 tests Rust ordinaires passent**, dont 22 de l’adaptateur Tauri et 10 de l’historique. Les tests couvrent la migration depuis le schéma 1, la sauvegarde préalable du catalogue, la lecture mixte brut/Zstd, la corruption, les limites de décompression, la rotation, les protections entre variantes, l’estimation et la libération d’espace, les plans périmés et la conversion explicite de l’historique ancien.
- Les restaurations sont vérifiées avec ajout/suppression de fichiers non versionnés, retour vers l’état remplacé, écriture concurrente, fichier Windows verrouillé et annulation après publication du journal. Un sous-processus est arrêté aux quatre frontières de publication ; la réouverture récupère un état cohérent. Un backup corrompu après publication conserve le dossier précédent et bloque la purge.
- **106 tests frontend passent dans 13 fichiers**, dont les confirmations de stockage, la nouvelle analyse après plan périmé et la confirmation de fermeture de Blender avant restauration.
- Le typage TypeScript, ESLint, le build Vite, Clippy du workspace et de toutes ses cibles avec `-D warnings`, `cargo fmt --all --check` et le contrôle du diff passent. Prettier passe sur les fichiers frontend modifiés ; le contrôle global signale deux fichiers préexistants non modifiés (`App.test.tsx` et `Icon.tsx`).
- Les suites `blender_integration`, `preview` et `sync`, avec les cas ignorés activés, passent avec **Blender 5.1.1** : 24 tests, dont 17 scénarios nécessitant Blender. Un passage préalable avec Blender 5.2.3 a trouvé une incompatibilité de fixture existante (`eraser_brush` retiré) ; la suite complète n’est donc pas certifiée avec 5.2.
- Le test opt-in de stockage crée 1 000 assets en environ 35,4 s ; la pagination de 50 éléments a une médiane de 18,4 ms et un p95 de 23,5 ms. Une capture synthétique de 128 Mio prend environ 1,70 s, avec un pic mémoire passant de 11,3 à 21,0 Mio. Mesures en debug pendant d’autres contrôles : elles ne prédisent pas le gain de compression de fichiers 3D réels.

Les temporaires Rust utilisent `.test-data/storage-temp`. Le temporaire système et les liens physiques du cache incrémental rencontrent des restrictions dans le sandbox ; les tests passent avec le temporaire isolé du workspace et la copie du cache. Aucun scénario n’utilise la bibliothèque personnelle comme donnée de test.

### Application Windows

Le build Tauri debug sans bundle réussit, avec versions fichier et produit 0.2.0. L’exécutable `target/storage-plan/debug/edge-lock.exe` mesure 24 647 680 octets et son SHA-256 est `D47E87A6FA1FA9FE07037FB88E7830D79CB395D6774E1708585477A211C2124B`. Le parcours WebView2 final `.test-data/desktop-iLyPgQ/report.json` est `passed` : il vérifie notamment la consultation et l’export exact d’un ancien GLB, la restauration du travail non versionné et son retour arrière, la libération du backup, la suppression d’une version et la confirmation de recompression. L’accès localhost du pilote a nécessité une relance hors des restrictions réseau du sandbox, toujours avec bibliothèque, réglages et profil WebView jetables.

Le benchmark desktop de 50 imports est explicitement ignoré ; les dix allers-retours, le rendu des textures et la persistance après redémarrage restent vérifiés. L’inspection visuelle a détecté une case de confirmation étirée ; elle a été alignée sur le composant de case existant. Les captures finales de l’historique et des états coché/décoché de la confirmation ont été examinées ; la case est carrée, les libellés sont lisibles et les boutons de l’historique restent accessibles. Les captures du plan et du résultat de compression du premier passage `.test-data/desktop-1p8wLv` ont également été examinées.

Aucun installateur, déploiement ou test de mise à niveau en VM n’a été produit pour cette évolution. La migration de données est validée sur une bibliothèque synthétique ; les anciens exécutables refusent désormais le schéma 3.

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
