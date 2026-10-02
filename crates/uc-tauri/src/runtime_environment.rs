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

pub(crate) fn should_disable_gui_single_instance(explicit_disable: Option<&str>) -> bool {
    explicit_disable == Some("1")
}

#[cfg(test)]
mod tests {
    use super::should_disable_gui_single_instance;

    #[test]
    fn gui_single_instance_is_enabled_by_default() {
        assert!(!should_disable_gui_single_instance(None));
    }

    #[test]
    fn explicit_override_still_disables_gui_single_instance() {
        assert!(should_disable_gui_single_instance(Some("1")));
    }
}
