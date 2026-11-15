use super::*;

impl Backdrop {
    /// Prepare the alternate material off the UI thread before changing any
    /// colors. Retain both palettes so replayed outgoing spans keep valid tiles.
    pub fn prepare_appearance(&mut self, dark: bool, window: &mut Window, cx: &mut App) -> bool {
        let size = window.viewport_size();
        let source = Source {
            width: f32::from(size.width).to_bits(),
            height: f32::from(size.height).to_bits(),
            dark,
            hero: self.hero,
        };
        if let Some(pending) = self.appearance_pending {
            if pending != source || dark == self.dark {
                cx.remove_asset::<SceneAsset>(&pending);
                self.appearance_pending = None;
            }
        }
        if dark == self.dark
            || self.scene.as_ref().is_some_and(|s| s.source == source)
            || self.alternate.as_ref().is_some_and(|s| s.source == source)
        {
            return true;
        }
        self.appearance_pending = Some(source);
        match window.use_asset::<SceneAsset>(&source, cx) {
            Some(Ok(scene)) => {
                if let Some(previous) = self.alternate.replace(scene) {
                    previous.release_atlas(window);
                }
                cx.remove_asset::<SceneAsset>(&source);
                self.appearance_pending = None;
                true
            }
            Some(Err(error)) => {
                self.error = Some(error);
                cx.remove_asset::<SceneAsset>(&source);
                self.appearance_pending = None;
                false
            }
            None => false,
        }
    }
    pub fn apply_appearance(&mut self, dark: bool) {
        if self.dark != dark || self.scene.is_none() {
            std::mem::swap(&mut self.scene, &mut self.alternate);
            self.dark = dark;
            self.error = None;
            // Keep the original animation clock: changing theme must not reset drift.
        }
    }
}
