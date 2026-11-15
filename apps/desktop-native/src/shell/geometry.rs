//! Logical window size persistence, independent of the renderer and OS scale.
use anyhow::Result;
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}
impl Default for Size {
    fn default() -> Self {
        Self {
            width: 1280.,
            height: 820.,
        }
    }
}
impl Size {
    pub fn fit(self, width: f32, height: f32) -> Self {
        Self {
            width: self.width.min(width).max(960.),
            height: self.height.min(height).max(640.),
        }
    }
    fn valid(self) -> bool {
        self.width.is_finite()
            && self.height.is_finite()
            && self.width >= 960.
            && self.height >= 640.
    }
}
fn path() -> Result<std::path::PathBuf> {
    if let Some(path) = crate::runtime_check::path("window-size.json") {
        return Ok(path);
    }
    Ok(aipass_desktop_platform::preferences::default_path()
        .map_err(anyhow::Error::msg)?
        .with_file_name("window-size.json"))
}
pub fn load() -> Size {
    let default = Size::default();
    path()
        .ok()
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice::<Size>(&bytes).ok())
        .filter(|saved| {
            saved.valid()
                && (saved.width - default.width).abs() / default.width <= 0.25
                && (saved.height - default.height).abs() / default.height <= 0.25
        })
        .unwrap_or(default)
}
pub fn save(size: Size) -> Result<()> {
    anyhow::ensure!(size.valid(), "invalid desktop dimensions");
    aipass_storage::atomic_write_bytes(path()?, &serde_json::to_vec(&size)?)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn minimum_size_and_retina_logical_coordinates_are_preserved() {
        let value = Size::default().fit(1000., 700.);
        assert_eq!((value.width, value.height), (1000., 700.));
        let value = Size::default().fit(800., 600.);
        assert_eq!((value.width, value.height), (960., 640.));
        assert!(!Size {
            width: f32::NAN,
            height: 640.
        }
        .valid());
    }
}
