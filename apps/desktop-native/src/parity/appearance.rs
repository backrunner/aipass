//! Synthetic, app-local appearance notifications; never changes macOS preferences.
use crate::desktop::Desktop;
use gpui::{AppContext, AsyncApp, Entity, WindowAppearance, WindowHandle};
use std::time::{Duration, Instant};

pub struct Sample {
    dark: bool,
    milliseconds: u64,
}

pub fn parse(args: &[String]) -> anyhow::Result<Vec<Sample>> {
    let mut samples = Vec::new();
    for value in args
        .iter()
        .filter_map(|arg| arg.strip_prefix("--appearance-sample="))
    {
        let (theme, time) = value
            .split_once(':')
            .ok_or_else(|| anyhow::anyhow!("Expected light:milliseconds or dark:milliseconds"))?;
        let dark = match theme {
            "light" => false,
            "dark" => true,
            _ => anyhow::bail!("Unknown appearance {theme}"),
        };
        let milliseconds = time.parse::<u64>()?;
        anyhow::ensure!(
            milliseconds <= 220,
            "Appearance time must be within 0..220 milliseconds"
        );
        samples.push(Sample { dark, milliseconds });
    }
    if !samples.is_empty() {
        anyhow::ensure!(
            samples.len() <= 8
                && args.iter().any(|a| a.starts_with("--capture="))
                && !args
                    .iter()
                    .any(|a| a.starts_with("--vault=") || a.starts_with("--update-config=")),
            "Appearance sampling requires an isolated synthetic capture (up to eight changes)"
        );
    }
    Ok(samples)
}

pub async fn apply(
    samples: &[Sample],
    window: WindowHandle<gpui_base::Root>,
    desktop: &Entity<Desktop>,
    cx: &mut AsyncApp,
) -> anyhow::Result<()> {
    for sample in samples {
        desktop.update(cx, |view, _| view.begin_appearance_sample());
        cx.update(|cx| {
            cx.set_window_appearance(Some(if sample.dark {
                WindowAppearance::Dark
            } else {
                WindowAppearance::Light
            }))
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            cx.background_executor()
                .timer(Duration::from_millis(20))
                .await;
            cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))?;
            let state = desktop.read_with(cx, |view, _| view.appearance_state());
            if state["dark"] == sample.dark && state["requested"] == sample.dark {
                break;
            }
            anyhow::ensure!(
                Instant::now() < deadline,
                "Appearance observer or material preparation did not settle: {state}"
            );
        }
        desktop.update(cx, |view, cx| {
            view.freeze_appearance(sample.milliseconds, cx)
        });
        cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))?;
    }
    Ok(())
}
