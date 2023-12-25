use rust_fontconfig::{FcFontCache, FcPattern};
//use msdf::{GlyphLoader, Projection, SDFTrait};
use ttf_parser::Face;
use msdfgen::{FontExt, Bitmap, Range, MsdfGeneratorConfig, FillRule, Bound};
use std::{fs::File, io::{BufReader, Read}, collections::{HashMap, hash_map::Entry}};
use std::num::NonZeroUsize;
use lru::LruCache;
use crate::texture::Texture;

const ATLAS_SIZE: u32 = 2048;
const MSDF_SIZE: u32 = 32;
const MSDF_RANGE: f32 = 4.0;

pub struct FaceMetrics {
    pub height: f32,
    pub space_size: f32
}

#[derive(Clone, Copy)]
pub struct GlyphMetrics {
    pub atlas_index: u32,
    pub advance: [f32; 2],
    pub glyph_bound: Bound<f32>,
    pub atlas_bound: Bound<f32>
}

pub struct GlyphData {
    pixels: Vec<f32>,
    metrics: GlyphMetrics
}

pub struct Font {
    name: String,
    tab_width: f32,
    font_data: Vec<u8>,
    glyph_cache: HashMap<char, GlyphData>,
    glyph_lru: LruCache<char, GlyphMetrics>,
    atlas_free_list: u32,
    atlas_tex: Texture<f32>
}

impl Font {
    pub fn new(cache: &FcFontCache, name: &str) -> Result<Self, String> {
        let result = cache.query(&FcPattern {
            name: Some(name.to_string()),
            ..Default::default()
        });

        if result.is_none() {
            return Err("Font not found!".to_string());
        }

        let file = File::open(&result.unwrap().path).unwrap();
        let mut reader = BufReader::new(file);
        
        let mut font_data = vec![];
        reader.read_to_end(&mut font_data).unwrap();

        // Validate
        match Face::parse(font_data.as_slice(), 0) {
            Err(err) => return Err(err.to_string()),
            _ => {}
        }

        // Generate font atlas data
        let max_glyphs = (ATLAS_SIZE / MSDF_SIZE) * (ATLAS_SIZE / MSDF_SIZE);
        let atlas_tex = Texture::new(ATLAS_SIZE, ATLAS_SIZE, 4, true, None)?;

        Ok(Self {
            name: name.to_string(),
            tab_width: 4.0,
            font_data,
            glyph_cache: Default::default(),
            glyph_lru: LruCache::new(NonZeroUsize::new(max_glyphs as usize).unwrap()),
            atlas_free_list: 0,
            atlas_tex
        })
    }

    pub fn get_name(&self) -> &str {
        &self.name
    }

    pub fn get_tab_width(&self) -> f32 {
        self.tab_width
    }

    pub fn get_atlas_tex(&self) -> &Texture<f32> {
        &self.atlas_tex
    }

    pub fn get_atlas_size(&self) -> u32 {
        ATLAS_SIZE
    }

    pub fn get_glyph_size(&self) -> u32 {
        MSDF_SIZE
    }

    pub fn get_pixel_range(&self) -> f32 {
        MSDF_RANGE
    }

    pub fn get_face_metrics(&self) -> FaceMetrics {
        let face = Face::parse(self.font_data.as_slice(), 0).unwrap();
        let scale = face.units_per_em() as f32;
        let space_glyph = face.glyph_index(' ').unwrap();
        FaceMetrics {
            height: face.height() as f32 / scale,
            space_size: face.glyph_hor_advance(space_glyph).unwrap() as f32 / scale
        }
    }

    pub fn get_atlas_texcoord(atlas_index: u32) -> [f32; 2] {
        let offset = Self::convert_atlas_index_to_offset(atlas_index);
        return Self::convert_atlas_offset_to_texcoord(offset);
    }

    pub fn get_glyph_data(&mut self, key: char) -> GlyphMetrics {
        let atlas_index: u32;
        match self.glyph_lru.get(&key) {
            Some(v) => {
                // Already present, return immediately
                return *v;
            },
            None => {
                // Pull from free list first
                if self.atlas_free_list < self.glyph_lru.cap().get() as u32 {
                    atlas_index = self.atlas_free_list;
                    self.atlas_free_list += 1;
                } else {
                    // LRU Will always be full here since the LRU is the same
                    // size as the free list, so we can safely evict
                    let lru = self.glyph_lru.pop_lru().unwrap();
                    atlas_index = lru.1.atlas_index;
                }
            }
        }

        // Code reaching this point indicates atlas tex needs to be updated

        let glyph_data = self.glyph_cache.entry(key).or_insert_with(|| {
            // TODO: handle invalid glyphs
            let face = Face::parse(self.font_data.as_slice(), 0).unwrap();
            let glyph_index = face.glyph_index(key).unwrap();
            let mut shape = face.glyph_shape(glyph_index).unwrap();
            let bound = shape.get_bound();
            let framing = bound.autoframe(
                MSDF_SIZE,
                MSDF_SIZE,
                Range::Px(MSDF_RANGE as f64),
                None
            ).unwrap();

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

            let scale = face.units_per_em() as f32;
            GlyphData {
                pixels,
                metrics: GlyphMetrics {
                    atlas_index: 0,
                    advance: [
                        face.glyph_hor_advance(glyph_index).unwrap_or(0) as f32 / scale as f32,
                        face.glyph_ver_advance(glyph_index).unwrap_or(0) as f32 / scale as f32
                    ],
                    glyph_bound: Bound::new(
                        bound.left as f32 / scale,
                        bound.bottom as f32 / scale,
                        bound.right as f32 / scale,
                        bound.top as f32 / scale
                    ),
                    atlas_bound: Bound::new(
                        ((bound.left + framing.translate.x) * framing.scale.x) as f32,
                        ((bound.bottom + framing.translate.y) * framing.scale.y) as f32,
                        ((bound.right + framing.translate.x) * framing.scale.x) as f32,
                        ((bound.top + framing.translate.y) * framing.scale.y) as f32
                    )
                }
            }
        });

        let offset = Self::convert_atlas_index_to_offset(atlas_index);
        self.atlas_tex.update_pixels(
            offset[0], offset[1],
            MSDF_SIZE, MSDF_SIZE,
            &glyph_data.pixels
        );

        glyph_data.metrics.atlas_index = atlas_index;
        self.glyph_lru.put(key, glyph_data.metrics);

        glyph_data.metrics
    }

    fn convert_atlas_index_to_offset(index: u32) -> [u32; 2] {
        let tex_index = index * MSDF_SIZE;
        let x = tex_index % ATLAS_SIZE;
        let y = tex_index / ATLAS_SIZE;
        [x, y]
    }

    fn convert_atlas_offset_to_texcoord(offset: [u32; 2]) -> [f32; 2] {
        let atlas_f32 = ATLAS_SIZE as f32;
        [offset[0] as f32 / atlas_f32, offset[1] as f32 / atlas_f32]
    }
}
