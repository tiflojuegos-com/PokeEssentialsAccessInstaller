use std::collections::HashMap;

const ERR_SEP: char = '\u{1}';

/// The languages the installer speaks, in the order the options dialog lists them. Spanish is the
/// reference table every other one is checked against and the fallback for a missing key.
pub const LANGS: [&str; 6] = ["es", "en", "fr", "pt", "de", "pl"];

/// Packs an i18n key together with its argument so the core layers can raise
/// translatable errors without knowing the active language.
pub fn err_key(key: &str, arg: &str) -> String {
    format!("{}{}{}", key, ERR_SEP, arg)
}

pub struct I18n {
    lang: String,
    tables: HashMap<&'static str, HashMap<&'static str, &'static str>>,
}

fn known(lang: &str) -> String {
    if LANGS.contains(&lang) {
        lang.to_string()
    } else {
        "es".to_string()
    }
}

impl I18n {
    pub fn new(lang: &str) -> I18n {
        let mut tables = HashMap::new();
        tables.insert("es", es_map());
        tables.insert("en", en_map());
        tables.insert("fr", fr_map());
        tables.insert("pt", pt_map());
        tables.insert("de", de_map());
        tables.insert("pl", pl_map());
        I18n { lang: known(lang), tables }
    }

    pub fn set_lang(&mut self, lang: &str) {
        self.lang = known(lang);
    }

    pub fn t(&self, key: &str) -> String {
        self.tables
            .get(self.lang.as_str())
            .and_then(|m| m.get(key))
            .or_else(|| self.tables.get("es").and_then(|m| m.get(key)))
            .copied()
            .unwrap_or(key)
            .to_string()
    }

    pub fn tf(&self, key: &str, arg: &str) -> String {
        self.t(key).replace("{}", arg)
    }

    /// Resolves an error raised by the core layers: a bare i18n key, a key packed
    /// with its argument by `err_key`, or a literal message that passes through.
    pub fn t_err(&self, payload: &str) -> String {
        match payload.split_once(ERR_SEP) {
            Some((key, arg)) => self.tf(key, arg),
            None => self.t(payload),
        }
    }
}

fn es_map() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert("app_title", "PokeEssentialsAccess - Instalador");
    m.insert("my_games", "Mis juegos");
    m.insert("install", "Instalar");
    m.insert("uninstall", "Desinstalar");
    m.insert("remove_from_list", "Quitar de la lista");
    m.insert("status_uptodate", "Al día");
    m.insert("status_update", "Actualización disponible");
    m.insert("status_notinstalled", "No instalado");
    m.insert("status_outdated", "Desfasado o dañado");
    m.insert("status_unknown", "Versión desconocida (sin conexión)");
    m.insert("err_rate_limited", "GitHub ha limitado las peticiones. Espera unos {} minutos y vuelve a intentarlo.");
    m.insert("err_rate_limited_short", "GitHub ha limitado las peticiones. Espera un rato y vuelve a intentarlo.");
    m.insert("err_download", "No se pudo descargar: {}");
    m.insert("err_download_status", "La descarga falló (estado {}).");
    m.insert("err_tree_truncated", "La lista de archivos del mod es demasiado grande para GitHub (respuesta truncada). Avisa al autor del mod.");
    m.insert("err_list_files", "No pude obtener la lista de archivos del mod: {}");
    m.insert("err_io_mkdir", "No pude crear la carpeta de {}");
    m.insert("err_io_write", "No pude escribir {}");
    m.insert("err_io_delete", "No pude borrar {}");
    m.insert("err_io_read", "No pude leer {}");
    m.insert("err_io_create", "No pude crear {}");
    m.insert("err_selfupdate_download", "No pude descargar el instalador nuevo: {}");
    m.insert("err_selfupdate_invalid", "La descarga del instalador no es un ejecutable válido.");
    m.insert("err_selfupdate_replace", "No pude reemplazar el instalador: {}");
    m.insert("game_folder_missing", "No encuentro la carpeta del juego: {}. ¿La has movido o renombrado? Quítala de la lista y vuelve a añadirla.");
    m.insert("status_missing", "Carpeta no encontrada");
    m.insert("profile_label", "perfil: {}");
    m.insert("profile_generic", "genérico");
    m.insert("pick_folder", "Elige la carpeta del juego");
    m.insert("not_compatible", "Este juego no es compatible (no usa mkxp-z).");
    m.insert("detected_profile", "Detectado el perfil para {}. ¿Cómo quieres instalarlo?");
    m.insert("installed_profile", "Este juego ya tiene instalado el perfil de {}. ¿Cómo quieres instalarlo?");
    m.insert("install_specific", "Instalar perfil de {}");
    m.insert("install_generic", "Instalar perfil genérico");
    m.insert("generic_hint", "Usa el genérico si tu versión del juego difiere de la soportada.");
    m.insert("not_detected", "No he reconocido el juego. Elige el perfil:");
    m.insert("choose_manual", "Elegir perfil manualmente...");
    m.insert("manual_profile_title", "Elige el perfil del juego:");
    m.insert("installing", "Instalando: {}");
    m.insert("downloading_file", "Descargando {}");
    m.insert("done_installed", "Instalado correctamente ({}).");
    m.insert("done_uninstalled", "Desinstalado.");
    m.insert("error", "Error: {}");
    m.insert("confirm_uninstall", "¿Quitar el mod de este juego?");
    m.insert("language", "Idioma");
    m.insert("no_selection", "Selecciona un juego de la lista primero.");
    m.insert("updating_all", "Actualizando todos los juegos instalados...");
    m.insert("nothing_to_update", "No hay juegos instalados que actualizar.");
    m.insert("profile_changed", "Perfil cambiado a {}. Reinstalando...");
    m.insert("lang_es", "Español");
    m.insert("lang_en", "Inglés");
    m.insert("lang_fr", "Francés");
    m.insert("lang_pt", "Portugués");
    m.insert("lang_de", "Alemán");
    m.insert("lang_pl", "Polaco");
    m.insert("lang_changed", "Idioma cambiado. Reinicia el instalador para verlo del todo.");
    m.insert("launcher_update_title", "Actualización del instalador");
    m.insert("launcher_update_available", "Hay una versión nueva del instalador ({}). ¿Quieres actualizar?");
    m.insert("launcher_update_none", "El instalador ya está actualizado.");
    m.insert("launcher_update_applied", "Instalador actualizado. Se reiniciará ahora.");
    m.insert("check_launcher_update", "Buscar actualización del instalador");
    m.insert("log_label", "Registro de actividad");
    m.insert("progress", "Progreso");
    m.insert("working_wait", "Trabajando, espera un momento...");
    m.insert("ready", "Listo");
    m.insert("checking_games", "Comprobando los archivos del juego, un momento...");
    m.insert("loading_catalog", "Cargando lista de juegos...");
    m.insert("choose_profile_title", "Elegir perfil");
    m.insert("worker_crashed", "La instalación se interrumpió de forma inesperada.");
    m.insert("restart_failed", "Actualizado, pero no pude reiniciar: {}. Abre el instalador de nuevo manualmente.");
    m.insert("confirm_remove", "¿Quitar este juego de la lista? No borra el mod del juego.");
    m.insert("removed_from_list", "Juego quitado de la lista.");
    m.insert("add_game_btn", "Añadir juego (Ctrl+A)");
    m.insert("install_btn", "Instalar o actualizar (Ctrl+I)");
    m.insert("update_all_btn", "Actualizar todos (Ctrl+U)");
    m.insert("change_profile_btn", "Cambiar perfil (Ctrl+P)");
    m.insert("uninstall_btn", "Desinstalar (Ctrl+D)");
    m.insert("remove_from_list_btn", "Quitar de la lista (Ctrl+Q)");
    m.insert("options_btn", "Opciones (Ctrl+O)");
    m.insert("check_launcher_update_btn", "Buscar actualización del instalador (Ctrl+B)");
    m.insert("ask_retry", "Ha fallado. ¿Quieres reintentarlo?");
    m.insert("retrying", "Reintentando...");
    m.insert("no_write_perm", "No tengo permiso para escribir en esa carpeta. Puede estar protegida, ser de solo lectura o estar en OneDrive. Mueve el juego a otra ubicación (por ejemplo tu carpeta de usuario) e inténtalo de nuevo.");
    m.insert("launcher_update_ready", "Hay una versión nueva del instalador ({}). Usa el botón Buscar actualización del instalador para instalarla.");
    m.insert("ready_offline", "Listo, pero sin conexión: no he podido comprobar la versión del mod ni la lista de juegos.");
    m.insert("game_running", "Parece que {} está abierto. Cierra el juego y vuelve a intentarlo.");
    m.insert("preload_missing_warn", "No detecto soporte de preloadScript en el ejecutable; el mod podría no funcionar. ¿Instalar igualmente?");
    m.insert("err_launcher_too_old", "Este mod requiere un instalador más nuevo (mínimo {}). Actualízalo con el botón Buscar actualización del instalador.");
    m.insert("err_download_corrupt", "La descarga de {} llegó dañada o el mod cambió mientras se instalaba. Vuelve a intentarlo.");
    m.insert("err_write_locked", "No he podido escribir {}: el archivo está bloqueado. Cierra el juego y vuelve a intentarlo.");
    m.insert("err_mkxp_no_root", "El mkxp.json de este juego no tiene un objeto JSON válido, así que no puedo registrar el mod. Revísalo o bórralo y vuelve a intentarlo.");
    m.insert("err_profile_missing", "El perfil {} ya no existe en el mod, así que este juego se quedaría sin sus pantallas. Elige otro con el botón Cambiar perfil, o el genérico.");
    m
}

fn en_map() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert("app_title", "PokeEssentialsAccess - Installer");
    m.insert("my_games", "My games");
    m.insert("install", "Install");
    m.insert("uninstall", "Uninstall");
    m.insert("remove_from_list", "Remove from list");
    m.insert("status_uptodate", "Up to date");
    m.insert("status_update", "Update available");
    m.insert("status_notinstalled", "Not installed");
    m.insert("status_outdated", "Outdated or broken");
    m.insert("status_unknown", "Unknown version (offline)");
    m.insert("err_rate_limited", "GitHub is rate-limiting requests. Wait about {} minutes and try again.");
    m.insert("err_rate_limited_short", "GitHub is rate-limiting requests. Wait a while and try again.");
    m.insert("err_download", "Download failed: {}");
    m.insert("err_download_status", "The download failed (status {}).");
    m.insert("err_tree_truncated", "The mod's file list is too large for GitHub (truncated response). Tell the mod's author.");
    m.insert("err_list_files", "I couldn't get the mod's file list: {}");
    m.insert("err_io_mkdir", "I couldn't create the folder for {}");
    m.insert("err_io_write", "I couldn't write {}");
    m.insert("err_io_delete", "I couldn't delete {}");
    m.insert("err_io_read", "I couldn't read {}");
    m.insert("err_io_create", "I couldn't create {}");
    m.insert("err_selfupdate_download", "I couldn't download the new installer: {}");
    m.insert("err_selfupdate_invalid", "The downloaded installer is not a valid executable.");
    m.insert("err_selfupdate_replace", "I couldn't replace the installer: {}");
    m.insert("game_folder_missing", "I can't find the game folder: {}. Did you move or rename it? Remove it from the list and add it again.");
    m.insert("status_missing", "Folder not found");
    m.insert("profile_label", "profile: {}");
    m.insert("profile_generic", "generic");
    m.insert("pick_folder", "Choose the game folder");
    m.insert("not_compatible", "This game is not compatible (not mkxp-z).");
    m.insert("detected_profile", "Detected the profile for {}. How do you want to install it?");
    m.insert("installed_profile", "This game already has the {} profile installed. How do you want to install it?");
    m.insert("install_specific", "Install the {} profile");
    m.insert("install_generic", "Install the generic profile");
    m.insert("generic_hint", "Use the generic one if your game version differs from the supported one.");
    m.insert("not_detected", "I didn't recognize the game. Choose the profile:");
    m.insert("choose_manual", "Choose a profile manually...");
    m.insert("manual_profile_title", "Pick the game's profile:");
    m.insert("installing", "Installing: {}");
    m.insert("downloading_file", "Downloading {}");
    m.insert("done_installed", "Installed successfully ({}).");
    m.insert("done_uninstalled", "Uninstalled.");
    m.insert("error", "Error: {}");
    m.insert("confirm_uninstall", "Remove the mod from this game?");
    m.insert("language", "Language");
    m.insert("no_selection", "Select a game from the list first.");
    m.insert("updating_all", "Updating all installed games...");
    m.insert("nothing_to_update", "No installed games to update.");
    m.insert("profile_changed", "Profile changed to {}. Reinstalling...");
    m.insert("lang_es", "Spanish");
    m.insert("lang_en", "English");
    m.insert("lang_fr", "French");
    m.insert("lang_pt", "Portuguese");
    m.insert("lang_de", "German");
    m.insert("lang_pl", "Polish");
    m.insert("lang_changed", "Language changed. Restart the installer to fully apply it.");
    m.insert("launcher_update_title", "Installer update");
    m.insert("launcher_update_available", "A new installer version is available ({}). Update now?");
    m.insert("launcher_update_none", "The installer is up to date.");
    m.insert("launcher_update_applied", "Installer updated. It will restart now.");
    m.insert("check_launcher_update", "Check for installer update");
    m.insert("log_label", "Activity log");
    m.insert("progress", "Progress");
    m.insert("working_wait", "Working, please wait...");
    m.insert("ready", "Ready");
    m.insert("checking_games", "Checking the game files, one moment...");
    m.insert("loading_catalog", "Loading the game list...");
    m.insert("choose_profile_title", "Choose profile");
    m.insert("worker_crashed", "The installation stopped unexpectedly.");
    m.insert("restart_failed", "Updated, but I couldn't restart: {}. Please open the installer again manually.");
    m.insert("confirm_remove", "Remove this game from the list? It won't delete the mod from the game.");
    m.insert("removed_from_list", "Game removed from the list.");
    m.insert("add_game_btn", "Add game (Ctrl+A)");
    m.insert("install_btn", "Install or update (Ctrl+I)");
    m.insert("update_all_btn", "Update all (Ctrl+U)");
    m.insert("change_profile_btn", "Change profile (Ctrl+P)");
    m.insert("uninstall_btn", "Uninstall (Ctrl+D)");
    m.insert("remove_from_list_btn", "Remove from list (Ctrl+Q)");
    m.insert("options_btn", "Options (Ctrl+O)");
    m.insert("check_launcher_update_btn", "Check for installer update (Ctrl+B)");
    m.insert("ask_retry", "It failed. Do you want to retry?");
    m.insert("retrying", "Retrying...");
    m.insert("no_write_perm", "I can't write to that folder. It may be protected, read-only, or in OneDrive. Move the game somewhere else (for example your user folder) and try again.");
    m.insert("launcher_update_ready", "A new installer version is available ({}). Use the Check for installer update button to install it.");
    m.insert("ready_offline", "Ready, but offline: I couldn't check the mod version or the game list.");
    m.insert("game_running", "It looks like {} is running. Close the game and try again.");
    m.insert("preload_missing_warn", "I can't find preloadScript support in the executable; the mod might not work. Install anyway?");
    m.insert("err_launcher_too_old", "This mod needs a newer installer (at least {}). Update it with the Check for installer update button.");
    m.insert("err_download_corrupt", "The download of {} arrived damaged, or the mod changed mid-install. Please try again.");
    m.insert("err_write_locked", "I couldn't write {}: the file is locked. Close the game and try again.");
    m.insert("err_mkxp_no_root", "This game's mkxp.json has no valid JSON object, so I can't register the mod. Fix it or delete it and try again.");
    m.insert("err_profile_missing", "The {} profile is no longer in the mod, so this game would end up without its own screens. Pick another one with the Change profile button, or the generic one.");
    m
}

fn fr_map() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert("app_title", "PokeEssentialsAccess - Installateur");
    m.insert("my_games", "Mes jeux");
    m.insert("install", "Installer");
    m.insert("uninstall", "Désinstaller");
    m.insert("remove_from_list", "Retirer de la liste");
    m.insert("status_uptodate", "À jour");
    m.insert("status_update", "Mise à jour disponible");
    m.insert("status_notinstalled", "Non installé");
    m.insert("status_outdated", "Obsolète ou endommagé");
    m.insert("status_unknown", "Version inconnue (hors ligne)");
    m.insert("err_rate_limited", "GitHub limite les requêtes. Attends environ {} minutes et réessaie.");
    m.insert("err_rate_limited_short", "GitHub limite les requêtes. Attends un peu et réessaie.");
    m.insert("err_download", "Téléchargement impossible : {}");
    m.insert("err_download_status", "Le téléchargement a échoué (état {}).");
    m.insert("err_tree_truncated", "La liste des fichiers du mod est trop grande pour GitHub (réponse tronquée). Préviens l'auteur du mod.");
    m.insert("err_list_files", "Impossible d'obtenir la liste des fichiers du mod : {}");
    m.insert("err_io_mkdir", "Impossible de créer le dossier de {}");
    m.insert("err_io_write", "Impossible d'écrire {}");
    m.insert("err_io_delete", "Impossible de supprimer {}");
    m.insert("err_io_read", "Impossible de lire {}");
    m.insert("err_io_create", "Impossible de créer {}");
    m.insert("err_selfupdate_download", "Impossible de télécharger le nouvel installateur : {}");
    m.insert("err_selfupdate_invalid", "L'installateur téléchargé n'est pas un exécutable valide.");
    m.insert("err_selfupdate_replace", "Impossible de remplacer l'installateur : {}");
    m.insert("game_folder_missing", "Je ne trouve pas le dossier du jeu : {}. L'as-tu déplacé ou renommé ? Retire-le de la liste et ajoute-le à nouveau.");
    m.insert("status_missing", "Dossier introuvable");
    m.insert("profile_label", "profil : {}");
    m.insert("profile_generic", "générique");
    m.insert("pick_folder", "Choisis le dossier du jeu");
    m.insert("not_compatible", "Ce jeu n'est pas compatible (il n'utilise pas mkxp-z).");
    m.insert("detected_profile", "Profil détecté pour {}. Comment veux-tu l'installer ?");
    m.insert("installed_profile", "Ce jeu a déjà le profil de {} installé. Comment veux-tu l'installer ?");
    m.insert("install_specific", "Installer le profil de {}");
    m.insert("install_generic", "Installer le profil générique");
    m.insert("generic_hint", "Utilise le générique si ta version du jeu diffère de celle prise en charge.");
    m.insert("not_detected", "Je n'ai pas reconnu le jeu. Choisis le profil :");
    m.insert("choose_manual", "Choisir un profil manuellement...");
    m.insert("manual_profile_title", "Choisis le profil du jeu :");
    m.insert("installing", "Installation : {}");
    m.insert("downloading_file", "Téléchargement de {}");
    m.insert("done_installed", "Installé correctement ({}).");
    m.insert("done_uninstalled", "Désinstallé.");
    m.insert("error", "Erreur : {}");
    m.insert("confirm_uninstall", "Retirer le mod de ce jeu ?");
    m.insert("language", "Langue");
    m.insert("no_selection", "Sélectionne d'abord un jeu dans la liste.");
    m.insert("updating_all", "Mise à jour de tous les jeux installés...");
    m.insert("nothing_to_update", "Aucun jeu installé à mettre à jour.");
    m.insert("profile_changed", "Profil changé pour {}. Réinstallation...");
    m.insert("lang_es", "Espagnol");
    m.insert("lang_en", "Anglais");
    m.insert("lang_fr", "Français");
    m.insert("lang_pt", "Portugais");
    m.insert("lang_de", "Allemand");
    m.insert("lang_pl", "Polonais");
    m.insert("lang_changed", "Langue changée. Redémarre l'installateur pour l'appliquer complètement.");
    m.insert("launcher_update_title", "Mise à jour de l'installateur");
    m.insert("launcher_update_available", "Une nouvelle version de l'installateur est disponible ({}). Mettre à jour maintenant ?");
    m.insert("launcher_update_none", "L'installateur est déjà à jour.");
    m.insert("launcher_update_applied", "Installateur mis à jour. Il va redémarrer maintenant.");
    m.insert("check_launcher_update", "Rechercher une mise à jour de l'installateur");
    m.insert("log_label", "Journal d'activité");
    m.insert("progress", "Progression");
    m.insert("working_wait", "Travail en cours, un instant...");
    m.insert("ready", "Prêt");
    m.insert("checking_games", "Vérification des fichiers du jeu, un instant...");
    m.insert("loading_catalog", "Chargement de la liste des jeux...");
    m.insert("choose_profile_title", "Choisir le profil");
    m.insert("worker_crashed", "L'installation s'est interrompue de façon inattendue.");
    m.insert("restart_failed", "Mis à jour, mais impossible de redémarrer : {}. Rouvre l'installateur manuellement.");
    m.insert("confirm_remove", "Retirer ce jeu de la liste ? Le mod n'est pas supprimé du jeu.");
    m.insert("removed_from_list", "Jeu retiré de la liste.");
    m.insert("add_game_btn", "Ajouter un jeu (Ctrl+A)");
    m.insert("install_btn", "Installer ou mettre à jour (Ctrl+I)");
    m.insert("update_all_btn", "Tout mettre à jour (Ctrl+U)");
    m.insert("change_profile_btn", "Changer de profil (Ctrl+P)");
    m.insert("uninstall_btn", "Désinstaller (Ctrl+D)");
    m.insert("remove_from_list_btn", "Retirer de la liste (Ctrl+Q)");
    m.insert("options_btn", "Options (Ctrl+O)");
    m.insert("check_launcher_update_btn", "Rechercher une mise à jour de l'installateur (Ctrl+B)");
    m.insert("ask_retry", "Échec. Veux-tu réessayer ?");
    m.insert("retrying", "Nouvelle tentative...");
    m.insert("no_write_perm", "Je n'ai pas la permission d'écrire dans ce dossier. Il est peut-être protégé, en lecture seule ou dans OneDrive. Déplace le jeu ailleurs (par exemple dans ton dossier utilisateur) et réessaie.");
    m.insert("launcher_update_ready", "Une nouvelle version de l'installateur est disponible ({}). Utilise le bouton Rechercher une mise à jour de l'installateur pour l'installer.");
    m.insert("ready_offline", "Prêt, mais hors ligne : impossible de vérifier la version du mod et la liste des jeux.");
    m.insert("game_running", "Il semble que {} soit ouvert. Ferme le jeu et réessaie.");
    m.insert("preload_missing_warn", "Je ne détecte pas la prise en charge de preloadScript dans l'exécutable ; le mod pourrait ne pas fonctionner. Installer quand même ?");
    m.insert("err_launcher_too_old", "Ce mod nécessite un installateur plus récent (au minimum {}). Mets-le à jour avec le bouton Rechercher une mise à jour de l'installateur.");
    m.insert("err_download_corrupt", "Le téléchargement de {} est arrivé endommagé, ou le mod a changé pendant l'installation. Réessaie.");
    m.insert("err_write_locked", "Impossible d'écrire {} : le fichier est verrouillé. Ferme le jeu et réessaie.");
    m.insert("err_mkxp_no_root", "Le mkxp.json de ce jeu ne contient pas d'objet JSON valide, je ne peux donc pas enregistrer le mod. Corrige-le ou supprime-le, puis réessaie.");
    m.insert("err_profile_missing", "Le profil {} n'existe plus dans le mod, ce jeu resterait donc sans ses écrans. Choisis-en un autre avec le bouton Changer de profil, ou le générique.");
    m
}

fn pt_map() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert("app_title", "PokeEssentialsAccess - Instalador");
    m.insert("my_games", "Meus jogos");
    m.insert("install", "Instalar");
    m.insert("uninstall", "Desinstalar");
    m.insert("remove_from_list", "Remover da lista");
    m.insert("status_uptodate", "Atualizado");
    m.insert("status_update", "Atualização disponível");
    m.insert("status_notinstalled", "Não instalado");
    m.insert("status_outdated", "Desatualizado ou danificado");
    m.insert("status_unknown", "Versão desconhecida (sem conexão)");
    m.insert("err_rate_limited", "O GitHub limitou as solicitações. Aguarde uns {} minutos e tente de novo.");
    m.insert("err_rate_limited_short", "O GitHub limitou as solicitações. Aguarde um pouco e tente de novo.");
    m.insert("err_download", "Não foi possível baixar: {}");
    m.insert("err_download_status", "O download falhou (status {}).");
    m.insert("err_tree_truncated", "A lista de arquivos do mod é grande demais para o GitHub (resposta truncada). Avise o autor do mod.");
    m.insert("err_list_files", "Não consegui obter a lista de arquivos do mod: {}");
    m.insert("err_io_mkdir", "Não consegui criar a pasta de {}");
    m.insert("err_io_write", "Não consegui escrever {}");
    m.insert("err_io_delete", "Não consegui apagar {}");
    m.insert("err_io_read", "Não consegui ler {}");
    m.insert("err_io_create", "Não consegui criar {}");
    m.insert("err_selfupdate_download", "Não consegui baixar o novo instalador: {}");
    m.insert("err_selfupdate_invalid", "O instalador baixado não é um executável válido.");
    m.insert("err_selfupdate_replace", "Não consegui substituir o instalador: {}");
    m.insert("game_folder_missing", "Não encontro a pasta do jogo: {}. Você a moveu ou renomeou? Remova-a da lista e adicione-a de novo.");
    m.insert("status_missing", "Pasta não encontrada");
    m.insert("profile_label", "perfil: {}");
    m.insert("profile_generic", "genérico");
    m.insert("pick_folder", "Escolha a pasta do jogo");
    m.insert("not_compatible", "Este jogo não é compatível (não usa mkxp-z).");
    m.insert("detected_profile", "Perfil detectado para {}. Como você quer instalá-lo?");
    m.insert("installed_profile", "Este jogo já tem o perfil de {} instalado. Como você quer instalá-lo?");
    m.insert("install_specific", "Instalar o perfil de {}");
    m.insert("install_generic", "Instalar o perfil genérico");
    m.insert("generic_hint", "Use o genérico se a sua versão do jogo for diferente da suportada.");
    m.insert("not_detected", "Não reconheci o jogo. Escolha o perfil:");
    m.insert("choose_manual", "Escolher perfil manualmente...");
    m.insert("manual_profile_title", "Escolha o perfil do jogo:");
    m.insert("installing", "Instalando: {}");
    m.insert("downloading_file", "Baixando {}");
    m.insert("done_installed", "Instalado com sucesso ({}).");
    m.insert("done_uninstalled", "Desinstalado.");
    m.insert("error", "Erro: {}");
    m.insert("confirm_uninstall", "Remover o mod deste jogo?");
    m.insert("language", "Idioma");
    m.insert("no_selection", "Selecione primeiro um jogo da lista.");
    m.insert("updating_all", "Atualizando todos os jogos instalados...");
    m.insert("nothing_to_update", "Não há jogos instalados para atualizar.");
    m.insert("profile_changed", "Perfil alterado para {}. Reinstalando...");
    m.insert("lang_es", "Espanhol");
    m.insert("lang_en", "Inglês");
    m.insert("lang_fr", "Francês");
    m.insert("lang_pt", "Português");
    m.insert("lang_de", "Alemão");
    m.insert("lang_pl", "Polonês");
    m.insert("lang_changed", "Idioma alterado. Reinicie o instalador para aplicar por completo.");
    m.insert("launcher_update_title", "Atualização do instalador");
    m.insert("launcher_update_available", "Há uma nova versão do instalador ({}). Quer atualizar agora?");
    m.insert("launcher_update_none", "O instalador já está atualizado.");
    m.insert("launcher_update_applied", "Instalador atualizado. Ele vai reiniciar agora.");
    m.insert("check_launcher_update", "Procurar atualização do instalador");
    m.insert("log_label", "Registro de atividade");
    m.insert("progress", "Progresso");
    m.insert("working_wait", "Trabalhando, aguarde um momento...");
    m.insert("ready", "Pronto");
    m.insert("checking_games", "Verificando os arquivos do jogo, um momento...");
    m.insert("loading_catalog", "Carregando a lista de jogos...");
    m.insert("choose_profile_title", "Escolher perfil");
    m.insert("worker_crashed", "A instalação foi interrompida de forma inesperada.");
    m.insert("restart_failed", "Atualizado, mas não consegui reiniciar: {}. Abra o instalador de novo manualmente.");
    m.insert("confirm_remove", "Remover este jogo da lista? O mod não é apagado do jogo.");
    m.insert("removed_from_list", "Jogo removido da lista.");
    m.insert("add_game_btn", "Adicionar jogo (Ctrl+A)");
    m.insert("install_btn", "Instalar ou atualizar (Ctrl+I)");
    m.insert("update_all_btn", "Atualizar todos (Ctrl+U)");
    m.insert("change_profile_btn", "Mudar perfil (Ctrl+P)");
    m.insert("uninstall_btn", "Desinstalar (Ctrl+D)");
    m.insert("remove_from_list_btn", "Remover da lista (Ctrl+Q)");
    m.insert("options_btn", "Opções (Ctrl+O)");
    m.insert("check_launcher_update_btn", "Procurar atualização do instalador (Ctrl+B)");
    m.insert("ask_retry", "Falhou. Quer tentar de novo?");
    m.insert("retrying", "Tentando de novo...");
    m.insert("no_write_perm", "Não tenho permissão para escrever nessa pasta. Ela pode estar protegida, ser somente leitura ou estar no OneDrive. Mova o jogo para outro lugar (por exemplo, a sua pasta de usuário) e tente de novo.");
    m.insert("launcher_update_ready", "Há uma nova versão do instalador ({}). Use o botão Procurar atualização do instalador para instalá-la.");
    m.insert("ready_offline", "Pronto, mas sem conexão: não consegui verificar a versão do mod nem a lista de jogos.");
    m.insert("game_running", "Parece que {} está aberto. Feche o jogo e tente de novo.");
    m.insert("preload_missing_warn", "Não detecto suporte a preloadScript no executável; o mod pode não funcionar. Instalar mesmo assim?");
    m.insert("err_launcher_too_old", "Este mod exige um instalador mais novo (no mínimo {}). Atualize-o com o botão Procurar atualização do instalador.");
    m.insert("err_download_corrupt", "O download de {} chegou danificado, ou o mod mudou durante a instalação. Tente de novo.");
    m.insert("err_write_locked", "Não consegui escrever {}: o arquivo está bloqueado. Feche o jogo e tente de novo.");
    m.insert("err_mkxp_no_root", "O mkxp.json deste jogo não tem um objeto JSON válido, então não consigo registrar o mod. Corrija-o ou apague-o e tente de novo.");
    m.insert("err_profile_missing", "O perfil {} já não existe no mod, então este jogo ficaria sem as suas telas. Escolha outro com o botão Mudar perfil, ou o genérico.");
    m
}

fn de_map() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert("app_title", "PokeEssentialsAccess - Installer");
    m.insert("my_games", "Meine Spiele");
    m.insert("install", "Installieren");
    m.insert("uninstall", "Deinstallieren");
    m.insert("remove_from_list", "Aus der Liste entfernen");
    m.insert("status_uptodate", "Aktuell");
    m.insert("status_update", "Update verfügbar");
    m.insert("status_notinstalled", "Nicht installiert");
    m.insert("status_outdated", "Veraltet oder beschädigt");
    m.insert("status_unknown", "Unbekannte Version (offline)");
    m.insert("err_rate_limited", "GitHub begrenzt gerade die Anfragen. Warte etwa {} Minuten und versuche es erneut.");
    m.insert("err_rate_limited_short", "GitHub begrenzt gerade die Anfragen. Warte ein wenig und versuche es erneut.");
    m.insert("err_download", "Download fehlgeschlagen: {}");
    m.insert("err_download_status", "Der Download ist fehlgeschlagen (Status {}).");
    m.insert("err_tree_truncated", "Die Dateiliste des Mods ist zu groß für GitHub (abgeschnittene Antwort). Sag dem Autor des Mods Bescheid.");
    m.insert("err_list_files", "Ich konnte die Dateiliste des Mods nicht abrufen: {}");
    m.insert("err_io_mkdir", "Ich konnte den Ordner für {} nicht anlegen");
    m.insert("err_io_write", "Ich konnte {} nicht schreiben");
    m.insert("err_io_delete", "Ich konnte {} nicht löschen");
    m.insert("err_io_read", "Ich konnte {} nicht lesen");
    m.insert("err_io_create", "Ich konnte {} nicht erstellen");
    m.insert("err_selfupdate_download", "Ich konnte den neuen Installer nicht herunterladen: {}");
    m.insert("err_selfupdate_invalid", "Der heruntergeladene Installer ist keine gültige ausführbare Datei.");
    m.insert("err_selfupdate_replace", "Ich konnte den Installer nicht ersetzen: {}");
    m.insert("game_folder_missing", "Ich finde den Spielordner nicht: {}. Hast du ihn verschoben oder umbenannt? Entferne ihn aus der Liste und füge ihn erneut hinzu.");
    m.insert("status_missing", "Ordner nicht gefunden");
    m.insert("profile_label", "Profil: {}");
    m.insert("profile_generic", "generisch");
    m.insert("pick_folder", "Wähle den Spielordner");
    m.insert("not_compatible", "Dieses Spiel ist nicht kompatibel (es verwendet kein mkxp-z).");
    m.insert("detected_profile", "Profil für {} erkannt. Wie möchtest du es installieren?");
    m.insert("installed_profile", "Dieses Spiel hat bereits das Profil von {} installiert. Wie möchtest du es installieren?");
    m.insert("install_specific", "Profil von {} installieren");
    m.insert("install_generic", "Generisches Profil installieren");
    m.insert("generic_hint", "Nimm das generische, wenn deine Spielversion von der unterstützten abweicht.");
    m.insert("not_detected", "Ich habe das Spiel nicht erkannt. Wähle das Profil:");
    m.insert("choose_manual", "Profil manuell wählen...");
    m.insert("manual_profile_title", "Wähle das Profil des Spiels:");
    m.insert("installing", "Installiere: {}");
    m.insert("downloading_file", "Lade {} herunter");
    m.insert("done_installed", "Erfolgreich installiert ({}).");
    m.insert("done_uninstalled", "Deinstalliert.");
    m.insert("error", "Fehler: {}");
    m.insert("confirm_uninstall", "Den Mod aus diesem Spiel entfernen?");
    m.insert("language", "Sprache");
    m.insert("no_selection", "Wähle zuerst ein Spiel aus der Liste.");
    m.insert("updating_all", "Aktualisiere alle installierten Spiele...");
    m.insert("nothing_to_update", "Keine installierten Spiele zu aktualisieren.");
    m.insert("profile_changed", "Profil auf {} geändert. Installiere neu...");
    m.insert("lang_es", "Spanisch");
    m.insert("lang_en", "Englisch");
    m.insert("lang_fr", "Französisch");
    m.insert("lang_pt", "Portugiesisch");
    m.insert("lang_de", "Deutsch");
    m.insert("lang_pl", "Polnisch");
    m.insert("lang_changed", "Sprache geändert. Starte den Installer neu, damit sie überall greift.");
    m.insert("launcher_update_title", "Update des Installers");
    m.insert("launcher_update_available", "Es gibt eine neue Version des Installers ({}). Jetzt aktualisieren?");
    m.insert("launcher_update_none", "Der Installer ist bereits aktuell.");
    m.insert("launcher_update_applied", "Installer aktualisiert. Er startet jetzt neu.");
    m.insert("check_launcher_update", "Nach Installer-Update suchen");
    m.insert("log_label", "Aktivitätsprotokoll");
    m.insert("progress", "Fortschritt");
    m.insert("working_wait", "Arbeite, einen Moment bitte...");
    m.insert("ready", "Bereit");
    m.insert("checking_games", "Prüfe die Spieldateien, einen Moment...");
    m.insert("loading_catalog", "Lade die Spieleliste...");
    m.insert("choose_profile_title", "Profil wählen");
    m.insert("worker_crashed", "Die Installation wurde unerwartet abgebrochen.");
    m.insert("restart_failed", "Aktualisiert, aber Neustart nicht möglich: {}. Öffne den Installer bitte manuell erneut.");
    m.insert("confirm_remove", "Dieses Spiel aus der Liste entfernen? Der Mod wird nicht aus dem Spiel gelöscht.");
    m.insert("removed_from_list", "Spiel aus der Liste entfernt.");
    m.insert("add_game_btn", "Spiel hinzufügen (Strg+A)");
    m.insert("install_btn", "Installieren oder aktualisieren (Strg+I)");
    m.insert("update_all_btn", "Alle aktualisieren (Strg+U)");
    m.insert("change_profile_btn", "Profil wechseln (Strg+P)");
    m.insert("uninstall_btn", "Deinstallieren (Strg+D)");
    m.insert("remove_from_list_btn", "Aus der Liste entfernen (Strg+Q)");
    m.insert("options_btn", "Optionen (Strg+O)");
    m.insert("check_launcher_update_btn", "Nach Installer-Update suchen (Strg+B)");
    m.insert("ask_retry", "Fehlgeschlagen. Noch einmal versuchen?");
    m.insert("retrying", "Versuche es erneut...");
    m.insert("no_write_perm", "Ich darf in diesen Ordner nicht schreiben. Er ist vielleicht geschützt, schreibgeschützt oder liegt in OneDrive. Verschiebe das Spiel woanders hin (zum Beispiel in deinen Benutzerordner) und versuche es erneut.");
    m.insert("launcher_update_ready", "Es gibt eine neue Version des Installers ({}). Installiere sie über die Schaltfläche Nach Installer-Update suchen.");
    m.insert("ready_offline", "Bereit, aber offline: Ich konnte weder die Mod-Version noch die Spieleliste prüfen.");
    m.insert("game_running", "Anscheinend ist {} geöffnet. Schließe das Spiel und versuche es erneut.");
    m.insert("preload_missing_warn", "Ich finde keine preloadScript-Unterstützung in der ausführbaren Datei; der Mod funktioniert möglicherweise nicht. Trotzdem installieren?");
    m.insert("err_launcher_too_old", "Dieser Mod braucht einen neueren Installer (mindestens {}). Aktualisiere ihn über die Schaltfläche Nach Installer-Update suchen.");
    m.insert("err_download_corrupt", "Der Download von {} kam beschädigt an, oder der Mod hat sich während der Installation geändert. Versuche es erneut.");
    m.insert("err_write_locked", "Ich konnte {} nicht schreiben: Die Datei ist gesperrt. Schließe das Spiel und versuche es erneut.");
    m.insert("err_mkxp_no_root", "Die mkxp.json dieses Spiels enthält kein gültiges JSON-Objekt, deshalb kann ich den Mod nicht registrieren. Korrigiere oder lösche sie und versuche es erneut.");
    m.insert("err_profile_missing", "Das Profil {} gibt es im Mod nicht mehr, dieses Spiel bliebe also ohne seine eigenen Bildschirme. Wähle über die Schaltfläche Profil wechseln ein anderes oder das generische.");
    m
}

fn pl_map() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert("app_title", "PokeEssentialsAccess - Instalator");
    m.insert("my_games", "Moje gry");
    m.insert("install", "Zainstaluj");
    m.insert("uninstall", "Odinstaluj");
    m.insert("remove_from_list", "Usuń z listy");
    m.insert("status_uptodate", "Aktualna");
    m.insert("status_update", "Dostępna aktualizacja");
    m.insert("status_notinstalled", "Niezainstalowany");
    m.insert("status_outdated", "Nieaktualny lub uszkodzony");
    m.insert("status_unknown", "Nieznana wersja (brak połączenia)");
    m.insert("err_rate_limited", "GitHub ogranicza liczbę żądań. Poczekaj około {} minut i spróbuj ponownie.");
    m.insert("err_rate_limited_short", "GitHub ogranicza liczbę żądań. Poczekaj chwilę i spróbuj ponownie.");
    m.insert("err_download", "Nie udało się pobrać: {}");
    m.insert("err_download_status", "Pobieranie nie powiodło się (stan {}).");
    m.insert("err_tree_truncated", "Lista plików moda jest za duża dla GitHuba (obcięta odpowiedź). Powiadom autora moda.");
    m.insert("err_list_files", "Nie udało się pobrać listy plików moda: {}");
    m.insert("err_io_mkdir", "Nie udało się utworzyć folderu dla {}");
    m.insert("err_io_write", "Nie udało się zapisać {}");
    m.insert("err_io_delete", "Nie udało się usunąć {}");
    m.insert("err_io_read", "Nie udało się odczytać {}");
    m.insert("err_io_create", "Nie udało się utworzyć {}");
    m.insert("err_selfupdate_download", "Nie udało się pobrać nowego instalatora: {}");
    m.insert("err_selfupdate_invalid", "Pobrany instalator nie jest prawidłowym plikiem wykonywalnym.");
    m.insert("err_selfupdate_replace", "Nie udało się zastąpić instalatora: {}");
    m.insert("game_folder_missing", "Nie mogę znaleźć folderu gry: {}. Przeniesiono go lub zmieniono nazwę? Usuń go z listy i dodaj ponownie.");
    m.insert("status_missing", "Nie znaleziono folderu");
    m.insert("profile_label", "profil: {}");
    m.insert("profile_generic", "ogólny");
    m.insert("pick_folder", "Wybierz folder gry");
    m.insert("not_compatible", "Ta gra nie jest zgodna (nie używa mkxp-z).");
    m.insert("detected_profile", "Wykryto profil dla {}. Jak chcesz go zainstalować?");
    m.insert("installed_profile", "Ta gra ma już zainstalowany profil {}. Jak chcesz go zainstalować?");
    m.insert("install_specific", "Zainstaluj profil {}");
    m.insert("install_generic", "Zainstaluj profil ogólny");
    m.insert("generic_hint", "Użyj ogólnego, jeśli twoja wersja gry różni się od obsługiwanej.");
    m.insert("not_detected", "Nie rozpoznałem gry. Wybierz profil:");
    m.insert("choose_manual", "Wybierz profil ręcznie...");
    m.insert("manual_profile_title", "Wybierz profil gry:");
    m.insert("installing", "Instalowanie: {}");
    m.insert("downloading_file", "Pobieranie {}");
    m.insert("done_installed", "Zainstalowano poprawnie ({}).");
    m.insert("done_uninstalled", "Odinstalowano.");
    m.insert("error", "Błąd: {}");
    m.insert("confirm_uninstall", "Usunąć mod z tej gry?");
    m.insert("language", "Język");
    m.insert("no_selection", "Najpierw wybierz grę z listy.");
    m.insert("updating_all", "Aktualizowanie wszystkich zainstalowanych gier...");
    m.insert("nothing_to_update", "Brak zainstalowanych gier do aktualizacji.");
    m.insert("profile_changed", "Profil zmieniony na {}. Ponowna instalacja...");
    m.insert("lang_es", "Hiszpański");
    m.insert("lang_en", "Angielski");
    m.insert("lang_fr", "Francuski");
    m.insert("lang_pt", "Portugalski");
    m.insert("lang_de", "Niemiecki");
    m.insert("lang_pl", "Polski");
    m.insert("lang_changed", "Język zmieniony. Uruchom instalator ponownie, aby zastosować go w pełni.");
    m.insert("launcher_update_title", "Aktualizacja instalatora");
    m.insert("launcher_update_available", "Dostępna jest nowa wersja instalatora ({}). Zaktualizować teraz?");
    m.insert("launcher_update_none", "Instalator jest już aktualny.");
    m.insert("launcher_update_applied", "Instalator zaktualizowany. Zaraz uruchomi się ponownie.");
    m.insert("check_launcher_update", "Sprawdź aktualizację instalatora");
    m.insert("log_label", "Dziennik aktywności");
    m.insert("progress", "Postęp");
    m.insert("working_wait", "Pracuję, chwileczkę...");
    m.insert("ready", "Gotowe");
    m.insert("checking_games", "Sprawdzam pliki gry, chwileczkę...");
    m.insert("loading_catalog", "Wczytywanie listy gier...");
    m.insert("choose_profile_title", "Wybór profilu");
    m.insert("worker_crashed", "Instalacja została nieoczekiwanie przerwana.");
    m.insert("restart_failed", "Zaktualizowano, ale nie udało się uruchomić ponownie: {}. Otwórz instalator jeszcze raz ręcznie.");
    m.insert("confirm_remove", "Usunąć tę grę z listy? Mod nie zostanie usunięty z gry.");
    m.insert("removed_from_list", "Gra usunięta z listy.");
    m.insert("add_game_btn", "Dodaj grę (Ctrl+A)");
    m.insert("install_btn", "Zainstaluj lub zaktualizuj (Ctrl+I)");
    m.insert("update_all_btn", "Zaktualizuj wszystkie (Ctrl+U)");
    m.insert("change_profile_btn", "Zmień profil (Ctrl+P)");
    m.insert("uninstall_btn", "Odinstaluj (Ctrl+D)");
    m.insert("remove_from_list_btn", "Usuń z listy (Ctrl+Q)");
    m.insert("options_btn", "Opcje (Ctrl+O)");
    m.insert("check_launcher_update_btn", "Sprawdź aktualizację instalatora (Ctrl+B)");
    m.insert("ask_retry", "Nie udało się. Spróbować ponownie?");
    m.insert("retrying", "Ponawiam próbę...");
    m.insert("no_write_perm", "Nie mam uprawnień do zapisu w tym folderze. Może być chroniony, tylko do odczytu albo znajdować się w OneDrive. Przenieś grę w inne miejsce (na przykład do swojego folderu użytkownika) i spróbuj ponownie.");
    m.insert("launcher_update_ready", "Dostępna jest nowa wersja instalatora ({}). Użyj przycisku Sprawdź aktualizację instalatora, aby ją zainstalować.");
    m.insert("ready_offline", "Gotowe, ale bez połączenia: nie udało się sprawdzić wersji moda ani listy gier.");
    m.insert("game_running", "Wygląda na to, że gra {} jest uruchomiona. Zamknij grę i spróbuj ponownie.");
    m.insert("preload_missing_warn", "Nie wykrywam obsługi preloadScript w pliku wykonywalnym; mod może nie działać. Zainstalować mimo to?");
    m.insert("err_launcher_too_old", "Ten mod wymaga nowszego instalatora (co najmniej {}). Zaktualizuj go przyciskiem Sprawdź aktualizację instalatora.");
    m.insert("err_download_corrupt", "Pobieranie {} zakończyło się uszkodzonym plikiem albo mod zmienił się w trakcie instalacji. Spróbuj ponownie.");
    m.insert("err_write_locked", "Nie udało się zapisać {}: plik jest zablokowany. Zamknij grę i spróbuj ponownie.");
    m.insert("err_mkxp_no_root", "Plik mkxp.json tej gry nie zawiera poprawnego obiektu JSON, więc nie mogę zarejestrować moda. Popraw go lub usuń i spróbuj ponownie.");
    m.insert("err_profile_missing", "Profil {} nie istnieje już w modzie, więc ta gra zostałaby bez swoich ekranów. Wybierz inny przyciskiem Zmień profil albo ogólny.");
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falls_back_to_key_then_spanish() {
        let i = I18n::new("en");
        assert_eq!(i.t("install"), "Install");
        assert_eq!(i.t("clave_inexistente"), "clave_inexistente");
    }

    #[test]
    fn spanish_default_and_switch() {
        let mut i = I18n::new("es");
        assert_eq!(i.t("install"), "Instalar");
        i.set_lang("en");
        assert_eq!(i.t("install"), "Install");
        i.set_lang("pl");
        assert_eq!(i.t("install"), "Zainstaluj");
    }

    #[test]
    fn format_arg() {
        let i = I18n::new("es");
        assert_eq!(i.tf("profile_label", "Pokemon Z"), "perfil: Pokemon Z");
    }

    #[test]
    fn unknown_lang_defaults_spanish() {
        let i = I18n::new("xx");
        assert_eq!(i.t("install"), "Instalar");
    }

    #[test]
    fn every_listed_language_has_its_own_table() {
        for lang in LANGS {
            let i = I18n::new(lang);
            assert_ne!(i.t("app_title"), "app_title", "sin tabla para {}", lang);
            assert_eq!(i.t(&format!("lang_{}", lang)).is_empty(), false);
        }
        assert_eq!(I18n::new("fr").t("install"), "Installer");
        assert_eq!(I18n::new("de").t("install"), "Installieren");
    }

    #[test]
    fn t_err_resolves_bare_key() {
        let i = I18n::new("en");
        assert_eq!(i.t_err("status_unknown"), "Unknown version (offline)");
    }

    #[test]
    fn t_err_resolves_key_with_argument() {
        let i = I18n::new("es");
        let msg = i.t_err(&err_key("err_download_corrupt", "core/nav/locator.rb"));
        assert!(msg.contains("core/nav/locator.rb"));
        assert!(!msg.contains("err_download_corrupt"));
        assert!(!msg.contains('\u{1}'));
    }

    #[test]
    fn t_err_passes_literal_messages_through() {
        let i = I18n::new("es");
        assert_eq!(i.t_err("descarga estado 404"), "descarga estado 404");
    }

    #[test]
    fn every_key_is_translated_in_every_language() {
        let es = es_map();
        for (lang, table) in [("en", en_map()), ("fr", fr_map()), ("pt", pt_map()), ("de", de_map()), ("pl", pl_map())] {
            let mut missing: Vec<&str> = es.keys().filter(|k| !table.contains_key(*k)).copied().collect();
            missing.extend(table.keys().filter(|k| !es.contains_key(*k)).copied());
            missing.sort_unstable();
            assert!(missing.is_empty(), "claves sin pareja es/{}: {:?}", lang, missing);
        }
    }

    #[test]
    fn placeholders_survive_every_translation() {
        let es = es_map();
        for table in [en_map(), fr_map(), pt_map(), de_map(), pl_map()] {
            for (k, v) in &es {
                let want = v.matches("{}").count();
                assert_eq!(table[k].matches("{}").count(), want, "hueco {{}} perdido o sobrante en {}", k);
            }
        }
    }
}
