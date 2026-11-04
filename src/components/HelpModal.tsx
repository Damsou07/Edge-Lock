import { Modal } from "./Modal";

export function HelpModal({ onClose }: { onClose: () => void }) {
  return (
    <Modal
      title="Aide et mode d’emploi"
      subtitle="Les repères pour créer, synchroniser et retrouver vos assets."
      onClose={onClose}
      wide
    >
      <div className="help-content">
        <nav className="help-contents" aria-label="Sommaire de l’aide">
          <a href="#help-rule">Sources et aperçus</a>
          <a href="#help-start">Premiers pas</a>
          <a href="#help-history">Versions et variantes</a>
          <a href="#help-files">Fichiers et dépannage</a>
        </nav>
        <section className="help-rule" id="help-rule" tabIndex={-1}>
          <h3>Plusieurs modèles par workspace</h3>
          <p>
            <strong>
              Un workspace peut contenir plusieurs .blend et .glb, sous-dossiers
              compris. L’import nécessite au moins un .blend.
            </strong>{" "}
            La casse des extensions ne change pas cette règle.
          </p>
          <p>
            Edge Lock déduit un fichier principal à partir de la taille, de la
            date, du nom et de son emplacement. Ce choix reste stable ;
            changez-le avec Fichier principal ou Re-détecter automatiquement.
          </p>
          <p>
            Le principal est synchronisé automatiquement. Les sélecteurs Source
            de l’aperçu et Source de l’export permettent de choisir
            indépendamment un .blend ou un .glb. Les autres projets Blender sont
            préparés à la demande. L’import conserve le dossier original et les
            GLB utilisateur.
          </p>
          <p>
            Les sauvegardes .blend1, textures et références sont autorisées.
            Chaque variante garde son dossier indépendant. Une régénération du
            seul GLB géré ne crée pas de changement à versionner ; un GLB
            utilisateur ajouté ou modifié compte bien comme un changement.
          </p>
          <p>
            L’aperçu n’impose plus de plafond de taille GLB, de géométrie ou de
            textures. Les ressources réelles de la machine et les fonctions glTF
            prises en charge restent déterminantes.
          </p>
        </section>
        <section>
          <h3>Comprendre votre bibliothèque</h3>
          <p>
            Edge Lock organise des dossiers d’assets 3D sur votre ordinateur. La
            bibliothèque regroupe les assets ; un asset peut avoir plusieurs
            variantes indépendantes. Chaque variante possède un workspace stable
            : le dossier dans lequel vous travaillez.
          </p>
          <p>
            Catégories, tags et projets de jeu servent à retrouver vos
            créations. Archiver un asset le retire de la vue courante tout en
            conservant ses fichiers et ses versions. Une seule instance peut
            ouvrir une bibliothèque ; utilisez un disque local.
          </p>
          <p>
            Depuis Archives, Supprimer définitivement efface les fichiers,
            variantes et versions attribuables à un asset. Vider les archives
            concerne toutes les archives, même filtrées, après confirmation. Les
            objets partagés restent conservés pour les autres assets ; les
            anciennes sauvegardes communes non attribuables sont signalées et
            préservées. Enregistrez et fermez les fichiers concernés dans
            Blender.
          </p>
          <p>
            Supprimer une catégorie retire aussi ses sous-catégories et archive
            leurs assets. Supprimer un projet de jeu archive tous ses assets,
            même liés à d’autres projets, en conservant ces autres associations.
          </p>
        </section>
        <section id="help-start" tabIndex={-1}>
          <h3>Démarrer et travailler dans Blender</h3>
          <ol>
            <li>
              Choisissez un dossier vide pour créer une bibliothèque, ou ouvrez
              une bibliothèque existante. Dans Réglages, sélectionnez
              blender.exe.
            </li>
            <li>
              Importez un dossier conforme ou utilisez Nouveau projet. L’import
              crée la première version après une génération GLB réussie.
            </li>
            <li>
              Depuis la fiche de l’asset, ouvrez le workspace ou le projet dans
              Blender. Modifiez le projet puis enregistrez le .blend.
            </li>
            <li>
              Gardez Edge Lock ouvert : il détecte la sauvegarde et synchronise
              le GLB de la scène active enregistrée, avec ses modificateurs
              actifs. Le worker ne réenregistre pas votre .blend.
            </li>
            <li>
              Après synchronisation, créez une version pour conserver
              volontairement cet état. Modifier seulement le GLB géré ne crée
              pas de changement source à versionner.
            </li>
          </ol>
          <p>
            Le dernier aperçu reste visible pendant le traitement et en cas
            d’échec. Certains matériaux procéduraux et certaines fonctions
            propres à Blender demandent une préparation compatible glTF.
          </p>
        </section>
        <section id="help-history" tabIndex={-1}>
          <h3>Versions, variantes et sauvegardes</h3>
          <p>
            Les versions sont manuelles et immuables. Dans Historique, consultez
            un état ou exportez son GLB enregistré. Créer une variante depuis
            une version complète produit un nouveau workspace indépendant.
          </p>
          <p>
            Le courant et son backup sont normalement conservés complets ; les
            versions plus anciennes peuvent ne conserver que tous leurs GLB de
            consultation, au choix. Ces versions ne permettent plus de reprendre
            l’édition Blender.
          </p>
          <p>
            Pour restaurer un backup, enregistrez et fermez le projet dans
            Blender, puis confirmez. Le workspace est remplacé et son état
            précédent devient le nouveau backup.
          </p>
          <p>
            Optimisation du stockage regroupe un diagnostic manuel, la
            compression Zstd sans perte et la purge des objets internes
            inutilisés. Les opérations qui libèrent des données présentent leur
            portée avant confirmation ; elles ne nettoient pas les fichiers de
            votre workspace.
          </p>
          <p>
            <strong>
              L’historique local ne remplace pas une sauvegarde externe.
            </strong>{" "}
            Fermez Edge Lock puis copiez la bibliothèque complète sur un autre
            support.
          </p>
        </section>
        <section id="help-files" tabIndex={-1}>
          <h3>Fichiers et dépannage</h3>
          <p>
            Dans la fiche, l’onglet Fichiers filtre le workspace ou la version
            consultée par type et par taille minimum/maximum. Les bornes sont
            inclusives ; 1 Ko = 1 024 octets. Un filtre de taille exclut les
            dossiers. Réinitialiser les filtres retrouve la liste complète.
          </p>
          <p>
            Si la synchronisation échoue, lisez le diagnostic dans la fiche,
            vérifiez le fichier principal et les textures ou dépendances
            utilisées, puis relancez la synchronisation. Une erreur ne remplace
            pas le dernier GLB valide et laisse le .blend accessible.
          </p>
          <p>
            Une erreur de génération conserve le dernier GLB valide. Vous pouvez
            choisir une autre source d’aperçu ou exporter un GLB existant. Les
            versions complètes conservent les sources ; les versions allégées
            proposent uniquement les GLB enregistrés.
          </p>
        </section>
      </div>
      <footer className="modal-actions">
        <button type="button" className="button primary" onClick={onClose}>
          Terminé
        </button>
      </footer>
    </Modal>
  );
}
