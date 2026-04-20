use std::path::Path;

use hyperindex_protocol::symbols::LanguageId;
use tree_sitter::Language;

use crate::{ParserError, ParserResult};

pub trait LanguagePack {
    type Language: Copy + Eq;

    fn pack_id(&self) -> &'static str;
    fn detect_path(&self, path: &str) -> Option<Self::Language>;

    fn supports_path(&self, path: &str) -> bool {
        self.detect_path(path).is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TsJsLanguage {
    TypeScript,
    TypeScriptReact,
    JavaScript,
    JavaScriptReact,
    TypeScriptModule,
    TypeScriptCommonJs,
}

impl TsJsLanguage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TypeScript => "typescript",
            Self::TypeScriptReact => "tsx",
            Self::JavaScript => "javascript",
            Self::JavaScriptReact => "jsx",
            Self::TypeScriptModule => "mts",
            Self::TypeScriptCommonJs => "cts",
        }
    }

    pub fn to_protocol_language(self) -> LanguageId {
        match self {
            Self::TypeScript => LanguageId::Typescript,
            Self::TypeScriptReact => LanguageId::Tsx,
            Self::JavaScript => LanguageId::Javascript,
            Self::JavaScriptReact => LanguageId::Jsx,
            Self::TypeScriptModule => LanguageId::Mts,
            Self::TypeScriptCommonJs => LanguageId::Cts,
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct TsJsLanguagePack;

impl TsJsLanguagePack {
    pub const PACK_ID: &str = "ts_js_core";

    pub fn parser_language(&self, language: TsJsLanguage) -> ParserResult<Language> {
        match language {
            // TypeScript is a strict superset of JavaScript, so `.js` and `.mts/.cts`
            // share the same grammar in this first pack.
            TsJsLanguage::TypeScript
            | TsJsLanguage::JavaScript
            | TsJsLanguage::TypeScriptModule
            | TsJsLanguage::TypeScriptCommonJs => {
                Ok(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
            }
            // TSX also handles plain JSX files, which keeps the pack narrow for this slice.
            TsJsLanguage::TypeScriptReact | TsJsLanguage::JavaScriptReact => {
                Ok(tree_sitter_typescript::LANGUAGE_TSX.into())
            }
        }
    }

    pub fn new_parser(&self, language: TsJsLanguage) -> ParserResult<tree_sitter::Parser> {
        let mut parser = tree_sitter::Parser::new();
        let parser_language = self.parser_language(language)?;
        parser.set_language(&parser_language).map_err(|error| {
            ParserError::Message(format!("failed to load parser grammar: {error}"))
        })?;
        Ok(parser)
    }
}

impl LanguagePack for TsJsLanguagePack {
    type Language = TsJsLanguage;

    fn pack_id(&self) -> &'static str {
        Self::PACK_ID
    }

    fn detect_path(&self, path: &str) -> Option<Self::Language> {
        match Path::new(path).extension().and_then(|value| value.to_str()) {
            Some("ts") => Some(TsJsLanguage::TypeScript),
            Some("tsx") => Some(TsJsLanguage::TypeScriptReact),
            Some("js") => Some(TsJsLanguage::JavaScript),
            Some("jsx") => Some(TsJsLanguage::JavaScriptReact),
            Some("mts") => Some(TsJsLanguage::TypeScriptModule),
            Some("cts") => Some(TsJsLanguage::TypeScriptCommonJs),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{LanguagePack, TsJsLanguage, TsJsLanguagePack};

    #[test]
    fn detects_supported_ts_js_extensions() {
        let pack = TsJsLanguagePack;
        assert_eq!(
            pack.detect_path("src/app.ts"),
            Some(TsJsLanguage::TypeScript)
        );
        assert_eq!(
            pack.detect_path("src/app.tsx"),
            Some(TsJsLanguage::TypeScriptReact)
        );
        assert_eq!(
            pack.detect_path("src/app.js"),
            Some(TsJsLanguage::JavaScript)
        );
        assert_eq!(
            pack.detect_path("src/app.jsx"),
            Some(TsJsLanguage::JavaScriptReact)
        );
        assert_eq!(
            pack.detect_path("src/app.mts"),
            Some(TsJsLanguage::TypeScriptModule)
        );
        assert_eq!(
            pack.detect_path("src/app.cts"),
            Some(TsJsLanguage::TypeScriptCommonJs)
        );
        assert_eq!(pack.detect_path("README.md"), None);
    }

    #[test]
    fn maps_js_and_jsx_to_real_grammars() {
        let pack = TsJsLanguagePack;
        let mut js_parser = pack.new_parser(TsJsLanguage::JavaScript).unwrap();
        let mut jsx_parser = pack.new_parser(TsJsLanguage::JavaScriptReact).unwrap();
        let js_tree = js_parser.parse("export const value = 1;", None).unwrap();
        let jsx_tree = jsx_parser
            .parse("export const View = () => <div />;", None)
            .unwrap();

        assert!(!js_tree.root_node().has_error());
        assert!(!jsx_tree.root_node().has_error());
        assert_eq!(pack.pack_id(), "ts_js_core");
    }
}
