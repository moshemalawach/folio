use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::path::{Path, PathBuf};

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageReader};

/// Largest texture side we upload. Big photos are scaled down to this; it also stays
/// under the GPU's own limit.
const MAX_SIDE: u32 = 4096;

/// Pictures are uploaded at the smallest of these sizes (longest side, in pixels) that
/// covers how big they are on screen, so a long printout seen whole costs little.
const TIERS: [u32; 5] = [256, 512, 1024, 2048, MAX_SIDE];
/// New pictures decoded per frame. A printout page takes 5–15 ms to decode, so the
/// rest wait a frame or two (showing a blank sheet) instead of freezing the window.
const LOADS_PER_FRAME: usize = 2;
/// Texture memory kept before pictures that weren't drawn last frame are dropped.
const BUDGET_BYTES: usize = 384 * 1024 * 1024;

/// A texture lookup for a picture drawn at a given size.
pub enum Lookup {
    Ready(TextureHandle),
    /// Not loaded yet: this frame's loads are used up. A repaint is requested.
    Loading,
    /// The file is missing or isn't a picture.
    Missing,
}

struct Entry {
    tier: u32,
    texture: TextureHandle,
    bytes: usize,
    /// The frame it was last drawn in.
    used: u64,
}

pub struct TextureCache {
    /// Per attachment id, the sizes loaded so far.
    textures: HashMap<String, Vec<Entry>>,
    failed: HashSet<String>,
    frame: u64,
    loads: usize,
    bytes: usize,
    budget: usize,
}

impl Default for TextureCache {
    fn default() -> Self {
        Self {
            textures: HashMap::new(),
            failed: HashSet::new(),
            frame: 0,
            loads: 0,
            bytes: 0,
            budget: BUDGET_BYTES,
        }
    }
}

impl TextureCache {
    /// Returns the texture for `id` at full size, resolving its file with `path` only on
    /// a cache miss, so callers can skip database lookups on every frame. It always
    /// loads at once (pictures in a note's text).
    pub fn get_with(
        &mut self,
        ctx: &egui::Context,
        id: &str,
        path: impl FnOnce() -> Option<PathBuf>,
    ) -> Option<TextureHandle> {
        match self.lookup(ctx, id, MAX_SIDE, false, path) {
            Lookup::Ready(texture) => Some(texture),
            Lookup::Loading | Lookup::Missing => None,
        }
    }

    /// The texture for `id` drawn `side` pixels long on screen: the smallest size that
    /// stays sharp. At most a couple of new pictures load per frame; the others get
    /// `Loading` (or a smaller size already loaded) until their turn.
    pub fn get_sized(
        &mut self,
        ctx: &egui::Context,
        id: &str,
        side: f32,
        path: impl FnOnce() -> Option<PathBuf>,
    ) -> Lookup {
        let tier = TIERS
            .iter()
            .copied()
            .find(|tier| *tier as f32 >= side)
            .unwrap_or(MAX_SIDE);
        self.lookup(ctx, id, tier, true, path)
    }

    fn lookup(
        &mut self,
        ctx: &egui::Context,
        id: &str,
        tier: u32,
        budgeted: bool,
        path: impl FnOnce() -> Option<PathBuf>,
    ) -> Lookup {
        self.next_frame(ctx);
        let frame = self.frame;
        if let Some(entries) = self.textures.get_mut(id) {
            // The smallest loaded size that is big enough.
            if let Some(entry) = entries
                .iter_mut()
                .filter(|entry| entry.tier >= tier)
                .min_by_key(|entry| entry.tier)
            {
                entry.used = frame;
                return Lookup::Ready(entry.texture.clone());
            }
        }
        if self.failed.contains(id) {
            return Lookup::Missing;
        }
        if budgeted && self.loads >= LOADS_PER_FRAME {
            ctx.request_repaint();
            // A smaller size already loaded stands in until the sharp one comes.
            if let Some(entry) = self
                .textures
                .get_mut(id)
                .and_then(|entries| entries.iter_mut().max_by_key(|entry| entry.tier))
            {
                entry.used = frame;
                return Lookup::Ready(entry.texture.clone());
            }
            return Lookup::Loading;
        }
        self.loads += 1;
        match path().and_then(|path| load_texture(ctx, id, tier, &path)) {
            Some((texture, whole)) => {
                let bytes = texture.byte_size();
                self.bytes += bytes;
                // A picture smaller than the size asked for loaded whole: it serves every
                // bigger size too, rather than being loaded again for each.
                let tier = if whole { MAX_SIDE } else { tier };
                self.textures
                    .entry(id.to_string())
                    .or_default()
                    .push(Entry {
                        tier,
                        texture: texture.clone(),
                        bytes,
                        used: frame,
                    });
                Lookup::Ready(texture)
            }
            None => {
                self.failed.insert(id.to_string());
                Lookup::Missing
            }
        }
    }

    /// Once per frame: the load allowance starts again, and if the textures are over
    /// budget, the ones not drawn last frame are dropped, least recently drawn first.
    fn next_frame(&mut self, ctx: &egui::Context) {
        let frame = ctx.cumulative_frame_nr();
        if frame == self.frame {
            return;
        }
        self.frame = frame;
        self.loads = 0;
        if self.bytes <= self.budget {
            return;
        }
        let mut stale: Vec<(u64, String, u32)> = self
            .textures
            .iter()
            .flat_map(|(id, entries)| {
                entries
                    .iter()
                    .filter(|entry| entry.used + 1 < frame)
                    .map(|entry| (entry.used, id.clone(), entry.tier))
            })
            .collect();
        stale.sort_unstable();
        for (_, id, tier) in stale {
            if self.bytes <= self.budget / 4 * 3 {
                break;
            }
            if let Some(entries) = self.textures.get_mut(&id) {
                if let Some(at) = entries.iter().position(|entry| entry.tier == tier) {
                    self.bytes -= entries.swap_remove(at).bytes;
                }
                if entries.is_empty() {
                    self.textures.remove(&id);
                }
            }
        }
    }

    /// A smaller budget, for tests.
    #[cfg(test)]
    fn with_budget(budget: usize) -> Self {
        Self {
            budget,
            ..Self::default()
        }
    }
}

/// Loads a picture no longer than `tier` on its longest side. Also says whether it came
/// whole: smaller than that, so not scaled down.
fn load_texture(
    ctx: &egui::Context,
    id: &str,
    tier: u32,
    path: &Path,
) -> Option<(TextureHandle, bool)> {
    let bytes = std::fs::read(path).ok()?;
    let max_side = ctx.input(|input| input.max_texture_side) as u32;
    let limit = tier.min(max_side.max(256));
    let image = decode(&bytes, limit)?.to_rgba8();
    let whole = image.width().max(image.height()) < limit;
    let size = [image.width() as usize, image.height() as usize];
    let pixels = image.into_raw();
    let color = ColorImage::from_rgba_unmultiplied(size, &pixels);
    let texture = ctx.load_texture(
        format!("attachment-{id}@{tier}"),
        color,
        TextureOptions::LINEAR,
    );
    Some((texture, whole))
}

/// Decodes an image upright (honouring the camera's EXIF orientation) and no larger
/// than `max_side` on either axis.
pub fn decode(bytes: &[u8], max_side: u32) -> Option<DynamicImage> {
    let mut decoder = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .into_decoder()
        .ok()?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut image = DynamicImage::from_decoder(decoder).ok()?;
    image.apply_orientation(orientation);
    if image.width().max(image.height()) > max_side {
        image = image.resize(max_side, max_side, image::imageops::FilterType::Triangle);
    }
    Some(image)
}

/// The upright pixel size of an image file, without decoding all of it.
pub fn upright_size(path: &Path) -> Option<[f32; 2]> {
    let mut decoder = ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?
        .into_decoder()
        .ok()?;
    let (width, height) = decoder.dimensions();
    let swapped = matches!(
        decoder.orientation().unwrap_or(Orientation::NoTransforms),
        Orientation::Rotate90
            | Orientation::Rotate270
            | Orientation::Rotate90FlipH
            | Orientation::Rotate270FlipH
    );
    let (width, height) = if swapped {
        (height, width)
    } else {
        (width, height)
    };
    (width > 0 && height > 0).then_some([width as f32, height as f32])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A frame of a bare egui context: a new frame number for the cache. Its texture
    /// uploads have no renderer to go to, so they are cleared (egui asserts otherwise).
    fn frame(ctx: &egui::Context, mut body: impl FnMut(&egui::Context)) {
        ctx.run_ui(egui::RawInput::default(), |ui| body(ui.ctx()))
            .textures_delta
            .clear();
    }

    fn picture(dir: &Path, name: &str, width: u32, height: u32) -> PathBuf {
        let path = dir.join(name);
        image::RgbaImage::from_pixel(width, height, image::Rgba([10, 20, 30, 255]))
            .save(&path)
            .unwrap();
        path
    }

    #[test]
    fn a_picture_loads_at_the_size_it_is_drawn() {
        let dir = tempfile::tempdir().unwrap();
        let path = picture(dir.path(), "a.png", 1200, 800);
        let ctx = egui::Context::default();
        let mut cache = TextureCache::default();
        frame(&ctx, |ctx| {
            match cache.get_sized(ctx, "a", 300.0, || Some(path.clone())) {
                Lookup::Ready(texture) => assert_eq!(texture.size(), [512, 341]),
                _ => panic!("loaded"),
            }
            // Drawn bigger, it loads sharper; drawn smaller again, the bigger one serves.
            match cache.get_sized(ctx, "a", 900.0, || Some(path.clone())) {
                Lookup::Ready(texture) => assert_eq!(texture.size(), [1024, 683]),
                _ => panic!("loaded"),
            }
        });
        frame(&ctx, |ctx| {
            match cache.get_sized(ctx, "a", 200.0, || panic!("cached")) {
                Lookup::Ready(texture) => assert_eq!(texture.size(), [512, 341]),
                _ => panic!("cached"),
            }
        });
        // get_with is full size.
        frame(&ctx, |ctx| {
            let texture = cache.get_with(ctx, "a", || Some(path.clone())).unwrap();
            assert_eq!(texture.size(), [1200, 800]);
        });
    }

    #[test]
    fn a_small_picture_loads_once_whatever_its_zoom() {
        let dir = tempfile::tempdir().unwrap();
        let path = picture(dir.path(), "small.png", 400, 300);
        let ctx = egui::Context::default();
        let mut cache = TextureCache::default();
        frame(&ctx, |ctx| {
            match cache.get_sized(ctx, "small", 200.0, || Some(path.clone())) {
                Lookup::Ready(texture) => assert_eq!(texture.size(), [256, 192]),
                _ => panic!("loaded"),
            }
            // 300 px fits in the 512 size whole: that one serves every bigger size.
            match cache.get_sized(ctx, "small", 300.0, || Some(path.clone())) {
                Lookup::Ready(texture) => assert_eq!(texture.size(), [400, 300]),
                _ => panic!("loaded"),
            }
        });
        frame(&ctx, |ctx| {
            for side in [900.0, 3000.0] {
                match cache.get_sized(ctx, "small", side, || panic!("loaded once")) {
                    Lookup::Ready(texture) => assert_eq!(texture.size(), [400, 300]),
                    _ => panic!("cached"),
                }
            }
            let texture = cache.get_with(ctx, "small", || panic!("loaded once"));
            assert_eq!(texture.unwrap().size(), [400, 300]);
        });
        assert_eq!(cache.textures["small"].len(), 2);
    }

    #[test]
    fn only_a_few_pictures_load_per_frame() {
        let dir = tempfile::tempdir().unwrap();
        let paths: Vec<PathBuf> = (0..3)
            .map(|i| picture(dir.path(), &format!("{i}.png"), 64, 64))
            .collect();
        let ctx = egui::Context::default();
        let mut cache = TextureCache::default();
        frame(&ctx, |ctx| {
            let states: Vec<bool> = paths
                .iter()
                .enumerate()
                .map(|(i, path)| {
                    matches!(
                        cache.get_sized(ctx, &i.to_string(), 64.0, || Some(path.clone())),
                        Lookup::Ready(_)
                    )
                })
                .collect();
            assert_eq!(states, [true, true, false]);
        });
        frame(&ctx, |ctx| {
            assert!(matches!(
                cache.get_sized(ctx, "2", 64.0, || Some(paths[2].clone())),
                Lookup::Ready(_)
            ));
        });
    }

    #[test]
    fn over_budget_pictures_out_of_view_are_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let a = picture(dir.path(), "a.png", 256, 256);
        let b = picture(dir.path(), "b.png", 256, 256);
        let ctx = egui::Context::default();
        // One 256² texture is 256 KB: a budget of 300 KB holds one, not two.
        let mut cache = TextureCache::with_budget(300 * 1024);
        frame(&ctx, |ctx| {
            assert!(matches!(
                cache.get_sized(ctx, "a", 256.0, || Some(a.clone())),
                Lookup::Ready(_)
            ));
        });
        frame(&ctx, |ctx| {
            assert!(matches!(
                cache.get_sized(ctx, "b", 256.0, || Some(b.clone())),
                Lookup::Ready(_)
            ));
        });
        // Two frames on, "a" (not drawn since) goes; "b", drawn last frame, stays.
        frame(&ctx, |ctx| {
            assert!(matches!(
                cache.get_sized(ctx, "b", 256.0, || panic!("cached")),
                Lookup::Ready(_)
            ));
        });
        frame(&ctx, |_| {});
        assert!(!cache.textures.contains_key("a"), "evicted");
        assert!(cache.textures.contains_key("b"));
        assert!(cache.bytes <= 300 * 1024);
    }

    #[test]
    fn a_missing_file_is_missing() {
        let ctx = egui::Context::default();
        let mut cache = TextureCache::default();
        frame(&ctx, |ctx| {
            assert!(matches!(
                cache.get_sized(ctx, "gone", 100.0, || None),
                Lookup::Missing
            ));
            assert!(matches!(
                cache.get_sized(ctx, "gone", 100.0, || panic!("not asked again")),
                Lookup::Missing
            ));
        });
    }
}
