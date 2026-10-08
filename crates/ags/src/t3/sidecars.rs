use super::registration::Registration;
use crate::config::ValidatedConfig;
use std::io;
use std::path::Path;

pub struct Sidecars {
    pub _browser: Option<crate::browser::BrowserSidecar>,
    pub ui: Option<crate::host_ui::HostUiGuard>,
    pub clipboard: Option<crate::clipboard::ClipboardGuard>,
    pub relay: Option<crate::webview_relay::WebviewRelayGuard>,
    pub auth: Option<crate::auth_proxy::host::AuthProxyGuard>,
    pub psp: Option<crate::psp::PspGuard>,
}

fn optional<T>(result: Result<T, impl std::fmt::Display>) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            eprintln!("warning: T3 sidecar: {error}");
            None
        }
    }
}

impl Sidecars {
    pub fn start(
        registration: &Registration,
        config: &mut ValidatedConfig,
        base: &Path,
    ) -> io::Result<Self> {
        let browser = if registration.settings.browser {
            let browser = crate::browser::start_if_needed(true, &config.browser)
                .map_err(|error| io::Error::other(error.to_string()))?;
            if let Some(browser) = &browser {
                config.browser.debug_port = browser.port;
            }
            browser
        } else {
            None
        };
        let ui = config
            .host_ui
            .enabled
            .then(|| {
                optional(crate::host_ui::start(
                    &base.join("host-ui"),
                    registration.name(),
                    &config.host_ui,
                ))
            })
            .flatten();
        let mode = config.clipboard.effective_mode();
        let clipboard = if mode.can_read() {
            let approval = crate::clipboard::ClipboardApprovalConfig {
                required: config.clipboard.approval_required,
                window_seconds: config.clipboard.approval_seconds,
                approve_writes: config.clipboard.approve_writes,
            };
            let guard = optional(crate::clipboard::start(
                &base.join("clipboard"),
                mode,
                config.clipboard.max_bytes,
                approval,
                ui.as_ref().map(|guard| guard.socket_path.as_path()),
            ));
            if let Some(guard) = &guard {
                crate::assets::ensure_clipboard_assets(&guard.runtime_dir)?;
            }
            guard
        } else {
            None
        };
        let relay = optional(crate::webview_relay::start(&base.join("webview-relay")));
        if let Some(guard) = &relay {
            crate::assets::ensure_webview_relay_assets(&guard.runtime_dir)?;
        }
        let auth = optional(crate::auth_proxy::start(
            &base.join("auth-proxy"),
            config.auth_proxy.auto_allow_domains.clone(),
            relay
                .as_ref()
                .map(|guard| guard.runtime_dir.join(crate::webview_relay::SOCKET_NAME)),
            ui.as_ref().map(|guard| guard.socket_path.clone()),
        ));
        if let Some(guard) = &auth {
            crate::assets::ensure_auth_proxy_shim(&guard.runtime_dir)?;
        }
        let psp = if registration.settings.psp {
            Some(
                crate::psp::start_in(
                    &config.psp.binary,
                    registration.settings.psp_keep,
                    base.join("psp"),
                )
                .map_err(io::Error::other)?,
            )
        } else {
            None
        };
        Ok(Self {
            _browser: browser,
            ui,
            clipboard,
            relay,
            auth,
            psp,
        })
    }
}
