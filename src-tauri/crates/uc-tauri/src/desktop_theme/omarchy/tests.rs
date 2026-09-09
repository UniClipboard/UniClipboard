use super::*;

const DARK: &str =
    "mode = 'dark'\nbackground = '#2d353b'\nforeground = '#d3c6aa'\naccent = '#7fbbb3'";
const LIGHT: &str =
    "mode = 'light'\nbackground = '#ffffff'\nforeground = '#202020'\naccent = '#1244aa'";

#[test]
fn requires_active_omarchy_and_current_directory() {
    let home = tempfile::tempdir().unwrap();
    let install = tempfile::tempdir().unwrap();
    let paths = || {
        (
            Some(home.path().to_owned()),
            Some(install.path().to_owned()),
        )
    };
    let (h, i) = paths();
    assert!(source_directory(h, i).is_none());
    std::fs::create_dir_all(install.path().join("bin")).unwrap();
    std::fs::write(install.path().join("bin/omarchy-theme-set"), "").unwrap();
    std::fs::create_dir_all(home.path().join(".local/state/omarchy/current")).unwrap();
    let (h, i) = paths();
    assert!(source_directory(h.clone(), i).is_some());
    assert!(source_directory(h, None).is_none());
}

#[test]
fn palettes_supply_all_semantic_tokens_and_contrasting_button_text() {
    let dark = parse_palette(DARK, false).unwrap();
    let light = parse_palette(LIGHT, false).unwrap();
    assert!(dark.dark);
    assert!(!light.dark);
    assert_eq!(dark.variables.len(), 32);
    assert_eq!(dark.variables["--background"], "#2d353b");
    assert_eq!(dark.variables["--primary-foreground"], "#000000");
    assert_eq!(light.variables["--primary-foreground"], "#ffffff");
    assert_eq!(
        dark.variables.keys().collect::<Vec<_>>(),
        light.variables.keys().collect::<Vec<_>>()
    );
    for value in dark.variables.values() {
        assert!(Color::parse(value).is_some());
    }
    let wire = serde_json::to_value(super::super::DesktopThemeSnapshot {
        revision: 1,
        theme: Some(dark),
    })
    .unwrap();
    assert_eq!(wire["theme"]["dark"], true);
    assert!(wire["theme"]["variables"].is_object());
}

#[test]
fn rejects_invalid_palettes_without_exposing_contents() {
    assert_eq!(
        parse_palette("private malformed content", false).unwrap_err(),
        "invalid_toml"
    );
    assert_eq!(
        parse_palette("background = 'url(secret)'", false).unwrap_err(),
        "missing_background"
    );
    assert!(parse_palette(&DARK.replace("'dark'", "'invalid'"), false).is_err());
    assert!(Color::parse("#💩ab").is_none());
}

#[test]
fn resolves_mode_without_explicit_metadata() {
    assert!(
        parse_palette(&DARK.replace("mode = 'dark'\n", ""), false)
            .unwrap()
            .dark
    );
    assert!(
        !parse_palette(&LIGHT.replace("mode = 'light'\n", ""), false)
            .unwrap()
            .dark
    );
    assert!(
        !parse_palette(&DARK.replace("mode = 'dark'\n", ""), true)
            .unwrap()
            .dark
    );
}

#[tokio::test]
async fn watcher_survives_repeated_directory_replacement_and_in_place_edits() {
    let temp = tempfile::tempdir().unwrap();
    let current = temp.path().join("current");
    std::fs::create_dir_all(current.join("theme")).unwrap();
    let (tx, mut rx) = mpsc::channel(1);
    let watcher = watch_directory(&current, tx).unwrap();
    for palette in [DARK, LIGHT, DARK] {
        let next = temp.path().join("next-theme");
        std::fs::create_dir(&next).unwrap();
        std::fs::write(next.join("colors.toml"), palette).unwrap();
        std::fs::remove_dir_all(current.join("theme")).unwrap();
        std::fs::rename(next, current.join("theme")).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(3), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            read_theme(&current).unwrap(),
            parse_palette(palette, false).unwrap()
        );
        tokio::time::sleep(std::time::Duration::from_millis(180)).await;
        while rx.try_recv().is_ok() {}
    }
    std::fs::write(current.join("theme/colors.toml"), LIGHT).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(3), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(!read_theme(&current).unwrap().dark);
    drop(watcher);
}

#[test]
fn broadcasts_only_valid_changes_and_retains_the_last_palette_on_failure() {
    use tauri::Listener;
    let app = tauri::test::mock_builder()
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    let state = DesktopThemeState::default();
    let (tx, rx) = std::sync::mpsc::channel();
    app.listen(EVENT, move |event| {
        tx.send(event.payload().to_owned()).unwrap();
    });
    publish(app.handle(), &state, parse_palette(DARK, false));
    publish(app.handle(), &state, parse_palette(DARK, false));
    publish(app.handle(), &state, Err("invalid_toml"));
    assert_eq!(state.snapshot().unwrap().revision, 1);
    assert_eq!(
        state.snapshot().unwrap().theme.unwrap().variables["--background"],
        "#2d353b"
    );
    publish(app.handle(), &state, parse_palette(LIGHT, false));
    let events: Vec<serde_json::Value> = rx
        .try_iter()
        .map(|value| serde_json::from_str(&value).unwrap())
        .collect();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["revision"], 1);
    assert_eq!(events[1]["revision"], 2);
    assert_eq!(events[1]["theme"]["dark"], false);
}
