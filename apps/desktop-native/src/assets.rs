use gpui::{AssetSource, SharedString};
use std::borrow::Cow;

pub struct Assets;

macro_rules! icons {
    ($($name:literal),+ $(,)?) => {
        fn icon(path: &str) -> Option<&'static [u8]> {
            match path {
                $($name => Some(include_bytes!(concat!("../assets/icons/", $name, ".svg"))),)+
                _ => None,
            }
        }
    }
}
icons!(
    "pencil",
    "undo-2",
    "chevron-right",
    "inbox",
    "star",
    "sparkles",
    "shield-check",
    "wifi",
    "terminal",
    "bot",
    "server",
    "archive",
    "trash-2",
    "key-round",
    "plus",
    "plug",
    "search",
    "list-filter",
    "refresh-cw",
    "ellipsis",
    "chevron-down",
    "chevron-up",
    "eye",
    "eye-off",
    "menu",
    "sliders-horizontal",
    "settings",
    "lock",
    "spinner",
    "circle-alert",
    "x"
);

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        let tool = match path {
            "tool-codex" => {
                Some(include_bytes!("../../desktop/public/tool-icons/codex.svg").as_slice())
            }
            "tool-claude-code" => {
                Some(include_bytes!("../../desktop/public/tool-icons/claude-code.svg").as_slice())
            }
            "tool-gemini-cli" => {
                Some(include_bytes!("../../desktop/public/tool-icons/gemini-cli.svg").as_slice())
            }
            "tool-opencode" => {
                Some(include_bytes!("../../desktop/public/tool-icons/opencode.svg").as_slice())
            }
            "tool-grok" => {
                Some(include_bytes!("../../desktop/public/tool-icons/grok.svg").as_slice())
            }
            "tool-pi" => Some(include_bytes!("../../desktop/public/tool-icons/pi.svg").as_slice()),
            "tool-cursor" => {
                Some(include_bytes!("../../desktop/public/tool-icons/cursor.svg").as_slice())
            }
            "tool-kimi-code" => {
                Some(include_bytes!("../../desktop/public/tool-icons/kimi-code.svg").as_slice())
            }
            "tool-zed" => {
                Some(include_bytes!("../../desktop/public/tool-icons/zed.svg").as_slice())
            }
            "tool-deepseek-harness" => Some(
                include_bytes!("../../desktop/public/tool-icons/deepseek-harness.svg").as_slice(),
            ),
            "tool-antigravity-cli" => Some(
                include_bytes!("../../desktop/public/tool-icons/antigravity-cli.svg").as_slice(),
            ),
            "tool-command-code" => {
                Some(include_bytes!("../../desktop/public/tool-icons/command-code.svg").as_slice())
            }
            "tool-workbuddy" => {
                Some(include_bytes!("../../desktop/public/tool-icons/workbuddy.svg").as_slice())
            }
            _ => None,
        };
        if let Some(bytes) = tool {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        if path == "star-filled" {
            return Ok(Some(Cow::Owned(
                include_str!("../assets/icons/star.svg")
                    .replace("fill=\"none\"", "fill=\"black\"")
                    .into_bytes(),
            )));
        }
        if path == "brand-anthropic" {
            return Ok(Some(Cow::Borrowed(include_bytes!(
                "../../../packages/ui/src/assets/provider-icons/anthropic.svg"
            ))));
        }
        if path == "brand-openai" {
            return Ok(Some(Cow::Borrowed(include_bytes!(
                "../../../packages/ui/src/assets/provider-icons/openai.svg"
            ))));
        }
        if let Some(image) = crate::logo::asset(path)? {
            return Ok(Some(Cow::Owned(image)));
        }
        Ok(icon(path).map(Cow::Borrowed))
    }

    fn list(&self, _path: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(vec![])
    }
}
