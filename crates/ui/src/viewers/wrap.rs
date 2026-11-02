//! The viewers the Watched panel had before the registry, as [`Viewer`]s: a picture, an SVG, a
//! craft document, an array's pictures (its views, stats and histograms) or plot, and audio.
//! Each wraps its own instance, so every window has its own zoom, view and player.

use std::sync::Arc;

use serde_json::Value;

use super::{ViewInput, Viewer, fitted_image};
use crate::array_view::ArrayView;
use crate::audio::AudioView;
use crate::canvas::Action;
use crate::media::{DocView, ImageView, MediaViewer, SvgView};
use crate::theme::Tokens;
use crate::watched::{numbers, plot};

macro_rules! media_viewer {
    ($name:ident, $inner:ty) => {
        #[derive(Default)]
        pub struct $name(pub $inner);

        impl Viewer for $name {
            fn sync(&mut self, ctx: &egui::Context, v: &ViewInput<'_>) {
                self.0.sync(ctx, Some(v.display), v.full, v.rendition);
            }

            fn full(&mut self, ui: &mut egui::Ui, _: &ViewInput<'_>, tokens: &Tokens) -> Vec<Action> {
                self.0.ui(ui, tokens);
                self.0.asks()
            }

            fn inspect(&self) -> Value {
                self.0.inspect()
            }
        }
    };
}

media_viewer!(Doc, DocView);

/// A raster image, with the probe and zooms.
#[derive(Default)]
pub struct Picture(pub ImageView);

impl Viewer for Picture {
    fn sync(&mut self, ctx: &egui::Context, v: &ViewInput<'_>) {
        self.0.tools.take(v.tools);
        self.0.sync(ctx, Some(v.display), v.full, v.rendition);
    }

    fn full(&mut self, ui: &mut egui::Ui, _: &ViewInput<'_>, tokens: &Tokens) -> Vec<Action> {
        self.0.ui(ui, tokens);
        self.0.asks()
    }

    fn inspect(&self) -> Value {
        self.0.inspect()
    }
}

/// SVG: rasterised by us (resvg), crisp at any zoom; its preview is its own raster, so an SVG
/// shows on a wire and in a list even when its engine sent no picture.
#[derive(Default)]
pub struct Svg(pub SvgView);

impl Viewer for Svg {
    fn sync(&mut self, ctx: &egui::Context, v: &ViewInput<'_>) {
        self.0.sync(ctx, Some(v.display), v.full, v.rendition);
    }

    fn preview(&mut self, ui: &mut egui::Ui, v: &ViewInput<'_>, max: egui::Vec2, tokens: &Tokens) {
        match self.0.texture(ui.ctx()) {
            Some(t) => {
                fitted_image(ui, &t, max);
                ui.label(egui::RichText::new(&v.display.summary).small().color(tokens.text_dim));
            }
            None => super::preview_default(ui, v, max, tokens),
        }
    }

    fn full(&mut self, ui: &mut egui::Ui, _: &ViewInput<'_>, tokens: &Tokens) -> Vec<Action> {
        self.0.ui(ui, tokens);
        Vec::new()
    }

    fn inspect(&self) -> Value {
        self.0.inspect()
    }
}

/// An array: its view's pictures with a view picker, channel stats and histograms; a 1-d array
/// as a plot.
#[derive(Default)]
pub struct ArrayPictures(pub ArrayView);

impl Viewer for ArrayPictures {
    fn sync(&mut self, _: &egui::Context, v: &ViewInput<'_>) {
        self.0.sync(Some(v.display), v.pictures.to_vec(), v.full, v.missing);
        self.0.tools.take(v.tools);
        self.0.offered = v.offered_views.to_vec();
    }

    fn preview(&mut self, ui: &mut egui::Ui, v: &ViewInput<'_>, max: egui::Vec2, tokens: &Tokens) {
        match (v.pictures.first(), v.full.and_then(|b| numbers(b))) {
            (Some((_, t)), _) => fitted_image(ui, t, max),
            (None, Some(xs)) => {
                ui.set_max_width(max.x);
                plot(ui, &xs, tokens);
            }
            _ => super::preview_default(ui, v, max, tokens),
        }
    }

    fn full(&mut self, ui: &mut egui::Ui, v: &ViewInput<'_>, tokens: &Tokens) -> Vec<Action> {
        let mut asks = Vec::new();
        if self.0.has_pictures() {
            asks.extend(self.0.view_bar(ui, v.display, tokens));
            asks.extend(self.0.pictures_ui(ui, tokens));
        } else if let Some(xs) = v.full.and_then(|b| numbers(b)) {
            plot(ui, &xs, tokens);
        } else {
            ui.label(egui::RichText::new("no pictures of this value").color(tokens.text_dim));
        }
        self.0.stats_ui(ui, v.display, tokens);
        asks
    }

    fn inspect(&self) -> Value {
        self.0.inspect()
    }
}

/// Audio: waveform, spectrogram and a player of its own.
#[derive(Default)]
pub struct Audio(pub AudioView);

impl Viewer for Audio {
    fn sync(&mut self, _: &egui::Context, v: &ViewInput<'_>) {
        self.0.sync(Some(v.display), v.full.map(Arc::as_ref));
    }

    fn full(&mut self, ui: &mut egui::Ui, v: &ViewInput<'_>, tokens: &Tokens) -> Vec<Action> {
        self.0.ui(ui, v.display, v.missing, tokens);
        Vec::new()
    }

    fn inspect(&self) -> Value {
        self.0.inspect()
    }
}
