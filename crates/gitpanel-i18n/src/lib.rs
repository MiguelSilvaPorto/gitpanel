//! Interface translations.
//!
//! Each locale is a directory under `locales/<tag>/` containing Fluent
//! catalogs. To add a language, create that directory, add the resources to
//! `LOCALES`, and run the tests: the parity test checks every key in
//! `keys::ALL` against every registered catalog.
#![forbid(unsafe_code)]

pub mod keys;

use std::borrow::Cow;
use std::fmt;

use fluent_bundle::{FluentArgs, FluentBundle, FluentResource, FluentValue};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;
use unic_langid::LanguageIdentifier;

const FALLBACK_LOCALE_ID: &str = "pt-BR";

const PT_BR_COMMON: &str = include_str!("../locales/pt-BR/common.ftl");
const PT_BR_REPOS: &str = include_str!("../locales/pt-BR/repos.ftl");
const PT_BR_COMMITS: &str = include_str!("../locales/pt-BR/commits.ftl");
const PT_BR_SYNC: &str = include_str!("../locales/pt-BR/sync.ftl");
const PT_BR_CHARTS: &str = include_str!("../locales/pt-BR/charts.ftl");
const PT_BR_DIFF: &str = include_str!("../locales/pt-BR/diff.ftl");
const PT_BR_SETTINGS: &str = include_str!("../locales/pt-BR/settings.ftl");
const PT_BR_ERRORS: &str = include_str!("../locales/pt-BR/errors.ftl");
const PT_BR_TIME: &str = include_str!("../locales/pt-BR/time.ftl");
const PT_BR_VALIDATION: &str = include_str!("../locales/pt-BR/validation.ftl");

const PT_BR_RESOURCES: &[&str] = &[
    PT_BR_COMMON,
    PT_BR_REPOS,
    PT_BR_COMMITS,
    PT_BR_SYNC,
    PT_BR_CHARTS,
    PT_BR_DIFF,
    PT_BR_SETTINGS,
    PT_BR_ERRORS,
    PT_BR_TIME,
    PT_BR_VALIDATION,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocaleMeta {
    pub id: &'static str,
    pub name: &'static str,
    pub date_short: &'static str,
    pub date_time: &'static str,
    pub resources: &'static [&'static str],
}

static LOCALES_ARRAY: [LocaleMeta; 1] = [LocaleMeta {
    id: FALLBACK_LOCALE_ID,
    name: "Português (Brasil)",
    date_short: "%d/%m/%Y",
    date_time: "%d/%m/%Y %H:%M",
    resources: PT_BR_RESOURCES,
}];

pub static LOCALES: &[LocaleMeta] = &LOCALES_ARRAY;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Locale {
    meta: &'static LocaleMeta,
}

impl Locale {
    fn from_meta(meta: &'static LocaleMeta) -> Self {
        Self { meta }
    }

    pub fn resolve(preference: &LanguagePreference) -> Self {
        let fallback = fallback_meta();
        let requested = match preference {
            LanguagePreference::System => system_locale_tag(),
            LanguagePreference::Locale(id) => Some(id.clone()),
        };
        let Some(requested) = requested.and_then(|tag| normalize_tag(&tag)) else {
            return Self::from_meta(fallback);
        };
        let requested_language = requested
            .to_string()
            .split('-')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        if let Some(meta) = LOCALES.iter().find(|meta| {
            meta.id
                .split('-')
                .next()
                .unwrap_or_default()
                .eq_ignore_ascii_case(&requested_language)
        }) {
            return Self::from_meta(meta);
        }
        Self::from_meta(fallback)
    }

    pub fn available() -> impl Iterator<Item = Locale> {
        LOCALES.iter().map(Self::from_meta)
    }

    pub fn id(&self) -> &'static str {
        self.meta.id
    }

    pub fn name(&self) -> &'static str {
        self.meta.name
    }

    pub fn date_short(&self) -> &'static str {
        self.meta.date_short
    }

    pub fn date_time(&self) -> &'static str {
        self.meta.date_time
    }
}

fn fallback_meta() -> &'static LocaleMeta {
    &LOCALES_ARRAY[0]
}

fn normalize_tag(tag: &str) -> Option<LanguageIdentifier> {
    let cleaned = tag
        .split('.')
        .next()?
        .trim()
        .replace('_', "-")
        .to_ascii_lowercase();
    cleaned.parse().ok()
}

fn system_locale_tag() -> Option<String> {
    sys_locale::get_locale()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LanguagePreference {
    System,
    Locale(String),
}

impl Default for LanguagePreference {
    fn default() -> Self {
        Self::Locale(FALLBACK_LOCALE_ID.to_owned())
    }
}

impl LanguagePreference {
    pub fn locale_id(&self) -> Option<&str> {
        match self {
            Self::System => None,
            Self::Locale(id) => Some(id),
        }
    }
}

impl fmt::Display for LanguagePreference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::System => f.write_str("system"),
            Self::Locale(id) => f.write_str(id),
        }
    }
}

impl Serialize for LanguagePreference {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for LanguagePreference {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        if value.eq_ignore_ascii_case("system") {
            Ok(Self::System)
        } else {
            Ok(Self::Locale(value))
        }
    }
}

#[derive(Debug, Error)]
pub enum I18nError {
    #[error("invalid locale: {0}")]
    InvalidLocale(String),
    #[error("could not parse {locale} catalog: {details}")]
    Parse {
        locale: &'static str,
        details: String,
    },
    #[error("could not build {locale} bundle: {details}")]
    Build {
        locale: &'static str,
        details: String,
    },
}

pub struct Translator {
    locale: Locale,
    bundle: FluentBundle<FluentResource>,
    fallback: Option<FluentBundle<FluentResource>>,
}

impl Translator {
    pub fn new(preference: &LanguagePreference) -> Result<Self, I18nError> {
        let locale = Locale::resolve(preference);
        let bundle = build_bundle(locale.meta)?;
        let fallback = if locale.id() == FALLBACK_LOCALE_ID {
            None
        } else {
            Some(build_bundle(fallback_meta())?)
        };
        Ok(Self {
            locale,
            bundle,
            fallback,
        })
    }

    pub fn locale(&self) -> Locale {
        self.locale
    }

    pub fn available() -> impl Iterator<Item = Locale> {
        Locale::available()
    }

    pub fn has_key(&self, key: &str) -> bool {
        self.bundle.get_message(key).is_some()
            || self
                .fallback
                .as_ref()
                .is_some_and(|bundle| bundle.get_message(key).is_some())
    }

    pub fn text(&self, key: &'static str) -> String {
        self.text_args(key, &[])
    }

    pub fn text_args(&self, key: &'static str, args: &[Arg<'_>]) -> String {
        let fluent_args = make_args(args);
        if let Some(value) = format_message(&self.bundle, key, fluent_args.as_ref()) {
            return value;
        }
        if let Some(fallback) = &self.fallback
            && let Some(value) = format_message(fallback, key, fluent_args.as_ref())
        {
            return value;
        }
        tracing::warn!(key, "missing translation key");
        key.to_owned()
    }
}

#[derive(Clone, Copy, Debug)]
pub enum ArgValue<'a> {
    Str(&'a str),
    Int(i64),
}

#[derive(Clone, Copy, Debug)]
pub struct Arg<'a> {
    pub name: &'a str,
    pub value: ArgValue<'a>,
}

impl<'a> Arg<'a> {
    pub fn str(name: &'a str, value: &'a str) -> Self {
        Self {
            name,
            value: ArgValue::Str(value),
        }
    }

    pub fn int(name: &'a str, value: i64) -> Self {
        Self {
            name,
            value: ArgValue::Int(value),
        }
    }
}

impl<'a> From<(&'a str, &'a str)> for Arg<'a> {
    fn from((name, value): (&'a str, &'a str)) -> Self {
        Self::str(name, value)
    }
}

fn make_args<'a>(args: &'a [Arg<'a>]) -> Option<FluentArgs<'a>> {
    if args.is_empty() {
        return None;
    }
    let mut fluent_args = FluentArgs::new();
    for arg in args {
        match arg.value {
            ArgValue::Str(value) => fluent_args.set(arg.name, FluentValue::from(value)),
            ArgValue::Int(value) => fluent_args.set(arg.name, FluentValue::from(value)),
        }
    }
    Some(fluent_args)
}

fn format_message(
    bundle: &FluentBundle<FluentResource>,
    key: &'static str,
    args: Option<&FluentArgs<'_>>,
) -> Option<String> {
    let message = bundle.get_message(key)?;
    let pattern = message.value()?;
    let mut errors = Vec::new();
    let value: Cow<'_, str> = bundle.format_pattern(pattern, args, &mut errors);
    if !errors.is_empty() {
        tracing::debug!(key, ?errors, "translation formatting errors");
    }
    Some(value.into_owned())
}

fn build_bundle(meta: &'static LocaleMeta) -> Result<FluentBundle<FluentResource>, I18nError> {
    let language: LanguageIdentifier = meta
        .id
        .parse()
        .map_err(|_| I18nError::InvalidLocale(meta.id.to_owned()))?;
    let mut bundle = FluentBundle::new(vec![language]);
    bundle.set_use_isolating(false);
    for source in meta.resources {
        let resource = FluentResource::try_new((*source).to_owned()).map_err(|(_, errors)| {
            I18nError::Parse {
                locale: meta.id,
                details: errors
                    .iter()
                    .map(|error| error.to_string())
                    .collect::<Vec<_>>()
                    .join("; "),
            }
        })?;
        bundle
            .add_resource(resource)
            .map_err(|errors| I18nError::Build {
                locale: meta.id,
                details: errors
                    .iter()
                    .map(|error| error.to_string())
                    .collect::<Vec<_>>()
                    .join("; "),
            })?;
    }
    Ok(bundle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn translator() -> Translator {
        Translator::new(&LanguagePreference::Locale("pt-BR".to_owned())).expect("translator")
    }

    #[test]
    fn catalogs_parse_and_contain_every_key() {
        let translator = translator();
        for locale in Locale::available() {
            let built = build_bundle(locale.meta).expect("bundle");
            for key in keys::ALL {
                assert!(
                    built.get_message(key).is_some(),
                    "missing key {key} in {}",
                    locale.id()
                );
            }
        }
        assert!(translator.has_key(keys::REPOS_TITLE));
    }

    #[test]
    fn missing_key_is_visible() {
        assert_eq!(
            translator().text("this-key-does-not-exist"),
            "this-key-does-not-exist"
        );
    }

    #[test]
    fn unsupported_locale_falls_back_to_portuguese() {
        let locale = Locale::resolve(&LanguagePreference::Locale("en-US".to_owned()));
        assert_eq!(locale.id(), "pt-BR");
    }

    #[test]
    fn formats_plurals() {
        let translator = translator();
        let one = translator.text_args(keys::REPOS_COUNT, &[Arg::int("count", 1)]);
        let many = translator.text_args(keys::REPOS_COUNT, &[Arg::int("count", 2)]);
        assert!(one.contains('1'));
        assert!(many.contains('2'));
    }

    #[test]
    fn language_preference_roundtrips() {
        let system: LanguagePreference =
            serde_json::from_str("\"system\"").expect("system preference");
        assert_eq!(system, LanguagePreference::System);
        let locale: LanguagePreference =
            serde_json::from_str("\"pt-BR\"").expect("locale preference");
        assert_eq!(locale, LanguagePreference::Locale("pt-BR".to_owned()));
        assert_eq!(
            serde_json::to_string(&LanguagePreference::System).expect("serialize"),
            "\"system\""
        );
    }
}
