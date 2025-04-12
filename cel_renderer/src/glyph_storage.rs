use lru::LruCache;
use std::num::NonZeroUsize;
use ttf_parser::{Face, GlyphId, RasterGlyphImage};
use msdfgen::{FontExt, Bitmap, MsdfGeneratorConfig, FillRule, Bound, Shape};
use cosmic_text::{FontSystem, fontdb::{self}};

use crate::texture::Texture;

const ATLAS_SIZE: u32 = 1024;
const MSDF_SIZE: u32 = 32;
const MSDF_RANGE: f32 = 3.0;

#[derive(Clone, Copy, Debug)]
pub enum RenderType {
    MSDF,
    RASTER
}

#[derive(Clone, Copy, Debug)]
pub struct GlyphMetrics {
    pub atlas_index: u32,
    pub atlas_uv: Bound<f32>,
    pub atlas_bound: Bound<f64>,
    pub glyph_bound: Bound<f32>,
    pub render_type: RenderType,
}

#[derive(Clone, Copy, Default)]
struct GlyphBox {
    range: f64,
    scale: f64,
    rect: msdfgen::Vector2<f64>,
    translate: msdfgen::Vector2<f64>
}

// GlyphID, FontID
type CacheKey = (u16, fontdb::ID);

pub struct GlyphStorage {
    atlas_tex: Texture<f32>,
    glyph_lru: LruCache<CacheKey, GlyphMetrics>,
    atlas_free_list: u32,
    target_aspect: f32,
}

impl Default for GlyphMetrics {
    fn default() -> Self {
        Self {
            atlas_index: 0,
            atlas_uv: Default::default(),
            atlas_bound: Default::default(),
            glyph_bound: Default::default(),
            render_type: RenderType::RASTER,
        }
    }
}

impl GlyphStorage {
    pub fn new() -> Result<Self, String> {
        let max_glyphs = (ATLAS_SIZE / MSDF_SIZE) * (ATLAS_SIZE / MSDF_SIZE);
        let mut atlas_tex = Texture::new(ATLAS_SIZE, ATLAS_SIZE, 4, true, None)?;

        // Populate index zero
        atlas_tex.update_pixels(
            0, 0,
            MSDF_SIZE, MSDF_SIZE,
            &vec![1.0; (MSDF_SIZE * MSDF_SIZE * 4) as usize]
        );

        Ok(Self {
            glyph_lru: LruCache::new(NonZeroUsize::new(max_glyphs as usize).unwrap()),
            atlas_tex,
            atlas_free_list: 1, // Spot zero is always empty
            target_aspect: 1.0
        })
    }


    pub fn get_atlas_size(&self) -> u32 { ATLAS_SIZE }
    pub fn get_glyph_size(&self) -> u32 { MSDF_SIZE }
    pub fn get_pixel_range(&self) -> f32 { MSDF_RANGE }
    pub fn get_atlas_texture(&self) -> &Texture<f32> { &self.atlas_tex }
    pub fn set_target_aspect(&mut self, aspect: f32) { self.target_aspect = aspect }

    pub fn get_glyph_metrics(&mut self, key: &CacheKey) -> Option<&GlyphMetrics> {
        self.glyph_lru.get(key)
    }

    pub fn get_atlas_texcoord(atlas_index: u32) -> [f64; 2] {
        let offset = Self::convert_atlas_index_to_offset(atlas_index);
        return Self::convert_atlas_offset_to_texcoord(offset);
    }

    pub fn make_glyph_resident(&mut self, font_system: &mut FontSystem, key: &CacheKey) {
        // TODO: batching for face parsing 
        if self.glyph_lru.contains(key) {
            return;
        }

        // TODO: don't need to evict if new glyph is invalid (default to index 0)

        let font = font_system.get_font(key.1).unwrap();
        let face: &Face = font.rustybuzz().as_ref();
        //let name = font.rustybuzz().names().get(0).unwrap().to_string().unwrap();
        //log::warn!("{:?} {}", key, name);
        if let Some((pixels, mut metrics)) = self.load_glyph(face, key.0) {
            // Select atlas position
            let atlas_index: u32;
            if self.atlas_free_list < self.glyph_lru.cap().get() as u32 {
                // Pull from free list first
                atlas_index = self.atlas_free_list;
                self.atlas_free_list += 1;
            } else {
                // LRU Will always be full here since the LRU is the same
                // size as the free list, so we can safely evict
                let lru = self.glyph_lru.pop_lru().unwrap();
                atlas_index = lru.1.atlas_index;
            }

            // Update atlas pixels
            let offset = Self::convert_atlas_index_to_offset(atlas_index);
            self.atlas_tex.update_pixels(
                offset[0], offset[1],
                MSDF_SIZE, MSDF_SIZE,
                &pixels
            );

            // Update the atlas position of the glyph & compute new UV
            // UV.y is flipped since the underlying atlas bitmaps have flipped y
            metrics.atlas_index = atlas_index;
            let uv = Self::get_atlas_texcoord(atlas_index);
            let uv_min = [
                uv[0] + metrics.atlas_bound.left,
                uv[1] + metrics.atlas_bound.top,
            ];
            let uv_max = [
                uv_min[0] + metrics.atlas_bound.width(),
                uv_min[1] - metrics.atlas_bound.height(),
            ];
            metrics.atlas_uv = Bound::new(
                uv_min[0] as f32,
                uv_max[1] as f32,
                uv_max[0] as f32,
                uv_min[1] as f32,
            );

            self.glyph_lru.put(*key, metrics);
        } else {
            // Invalid, return default metrics 

            self.glyph_lru.put(*key, Default::default());
        }
    }

    fn load_glyph(&self, face: &Face, id: u16) -> Option<(Vec<f32>, GlyphMetrics)> {
        let glyph_id = GlyphId(id);
        let render_type: RenderType;
        let raster: Option<RasterGlyphImage>;
        let mut shape: Option<Shape> = None;

        raster = face.glyph_raster_image(glyph_id, MSDF_SIZE as u16);
        if raster.is_some() {
            //log::trace!("Found raster for {}", id.0);
            render_type = RenderType::RASTER;
        } else {
            shape = face.glyph_shape(glyph_id);
            if shape.is_some() {
                //log::trace!("Found msdf for {}", id.0);
                render_type = RenderType::MSDF;
            } else {
                let has_svg = face.glyph_svg_image(glyph_id).is_some();
                log::warn!("Missing data for glyph id {} (has_svg={})", id, has_svg);
                return None;
            }
        }

        let (pixels, glyph_bound, pixel_bound) = match render_type {
            RenderType::MSDF => self.generate_msdf(face, GlyphId(id), shape.unwrap()),
            RenderType::RASTER => self.generate_raster(raster.unwrap())
        };

        // For debugging
        /*
        let bytes = pixels.iter().map(|p| (p * 255.0) as u8).collect::<Vec<u8>>();
        let _ = image::save_buffer(
            format!("glyphs/{}.png", id.0),
            &bytes, MSDF_SIZE, MSDF_SIZE, image::ColorType::Rgba8
        );
        let glyph_name = face.glyph_name(id).unwrap_or("unknown");
        println!("Pixel bound for {}: T:{} B:{} L:{} R:{}", glyph_name, pixel_bound.top, pixel_bound.bottom, pixel_bound.left, pixel_bound.right);
        println!("Glyph bound for {}: T:{} B:{} L:{} R:{}", glyph_name, glyph_bound.top, glyph_bound.bottom, glyph_bound.left, glyph_bound.right);
        */

        // Compute final atlas bound (uv coords)
        let texcoord_scale = 1.0 / self.get_atlas_size() as f64;
        let atlas_bound = Bound::new(
            pixel_bound.left * texcoord_scale,
            pixel_bound.bottom * texcoord_scale,
            pixel_bound.right * texcoord_scale,
            pixel_bound.top * texcoord_scale
        );

        let metrics = GlyphMetrics {
            atlas_index: 0,
            atlas_uv: Bound::new(0.0, 0.0, 0.0, 0.0),
            glyph_bound,
            atlas_bound,
            render_type
        };

        Some((pixels, metrics))
    }

    fn get_msdf_box(scale: f64, shape: &mut Shape) -> GlyphBox {
        // Loosely based on 
        // https://github.com/Chlumsky/msdf-atlas-gen/blob/master/msdf-atlas-gen/GlyphGeometry.cpp

        // Helper to compute boundaries and dimensions given a scale
        let compute_boundaries = |scale: f64, bounds: &msdfgen::Bound<f64>| -> (f64, f64, f64, f64, f64, f64) {
            let sl = (scale * bounds.left - 0.5).floor();
            let sr = (scale * bounds.right + 0.5).ceil();
            let sb = (scale * bounds.bottom - 0.5).floor();
            let st = (scale * bounds.top + 0.5).ceil();
            let width = sr - sl;
            let height = st - sb;
            (sl, sr, sb, st, width, height)
        };

        let mut bbox = GlyphBox::default();
        let mut bounds = shape.get_bound();
        let mut scale = MSDF_SIZE as f64 * scale;
        let range = MSDF_RANGE as f64 / scale;
        let miter_limit = 1.0;

        bbox.scale = scale;
        bbox.range = range;

        if bounds.left < bounds.right && bounds.bottom < bounds.top {
            if miter_limit > 0.0 {
                shape.bound_miters(&mut bounds, -range, miter_limit, msdfgen::Polarity::Positive);
            }

            // Compute provisional pixel boundaries
            let (mut sl, _, mut sb, _, mut width, mut height) = compute_boundaries(scale, &bounds);

            // Check if the glyph exceeds the maximum allowed size
            let scale_adjust = if width > MSDF_SIZE as f64 || height > MSDF_SIZE as f64 {
                let factor_width  = if width  > MSDF_SIZE as f64 { MSDF_SIZE as f64 / width } else { 1.0 };
                let factor_height = if height > MSDF_SIZE as f64 { MSDF_SIZE as f64 / height } else { 1.0 };
                factor_width.min(factor_height)
            } else {
                1.0
            };

            // If the glyph is too large, adjust the scale and recalc boundaries
            if scale_adjust < 1.0 {
                scale *= scale_adjust;
                bbox.scale = scale;
                bbox.range = MSDF_RANGE as f64 / scale;
                (sl, _, sb, _, width, height) = compute_boundaries(scale, &bounds);
            }
            
            bbox.translate.x = -sl / scale;
            bbox.translate.y = -sb / scale;
            bbox.rect.x = width;
            bbox.rect.y = height;
        } else {
            // Invalid bounds
            bbox.rect = msdfgen::Vector2::default();
            bbox.translate = msdfgen::Vector2::default();
        }

        bbox
    }

    fn generate_msdf(&self, face: &Face, glyph_id: GlyphId, shape: Shape) -> (Vec<f32>, Bound<f32>, Bound<f64>) {
        let mut shape = shape;
        shape.normalize();

        let pixel_scale = 1.0 / face.units_per_em() as f64;
        let bbox = GlyphStorage::get_msdf_box(pixel_scale, &mut shape);
        let framing = msdfgen::Framing {
            projection: msdfgen::Projection {
                translate: bbox.translate,
                scale: bbox.scale.into()
            },
            range: bbox.range
        };

        let config = MsdfGeneratorConfig::default();
        let fill_rule = FillRule::default();
        let mut bitmap = Bitmap::new(MSDF_SIZE, MSDF_SIZE);
        shape.edge_coloring_simple(3.0, 0);
        shape.generate_mtsdf(&mut bitmap, &framing, &config);

        shape.correct_sign(&mut bitmap, &framing, fill_rule);
        shape.correct_msdf_error(&mut bitmap, &framing, &config);
        
        let bmp_pixels = bitmap.pixels();
        let mut pixels: Vec<f32> = vec![0.0; (MSDF_SIZE * MSDF_SIZE * 4) as usize];
        for i in 0..bmp_pixels.len() {
            pixels[i * 4 + 0] = bmp_pixels[i].r;
            pixels[i * 4 + 1] = bmp_pixels[i].g;
            pixels[i * 4 + 2] = bmp_pixels[i].b;
            pixels[i * 4 + 3] = bmp_pixels[i].a;
        }

        // https://github.com/Chlumsky/msdf-atlas-gen/blob/master/msdf-atlas-gen/GlyphGeometry.cpp
        // Ensure we scale by the font's scale so that height is normalized to 1.0.
        // Also, divide by the glyph's advance since we may be pulling glyphs from
        // non-monospaced fonts. This way, width is always normalized
        let advance = face.glyph_hor_advance(glyph_id).unwrap_or(1) as f32;
        let scale_x = 1.0 / advance as f64;
        let scale_y = 1.0 / face.height() as f64;
        let mut glyph_bound = Bound::new(
            (scale_x * (-bbox.translate.x + 0.5 / bbox.scale)) as f32,
            (scale_y * (-bbox.translate.y + 0.5 / bbox.scale)) as f32,
            (scale_x * (-bbox.translate.x + (bbox.rect.x - 0.5) / bbox.scale)) as f32,
            (scale_y * (-bbox.translate.y + (bbox.rect.y - 0.5) / bbox.scale)) as f32,
        );

        // Bump t/b glyph bound to ensure baseline is normalized to 0
        let descender = face.descender().abs() as f32 * scale_y as f32;
        glyph_bound.bottom += descender;
        glyph_bound.top += descender;

        // Ensure glyph aspect ratio is maintained
        let aspect = (scale_x / scale_y) as f32;
        glyph_bound.right *= self.target_aspect / aspect;

        let mut pixel_bound = Bound::new(
            0.5,
            0.5,
            bbox.rect.x - 0.5,
            bbox.rect.y - 0.5,
        );

        // Clamp glyph bound and adjust pixel bound accordingly 
        if glyph_bound.bottom < 0.0 {
            let adjust = glyph_bound.bottom.abs();
            glyph_bound.bottom = 0.0;
            pixel_bound.bottom += bbox.rect.y * adjust as f64;
        }
        if glyph_bound.top > 1.0 {
            let adjust = glyph_bound.top - 1.0;
            glyph_bound.top = 1.0;
            pixel_bound.top -= bbox.rect.y * adjust as f64;
        }

        (pixels, glyph_bound, pixel_bound)
    }

    fn generate_raster(&self, raster: RasterGlyphImage) -> (Vec<f32>, Bound<f32>, Bound<f64>) {
        let glyph_bound = Bound::new(0.0, 0.05, 1.0, 0.95);
        let pixel_bound = Bound::new(0.5, 0.5, MSDF_SIZE as f64 - 0.5, MSDF_SIZE as f64 - 0.5);

        let mut image = match image::load_from_memory(raster.data) {
            Ok(img) => img,
            Err(err) => {
                log::warn!("Glyph had invalid raster image: {}", err.to_string());
                let pixels: Vec<f32> = vec![1.0; (MSDF_SIZE * MSDF_SIZE * 4) as usize];
                return (pixels, Default::default(), Default::default());
            }
        };

        image = image.flipv().resize(
            MSDF_SIZE,
            MSDF_SIZE,
            image::imageops::FilterType::Lanczos3
        );

        let pixels = image.to_rgba32f().to_vec();

        (pixels, glyph_bound, pixel_bound)
    }

    fn convert_atlas_index_to_offset(index: u32) -> [u32; 2] {
        let tex_index = index * MSDF_SIZE;
        let x = tex_index % ATLAS_SIZE;
        let y = tex_index / ATLAS_SIZE;
        [x, y * MSDF_SIZE]
    }

    fn convert_atlas_offset_to_texcoord(offset: [u32; 2]) -> [f64; 2] {
        let atlas_f64 = ATLAS_SIZE as f64;
        [offset[0] as f64 / atlas_f64, offset[1] as f64 / atlas_f64]
    }
}
