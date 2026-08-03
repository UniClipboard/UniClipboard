pub(crate) fn is_development_environment(value: Option<&str>) -> bool {
    value.is_some_and(|value| {
        value.eq_ignore_ascii_case("development") || value.eq_ignore_ascii_case("dev")
    })
}

pub(crate) fn development_mode() -> bool {
    let runtime_environment = std::env::var("UNICLIPBOARD_ENV").ok();
    is_development_environment(runtime_environment.as_deref())
}

pub(crate) fn runtime_profile() -> String {
    uc_app_paths::resolve_profile(None).unwrap_or_else(|| "default".to_string())
}

pub(crate) fn should_disable_gui_single_instance(
    explicit_disable: Option<&str>,
    development_mode: bool,
) -> bool {
    explicit_disable == Some("1") || development_mode
}

#[cfg(test)]
mod tests {
    use super::{is_development_environment, should_disable_gui_single_instance};

    #[test]
    fn development_environment_disables_gui_single_instance() {
        assert!(should_disable_gui_single_instance(
            None,
            is_development_environment(Some("development"))
        ));
    }

    #[test]
    fn production_environment_keeps_gui_single_instance() {
        assert!(!should_disable_gui_single_instance(
            None,
            is_development_environment(Some("production"))
        ));
        assert!(!should_disable_gui_single_instance(
            None,
            is_development_environment(None)
        ));
    }

    #[test]
    fn explicit_override_still_disables_gui_single_instance() {
        assert!(should_disable_gui_single_instance(Some("1"), false));
    }
}
