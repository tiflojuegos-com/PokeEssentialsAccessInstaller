use std::collections::HashMap;

pub struct I18n {
    lang: String,
    es: HashMap<&'static str, &'static str>,
    en: HashMap<&'static str, &'static str>,
}

impl I18n {
    pub fn new(lang: &str) -> I18n {
        I18n {
            lang: if lang == "en" { "en".to_string() } else { "es".to_string() },
            es: es_map(),
            en: en_map(),
        }
    }

    pub fn set_lang(&mut self, lang: &str) {
        self.lang = if lang == "en" { "en".to_string() } else { "es".to_string() };
    }

    pub fn t(&self, key: &str) -> String {
        let table = if self.lang == "en" { &self.en } else { &self.es };
        table
            .get(key)
            .copied()
            .or_else(|| self.es.get(key).copied())
            .unwrap_or(key)
            .to_string()
    }

    pub fn tf(&self, key: &str, arg: &str) -> String {
        self.t(key).replace("{}", arg)
    }
}

fn es_map() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert("app_title", "PokeEssentialsAccess - Instalador");
    m.insert("my_games", "Mis juegos");
    m.insert("add_game", "Añadir juego");
    m.insert("update_all", "Actualizar todos");
    m.insert("options", "Opciones");
    m.insert("install", "Instalar");
    m.insert("update", "Actualizar");
    m.insert("uninstall", "Desinstalar");
    m.insert("change_profile", "Cambiar perfil");
    m.insert("remove_from_list", "Quitar de la lista");
    m.insert("status_uptodate", "Al día");
    m.insert("status_update", "Actualización disponible");
    m.insert("status_notinstalled", "No instalado");
    m.insert("status_outdated", "Desfasado o dañado");
    m.insert("profile_label", "perfil: {}");
    m.insert("profile_generic", "genérico");
    m.insert("pick_folder", "Elige la carpeta del juego");
    m.insert("not_compatible", "Este juego no es compatible (no usa mkxp-z).");
    m.insert("detected_profile", "Detectado el perfil para {}. ¿Cómo quieres instalarlo?");
    m.insert("install_specific", "Instalar perfil de {}");
    m.insert("install_generic", "Instalar perfil genérico");
    m.insert("generic_hint", "Usa el genérico si tu versión del juego difiere de la soportada.");
    m.insert("not_detected", "No he reconocido el juego. Elige el perfil:");
    m.insert("installing", "Instalando: {}");
    m.insert("downloading_file", "Descargando {}");
    m.insert("done_installed", "Instalado correctamente ({}).");
    m.insert("done_uninstalled", "Desinstalado.");
    m.insert("error", "Error: {}");
    m.insert("confirm_uninstall", "¿Quitar el mod de este juego?");
    m.insert("language", "Idioma");
    m.insert("close", "Cerrar");
    m.insert("no_selection", "Selecciona un juego de la lista primero.");
    m.insert("updating_all", "Actualizando todos los juegos instalados...");
    m.insert("update_all_done", "Actualización de todos completada.");
    m.insert("nothing_to_update", "No hay juegos instalados que actualizar.");
    m.insert("choose_new_profile", "Elige el perfil para este juego:");
    m.insert("profile_changed", "Perfil cambiado a {}. Reinstalando...");
    m.insert("lang_es", "Español");
    m.insert("lang_en", "Inglés");
    m.insert("lang_changed", "Idioma cambiado. Reinicia el instalador para verlo del todo.");
    m.insert("launcher_update_title", "Actualización del instalador");
    m.insert("launcher_update_available", "Hay una versión nueva del instalador ({}). ¿Quieres actualizar?");
    m.insert("launcher_update_none", "El instalador ya está actualizado.");
    m.insert("launcher_update_applied", "Instalador actualizado. Se reiniciará ahora.");
    m.insert("open_download", "Abrir la descarga");
    m.insert("check_launcher_update", "Buscar actualización del instalador");
    m.insert("log_label", "Registro de actividad");
    m.insert("progress", "Progreso");
    m.insert("working_wait", "Trabajando, espera un momento...");
    m.insert("ready", "Listo");
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
    m
}

fn en_map() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert("app_title", "PokeEssentialsAccess - Installer");
    m.insert("my_games", "My games");
    m.insert("add_game", "Add game");
    m.insert("update_all", "Update all");
    m.insert("options", "Options");
    m.insert("install", "Install");
    m.insert("update", "Update");
    m.insert("uninstall", "Uninstall");
    m.insert("change_profile", "Change profile");
    m.insert("remove_from_list", "Remove from list");
    m.insert("status_uptodate", "Up to date");
    m.insert("status_update", "Update available");
    m.insert("status_notinstalled", "Not installed");
    m.insert("status_outdated", "Outdated or broken");
    m.insert("profile_label", "profile: {}");
    m.insert("profile_generic", "generic");
    m.insert("pick_folder", "Choose the game folder");
    m.insert("not_compatible", "This game is not compatible (not mkxp-z).");
    m.insert("detected_profile", "Detected the profile for {}. How do you want to install it?");
    m.insert("install_specific", "Install the {} profile");
    m.insert("install_generic", "Install the generic profile");
    m.insert("generic_hint", "Use the generic one if your game version differs from the supported one.");
    m.insert("not_detected", "I didn't recognize the game. Choose the profile:");
    m.insert("installing", "Installing: {}");
    m.insert("downloading_file", "Downloading {}");
    m.insert("done_installed", "Installed successfully ({}).");
    m.insert("done_uninstalled", "Uninstalled.");
    m.insert("error", "Error: {}");
    m.insert("confirm_uninstall", "Remove the mod from this game?");
    m.insert("language", "Language");
    m.insert("close", "Close");
    m.insert("no_selection", "Select a game from the list first.");
    m.insert("updating_all", "Updating all installed games...");
    m.insert("update_all_done", "All updates completed.");
    m.insert("nothing_to_update", "No installed games to update.");
    m.insert("choose_new_profile", "Choose the profile for this game:");
    m.insert("profile_changed", "Profile changed to {}. Reinstalling...");
    m.insert("lang_es", "Spanish");
    m.insert("lang_en", "English");
    m.insert("lang_changed", "Language changed. Restart the installer to fully apply it.");
    m.insert("launcher_update_title", "Installer update");
    m.insert("launcher_update_available", "A new installer version is available ({}). Update now?");
    m.insert("launcher_update_none", "The installer is up to date.");
    m.insert("launcher_update_applied", "Installer updated. It will restart now.");
    m.insert("open_download", "Open the download");
    m.insert("check_launcher_update", "Check for installer update");
    m.insert("log_label", "Activity log");
    m.insert("progress", "Progress");
    m.insert("working_wait", "Working, please wait...");
    m.insert("ready", "Ready");
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
    m.insert("uninstall_btn", "Delete (Ctrl+D)");
    m.insert("remove_from_list_btn", "Remove from list (Ctrl+Q)");
    m.insert("options_btn", "Options (Ctrl+O)");
    m.insert("check_launcher_update_btn", "Check for installer update (Ctrl+B)");
    m.insert("ask_retry", "It failed. Do you want to retry?");
    m.insert("retrying", "Retrying...");
    m.insert("no_write_perm", "I can't write to that folder. It may be protected, read-only, or in OneDrive. Move the game somewhere else (for example your user folder) and try again.");
    m.insert("launcher_update_ready", "A new installer version is available ({}). Use the Check for installer update button to install it.");
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
    }

    #[test]
    fn format_arg() {
        let i = I18n::new("es");
        assert_eq!(i.tf("profile_label", "Pokemon Z"), "perfil: Pokemon Z");
    }

    #[test]
    fn unknown_lang_defaults_spanish() {
        let i = I18n::new("fr");
        assert_eq!(i.t("install"), "Instalar");
    }
}
