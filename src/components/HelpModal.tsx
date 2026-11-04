import { Modal } from "./Modal";

export function HelpModal({ onClose }: { onClose: () => void }) {
  return (
    <Modal
      title="Aide et mode d’emploi"
      subtitle="Les repères pour créer, versionner et retrouver vos assets."
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
            Aucun GLB n’est généré automatiquement dans le workspace : vous
            gérez ces fichiers. Sans GLB existant, la vignette de bibliothèque
            utilise le .blend principal via le cache interne. Les sélecteurs
            Source de l’aperçu et Source de l’export choisissent indépendamment
            un .blend ou un .glb. Ces choix sont sauvegardés automatiquement par
            variante et version, même après redémarrage.
          </p>
          <p>
            Les sauvegardes .blend1, textures et références sont autorisées.
            Chaque variante garde son dossier indépendant. Tous les fichiers du
            workspace sont versionnables, y compris les anciens GLB gérés.
            L’import conserve le dossier original.
          </p>
          <p>
            L’aperçu n’impose plus de plafond de taille GLB, de géométrie ou de
            textures. Les ressources réelles de la machine et les fonctions glTF
            prises en charge restent déterminantes. Le sélecteur compact se
            trouve sous l’aperçu, entre sa provenance et Actualiser. L’icône
            près de Recentrer affiche l’aperçu en plein écran ; utilisez cette
            icône ou Échap pour revenir à la fiche.
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
            anciennes sauvegardes communes non attribuables sont ignorées et
            préservées sans alerte. Les fichiers bloqués par Windows sont
            conservés pour un nettoyage ultérieur, sans bloquer le retrait du
            catalogue ni les autres opérations. Une reprise est tentée à la
            réouverture ; les dossiers modifiés ou impossibles à identifier
            restent préservés pour intervention manuelle. Fermez les
            applications qui modifient les fichiers concernés.
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
              copie les fichiers et crée la première version sans conversion GLB
              ni installation Blender obligatoire.
            </li>
            <li>
              Depuis la fiche de l’asset, ouvrez le workspace ou le projet dans
              Blender. Modifiez le projet puis enregistrez le .blend.
            </li>
            <li>
              Gardez Edge Lock ouvert : il détecte les sauvegardes. Les aperçus
              sont préparés dans le cache interne, sans modifier votre .blend ni
              vos GLB de travail.
            </li>
            <li>
              Après détection des modifications, créez une version pour
              conserver volontairement cet état, même si l’aperçu échoue.
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
            Renommer la version change son libellé dans tous les historiques qui
            la partagent, sans changer sa date ni ses fichiers. Renommer la
            variante conserve son chemin de travail. Supprimer la variante
            demande confirmation et retire son workspace et ses versions
            exclusives ; les données partagées restent disponibles. La dernière
            variante est protégée. Un nettoyage différé des fichiers verrouillés
            est repris à la réouverture.
          </p>
          <p>
            Le courant et son backup sont normalement conservés complets ; les
            versions plus anciennes peuvent ne conserver que tous leurs GLB de
            consultation, au choix. Ces versions ne permettent plus de reprendre
            l’édition Blender. Sans GLB enregistré valide, les sources complètes
            sont conservées.
          </p>
          <p>
            Pour restaurer un backup, enregistrez et fermez le projet dans
            Blender, puis confirmez. Le workspace est remplacé et son état
            précédent devient le nouveau backup.
          </p>
          <p>
            Annuler les modifications, sous Créer une version, rétablit la
            dernière version créée de la variante active, fichiers et réglages
            compris. Après fermeture des applications d’écriture et
            confirmation, les ajouts et modifications non versionnés sont
            définitivement supprimés et les fichiers supprimés sont rétablis.
            Aucun nouveau backup ni aucune version n’est créé. Cette action
            reste disponible en cas d’échec de synchronisation.
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
            Si l’aperçu échoue, vérifiez le fichier principal et les textures ou
            dépendances utilisées, puis actualisez l’aperçu. Une erreur laisse
            les fichiers et le versionnement accessibles.
          </p>
          <p>
            Une erreur de conversion conserve le dernier aperçu. Vous pouvez
            choisir une autre source d’aperçu ou exporter un GLB existant. Si
            une source mémorisée a disparu, sélectionnez un autre fichier. Les
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
