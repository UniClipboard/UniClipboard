//! Process-wide environment policy required before Tauri starts.

const LOOPBACK_PROXY_BYPASS: [&str; 3] = ["localhost", "127.0.0.1", "::1"];

/// Ensure every network stack spawned by the GUI bypasses proxies for loopback.
///
/// This must run at process entry, before tracing workers, GTK, WebKit, or the
/// daemon sidecar create threads or inherit the environment. Native daemon
/// clients also disable proxies at the HTTP-client layer; this process policy
/// covers WebKit `fetch` and WebSocket traffic that does not use those clients.
pub fn prepare_process_environment() {
    let merged = merge_no_proxy_values([
        std::env::var("NO_PROXY").ok(),
        std::env::var("no_proxy").ok(),
    ]);

    // SAFETY: The binary calls this as the first operation in `main`, before
    // any worker threads, GTK/WebKit initialization, or sidecar spawning.
    unsafe {
        std::env::set_var("NO_PROXY", &merged);
        std::env::set_var("no_proxy", merged);
    }
}

fn merge_no_proxy_values(values: impl IntoIterator<Item = Option<String>>) -> String {
    let mut entries = Vec::new();

    for value in values.into_iter().flatten() {
        for entry in value
            .split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
        {
            if entry == "*" {
                return "*".to_string();
            }
            if !entries.iter().any(|existing| existing == entry) {
                entries.push(entry.to_string());
            }
        }
    }

    for loopback in LOOPBACK_PROXY_BYPASS {
        if !entries.iter().any(|existing| existing == loopback) {
            entries.push(loopback.to_string());
        }
    }

    entries.join(",")
}

#[cfg(test)]
mod tests {
    use super::merge_no_proxy_values;

    #[test]
    fn adds_loopback_hosts_when_proxy_bypass_is_absent() {
        assert_eq!(
            merge_no_proxy_values([None, None]),
            "localhost,127.0.0.1,::1"
        );
    }

    #[test]
    fn preserves_both_variable_values_and_deduplicates_loopback_hosts() {
        assert_eq!(
            merge_no_proxy_values([
                Some("example.com, localhost".to_string()),
                Some("internal.test,127.0.0.1".to_string()),
            ]),
            "example.com,localhost,internal.test,127.0.0.1,::1"
        );
    }

    #[test]
    fn normalizes_empty_entries_and_whitespace() {
        assert_eq!(
            merge_no_proxy_values([Some(" , example.com ,, ".to_string()), None]),
            "example.com,localhost,127.0.0.1,::1"
        );
    }

    #[test]
    fn preserves_wildcard_bypass() {
        assert_eq!(
            merge_no_proxy_values([Some("example.com,*".to_string()), None]),
            "*"
        );
    }
}
