use crate::browser::{default_browser, SUPPORTED_BROWSERS};
use anyhow::{bail, Context};
use serde::Deserialize;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "kebab-case", deny_unknown_fields)]
pub struct Config {
    yt_dlp: YtdlpConfig,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "kebab-case", deny_unknown_fields)]
struct YtdlpConfig {
    browser: Option<String>,
    use_cookies: bool,
}

impl Config {
    pub fn path() -> anyhow::Result<PathBuf> {
        // Intentionally use ~/.config on every OS, including Windows and macOS.
        Ok(std::env::home_dir()
            .context("Could not determine the home directory for mpvrun config")?
            .join(".config")
            .join("mpvrun")
            .join("config.toml"))
    }

    pub fn load() -> anyhow::Result<Self> {
        Self::load_from(&Self::path()?)
    }

    fn load_from(path: &Path) -> anyhow::Result<Self> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(Self::default()),
            Err(err) => {
                return Err(err).with_context(|| format!("Could not read {}", path.display()))
            }
        };
        Self::parse(&text).with_context(|| format!("Invalid config in {}", path.display()))
    }

    fn parse(text: &str) -> anyhow::Result<Self> {
        let mut config: Self = toml::from_str(text)?;
        if let Some(browser) = &mut config.yt_dlp.browser {
            *browser = browser.to_ascii_lowercase();
            if !SUPPORTED_BROWSERS.contains(&browser.as_str()) {
                bail!(
                    "Unsupported yt-dlp browser {browser:?}; expected one of: {}",
                    SUPPORTED_BROWSERS.join(", ")
                );
            }
        }
        Ok(config)
    }

    pub fn cookie_browser(&self, cli: Option<&Option<String>>) -> Option<String> {
        self.resolve_cookie_browser(cli, default_browser)
    }

    fn resolve_cookie_browser(
        &self,
        cli: Option<&Option<String>>,
        detect_browser: impl FnOnce() -> &'static str,
    ) -> Option<String> {
        if cli.is_none() && !self.yt_dlp.use_cookies {
            return None;
        }
        Some(
            cli.and_then(Option::as_deref)
                .or(self.yt_dlp.browser.as_deref())
                .map(str::to_ascii_lowercase)
                .unwrap_or_else(|| detect_browser().to_owned()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookie_precedence_covers_config_and_cli_combinations() {
        for use_cookies in [false, true] {
            for configured in [None, Some("firefox")] {
                let config = Config {
                    yt_dlp: YtdlpConfig {
                        browser: configured.map(str::to_owned),
                        use_cookies,
                    },
                };
                for cli in [None, Some(None), Some(Some("EDGE".to_owned()))] {
                    let expected = match &cli {
                        Some(Some(_)) => Some("edge"),
                        Some(None) => Some(configured.unwrap_or("brave")),
                        None if use_cookies => Some(configured.unwrap_or("brave")),
                        None => None,
                    };
                    let resolved = config.resolve_cookie_browser(cli.as_ref(), || "brave");
                    assert_eq!(resolved.as_deref(), expected);
                }
            }
        }
    }

    #[test]
    fn defaults_and_browser_only_config_do_not_enable_cookies() {
        for text in ["", "[yt-dlp]", "[yt-dlp]\nbrowser = 'FIREFOX'"] {
            let config = Config::parse(text).unwrap();
            assert_eq!(
                config.resolve_cookie_browser(None, || panic!("unneeded browser lookup")),
                None
            );
        }
        let config = Config::parse("[yt-dlp]\nbrowser = 'FIREFOX'").unwrap();
        assert_eq!(
            config.resolve_cookie_browser(Some(&None), || panic!("unneeded browser lookup")),
            Some("firefox".to_owned())
        );
    }

    #[test]
    fn invalid_settings_are_reported() {
        for text in [
            "[yt-dlp]\nbrowser = 'unsupported'",
            "[yt-dlp]\nbrowser = 42",
            "[yt-dlp]\nuse-cookies = 'true'",
            "[yt-dlp]\nuse-cookie = true",
            "[yt-dlp",
        ] {
            assert!(Config::parse(text).is_err(), "accepted {text:?}");
        }
    }

    #[test]
    fn missing_config_uses_defaults_and_example_is_valid() {
        let missing = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("missing-mpvrun-config")
            .join("config.toml");
        assert!(Config::load_from(&missing)
            .unwrap()
            .cookie_browser(None)
            .is_none());
        let example = Config::parse(include_str!("../config.example.toml")).unwrap();
        assert!(example.cookie_browser(None).is_none());
    }
}
