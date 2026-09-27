use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const NO_NETWORK: &str = "http://127.0.0.1:9";

const PLAYER: &str = "MZ fake RGSS player";

struct Ran {
    code: i32,
    out: String,
    err: String,
}

impl Ran {
    fn says(&self, text: &str) -> bool {
        self.out.contains(text)
    }

    fn complains(&self, text: &str) -> bool {
        self.err.contains(text)
    }
}

fn pea(config: &Path, args: &[&str]) -> Ran {
    pea_in(config, args, Path::new(env!("CARGO_MANIFEST_DIR")))
}

fn pea_in(config: &Path, args: &[&str], cwd: &Path) -> Ran {
    let run = Command::new(env!("CARGO_BIN_EXE_pokeessentialsaccess-launcher"))
        .args(args)
        .args(["--lang", "es"])
        .current_dir(cwd)
        .env("PEA_CONFIG_DIR", config)
        .env("HTTPS_PROXY", NO_NETWORK)
        .env("HTTP_PROXY", NO_NETWORK)
        .env("ALL_PROXY", NO_NETWORK)
        .env_remove("NO_PROXY")
        .env_remove("no_proxy")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    Ran {
        code: run.status.code().unwrap_or(-1),
        out: String::from_utf8_lossy(&run.stdout).into_owned(),
        err: String::from_utf8_lossy(&run.stderr).into_owned(),
    }
}

fn write(root: &Path, files: &[(&str, &str)]) {
    for (rel, text) in files {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
}

fn fake_mod(root: &Path) -> PathBuf {
    let dir = root.join("PokeEssentialsAccess");
    write(&dir, &[
        ("version.json", "{\"version\": \"0.6.0\", \"min_launcher\": \"0.1.0\"}"),
        ("games/catalog.json", r#"{"profiles":[
            {"key":"anil","display":"Pokemon Anil","titles":["pokemon anil"],"detect":"anil","exes":[]},
            {"key":"opalo","display":"Pokemon Opalo","titles":[],"detect":"opalo","exes":[]},
            {"key":"insurgence","display":"Pokemon Insurgence","titles":["pokemon insurgence"],"exes":[],
             "convert":"e1","markers":["Game.rgssad","MGC_Hmode7.dll"]},
            {"key":"uranium","display":"Pokemon Uranium","titles":["pokemon uranium"],"detect":"uranium",
             "exes":["Uranium.exe"],"convert":"e1","markers":["Uranium.rgssad","KleinBitmap.dll"],
             "compat":"compat.rb"}]}"#),
        ("core/manifest.rb", "core"),
        ("core/nav/locator.rb", "nav"),
        ("loader/preload_access.rb", "loader"),
        ("loader/boot.rb", "boot"),
        ("lang/es.txt", "hola=Hola"),
        ("games/anil/manifest.rb", "{ :imports => %w[anil_common] }"),
        ("games/anil/menus.rb", "menus"),
        ("games/anil_common/shared.rb", "shared"),
        ("games/opalo/manifest.rb", "{ :modules => %w[] }"),
        ("games/insurgence/manifest.rb", "{ :modules => %w[] }"),
        ("games/uranium/manifest.rb", "{ :modules => %w[] }"),
        ("games/uranium/compat.rb", "compat"),
        ("games/generic/manifest.rb", "{ :modules => %w[] }"),
        ("assets/engine/e1/mkxp-z.exe", "MZ bundled engine preloadScript"),
    ]);
    dir
}

fn fake_engine(root: &Path) -> PathBuf {
    let dir = root.join("build");
    write(&dir, &[("mkxp-z.exe", "MZ own engine preloadScript"), ("extras/font.sf2", "sf2")]);
    dir
}

fn mkxp_game(root: &Path, name: &str) -> PathBuf {
    let dir = root.join(name);
    write(&dir, &[("Game.exe", "MZ preloadScript")]);
    dir
}

fn player_game(root: &Path, name: &str) -> PathBuf {
    let dir = root.join(name);
    write(&dir, &[("Game.exe", "MZ player"), ("Game.ini", "[Game]\r\nLibrary=RGSS102E.dll\r\n")]);
    dir
}

fn titled_player(root: &Path, title: &str, stem: &str, markers: [&str; 2]) -> PathBuf {
    let dir = root.join(title);
    let (exe, ini) = (format!("{stem}.exe"), format!("{stem}.ini"));
    let settings = format!("[Game]\r\nLibrary=RGSS102E.dll\r\nScripts=Data\\Scripts.rxdata\r\nTitle={title}\r\n");
    write(&dir, &[(&exe, PLAYER), (&ini, &settings), ("RGSS102E.dll", "rgss")]);
    write(&dir, &markers.map(|marker| (marker, "mark")));
    dir
}

fn files(dir: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut dirs = vec![dir.to_path_buf()];
    while let Some(d) = dirs.pop() {
        for path in fs::read_dir(&d).unwrap().map(|e| e.unwrap().path()) {
            if path.is_dir() {
                dirs.push(path);
            } else {
                found.push(path.strip_prefix(dir).unwrap().to_string_lossy().replace('\\', "/"));
            }
        }
    }
    found.sort();
    found
}

fn text(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

fn snapshot(dir: &Path) -> Vec<(String, Vec<u8>)> {
    files(dir)
        .into_iter()
        .map(|rel| {
            let bytes = fs::read(dir.join(&rel)).unwrap();
            (rel, bytes)
        })
        .collect()
}

fn live_json(game: &Path) -> serde_json::Value {
    let json = text(&game.join("mkxp.json"));
    let live: Vec<&str> = json.lines().filter(|line| !line.trim_start().starts_with("//")).collect();
    serde_json::from_str(&live.join("\n")).unwrap_or_else(|e| panic!("{e}:\n{json}"))
}

#[test]
fn the_help_names_every_command_and_a_wrong_line_ends_with_2_and_says_why() {
    let config = tempfile::tempdir().unwrap();
    let help = pea(config.path(), &["help"]);
    assert_eq!(help.code, 0);
    for word in ["install", "update", "uninstall", "check", "status", "export", "import", "list", "play", "self-update",
        "version"] {
        assert!(help.says(word), "{word}: {}", help.out);
    }
    assert!(help.out.lines().all(|l| !l.contains(" | ")), "cada línea de la ayuda, sola");
    let install = pea(config.path(), &["install", "/?"]);
    assert!(install.code == 0 && install.says("install [juego|all]") && install.says("--profile"), "{}", install.out);
    assert_eq!(pea(config.path(), &["help", "local"]).code, 0);
    assert!(pea(config.path(), &["help", "exit-codes"]).says("7: el juego no admite el mod."));
    let version = pea(config.path(), &["version"]);
    assert!(version.code == 0 && version.says(env!("CARGO_PKG_VERSION")), "{}", version.out);
    let wrong = pea(config.path(), &["instalar", "D:\\Juegos\\Anil"]);
    assert_eq!(wrong.code, 2);
    assert!(wrong.complains("Error: No existe la orden instalar.") && wrong.complains("help"), "{}", wrong.err);
    assert!(wrong.out.is_empty());
    assert_eq!(pea(config.path(), &["uninstall", "all"]).code, 2);
    assert_eq!(pea(config.path(), &["install", "--perfil", "anil"]).code, 2);
    assert!(!config.path().join("config.json").exists(), "una línea que no se entiende no toca la lista");
}

#[test]
fn a_local_install_adds_the_game_with_the_profile_it_recognises_and_status_list_and_update_follow_it() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("config");
    let mod_dir = fake_mod(root.path());
    let anil = mkxp_game(root.path(), "Pokemon Anil");
    let from = mod_dir.to_string_lossy().into_owned();
    let game = anil.to_string_lossy().into_owned();
    let installed = pea(&config, &["local", "install", &game, "--from", &from, "--yes"]);
    assert_eq!(installed.code, 0, "{}{}", installed.out, installed.err);
    let first = installed.out.lines().next().unwrap_or_default();
    assert_eq!(first, format!("Origen: carpeta {}, mod 0.6.0.", mod_dir.display()));
    assert!(installed.says("Pokemon Anil: añadido a la lista con el número 1, perfil Pokemon Anil."), "{}", installed.out);
    assert!(installed.says("Pokemon Anil: instalado. Mod 0.6.0, perfil Pokemon Anil."), "{}", installed.out);
    assert!(!installed.out.contains('\r'), "nada que se redibuje");
    let acc = anil.join("accessibility");
    for file in ["common/anil_common/shared.rb", "game/menus.rb", "core/nav/locator.rb", "data/installed.json", "boot.rb"] {
        assert!(acc.join(file).is_file(), "{file}");
    }
    assert!(text(&anil.join("mkxp.json")).contains("accessibility/preload_access.rb"));
    assert!(text(&config.join("config.json")).contains("\"profile\": \"anil\""), "la lista es la de PEA_CONFIG_DIR");

    let list = pea(&config, &["list", "--paths"]);
    assert!(list.says("1. Pokemon Anil: mod 0.6.0, perfil anil.") && list.says(&game), "{}", list.out);
    let status = pea(&config, &["local", "status", "--from", &from, "--exit-code"]);
    assert_eq!(status.code, 0, "{}{}", status.out, status.err);
    assert!(status.says("1. Pokemon Anil: Al día. Mod instalado: 0.6.0. Perfil: Pokemon Anil."), "{}", status.out);

    write(&mod_dir, &[("version.json", "{\"version\": \"0.7.0\"}"), ("core/nav/locator.rb", "nav 2")]);
    let behind = pea(&config, &["local", "status", "1", "--from", &from, "--exit-code"]);
    assert_eq!(behind.code, 8, "{}", behind.out);
    let updated = pea(&config, &["local", "update", "pokemon anil", "--from", &from]);
    assert_eq!(updated.code, 0, "{}{}", updated.out, updated.err);
    assert!(updated.says("Pokemon Anil: actualizado de 0.6.0 a 0.7.0. Archivos copiados: 1."), "{}", updated.out);
    assert_eq!(text(&acc.join("core").join("nav").join("locator.rb")), "nav 2");
    let again = pea(&config, &["local", "install", "1", "--from", &from, "-q"]);
    assert_eq!(again.out.trim(), "Pokemon Anil: al día, mod 0.7.0.", "con --quiet, solo la línea del juego");
}

#[test]
fn all_says_each_game_in_the_order_of_the_list_skips_a_folder_gone_and_fails_an_open_game_alone() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("config");
    let from = fake_mod(root.path()).to_string_lossy().into_owned();
    let games: Vec<PathBuf> = ["Alfa", "Beta", "Gamma", "Delta"].iter().map(|n| mkxp_game(root.path(), n)).collect();
    for game in &games {
        let added = pea(&config, &["local", "install", &game.to_string_lossy(), "--from", &from, "--profile", "opalo"]);
        assert_eq!(added.code, 0, "{}", added.err);
    }
    fs::remove_dir_all(&games[2]).unwrap();
    let run = pea(&config, &["local", "install", "all", "--from", &from, "--jobs", "3"]);
    assert_eq!(run.code, 0, "{}{}", run.out, run.err);
    let lines = [
        "1 de 4, Alfa: al día, mod 0.6.0.",
        "2 de 4, Beta: al día, mod 0.6.0.",
        "3 de 4, Gamma: no se encuentra su carpeta, se salta.",
        "4 de 4, Delta: al día, mod 0.6.0.",
        "Resumen. Juegos: 4. Al día: 3. Saltados: 1.",
    ];
    let at: Vec<usize> = lines.iter().map(|l| run.out.find(l).unwrap_or_else(|| panic!("{l}\n{}", run.out))).collect();
    assert!(at.windows(2).all(|w| w[0] < w[1]), "en el orden de la lista:\n{}", run.out);
    assert!(run.says("Instalando en los juegos de la lista, 3 a la vez. Juegos: 4."));

    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let _open = fs::OpenOptions::new().read(true).share_mode(1).open(games[1].join("Game.exe")).unwrap();
        let busy = pea(&config, &["local", "update", "all", "--from", &from]);
        assert_eq!(busy.code, 6, "{}{}", busy.out, busy.err);
        assert!(busy.complains("Error: 2 de 3, Beta: Parece que Beta está abierto."), "{}", busy.err);
        assert!(busy.says("3 de 3, Delta: al día"), "los demás siguen:\n{}", busy.out);
        let summary = busy.out.find("Resumen.").unwrap();
        assert!(busy.out[summary..].contains("Con error: 1."), "{}", busy.out);
        assert!(busy.complains("Con error: Beta. Parece que Beta está abierto."), "el error se repite al final");
        let one = pea(&config, &["local", "update", "Beta", "--from", &from]);
        assert_eq!(one.code, 5, "{}", one.err);
    }
}

#[test]
fn uninstall_asks_first_keeps_the_players_data_on_request_and_never_takes_all() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("config");
    let from = fake_mod(root.path()).to_string_lossy().into_owned();
    let game = mkxp_game(root.path(), "Pokemon Anil");
    assert_eq!(pea(&config, &["local", "install", &game.to_string_lossy(), "--from", &from]).code, 0);
    let acc = game.join("accessibility");
    write(&acc, &[("data/settings.ini", "voz=1")]);
    let unasked = pea(&config, &["uninstall", "1"]);
    assert_eq!(unasked.code, 4, "sin consola no se pregunta: {}", unasked.out);
    assert!(unasked.complains("--yes") && acc.join("core").is_dir(), "{}", unasked.err);
    let kept = pea(&config, &["local", "uninstall", "Pokemon Anil", "--keep-data"]);
    assert_eq!(kept.code, 0, "{}{}", kept.out, kept.err);
    assert!(kept.says("Pokemon Anil: desinstalado. Se conservan en") && kept.says("data: ajustes."), "{}", kept.out);
    assert_eq!(files(&acc), vec!["data/settings.ini".to_string()]);
    assert!(!game.join("mkxp.json").exists(), "el mkxp.json que puso el instalador se va");
    let gone = pea(&config, &["uninstall", "1", "--yes"]);
    assert!(gone.code == 0 && gone.says("Pokemon Anil: desinstalado.") && !acc.exists(), "{}", gone.out);
    let nothing = pea(&config, &["uninstall", "1"]);
    assert!(nothing.code == 0 && nothing.says("no tiene el mod"), "{}", nothing.out);
    assert_eq!(files(&game), vec!["Game.exe".to_string()]);
    assert!(pea(&config, &["list"]).says("1. Pokemon Anil: sin el mod"), "desinstalar no lo quita de la lista");
}

#[test]
fn check_says_what_the_folder_runs_touches_nothing_and_ends_with_the_code_the_install_would() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("config");
    let from = fake_mod(root.path()).to_string_lossy().into_owned();
    let engine = fake_engine(root.path()).to_string_lossy().into_owned();
    let native = mkxp_game(root.path(), "Pokemon Opalo");
    let ok = pea(&config, &["local", "check", &native.to_string_lossy(), "--from", &from]);
    assert_eq!(ok.code, 0, "{}{}", ok.out, ok.err);
    for line in ["mkxp.json: no; se crea al instalar", "Motor: mkxp-z.", "preloadScript: sí, en Game.exe.",
        "Perfil reconocido: Pokemon Opalo.", "Resultado: compatible. Se puede instalar."] {
        assert!(ok.says(line), "{line}\n{}", ok.out);
    }
    let empty = root.path().join("Vacia");
    fs::create_dir_all(&empty).unwrap();
    let refused = pea(&config, &["local", "check", &empty.to_string_lossy(), "--from", &from]);
    assert_eq!(refused.code, 7);
    assert!(refused.says("Resultado: no compatible. Este juego no es compatible"), "{}", refused.out);
    let player = player_game(root.path(), "Juego raro");
    let beside = ["local", "check", "Juego raro", "--from", &from, "--engine", &engine];
    let convertible = pea_in(&config, &beside, root.path());
    assert_eq!(convertible.code, 0, "una carpeta bajo la actual: {}{}", convertible.out, convertible.err);
    assert!(convertible.says("Motor: el reproductor original de RPG Maker XP (RGSS102E.dll)."), "{}", convertible.out);
    assert!(convertible.says("se añade Game (PokeAccess).exe junto al ejecutable del juego"), "{}", convertible.out);
    assert_eq!(files(&player), vec!["Game.exe".to_string(), "Game.ini".to_string()], "check no toca nada");
    assert!(!config.join("config.json").exists());
}

#[test]
fn a_question_without_a_console_ends_with_4_and_says_which_option_answers_it() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("config");
    let from = fake_mod(root.path()).to_string_lossy().into_owned();
    let engine = fake_engine(root.path()).to_string_lossy().into_owned();
    let unknown = mkxp_game(root.path(), "Juego nuevo").to_string_lossy().into_owned();
    for yes in [&[][..], &["--yes"]] {
        let asked = pea(&config, &[&["local", "install", &unknown, "--from", &from][..], yes].concat());
        assert_eq!(asked.code, 4, "{}", asked.err);
        assert!(asked.complains("--profile"), "{}", asked.err);
    }
    let no_game = pea(&config, &["local", "install", "--from", &from]);
    assert!(no_game.code == 4 && no_game.complains("Falta el juego"), "sin consola no se abre el selector: {}", no_game.err);
    let player = player_game(root.path(), "Pokemon Insurgence");
    let dir = player.to_string_lossy().into_owned();
    let convert = ["local", "install", &dir, "--from", &from, "--engine", &engine, "--profile", "opalo"];
    let unasked = pea(&config, &convert);
    assert!(unasked.code == 4 && unasked.complains("--yes"), "{}", unasked.err);
    assert_eq!(files(&player).len(), 2, "sin respuesta no se convierte");
    assert!(!config.join("config.json").exists(), "ni se añade a la lista");

    let converted = pea(&config, &[&convert[..], &["--yes"]].concat());
    assert_eq!(converted.code, 0, "{}{}", converted.out, converted.err);
    assert!(player.join("Game (PokeAccess).exe").is_file() && player.join("font.sf2").is_file());
    let hint = "Pokemon Insurgence: Para jugar con accesibilidad, usa la orden play o abre «Game (PokeAccess).exe».";
    assert!(converted.says(hint), "el aviso de la consola, con su juego: {}", converted.out);
    assert!(!converted.says("botón"));
    assert!(text(&config.join("config.json")).contains("Game (PokeAccess).exe"), "la lista apunta al exe accesible");

    fs::remove_file(player.join("Game (PokeAccess).exe")).unwrap();
    let blocked = pea(&config, &["play", "1"]);
    assert_eq!(blocked.code, 1, "{}", blocked.out);
    assert!(blocked.complains("Falta Game (PokeAccess).exe") && blocked.complains("La orden install"), "{}", blocked.err);
    let restored = pea(&config, &["local", "install", "1", "--from", &from, "--engine", &engine]);
    assert_eq!(restored.code, 0, "{}{}", restored.out, restored.err);
    assert!(player.join("Game (PokeAccess).exe").is_file());
    let removed = pea(&config, &["uninstall", "1", "--yes"]);
    assert_eq!(removed.code, 0, "{}{}", removed.out, removed.err);
    assert_eq!(files(&player), vec!["Game.exe".to_string(), "Game.ini".to_string()], "la carpeta, como estaba");
}

#[test]
fn the_list_changes_by_itself_and_play_asks_for_an_executable_it_cannot_pick() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("config");
    let royal = root.path().join("Royal");
    write(&royal, &[("Royal.exe", "MZ"), ("Royal Classic.exe", "MZ"), ("mkxp.json", "{}")]);
    let path = royal.to_string_lossy().into_owned();
    let doubtful = pea(&config, &["list", "add", &path, "--profile", "opalo"]);
    assert!(doubtful.code == 4 && doubtful.complains("preloadScript"), "sin preloadScript se pregunta: {}", doubtful.err);
    let added = pea(&config, &["list", "add", &path, "--profile", "opalo", "--name", "Mi Royal", "--yes"]);
    assert_eq!(added.code, 0, "{}{}", added.out, added.err);
    assert!(added.says("Aviso: Royal: No aparece preloadScript en el ejecutable"), "{}", added.out);
    assert!(added.says("Mi Royal: añadido a la lista con el número 1, perfil opalo."), "{}", added.out);
    assert!(!royal.join("accessibility").exists(), "list add no instala nada");
    assert_eq!(pea(&config, &["list", "add", &royal.to_string_lossy(), "--profile", "opalo"]).code, 2);
    assert!(pea(&config, &["list"]).says("1. Mi Royal: sin el mod, perfil opalo."));

    let unasked = pea(&config, &["play", "1"]);
    assert_eq!(unasked.code, 4, "{}", unasked.out);
    assert!(unasked.complains("--exe") && unasked.complains("Royal Classic.exe"), "{}", unasked.err);
    assert_eq!(pea(&config, &["play", "1", "--exe", "Nada"]).code, 2);
    let bad_exe = pea(&config, &["list", "set", "mi royal", "--exe", "Nada.exe"]);
    assert!(bad_exe.code == 2 && bad_exe.complains("No hay ningún Nada.exe"), "{}", bad_exe.err);
    let renamed = pea(&config, &["list", "set", "1", "--name", "Royal", "--exe", "royal classic"]);
    assert_eq!(renamed.code, 0, "{}{}", renamed.out, renamed.err);
    let saved = text(&config.join("config.json"));
    assert!(saved.contains("\"name\": \"Royal\"") && saved.contains("\"executable\": \"Royal Classic.exe\""), "{saved}");
    assert_eq!(pea(&config, &["list", "set", "1"]).code, 2, "sin nada que cambiar");
    let removed = pea(&config, &["list", "remove", "royal"]);
    assert!(removed.code == 0 && removed.says("Royal: quitado de la lista."), "{}", removed.out);
    let empty = pea(&config, &["list"]);
    assert!(empty.says("La lista de juegos está vacía."), "{}", empty.out);
    assert_eq!(pea(&config, &["list", "remove", "1"]).code, 2);
}

#[test]
fn uninstall_takes_only_our_entry_out_of_a_crlf_mkxp_json_the_player_also_loads_scripts_from() {
    let root = tempfile::tempdir().unwrap();
    let game = root.path().join("Juego");
    write(&game, &[("mkxp.json", "{\r\n  // === MOD DE ACCESIBILIDAD (auto) ===\r\n  \"preloadScript\": [\r\n    \
        \"accessibility/preload_access.rb\",\r\n    \"mods/usuario.rb\"\r\n  ],\r\n  \"windowTitle\": \"Juego\"\r\n}\r\n")]);
    let run = pea(&root.path().join("config"), &["uninstall", &game.to_string_lossy(), "--yes"]);
    assert!(run.code == 0 && run.says("Juego: desinstalado."), "{}{}", run.out, run.err);
    let after = text(&game.join("mkxp.json"));
    assert!(!after.contains("preload_access.rb"), "nuestra entrada ya no está:\n{after}");
    assert!(!after.contains("MOD DE ACCESIBILIDAD"), "ni el rótulo del mod:\n{after}");
    assert!(after.contains("\"mods/usuario.rb\""), "la entrada del jugador sigue:\n{after}");
    assert_eq!(live_json(&game)["preloadScript"], serde_json::json!(["mods/usuario.rb"]), "el array sigue:\n{after}");
    assert!(after.ends_with("  ],\r\n  \"windowTitle\": \"Juego\"\r\n}\r\n"), "y el resto, intacto: {after:?}");
}

#[test]
fn a_game_deeper_than_126_characters_is_converted_without_a_warning_and_goes_back_byte_for_byte() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("config");
    let from = fake_mod(root.path()).to_string_lossy().into_owned();
    let engine_dir = fake_engine(root.path());
    let engine = engine_dir.to_string_lossy().into_owned();
    let deep = root.path().join("p".repeat(130usize.saturating_sub(root.path().as_os_str().len()).max(20)));
    let game = titled_player(&deep, "Pokemon Insurgence", "Game", ["Game.rgssad", "MGC_Hmode7.dll"]);
    let dir = game.to_string_lossy().into_owned();
    assert!(dir.len() > 126, "{dir}");
    let before = snapshot(&game);
    let converted = pea(&config, &["local", "install", &dir, "--from", &from, "--engine", &engine, "--yes"]);
    assert_eq!(converted.code, 0, "{}{}", converted.out, converted.err);
    let added = "Pokemon Insurgence: añadido a la lista con el número 1, perfil Pokemon Insurgence.";
    assert!(converted.says(added), "reconocido por el Title de Game.ini: {}", converted.out);
    let warned = converted.says("ruta larga (") || converted.says("ruta muy larga (");
    assert!(!warned, "sin aviso de ruta: {}", converted.out);
    let access = game.join("Game (PokeAccess).exe");
    assert_eq!(text(&game.join("Game.exe")), PLAYER, "el reproductor sigue intacto");
    assert_eq!(text(&access), text(&engine_dir.join("mkxp-z.exe")), "el motor llega aparte");
    assert!(!game.join("Game.exe.access.bak").exists());
    assert_eq!(text(&game.join("font.sf2")), "sf2", "llegan los extras");
    let json = live_json(&game);
    assert_eq!(json["rgssVersion"], 1, "{json}");
    assert_eq!(json["execName"], "Game", "execName nombra el Game.ini y el Game.rgssad del reproductor");
    assert_eq!(json["midiSoundFont"], "font.sf2");
    assert_eq!(json["preloadScript"], serde_json::json!(["accessibility/preload_access.rb"]));
    let data = game.join("accessibility").join("data");
    let record: serde_json::Value = serde_json::from_str(&text(&data.join("engine.json"))).unwrap();
    assert_eq!(record["profile"], "insurgence");
    assert_eq!(record["exe"], "Game (PokeAccess).exe");
    assert!(data.join("installed.json").is_file(), "la instalación queda sellada");
    let hint = "Pokemon Insurgence: Para jugar con accesibilidad, usa la orden play o abre «Game (PokeAccess).exe».";
    assert!(converted.says(hint), "{}", converted.out);

    let removed = pea(&config, &["uninstall", "1", "--yes"]);
    assert_eq!(removed.code, 0, "{}{}", removed.out, removed.err);
    assert!(snapshot(&game) == before, "la carpeta vuelve byte a byte a como estaba: {:?}", files(&game));

    let again = pea(&config, &["local", "install", "1", "--from", &from, "--engine", &engine]);
    assert_eq!(again.code, 0, "un juego de la lista se vuelve a convertir sin preguntar: {}{}", again.out, again.err);
    fs::write(&access, "MZ changed by hand preloadScript").unwrap();
    let kept = pea(&config, &["uninstall", "1", "--yes"]);
    assert_eq!(kept.code, 0, "{}{}", kept.out, kept.err);
    assert_eq!(text(&access), "MZ changed by hand preloadScript", "un exe accesible cambiado a mano no se borra");
    assert_eq!(text(&game.join("Game.exe")), PLAYER);
    assert!(kept.says("Game (PokeAccess).exe ha cambiado desde la conversión: no se borra."), "{}", kept.out);
}

#[test]
fn uranium_is_converted_with_its_exec_name_and_compat_script_and_a_reinstall_puts_back_what_is_missing() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("config");
    let from = fake_mod(root.path()).to_string_lossy().into_owned();
    let engine_dir = fake_engine(root.path());
    let engine = engine_dir.to_string_lossy().into_owned();
    let game = titled_player(root.path(), "Pokemon Uranium", "Uranium", ["Uranium.rgssad", "KleinBitmap.dll"]);
    let before = snapshot(&game);
    let loaders = serde_json::json!(["accessibility/game/compat.rb", "accessibility/preload_access.rb"]);
    let access = game.join("Uranium (PokeAccess).exe");
    let dir = game.to_string_lossy().into_owned();
    let converted = pea(&config, &["local", "install", &dir, "--from", &from, "--engine", &engine, "--yes"]);
    assert_eq!(converted.code, 0, "{}{}", converted.out, converted.err);
    let added = "Pokemon Uranium: añadido a la lista con el número 1, perfil Pokemon Uranium.";
    assert!(converted.says(added), "{}", converted.out);
    assert_eq!(text(&game.join("Uranium.exe")), PLAYER, "el reproductor sigue intacto");
    assert_eq!(text(&access), text(&engine_dir.join("mkxp-z.exe")), "el motor llega como Uranium (PokeAccess).exe");
    assert_eq!(live_json(&game)["execName"], "Uranium");
    assert_eq!(live_json(&game)["preloadScript"], loaders, "el script de compatibilidad, justo antes del cargador");
    assert!(game.join("accessibility").join("game").join("compat.rb").is_file(), "y llega con el perfil");
    assert!(converted.says("«Uranium (PokeAccess).exe»"), "se dice qué exe abrir: {}", converted.out);

    let reinstall = ["local", "install", "1", "--from", &from, "--engine", &engine];
    write(&engine_dir, &[("mkxp-z.exe", "MZ newer engine preloadScript")]);
    let newer = pea(&config, &reinstall);
    assert_eq!(newer.code, 0, "{}{}", newer.out, newer.err);
    assert_eq!(live_json(&game)["preloadScript"], loaders, "sin entradas repetidas");
    assert_eq!(text(&access), "MZ newer engine preloadScript", "el exe accesible pasa al motor nuevo");
    assert_eq!(text(&game.join("Uranium.exe")), PLAYER);
    assert!(!newer.says("Para jugar con accesibilidad"), "reinstalar no repite el aviso: {}", newer.out);

    fs::remove_file(&access).unwrap();
    let restored = pea(&config, &reinstall);
    assert_eq!(restored.code, 0, "sin el exe accesible no se pregunta ni se falla: {}{}", restored.out, restored.err);
    assert_eq!(text(&access), "MZ newer engine preloadScript");

    fs::remove_file(game.join("mkxp.json")).unwrap();
    let rebuilt = pea(&config, &reinstall);
    assert_eq!(rebuilt.code, 0, "{}{}", rebuilt.out, rebuilt.err);
    assert_eq!(live_json(&game)["execName"], "Uranium", "sin su mkxp.json, reinstalar lo rehace");
    assert_eq!(live_json(&game)["preloadScript"], loaders);

    let removed = pea(&config, &["uninstall", "1", "--yes"]);
    assert_eq!(removed.code, 0, "{}{}", removed.out, removed.err);
    assert!(snapshot(&game) == before, "la carpeta vuelve byte a byte a como estaba: {:?}", files(&game));
}

#[test]
fn an_imported_common_comes_with_its_profile_and_goes_when_the_listed_game_takes_another() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("config");
    let mod_dir = fake_mod(root.path());
    let from = mod_dir.to_string_lossy().into_owned();
    let game = mkxp_game(root.path(), "Fusion");
    write(&game, &[("mkxp.json", "{ \"windowTitle\": \"Fusion\" }")]);
    let acc = game.join("accessibility");
    let first = pea(&config, &["local", "install", &game.to_string_lossy(), "--from", &from, "--profile", "anil"]);
    assert_eq!(first.code, 0, "{}{}", first.out, first.err);
    let common = files(&mod_dir.join("games").join("anil_common"));
    assert_eq!(files(&acc.join("common").join("anil_common")), common, "el común llega entero a su carpeta");
    let sealed: serde_json::Value = serde_json::from_str(&text(&acc.join("data").join("installed.json"))).unwrap();
    assert!(sealed["files"].get("common/anil_common/shared.rb").is_some(), "y queda sellado con el resto: {sealed}");

    let other = pea(&config, &["local", "install", "1", "--from", &from, "--profile", "opalo"]);
    assert_eq!(other.code, 0, "{}{}", other.out, other.err);
    assert!(!acc.join("common").exists(), "el común que ya no se importa se va: {:?}", files(&acc));
    assert!(pea(&config, &["list"]).says("1. Fusion: mod 0.6.0, perfil opalo."), "la lista guarda el perfil nuevo");
    let removed = pea(&config, &["uninstall", "1", "--yes"]);
    assert!(removed.code == 0 && !acc.exists(), "{}{}", removed.out, removed.err);
}

#[test]
fn the_real_repository_is_found_from_inside_it_and_installs_the_mod() {
    let launcher = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo = launcher.parent().unwrap();
    if !repo.join("core").join("manifest.rb").is_file() {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("config");
    let game = mkxp_game(root.path(), "Juego");
    let version: serde_json::Value = serde_json::from_str(&text(&repo.join("version.json"))).unwrap();
    let run = pea_in(&config, &["local", "install", &game.to_string_lossy(), "--profile", "generic", "--yes"], launcher);
    assert_eq!(run.code, 0, "{}{}", run.out, run.err);
    let origin = format!("Origen: carpeta {}, mod {}.", repo.display(), version["version"].as_str().unwrap());
    assert_eq!(run.out.lines().next().unwrap_or_default(), origin);
    let installed = game.join("accessibility").join("core").join("manifest.rb");
    assert_eq!(fs::read(installed).unwrap(), fs::read(repo.join("core").join("manifest.rb")).unwrap());
}

fn stamped(game: &str, line: &str) -> String {
    format!("# PokeAccess\n# game: {game}\n{line}\n")
}

#[test]
fn export_and_import_carry_the_players_data_and_only_the_same_game_takes_what_is_bound_to_it() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("config");
    let from = fake_mod(root.path()).to_string_lossy().into_owned();
    for (name, profile) in [("Pokemon Anil", "anil"), ("Anil de nuevo", "anil"), ("Pokemon Opalo", "opalo")] {
        let dir = mkxp_game(root.path(), name).to_string_lossy().into_owned();
        let added = pea(&config, &["local", "install", &dir, "--from", &from, "--profile", profile]);
        assert_eq!(added.code, 0, "{}{}", added.out, added.err);
    }
    let data = |name: &str| root.path().join(name).join("accessibility").join("data");
    write(&data("Pokemon Anil"), &[
        ("settings.ini", "voz=2"),
        ("verbosity.txt", "corto=lectura:brief"),
        ("tags.txt", &stamped("anil", "1:2=Cofre")),
        ("marks.txt", &stamped("anil", "3:4=Puerta")),
        ("recordings/r1.txt", "grabado"),
    ]);
    let out = root.path().join("copias");
    fs::create_dir_all(&out).unwrap();
    let exported = pea_in(&config, &["export", "1"], &out);
    assert_eq!(exported.code, 0, "{}{}", exported.out, exported.err);
    assert!(out.join("Pokemon Anil - PokeAccess.zip").is_file(), "con el nombre de la lista: {:?}", files(&out));
    assert!(exported.says("Contiene: ajustes, esquemas de verbosidad, etiquetas, marcadores."), "{}", exported.out);
    let again = pea_in(&config, &["export", "pokemon anil"], &out);
    assert!(again.code == 4 && again.complains("--yes"), "no se sustituye sin preguntar: {}", again.err);
    assert_eq!(pea_in(&config, &["export", "1", "--yes"], &out).code, 0);
    assert_eq!(pea_in(&config, &["export", "1", "sin extension"], &out).code, 0);
    assert!(out.join("sin extension.zip").is_file(), "{:?}", files(&out));
    let nothing = pea_in(&config, &["export", "3"], &out);
    assert!(nothing.code == 0 && nothing.says("Pokemon Opalo: no hay datos del jugador que exportar."), "{}", nothing.out);
    assert_eq!(files(&out).len(), 2, "sin datos no se escribe nada");

    write(&data("Anil de nuevo"), &[("settings.ini", "voz=1")]);
    let same = pea_in(&config, &["import", "2", "Pokemon Anil - PokeAccess.zip"], &out);
    assert_eq!(same.code, 0, "{}{}", same.out, same.err);
    for line in ["Anil de nuevo: datos importados de", "Aplicados ya: ajustes.", "settings.ini.bak.",
        "Se fusionarán al abrir el juego: esquemas de verbosidad, etiquetas, marcadores."] {
        assert!(same.says(line), "{line}\n{}", same.out);
    }
    let copy = data("Anil de nuevo");
    assert_eq!((text(&copy.join("settings.ini")), text(&copy.join("settings.ini.bak"))), ("voz=2".into(), "voz=1".into()));
    assert_eq!(text(&copy.join("tags_import.txt")), stamped("anil", "1:2=Cofre"), "lo fusiona el mod al abrir el juego");
    assert!(copy.join("marks_import.txt").is_file() && copy.join("verbosity_import.txt").is_file());
    assert!(!copy.join("recordings").exists(), "las grabaciones no viajan");

    let other = pea_in(&config, &["import", "Pokemon Opalo", "Pokemon Anil - PokeAccess.zip"], &out);
    assert_eq!(other.code, 0, "{}{}", other.out, other.err);
    assert!(other.says("Rechazados, porque son de anil: etiquetas, marcadores."), "{}", other.out);
    assert_eq!(text(&data("Pokemon Opalo").join("settings.ini")), "voz=2", "lo general entra siempre");
    assert!(!data("Pokemon Opalo").join("tags_import.txt").exists());

    let unknown = pea_in(&config, &["import", "1", "no esta.zip"], &out);
    assert!(unknown.code == 2 && unknown.complains("No existe el archivo"), "{}", unknown.err);
    fs::write(out.join("roto.zip"), "no es un zip").unwrap();
    let broken = pea_in(&config, &["import", "1", "roto.zip"], &out);
    assert!(broken.code == 1 && broken.complains("no es un archivo de datos de PokeAccess"), "{}", broken.err);
    for line in [&["import", "1"][..], &["export"], &["export", "all"], &["import", "all", "roto.zip"]] {
        let wrong = pea_in(&config, line, &out);
        assert!(wrong.code == 2 && wrong.complains("help"), "{line:?}: {}", wrong.err);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let exe = root.path().join("Pokemon Opalo").join("Game.exe");
        let _open = fs::OpenOptions::new().read(true).share_mode(1).open(exe).unwrap();
        let busy = pea_in(&config, &["import", "3", "Pokemon Anil - PokeAccess.zip"], &out);
        assert_eq!(busy.code, 5, "con el juego abierto no se importa: {}", busy.err);
    }
}

#[test]
fn status_says_what_needs_repair_and_the_exit_code_counts_it() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("config");
    let from = fake_mod(root.path()).to_string_lossy().into_owned();
    let game = mkxp_game(root.path(), "Pokemon Anil");
    assert_eq!(pea(&config, &["local", "install", &game.to_string_lossy(), "--from", &from]).code, 0);
    let status = ["local", "status", "--from", &from, "--exit-code"];
    let sound = pea(&config, &status);
    assert!(sound.code == 0 && !sound.says("reparación"), "{}{}", sound.out, sound.err);
    fs::remove_file(game.join("accessibility").join("core").join("nav").join("locator.rb")).unwrap();
    fs::write(game.join("mkxp.json"), "{}").unwrap();
    let broken = pea(&config, &status);
    assert_eq!(broken.code, 8, "{}{}", broken.out, broken.err);
    let line = "1. Pokemon Anil: Al día. Mod instalado: 0.6.0. Perfil: Pokemon Anil. Necesita reparación: el juego ya no \
        carga el mod; archivos del mod que faltan o han cambiado: 1. La orden install lo repara.";
    assert!(broken.says(line), "{}", broken.out);
    assert_eq!(pea(&config, &["local", "status", "1", "--from", &from]).code, 0, "sin --exit-code sale con 0");
    let repaired = pea(&config, &["local", "install", "1", "--from", &from]);
    assert_eq!(repaired.code, 0, "{}{}", repaired.out, repaired.err);
    let after = pea(&config, &status);
    assert!(after.code == 0 && !after.says("reparación"), "instalar lo repara: {}", after.out);
}

fn cut_halfway(game: &Path, access: &str) {
    let record = game.join("accessibility").join("data").join("engine.json");
    let mut json: serde_json::Value = serde_json::from_str(&text(&record)).unwrap();
    json["pending"] = serde_json::Value::Bool(true);
    fs::write(&record, json.to_string()).unwrap();
    fs::write(game.join(access), "MZ").unwrap();
    fs::remove_file(game.join("mkxp.json")).unwrap();
}

#[test]
fn a_conversion_cut_halfway_is_finished_by_install_and_undone_by_uninstall() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("config");
    let from = fake_mod(root.path()).to_string_lossy().into_owned();
    let engine_dir = fake_engine(root.path());
    let engine = engine_dir.to_string_lossy().into_owned();
    let game = titled_player(root.path(), "Pokemon Insurgence", "Game", ["Game.rgssad", "MGC_Hmode7.dll"]);
    let before = snapshot(&game);
    let dir = game.to_string_lossy().into_owned();
    let converted = pea(&config, &["local", "install", &dir, "--from", &from, "--engine", &engine, "--yes"]);
    assert_eq!(converted.code, 0, "{}{}", converted.out, converted.err);
    let access = "Game (PokeAccess).exe";

    cut_halfway(&game, access);
    let check = pea(&config, &["local", "check", "1", "--from", &from, "--engine", &engine]);
    assert!(check.code == 0 && check.says("Resultado: la conversión a mkxp-z se cortó a medias."), "{}", check.out);
    let play = pea(&config, &["play", "1"]);
    assert!(play.code == 1 && play.complains("La orden install lo vuelve a poner"), "a medias no se abre: {}", play.err);
    let finished = pea(&config, &["local", "install", "1", "--from", &from, "--engine", &engine]);
    assert_eq!(finished.code, 0, "{}{}", finished.out, finished.err);
    assert_eq!(text(&game.join(access)), text(&engine_dir.join("mkxp-z.exe")), "el motor entero");
    assert_eq!(live_json(&game)["execName"], "Game");
    assert_eq!(live_json(&game)["preloadScript"], serde_json::json!(["accessibility/preload_access.rb"]));
    let record = text(&game.join("accessibility").join("data").join("engine.json"));
    assert!(!record.contains("pending"), "{record}");

    cut_halfway(&game, access);
    let removed = pea(&config, &["uninstall", "1", "--yes"]);
    assert_eq!(removed.code, 0, "{}{}", removed.out, removed.err);
    assert!(snapshot(&game) == before, "ningún archivo se queda sin dueño: {:?}", files(&game));
}

#[test]
fn a_converted_game_moves_to_the_engine_the_mod_names_now_and_is_told_when_its_engine_is_gone() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("config");
    let mod_dir = fake_mod(root.path());
    let from = mod_dir.to_string_lossy().into_owned();
    let game = titled_player(root.path(), "Pokemon Uranium", "Uranium", ["Uranium.rgssad", "KleinBitmap.dll"]);
    let before = snapshot(&game);
    let access = game.join("Uranium (PokeAccess).exe");
    let first = pea(&config, &["local", "install", &game.to_string_lossy(), "--from", &from, "--yes"]);
    assert_eq!(first.code, 0, "{}{}", first.out, first.err);
    assert_eq!(text(&access), "MZ bundled engine preloadScript");

    let catalog = text(&mod_dir.join("games").join("catalog.json"));
    let moved_on = catalog.replace(r#""exes":["Uranium.exe"],"convert":"e1""#, r#""exes":["Uranium.exe"],"convert":"e2""#);
    assert_ne!(catalog, moved_on);
    write(&mod_dir, &[
        ("games/catalog.json", &moved_on),
        ("assets/engine/e2/mkxp-z.exe", "MZ engine two preloadScript"),
        ("assets/engine/e2/extras/two.sf2", "two"),
    ]);
    let moved = pea(&config, &["local", "install", "1", "--from", &from]);
    assert_eq!(moved.code, 0, "{}{}", moved.out, moved.err);
    assert!(moved.says("Pokemon Uranium: El juego pasa al motor e2"), "{}", moved.out);
    assert_eq!(text(&access), "MZ engine two preloadScript");
    assert_eq!(text(&game.join("two.sf2")), "two");
    let json = live_json(&game);
    assert_eq!((json["execName"].as_str(), json["midiSoundFont"].as_str()), (Some("Uranium"), Some("two.sf2")));
    let loaders = serde_json::json!(["accessibility/game/compat.rb", "accessibility/preload_access.rb"]);
    assert_eq!(json["preloadScript"], loaders);

    fs::remove_dir_all(mod_dir.join("assets").join("engine").join("e2")).unwrap();
    let gone = pea(&config, &["local", "update", "1", "--from", &from]);
    assert_eq!(gone.code, 0, "{}{}", gone.out, gone.err);
    assert!(gone.says("Pokemon Uranium: El mod ya no trae el motor de este juego (e2)"), "{}", gone.out);
    assert_eq!(text(&access), "MZ engine two preloadScript", "no se toca");
    let removed = pea(&config, &["uninstall", "1", "--yes"]);
    assert_eq!(removed.code, 0, "{}{}", removed.out, removed.err);
    assert!(snapshot(&game) == before, "la carpeta vuelve byte a byte: {:?}", files(&game));
}
